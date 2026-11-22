use crate::{
    model::{normalize_address, Endpoint, Game, Protocol, Region},
    Result,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, ToSocketAddrs},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
struct SdrConfig {
    success: bool,
    pops: BTreeMap<String, Pop>,
}
#[derive(Deserialize)]
struct Pop {
    desc: String,
    #[serde(default)]
    relays: Vec<Relay>,
}
#[derive(Deserialize)]
struct Relay {
    ipv4: String,
    port_range: [u16; 2],
}

fn area(code: &str) -> &'static str {
    match code {
        "dxb" | "bah" => "Middle East",
        "gru" | "eze" | "lim" | "scl" => "South America",
        "ams" | "fra" | "lhr" | "sto" | "sto2" | "vie" | "waw" | "mad" | "par" | "hel" | "fsn"
        | "dfra" | "dvie" | "dsto" | "dlhr" => "Europe",
        "iad" | "ord" | "lax" | "sea" | "atl" | "dfw" | "eat" => "North America",
        "syd" | "mel" | "gum" => "Oceania",
        "jnb" => "Africa",
        _ => "Asia",
    }
}

/// Games with an official upstream feed that `refresh` can update. Anything else
/// needs a reviewed game file, so an automatic pass must leave it alone.
pub fn has_official_feed(id: &str) -> bool {
    matches!(id, "overwatch2" | "cs2" | "deadlock" | "dbd")
}

/// Merge bundled defaults, preserving custom data and configured probes.
pub fn merge_defaults(game: &mut Game, defaults: Game) {
    // Live Valve relays and GameLift beacons take precedence over seed addresses.
    if matches!(game.id.as_str(), "cs2" | "deadlock" | "dbd") {
        return;
    }
    let shipped: BTreeSet<String> = defaults
        .regions
        .iter()
        .map(|region| region.id.clone())
        .collect();
    for region in &mut game.regions {
        // A retired provider-fed region's addresses are regenerated, so only the
        // user's own are worth keeping.
        if !shipped.contains(&region.id) && region.provider_scope.is_some() {
            region.endpoints.retain(|endpoint| endpoint.custom);
        }
    }
    game.regions
        .retain(|region| shipped.contains(&region.id) || region.endpoints.iter().any(|e| e.custom));

    let mut endpoint_ids: BTreeSet<String> = game
        .regions
        .iter()
        .flat_map(|region| region.endpoints.iter().map(|endpoint| endpoint.id.clone()))
        .collect();
    for region in defaults.regions {
        if let Some(saved) = game.regions.iter_mut().find(|saved| saved.id == region.id) {
            saved.name = region.name;
            saved.area = region.area;
            if saved.probe_target.is_none() && !saved.probe_custom {
                saved.probe_target = region.probe_target;
            }
            if saved.provider_scope.is_none() && region.provider_scope.is_some() {
                saved.provider_scope = region.provider_scope;
            }
            for endpoint in region.endpoints {
                if let Some(saved_endpoint) = saved
                    .endpoints
                    .iter_mut()
                    .find(|saved_endpoint| saved_endpoint.id == endpoint.id)
                {
                    if !saved_endpoint.custom {
                        *saved_endpoint = endpoint;
                    }
                    continue;
                }
                let target_exists = saved.endpoints.iter().any(|saved_endpoint| {
                    saved_endpoint.address == endpoint.address
                        && saved_endpoint.protocol == endpoint.protocol
                        && saved_endpoint.ports == endpoint.ports
                });
                if !target_exists && endpoint_ids.insert(endpoint.id.clone()) {
                    saved.endpoints.push(endpoint);
                }
            }
        } else {
            endpoint_ids.extend(region.endpoints.iter().map(|endpoint| endpoint.id.clone()));
            game.regions.push(region);
        }
    }
}

