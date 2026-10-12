//! Read-only, conservative verification. Never infer active rules from disk
//! preferences, rule names alone, or an unsuccessful OS query.
use crate::{
    firewall::{OWNER, TABLE, WINDOWS_GROUP, WINDOWS_NETSECURITY},
    model::{
        normalize_address, parse_ports, windows_program, Endpoint, FirewallRule, FirewallScope,
        FirewallTarget, Game, Protocol,
    },
    Result,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, Ipv4Addr},
};

pub const MAX_READBACK_BYTES: usize = 5_000_000;

/// Keep permission failures distinct from invalid OS output. Parsing errors must
/// never cause another authentication prompt, and helper IPC still uses strings.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InspectionError {
    PermissionDenied(String),
    Other(String),
}

impl std::fmt::Display for InspectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PermissionDenied(message) | Self::Other(message) => f.write_str(message),
        }
    }
}

impl From<String> for InspectionError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<&str> for InspectionError {
    fn from(message: &str) -> Self {
        Self::Other(message.into())
    }
}

impl From<std::io::Error> for InspectionError {
    fn from(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            Self::PermissionDenied(error.to_string())
        } else {
            Self::Other(error.to_string())
        }
    }
}

pub(crate) type InspectionResult<T> = std::result::Result<T, InspectionError>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Verification {
    #[default]
    Unverified,
    Matched,
    Empty,
    Different,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleKey {
    address: String,
    protocol: String,
    ports: Option<(u16, u16)>,
    program: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedRule {
    pub key: RuleKey,
    pub source_id: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Inspection {
    pub rules: Vec<ObservedRule>,
}

impl Inspection {
    pub fn validate(&self) -> Result<()> {
        if self.rules.len() > 10000 {
            return Err("Too many observed firewall rules.".into());
        }
        for rule in &self.rules {
            if address_key(&rule.key.address)? != rule.key.address {
                return Err("Non-canonical observed address.".into());
            }
            if !matches!(rule.key.protocol.as_str(), "tcp" | "udp" | "any") {
                return Err("Invalid observed protocol.".into());
            }
            if let Some((start, end)) = rule.key.ports {
                if start == 0 || start > end || rule.key.protocol == "any" {
                    return Err("Invalid observed port filter.".into());
                }
            }
            if let Some(program) = &rule.key.program {
                if windows_program(program)?.to_lowercase() != *program {
                    return Err("Non-canonical observed program.".into());
                }
            }
            if let Some(id) = &rule.source_id {
                crate::model::validate_id(id)?;
            }
        }
        Ok(())
    }
    pub fn keys(&self) -> BTreeSet<RuleKey> {
        self.rules.iter().map(|rule| rule.key.clone()).collect()
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_READBACK_BYTES {
            return Err("Firewall readback is too large.".into());
        }
        let report: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        report.validate()?;
        Ok(report)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RegionState {
    Blocked,
    Partial,
    None,
    Unknown,
}

impl RuleKey {
    fn overlaps(&self, required: &Self) -> bool {
        let a = self.address.parse::<ipnet::IpNet>().ok();
        let b = required.address.parse::<ipnet::IpNet>().ok();
        let addresses = a
            .zip(b)
            .is_some_and(|(a, b)| a.contains(&b.network()) || b.contains(&a.network()));
        let program = self.program.is_none() || self.program == required.program;
        let proto = self.protocol == "any"
            || required.protocol == "any"
            || self.protocol == required.protocol;
        let ports = match (self.ports, required.ports) {
            (Some((a, b)), Some((c, d))) => a <= d && c <= b,
            _ => true,
        };
        addresses && program && proto && ports
    }
    fn covers(&self, required: &Self) -> bool {
        let a = self.address.parse::<ipnet::IpNet>().ok();
        let b = required.address.parse::<ipnet::IpNet>().ok();
        let addresses = a
            .zip(b)
            .is_some_and(|(a, b)| a.prefix_len() <= b.prefix_len() && a.contains(&b.network()));
        let ports = match (self.ports, required.ports) {
            (None, _) => true,
            (Some((a, b)), Some((c, d))) => a <= c && b >= d,
            _ => false,
        };
        addresses
            && ports
            && (self.program.is_none() || self.program == required.program)
            && (self.protocol == "any" || self.protocol == required.protocol)
    }
    fn endpoint(&self, index: usize) -> Endpoint {
        Endpoint {
            id: format!("observed-{index}"),
            address: self.address.clone(),
            protocol: match self.protocol.as_str() {
                "tcp" => Protocol::Tcp,
                "udp" => Protocol::Udp,
                _ => Protocol::Any,
            },
            ports: self.ports.map(|(a, b)| {
                if a == b {
                    a.to_string()
                } else {
                    format!("{a}-{b}")
                }
            }),
            custom: false,
        }
    }
}

fn endpoint_keys(
    endpoint: &Endpoint,
    game: &Game,
    scope: FirewallScope,
) -> Result<BTreeSet<RuleKey>> {
    expected_keys(&[FirewallRule {
        endpoint: endpoint.clone(),
        program: if scope == FirewallScope::Executable {
            game.executable_path.clone()
        } else {
            None
        },
    }])
}

fn address_range(net: ipnet::IpNet) -> (u128, u128) {
    match net {
        ipnet::IpNet::V4(net) => (
            u32::from(net.network()) as u128,
            u32::from(net.broadcast()) as u128,
        ),
        ipnet::IpNet::V6(net) => (u128::from(net.network()), u128::from(net.broadcast())),
    }
}

fn ranges_cover(mut ranges: Vec<(u128, u128)>, (start, end): (u128, u128)) -> bool {
    ranges.sort_unstable();
    let mut next = start;
    for (a, b) in ranges {
        if b < next {
            continue;
        }
        if a > next {
            return false;
        }
        if b >= end {
            return true;
        }
        next = b + 1;
    }
    false
}

/// Multiple narrow OS rules can collectively cover a catalogue range. Partition
/// port intervals, then merge address intervals in each partition. Do not count
/// separate TCP/UDP rules as covering a genuine Any-protocol/no-port target.
fn covered_by(actual: &BTreeSet<RuleKey>, required: &RuleKey) -> bool {
    if actual.iter().any(|key| key.covers(required)) {
        return true;
    }
    let Ok(net) = required.address.parse::<ipnet::IpNet>() else {
        return false;
    };
    let addresses = address_range(net);
    let candidates: Vec<_> = actual
        .iter()
        .filter(|key| {
            key.overlaps(required) && (key.protocol == "any" || key.protocol == required.protocol)
        })
        .filter_map(|key| {
            key.address
                .parse::<ipnet::IpNet>()
                .ok()
                .map(|net| (address_range(net), key.ports))
        })
        .collect();
    let Some((start, end)) = required.ports else {
        return ranges_cover(
            candidates
                .into_iter()
                .filter(|(_, ports)| ports.is_none())
                .map(|(range, _)| range)
                .collect(),
            addresses,
        );
    };
    let mut boundaries = BTreeSet::from([start as u32, end as u32 + 1]);
    for (_, ports) in &candidates {
        if let Some((a, b)) = ports {
            boundaries.insert((*a).max(start) as u32);
            boundaries.insert((*b).min(end) as u32 + 1);
        }
    }
    let boundaries: Vec<_> = boundaries.into_iter().collect();
    boundaries.windows(2).all(|segment| {
        ranges_cover(
            candidates
                .iter()
                .filter(|(_, ports)| {
                    ports.is_none_or(|(a, b)| a as u32 <= segment[0] && b as u32 >= segment[1] - 1)
                })
                .map(|(range, _)| *range)
                .collect(),
            addresses,
        )
    })
}

/// Region status ignores saved switches. Conservative coverage supports broader
/// existing address/port rules and partial TCP/UDP blocks without overclaiming.
pub fn region_states(
    games: &[Game],
    report: &Inspection,
    scope: FirewallScope,
) -> BTreeMap<String, RegionState> {
    let actual = report.keys();
    let mut states = BTreeMap::new();
    for game in games {
        for region in &game.regions {
            let keys = region
                .endpoints
                .iter()
                .map(|e| endpoint_keys(e, game, scope))
                .collect::<Result<Vec<_>>>();
            let state = if actual.is_empty() {
                RegionState::None
            } else {
                match keys {
                    Ok(keys) => {
                        let required: BTreeSet<_> = keys.into_iter().flatten().collect();
                        if required.is_empty() {
                            RegionState::None
                        } else if required
                            .iter()
                            .all(|required| covered_by(&actual, required))
                        {
                            RegionState::Blocked
                        } else if scope == FirewallScope::Executable
                            && game.executable_path.is_none()
                        {
                            RegionState::Unknown
                        } else if required
                            .iter()
                            .any(|required| actual.iter().any(|key| key.overlaps(required)))
                        {
                            RegionState::Partial
                        } else {
                            RegionState::None
                        }
                    }
                    Err(_) => RegionState::Unknown,
                }
            };
            states.insert(format!("{}:{}", game.id, region.id), state);
        }
    }
    states
}

/// Every displayed endpoint is from OS readback, including old/removed targets.
/// Catalogue data and owner IDs only supply labels; unmatched rules stay visible.
pub fn observed_targets(
    games: &[Game],
    report: &Inspection,
    scope: FirewallScope,
    relevant: &BTreeSet<String>,
) -> Vec<FirewallTarget> {
    let mut groups: BTreeMap<(String, String, Option<String>), FirewallTarget> = BTreeMap::new();
    for (index, observed) in report.rules.iter().enumerate() {
        let mut labels = Vec::new();
        for game in games {
            let owned_endpoint = observed
                .source_id
                .as_deref()
                .and_then(|id| id.strip_prefix(&format!("{}-", game.id)));
            for region in &game.regions {
                if region.endpoints.iter().any(|endpoint| {
                    owned_endpoint == Some(endpoint.id.as_str())
                        || (relevant.contains(&game.id)
                            && endpoint_keys(endpoint, game, scope).is_ok_and(|keys| {
                                keys.iter().any(|key| observed.key.overlaps(key))
                            }))
                }) {
                    labels.push((
                        game.id.clone(),
                        game.name.clone(),
                        region.id.clone(),
                        region.name.clone(),
                    ));
                }
            }
        }
        if labels.is_empty() {
            let owner = games
                .iter()
                .filter(|game| {
                    observed
                        .source_id
                        .as_deref()
                        .is_some_and(|id| id.starts_with(&format!("{}-", game.id)))
                })
                .max_by_key(|game| game.id.len());
            labels.push(
                owner
                    .map(|game| {
                        (
                            game.id.clone(),
                            game.name.clone(),
                            String::new(),
                            "Unmapped managed rules".into(),
                        )
                    })
                    .unwrap_or((
                        String::new(),
                        "Unmapped managed rules".into(),
                        String::new(),
                        "From the OS firewall".into(),
                    )),
            );
        }
        for (game_id, game_name, region_id, region_name) in labels {
            let target = groups
                .entry((
                    game_id.clone(),
                    region_id.clone(),
                    observed.key.program.clone(),
                ))
                .or_insert_with(|| FirewallTarget {
                    game_id,
                    game_name,
                    region_id,
                    region_name,
                    executable_path: observed.key.program.clone(),
                    endpoints: vec![],
                });
            target.endpoints.push(observed.key.endpoint(index));
        }
    }
    groups.into_values().collect()
}

fn address_key(address: &str) -> Result<String> {
    let normalized = normalize_address(address)?;
    let net = normalized
        .parse::<ipnet::IpNet>()
        .or_else(|_| normalized.parse::<IpAddr>().map(Into::into))
        .map_err(|e| e.to_string())?;
    Ok(net.to_string())
}

/// NetSecurity returns IPv4 subnets as address/dotted-mask even when Apply used
/// numeric CIDR. Convert only at the Windows readback boundary, then retain the
/// shared target validation and canonical key format.
fn windows_address_key(address: &str) -> Result<String> {
    let address = address.trim();
    let parse = || -> Result<String> {
        if let Some((ip, mask)) = address
            .split_once('/')
            .filter(|(_, mask)| mask.contains('.'))
        {
            let ip = ip
                .parse::<Ipv4Addr>()
                .map_err(|_| "Invalid IPv4 address in Windows firewall output.")?;
            let mask = mask
                .parse::<Ipv4Addr>()
                .map_err(|_| "Invalid IPv4 subnet mask in Windows firewall output.")?;
            let net = ipnet::Ipv4Net::with_netmask(ip, mask)
                .map_err(|_| "Windows firewall reported a non-contiguous IPv4 subnet mask.")?;
            address_key(&net.to_string())
        } else {
            address_key(address)
        }
    };
    parse().map_err(|error| {
        format!("Could not parse address reported by Windows ({address:?}): {error}")
    })
}

pub fn expected_keys(rules: &[FirewallRule]) -> Result<BTreeSet<RuleKey>> {
    let mut keys = BTreeSet::new();
    for rule in rules {
        rule.endpoint.validate()?;
        let ports = parse_ports(&rule.endpoint.ports)?;
        let protocols = match (&rule.endpoint.protocol, ports) {
            (Protocol::Any, Some(_)) => vec!["tcp", "udp"],
            (Protocol::Any, None) => vec!["any"],
            (Protocol::Tcp, _) => vec!["tcp"],
            (Protocol::Udp, _) => vec!["udp"],
        };
        for protocol in protocols {
            keys.insert(RuleKey {
                address: address_key(&rule.endpoint.address)?,
                protocol: protocol.into(),
                ports,
                program: rule
                    .program
                    .as_deref()
                    .map(windows_program)
                    .transpose()?
                    .map(|p| p.to_lowercase()),
            });
        }
    }
    Ok(keys)
}

pub fn compare(actual: &BTreeSet<RuleKey>, expected: &BTreeSet<RuleKey>) -> Verification {
    if actual.is_empty() {
        Verification::Empty
    } else if actual == expected {
        Verification::Matched
    } else {
        Verification::Different
    }
}

fn nft_address(value: &Value) -> Result<String> {
    if let Some(address) = value.as_str() {
        return address_key(address);
    }
    let prefix = &value["prefix"];
    let address = prefix["addr"]
        .as_str()
        .ok_or("Unsupported nft address expression.")?;
    let length = prefix["len"]
        .as_u64()
        .ok_or("Unsupported nft address prefix.")?;
    address_key(&format!("{address}/{length}"))
}

fn protocol(value: &Value) -> Result<String> {
    match value.as_str().map(str::to_ascii_lowercase).as_deref() {
        Some("tcp" | "6") => Ok("tcp".into()),
        Some("udp" | "17") => Ok("udp".into()),
        Some("any" | "256") => Ok("any".into()),
        _ => match value.as_u64() {
            Some(6) => Ok("tcp".into()),
            Some(17) => Ok("udp".into()),
            Some(256) => Ok("any".into()),
            _ => Err("Unsupported firewall protocol.".into()),
        },
    }
}

fn nft_ports(value: &Value) -> Result<(u16, u16)> {
    let (start, end) = if let Some(port) = value.as_u64() {
        (port, port)
    } else {
        let range = value["range"]
            .as_array()
            .filter(|r| r.len() == 2)
            .ok_or("Unsupported nft port expression.")?;
        (
            range[0].as_u64().ok_or("Invalid nft port.")?,
            range[1].as_u64().ok_or("Invalid nft port.")?,
        )
    };
    if start == 0 || end > 65535 || start > end {
        return Err("Invalid nft ports.".into());
    }
    Ok((start as u16, end as u16))
}

/// Only recognize the owned outbound chain and the exact expressions emitted by
/// LobbyLocker. An altered jump/allow/extra constraint must not look verified.
pub fn linux_keys(json: &Value) -> Result<BTreeSet<RuleKey>> {
    Ok(linux_snapshot(json)?.keys())
}

pub fn linux_snapshot(json: &Value) -> Result<Inspection> {
    let entries = json["nftables"]
        .as_array()
        .ok_or("Invalid nftables response.")?;
    let mut table = false;
    let mut chain = false;
    let mut rules = Vec::new();
    for entry in entries {
        if entry.get("metainfo").is_some() {
            continue;
        }
        if let Some(value) = entry.get("table") {
            if table
                || value["family"] != "inet"
                || value["name"] != TABLE
                || value["comment"] != OWNER
                || value
                    .get("flags")
                    .is_some_and(|flags| flags.as_array().is_none_or(|flags| !flags.is_empty()))
            {
                return Err("The firewall table is not owned by LobbyLocker.".into());
            }
            table = true;
            continue;
        }
        if let Some(value) = entry.get("chain") {
            if chain
                || value["family"] != "inet"
                || value["table"] != TABLE
                || value["name"] != "outbound"
                || value["type"] != "filter"
                || value["hook"] != "output"
                || value["prio"] != 10
                || value["policy"] != "accept"
                || value
                    .get("flags")
                    .is_some_and(|flags| flags.as_array().is_none_or(|flags| !flags.is_empty()))
            {
                return Err("LobbyLocker's firewall chain has changed.".into());
            }
            chain = true;
            continue;
        }
        let rule = entry
            .get("rule")
            .ok_or("Unsupported object in LobbyLocker's firewall table.")?;
        if rule["family"] != "inet" || rule["table"] != TABLE || rule["chain"] != "outbound" {
            return Err("Unexpected LobbyLocker rule chain.".into());
        }
        let id = rule["comment"]
            .as_str()
            .and_then(|c| c.strip_prefix("ll:"))
            .ok_or("Unrecognized firewall rule ownership.")?;
        crate::model::validate_id(id)?;
        let mut address = None;
        let mut proto = None;
        let mut ports = None;
        let mut dropped = false;
        let expressions = rule["expr"]
            .as_array()
            .filter(|e| e.len() <= 5)
            .ok_or("Unsupported nft rule expressions.")?;
        for expression in expressions {
            if dropped {
                return Err("Unexpected expression after drop.".into());
            }
            if expression.get("counter").is_some() {
                continue;
            }
            if expression.get("drop").is_some() {
                dropped = true;
                continue;
            }
            let matched = expression
                .get("match")
                .filter(|m| m["op"] == "==" || m["op"] == "in")
                .ok_or("Unsupported nft firewall expression.")?;
            let left = &matched["left"];
            let right = &matched["right"];
            if left["payload"]["field"] == "daddr"
                && ["ip", "ip6"]
                    .iter()
                    .any(|p| left["payload"]["protocol"] == *p)
                && address.is_none()
            {
                let parsed = nft_address(right)?;
                if parsed.contains(':') != (left["payload"]["protocol"] == "ip6") {
                    return Err("Mismatched nft address family.".into());
                }
                address = Some(parsed);
            } else if left["meta"]["key"] == "l4proto" && proto.is_none() {
                proto = Some(protocol(right)?);
            } else if left["payload"]["field"] == "dport" && ports.is_none() {
                let port_proto = protocol(&left["payload"]["protocol"])?;
                if proto.as_deref().is_some_and(|p| p != port_proto) || port_proto == "any" {
                    return Err("Unexpected nft port protocol.".into());
                }
                proto = Some(port_proto);
                ports = Some(nft_ports(right)?);
            } else {
                return Err("Unsupported or repeated nft match.".into());
            }
        }
        if !dropped {
            return Err("LobbyLocker rule does not drop traffic.".into());
        }
        rules.push(ObservedRule {
            source_id: Some(id.into()),
            key: RuleKey {
                address: address.ok_or("Missing nft destination address.")?,
                protocol: proto.unwrap_or("any".into()),
                ports,
                program: None,
            },
        });
        if rules.len() > 10000 {
            return Err("Too many firewall rules to verify.".into());
        }
    }
    if !table || !chain {
        return Err("Incomplete LobbyLocker firewall table.".into());
    }
    let report = Inspection { rules };
    report.validate()?;
    Ok(report)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WindowsReport {
    enforced: bool,
    supported: bool,
    rules: Vec<WindowsRule>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WindowsRule {
    address: String,
    protocol: Value,
    ports: String,
    program: Option<String>,
    #[serde(default)]
    source_id: Option<String>,
}

pub fn windows_keys(json: &Value) -> Result<BTreeSet<RuleKey>> {
    Ok(windows_snapshot(json)?.keys())
}

pub fn windows_snapshot(json: &Value) -> Result<Inspection> {
    let report: WindowsReport = serde_json::from_value(json.clone()).map_err(|e| e.to_string())?;
    if !report.supported || (!report.enforced && !report.rules.is_empty()) {
        return Err(
            "Windows firewall is disabled or LobbyLocker rules have unsupported filters.".into(),
        );
    }
    if report.rules.len() > 10000 {
        return Err("Too many firewall rules to verify.".into());
    }
    let rules = report
        .rules
        .into_iter()
        .map(|rule| {
            let ports = if rule.ports.eq_ignore_ascii_case("any") {
                None
            } else {
                Some(rule.ports)
            };
            Ok(ObservedRule {
                source_id: rule.source_id,
                key: RuleKey {
                    address: windows_address_key(&rule.address)?,
                    protocol: protocol(&rule.protocol)?,
                    ports: parse_ports(&ports)?,
                    program: rule
                        .program
                        .filter(|p| !p.eq_ignore_ascii_case("any"))
                        .as_deref()
                        .map(windows_program)
                        .transpose()?
                        .map(|p| p.to_lowercase()),
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let snapshot = Inspection { rules };
    snapshot.validate()?;
    Ok(snapshot)
}

/// ActiveStore reflects effective rules, not just disk configuration. This script
/// contains no write cmdlets and never runs a selected game executable.
pub fn windows_inspection_script() -> String {
    format!(
        r#"$ErrorActionPreference='Stop'; [Console]::OutputEncoding=(New-Object -TypeName System.Text.UTF8Encoding -ArgumentList $false); try {{ {WINDOWS_NETSECURITY}$supported=$true; $rows=@(); $profiles=@(Get-NetFirewallProfile -PolicyStore ActiveStore); $services=@(Get-Service -Name MpsSvc,BFE); $enforced=$profiles.Count -eq 3 -and $services.Count -eq 2 -and @($services | Where-Object {{[string]$_.Status -ne 'Running'}}).Count -eq 0 -and @($profiles | Where-Object {{[string]$_.Enabled -ne 'True' -or [string]$_.AllowLocalFirewallRules -eq 'False'}}).Count -eq 0;
$owned=@(Get-NetFirewallRule -PolicyStore ActiveStore | Where-Object {{$_.Group -eq '{WINDOWS_GROUP}'}});
foreach ($r in $owned) {{
 if ([string]$r.Enabled -ne 'True' -or [string]$r.Direction -ne 'Outbound' -or [string]$r.Action -ne 'Block') {{continue}}
 $a=$r|Get-NetFirewallAddressFilter; $p=$r|Get-NetFirewallPortFilter; $app=$r|Get-NetFirewallApplicationFilter; $svc=$r|Get-NetFirewallServiceFilter; $iface=$r|Get-NetFirewallInterfaceFilter; $type=$r|Get-NetFirewallInterfaceTypeFilter; $security=$r|Get-NetFirewallSecurityFilter;
  if ([string]$r.Profile -ne 'Any' -or @($a.RemoteAddress).Count -ne 1 -or [string]$a.LocalAddress -ne 'Any' -or [string]$p.LocalPort -ne 'Any' -or @($p.RemotePort).Count -ne 1 -or [string]$svc.Service -ne 'Any' -or [string]$iface.InterfaceAlias -ne 'Any' -or [string]$type.InterfaceType -ne 'Any' -or ($app.Package -and [string]$app.Package -ne 'Any') -or [string]$security.Authentication -ne 'NotRequired' -or [string]$security.Encryption -ne 'NotRequired' -or [string]$security.LocalUser -ne 'Any' -or [string]$security.RemoteUser -ne 'Any' -or [string]$security.RemoteMachine -ne 'Any') {{$supported=$false}}
 $source=$null; if ([string]$r.Name -match '^LobbyLocker\.[a-fA-F0-9]{{32}}\.([a-zA-Z0-9_-]{{1,96}})\.(TCP|UDP|Any)$') {{$source=$Matches[1]}}
 $rows += [pscustomobject]@{{address=[string]$a.RemoteAddress[0];protocol=[string]$p.Protocol;ports=[string]$p.RemotePort[0];program=[string]$app.Program;sourceId=$source}}
}}
[pscustomobject]@{{enforced=$enforced;supported=$supported;rules=@($rows)}}|ConvertTo-Json -Depth 5 -Compress
}} catch {{
 $denied=$_.CategoryInfo.Category -eq [System.Management.Automation.ErrorCategory]::PermissionDenied; $exception=$_.Exception;
 while ($null -ne $exception) {{
  if ($exception -is [System.UnauthorizedAccessException] -or $exception.HResult -eq -2147024891 -or $exception.HResult -eq -2147217405) {{$denied=$true}}
  $exception=$exception.InnerException;
 }}
 [pscustomobject]@{{permissionDenied=$denied;message=[string]$_.Exception.Message}}|ConvertTo-Json -Compress; exit 1
}}"#
    )
}

#[cfg(any(target_os = "windows", test))]
fn windows_query_error(stdout: &[u8], stderr: &[u8]) -> InspectionError {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct QueryError {
        permission_denied: bool,
        message: String,
    }
    // PowerShell reports an error category/HRESULT, not localized error text.
    if let Ok(error) = serde_json::from_slice::<QueryError>(stdout) {
        if error.permission_denied {
            InspectionError::PermissionDenied(error.message)
        } else {
            InspectionError::Other(error.message)
        }
    } else {
        InspectionError::Other(String::from_utf8_lossy(stderr).trim().to_owned())
    }
}

#[cfg(any(target_os = "linux", test))]
fn nft_query_error(stderr: &[u8]) -> InspectionError {
    // nft has no structured error category. Its queries run with LC_ALL=C so
    // these native permission diagnostics are stable across UI/system locales.
    let message = String::from_utf8_lossy(stderr).trim().to_owned();
    if message.contains("Operation not permitted") || message.contains("Permission denied") {
        InspectionError::PermissionDenied(message)
    } else {
        InspectionError::Other(message)
    }
}

fn query(command: &mut std::process::Command) -> InspectionResult<Value> {
    use std::{
        io::Read,
        process::Stdio,
        time::{Duration, Instant},
    };
    // A stalled OS query must not trap startup in Working forever. Drain both
    // pipes concurrently, with caps, so noisy errors cannot deadlock or exhaust memory.
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            let message = format!("Could not inspect firewall rules: {error}");
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                InspectionError::PermissionDenied(message)
            } else {
                InspectionError::Other(message)
            }
        })?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Missing inspection output pipe.")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Missing inspection error pipe.")?;
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(5_000_001)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.take(65_536).read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(error.to_string());
            }
            Ok(None) if started.elapsed() >= Duration::from_secs(15) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("Firewall inspection timed out. No rules were changed.".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let stdout = out
        .join()
        .map_err(|_| "Inspection output reader failed.")?
        .map_err(InspectionError::from)?;
    let stderr = err
        .join()
        .map_err(|_| "Inspection error reader failed.")?
        .map_err(InspectionError::from)?;
    if stdout.len() > MAX_READBACK_BYTES {
        return Err("Firewall inspection response is too large.".into());
    }
    if !status?.success() {
        #[cfg(target_os = "windows")]
        return Err(windows_query_error(&stdout, &stderr));
        #[cfg(target_os = "linux")]
        return Err(nft_query_error(&stderr));
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        return Err(String::from_utf8_lossy(&stderr).trim().to_owned().into());
    }
    serde_json::from_slice(&stdout)
        .map_err(|e| format!("Invalid firewall inspection response: {e}").into())
}

pub fn inspect() -> Result<Inspection> {
    inspect_with_error().map_err(|error| error.to_string())
}

pub(crate) fn inspect_with_error() -> InspectionResult<Inspection> {
    #[cfg(target_os = "linux")]
    {
        let tables = query(
            std::process::Command::new("/usr/bin/nft")
                .env("LC_ALL", "C")
                .args(["-j", "-n", "list", "tables"]),
        )?;
        if !owned_table_present(&tables)? {
            return Ok(Inspection::default());
        }
        let json = query(
            std::process::Command::new("/usr/bin/nft")
                .env("LC_ALL", "C")
                .args(["-j", "-n", "list", "table", "inet", TABLE]),
        )?;
        linux_snapshot(&json).map_err(Into::into)
    }
    #[cfg(target_os = "windows")]
    {
        let json = query(crate::firewall::powershell_command()?.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &windows_inspection_script(),
        ]))?;
        windows_snapshot(&json).map_err(Into::into)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err("Firewall verification supports Linux and Windows only.".into())
    }
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn owned_table_present(json: &Value) -> Result<bool> {
    let entries = json["nftables"]
        .as_array()
        .ok_or("Invalid nftables table response.")?;
    let mut found = false;
    for entry in entries {
        if entry.get("metainfo").is_some() {
            continue;
        }
        let table = entry
            .get("table")
            .ok_or("Unexpected nftables table response.")?;
        let family = table["family"]
            .as_str()
            .ok_or("Missing nftables table family.")?;
        let name = table["name"]
            .as_str()
            .ok_or("Missing nftables table name.")?;
        found |= family == "inet" && name == TABLE;
    }
    Ok(found)
}

#[cfg(test)]
#[path = "../tests/unit/inspection.rs"]
mod tests;
