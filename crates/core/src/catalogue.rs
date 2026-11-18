use crate::{
    model::{normalize_address, Endpoint, Game, Protocol, Region},
    Result,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
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
    // Valve's live relay list takes precedence over the bundled subset.
    if matches!(game.id.as_str(), "cs2" | "deadlock") {
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

const AWS_IP_RANGES_URL: &str = "https://ip-ranges.amazonaws.com/ip-ranges.json";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AwsIpRangesFeed {
    sync_token: Option<String>,
    prefixes: Vec<AwsIpPrefix>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AwsIpPrefix {
    ip_prefix: Option<String>,
    region: String,
    service: String,
}

fn refresh_dbd(game: &Game) -> Result<Game> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .https_only(true)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(AWS_IP_RANGES_URL)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    use std::io::Read;
    let mut body = Vec::new();
    response
        .take(5_000_001)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() > 5_000_000 {
        return Err("AWS IP ranges feed is too large.".into());
    }
    update_aws_regions(game, &body)
}

fn update_aws_regions(game: &Game, body: &[u8]) -> Result<Game> {
    game.validate()?;
    if body.len() > 5_000_000 {
        return Err("AWS IP ranges feed is too large.".into());
    }
    let requested: BTreeSet<_> = game
        .regions
        .iter()
        .filter_map(|r| r.provider_scope.as_deref())
        .collect();
    if requested.is_empty() {
        return Err("No AWS region mappings configured. Add providerScope to a region in this game's JSON file.".into());
    }
    let feed: AwsIpRangesFeed =
        serde_json::from_slice(body).map_err(|e| format!("Invalid AWS IP ranges feed: {e}"))?;
    if feed.prefixes.len() > 50_000 {
        return Err("AWS feed contains too many prefixes.".into());
    }
    let mut by_region: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for prefix in feed.prefixes {
        if prefix.service != "EC2" {
            continue;
        }
        let Some(address) = prefix.ip_prefix else {
            continue;
        };
        if address.contains(':') {
            continue;
        }
        if !requested.contains(prefix.region.as_str()) {
            continue;
        }
        if !address.contains('/') {
            continue;
        }
        let address = normalize_address(&address)?;
        by_region.entry(prefix.region).or_default().insert(address);
    }
    for region in &requested {
        if !by_region.contains_key(*region) || by_region[*region].is_empty() {
            return Err(format!("AWS returned no EC2 ranges for configured region {region}. Your saved file was not changed."));
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
        region.endpoints = by_region[scope]
            .iter()
            .enumerate()
            .map(|(i, address)| Endpoint {
                id: format!("{}-{i}", region.id),
                address: address.clone(),
                protocol: Protocol::Any,
                ports: None,
                custom: false,
            })
            .collect();
        region.endpoints.extend(custom);
    }
    updated.source = Some(AWS_IP_RANGES_URL.into());
    updated.updated_at = feed.sync_token;
    updated.note = "AWS EC2 ranges by region; shared cloud ranges can affect other apps on Linux. us-east-1 stays available because DBD backend services depend on it. Ping is an estimate; some servers may not reply.".into();
    updated.validate()?;
    Ok(updated)
}

#[cfg(test)]
#[path = "../tests/unit/catalogue.rs"]
mod tests;