pub fn refresh(game: &Game) -> Result<Game> {
    if game.id == "overwatch2" {
        return refresh_overwatch(game);
    }
    if game.id == "dbd" {
        return refresh_dbd(game);
    }
    // Upstream URL is selected by trusted code, not user input (no arbitrary URL fetch / SSRF).
    let appid = match game.id.as_str() {
        "cs2" => 730,
        "deadlock" => 1422450,
        _ => return Err(
            "This game has no official automatic feed. Import a reviewed game JSON file instead."
                .into(),
        ),
    };
    let url = format!("https://api.steampowered.com/ISteamApps/GetSDRConfig/v1/?appid={appid}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .https_only(true)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    if response.content_length().is_some_and(|n| n > 2_000_000) {
        return Err("Server feed is too large.".into());
    }
    // Bound the body even when the upstream omits Content-Length.
    use std::io::Read;
    let mut body = Vec::new();
    response
        .take(2_000_001)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() > 2_000_000 {
        return Err("Server feed is too large.".into());
    }
    let config: SdrConfig = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    if !config.success || config.pops.is_empty() {
        return Err("Valve returned no relay configuration.".into());
    }
    let mut updated = game.clone();
    let mut regions = Vec::new();
    for (code, pop) in config.pops {
        if pop.relays.is_empty() {
            continue;
        }
        let mut endpoints: Vec<_> = pop
            .relays
            .iter()
            .enumerate()
            .map(|(i, relay)| Endpoint {
                id: format!("{code}-{i}"),
                address: relay.ipv4.clone(),
                protocol: Protocol::Udp,
                ports: Some(format!("{}-{}", relay.port_range[0], relay.port_range[1])),
                custom: false,
            })
            .collect();
        let old = game.regions.iter().find(|r| r.id == code);
        if let Some(old) = old {
            endpoints.extend(old.endpoints.iter().filter(|e| e.custom).cloned());
        }
        regions.push(Region {
            id: code.clone(),
            name: pop.desc,
            area: area(&code).into(),
            probe_target: old.and_then(|r| r.probe_target.clone()).or_else(|| {
                if old.is_some_and(|r| r.probe_custom) {
                    None
                } else {
                    endpoints.first().map(|e| e.address.clone())
                }
            }),
            probe_custom: old.is_some_and(|r| r.probe_custom),
            provider_scope: None,
            endpoints,
        });
    }
    // Preserve manually added regions and custom endpoints for retired upstream locations.
    for old in &game.regions {
        if !regions.iter().any(|r| r.id == old.id) {
            let mut custom = old.clone();
            custom.endpoints.retain(|e| e.custom);
            if !custom.endpoints.is_empty() {
                regions.push(custom);
            }
        }
    }
    updated.regions = regions;
    updated.source = Some(url);
    updated.updated_at = Some(format!(
        "unix:{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    ));
    updated.note = "Official Valve SDR relays. Shared with other Steam games; region blocking does not guarantee matchmaking location. ICMP may be unavailable.".into();
    updated.validate()?;
    Ok(updated)
}

const GOOGLE_CLOUD_URL: &str = "https://www.gstatic.com/ipranges/cloud.json";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleNetworkFeed {
    creation_time: Option<String>,
    prefixes: Vec<GoogleNetworkPrefix>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleNetworkPrefix {
    ipv4_prefix: Option<String>,
    ipv6_prefix: Option<String>,
    service: String,
    scope: String,
}

fn refresh_overwatch(game: &Game) -> Result<Game> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .https_only(true)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(GOOGLE_CLOUD_URL)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    use std::io::Read;
    let mut body = Vec::new();
    response
        .take(2_000_001)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() > 2_000_000 {
        return Err("Google Cloud feed is too large.".into());
    }
    update_google_regions(game, &body)
}

/// Parse provider-owned data without relying on another blocker's code or IP list.
/// Only scopes explicitly configured in this game's file can contribute rules.
fn update_google_regions(game: &Game, body: &[u8]) -> Result<Game> {
    game.validate()?;
    if body.len() > 2_000_000 {
        return Err("Google Cloud feed is too large.".into());
    }
    let requested: BTreeSet<_> = game
        .regions
        .iter()
        .filter_map(|r| r.provider_scope.as_deref())
        .collect();
    if requested.is_empty() {
        return Err("No Google Cloud region mappings configured. Add providerScope to a region in this game's JSON file.".into());
    }
    let feed: GoogleNetworkFeed =
        serde_json::from_slice(body).map_err(|e| format!("Invalid Google Cloud feed: {e}"))?;
    if feed.prefixes.len() > 20_000 {
        return Err("Google Cloud feed contains too many prefixes.".into());
    }
    let mut by_scope: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for prefix in feed.prefixes {
        if prefix.service != "Google Cloud" || !requested.contains(prefix.scope.as_str()) {
            continue;
        }
        let (address, ipv6) = match (prefix.ipv4_prefix, prefix.ipv6_prefix) {
            (Some(address), None) => (address, false),
            (None, Some(address)) => (address, true),
            _ => {
                return Err(
                    "A configured Google Cloud prefix has missing or ambiguous address fields."
                        .into(),
                )
            }
        };
        if !address.contains('/') || address.contains(':') != ipv6 {
            return Err("Invalid Google Cloud CIDR or address family.".into());
        }
        let address = normalize_address(&address)?;
        by_scope.entry(prefix.scope).or_default().insert(address);
    }
    // Refuse incomplete updates instead of silently removing a location's rules.
    for scope in &requested {
        if !by_scope.contains_key(*scope) {
            return Err(format!("Google returned no ranges for configured scope {scope}. Your saved file was not changed."));
        }
    }
    let mut updated = game.clone();
    for region in &mut updated.regions {
        let Some(scope) = &region.provider_scope else {
            continue;
        };
        let custom: Vec<_> = region
            .endpoints
            .iter()
            .filter(|e| e.custom)
            .cloned()
            .collect();
        region.endpoints = by_scope[scope]
            .iter()
            .enumerate()
            .map(|(i, address)| Endpoint {
                id: format!("{}-{i}", region.id),
                address: address.clone(),
                protocol: Protocol::Udp,
                ports: None,
                custom: false,
            })
            .collect();
        region.endpoints.extend(custom);
        // A cloud subnet's base address is not a known responding server.
        // Preserve explicit probes, but never fabricate one from its CIDR.
    }
    updated.source = Some(GOOGLE_CLOUD_URL.into());
    updated.updated_at = feed.creation_time;
    updated.note = "Community-maintained ranges; shared cloud ranges can affect other apps on Linux. Ping is an estimate; some servers may not reply.".into();
    updated.validate()?;
    Ok(updated)
}

const LEGACY_DBD_SOURCE: &str = "https://ip-ranges.amazonaws.com/ip-ranges.json";
const DBD_BEACON_SOURCE: &str =
    "https://docs.aws.amazon.com/general/latest/gr/gamelift.html#udp-ping-beacons";
const DBD_BEACON_NOTE: &str = "Targets the selected GameLift latency beacons on UDP port 7770. The game decides where to place a match. ICMP ping may be unavailable.";
const DBD_BEACON_PORT: u16 = 7770;

fn is_beacon_target(endpoint: &Endpoint) -> bool {
    endpoint.address.parse::<IpAddr>().is_ok()
        && endpoint.protocol == Protocol::Udp
        && crate::model::parse_ports(&endpoint.ports)
            == Ok(Some((DBD_BEACON_PORT, DBD_BEACON_PORT)))
}

/// Project older saved catalogues onto the beacon targets before the engine
/// can resolve any selections. Loading a game does not rewrite its saved file.
pub(crate) fn migrate_dbd(game: &mut Game) -> Result<()> {
    if game.id != "dbd" {
        return Ok(());
    }
    let legacy_source = game.source.as_deref() == Some(LEGACY_DBD_SOURCE);
    let has_broad_targets = |region: &Region| {
        region.provider_scope.is_some()
            && region
                .endpoints
                .iter()
                .any(|endpoint| !endpoint.custom && !is_beacon_target(endpoint))
    };
    if !legacy_source && !game.regions.iter().any(has_broad_targets) {
        return Ok(());
    }

    let defaults = Game::from_json(include_bytes!("../../../data/games/dbd.json"))?;
    let needs_update = |region: &Region| {
        if legacy_source {
            region.provider_scope.is_some()
                || defaults.regions.iter().any(|seed| seed.id == region.id)
        } else {
            has_broad_targets(region)
        }
    };
    let mut reserved_ids: BTreeSet<_> = game
        .regions
        .iter()
        .flat_map(|region| {
            region
                .endpoints
                .iter()
                .filter(|endpoint| endpoint.custom || !needs_update(region))
                .map(|endpoint| endpoint.id.clone())
        })
        .collect();
    for region in game
        .regions
        .iter_mut()
        .filter(|region| needs_update(region))
    {
        let scope = region
            .provider_scope
            .get_or_insert_with(|| region.id.clone());
        let addresses = defaults
            .regions
            .iter()
            .find(|seed| seed.provider_scope.as_deref() == Some(scope.as_str()))
            .into_iter()
            .flat_map(|seed| &seed.endpoints)
            .filter_map(|endpoint| endpoint.address.parse().ok())
            .collect();
        set_dbd_beacons(region, addresses, &mut reserved_ids);
    }
    game.source = Some(DBD_BEACON_SOURCE.into());
    game.note = DBD_BEACON_NOTE.into();
    game.updated_at = defaults.updated_at;
    game.validate()
}

fn refresh_dbd(game: &Game) -> Result<Game> {
    refresh_dbd_with(game, |host| {
        (host, DBD_BEACON_PORT)
            .to_socket_addrs()
            .map(|addresses| addresses.map(|address| address.ip()).collect())
            .map_err(|error| format!("Could not resolve GameLift beacon {host}: {error}"))
    })
}

fn refresh_dbd_with(
    game: &Game,
    mut resolve: impl FnMut(&str) -> Result<BTreeSet<IpAddr>>,
) -> Result<Game> {
    game.validate()?;
    let mut updated = game.clone();
    let mut reserved_ids: BTreeSet<_> = game
        .regions
        .iter()
        .flat_map(|region| {
            region
                .endpoints
                .iter()
                .filter(|endpoint| endpoint.custom || region.provider_scope.is_none())
                .map(|endpoint| endpoint.id.clone())
        })
        .collect();
    let mut resolved = BTreeMap::<String, BTreeSet<IpAddr>>::new();
    for region in &mut updated.regions {
        let Some(scope) = &region.provider_scope else {
            continue;
        };
        if !resolved.contains_key(scope) {
            let hostname = format!("gamelift-ping.{scope}.api.aws");
            let mut addresses = resolve(&hostname)?;
            addresses.retain(|address| normalize_address(&address.to_string()).is_ok());
            if addresses.is_empty() {
                return Err(format!(
                    "No usable addresses for GameLift beacon {hostname}."
                ));
            }
            resolved.insert(scope.clone(), addresses);
        }
        set_dbd_beacons(region, resolved[scope].clone(), &mut reserved_ids);
    }
    if resolved.is_empty() {
        return Err("No GameLift beacon regions configured.".into());
    }
    updated.source = Some(DBD_BEACON_SOURCE.into());
    updated.note = DBD_BEACON_NOTE.into();
    if updated != *game {
        updated.updated_at = Some(crate::now_seconds().to_string());
    }
    updated.validate()?;
    Ok(updated)
}

fn set_dbd_beacons(
    region: &mut Region,
    addresses: BTreeSet<IpAddr>,
    reserved_ids: &mut BTreeSet<String>,
) {
    if !region.probe_custom {
        region.probe_target = addresses.first().map(ToString::to_string);
    }
    region.endpoints.retain(|endpoint| endpoint.custom);
    let mut index = 0;
    for address in addresses {
        let id = loop {
            let id = format!("{}-{index}", region.id);
            index += 1;
            if reserved_ids.insert(id.clone()) {
                break id;
            }
        };
        region.endpoints.push(Endpoint {
            id,
            address: address.to_string(),
            protocol: Protocol::Udp,
            ports: Some(DBD_BEACON_PORT.to_string()),
            custom: false,
        });
    }
}

#[cfg(test)]
#[path = "../tests/unit/catalogue.rs"]
mod tests;
