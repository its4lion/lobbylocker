//! Fill every `providerScope` region in `data/games/*.json` with the current
//! ranges from Google's public network feed, so a fresh install can block
//! those regions before the first refresh. The same feed backs the in-app
//! refresh, so these values match what that refresh would write.
//!
//! Run from the repository root:
//!   cargo run -p fetch-cloud-ranges

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const FEED: &str = "https://www.gstatic.com/ipranges/cloud.json";

fn main() -> ExitCode {
    let games_dir = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/games"));
    match run(&games_dir) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(games_dir: &Path) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .https_only(true)
        .build()
        .map_err(|error| error.to_string())?;
    let feed: serde_json::Value = client
        .get(FEED)
        .send()
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    let prefixes = feed
        .get("prefixes")
        .and_then(serde_json::Value::as_array)
        .ok_or("the feed has no prefixes")?;
    let creation_time = feed
        .get("creationTime")
        .and_then(serde_json::Value::as_str)
        .ok_or("the feed has no creationTime")?
        .to_string();

    let mut entries = fs::read_dir(games_dir)
        .map_err(|error| format!("{}: {error}", games_dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let text =
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut game: serde_json::Value =
            serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;

        let Some(regions) = game
            .get_mut("regions")
            .and_then(serde_json::Value::as_array_mut)
        else {
            continue;
        };
        let mut touched = false;
        for region in regions {
            let Some(scope) = region
                .get("providerScope")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
            else {
                continue;
            };
            let addresses = addresses_for(prefixes, &scope);
            if addresses.is_empty() {
                return Err(format!("{scope} returned no ranges"));
            }
            let id = region
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let mut endpoints: Vec<serde_json::Value> = addresses
                .into_iter()
                .enumerate()
                .map(|(index, address)| {
                    // Endpoint ids must stay identical to the ones the in-app
                    // refresh writes, otherwise a refresh would treat every
                    // bundled address as new.
                    serde_json::json!({
                        "id": format!("{id}-{index}"),
                        "address": address,
                        "protocol": "udp",
                        "ports": serde_json::Value::Null,
                        "custom": false,
                    })
                })
                .collect();
            if let Some(custom) = region
                .get("endpoints")
                .and_then(serde_json::Value::as_array)
            {
                endpoints.extend(
                    custom
                        .iter()
                        .filter(|endpoint| {
                            endpoint.get("custom").and_then(serde_json::Value::as_bool)
                                == Some(true)
                        })
                        .cloned(),
                );
            }
            region["endpoints"] = serde_json::Value::Array(endpoints);
            touched = true;
        }
        if !touched {
            continue;
        }

        game["updatedAt"] = serde_json::Value::String(creation_time.clone());
        let mut updated = serde_json::to_string_pretty(&game).map_err(|error| error.to_string())?;
        updated.push('\n');
        fs::write(&path, updated).map_err(|error| format!("{}: {error}", path.display()))?;

        let regions = game["regions"].as_array().map_or(0, Vec::len);
        let total: usize = game["regions"].as_array().map_or(0, |regions| {
            regions
                .iter()
                .map(|region| region["endpoints"].as_array().map_or(0, Vec::len))
                .sum()
        });
        println!(
            "{}: {regions} regions, {total} addresses",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
        );
    }
    Ok(())
}

/// The deduplicated, sorted `Google Cloud` prefixes in `scope`, mirroring
/// `core/src/catalogue.rs`.
fn addresses_for(prefixes: &[serde_json::Value], scope: &str) -> BTreeSet<String> {
    let mut addresses = BTreeSet::new();
    for prefix in prefixes {
        if prefix.get("scope").and_then(serde_json::Value::as_str) != Some(scope) {
            continue;
        }
        if prefix.get("service").and_then(serde_json::Value::as_str) != Some("Google Cloud") {
            continue;
        }
        let ipv4 = prefix.get("ipv4Prefix").and_then(serde_json::Value::as_str);
        let ipv6 = prefix.get("ipv6Prefix").and_then(serde_json::Value::as_str);
        match (ipv4, ipv6) {
            (Some(address), None) | (None, Some(address)) => {
                addresses.insert(address.to_string());
            }
            _ => {}
        }
    }
    addresses
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefix(
        scope: &str,
        service: &str,
        ipv4: Option<&str>,
        ipv6: Option<&str>,
    ) -> serde_json::Value {
        serde_json::json!({
            "scope": scope,
            "service": service,
            "ipv4Prefix": ipv4,
            "ipv6Prefix": ipv6,
        })
    }

    #[test]
    fn only_google_cloud_prefixes_in_the_scope_are_kept() {
        let prefixes = vec![
            prefix("europe-west1", "Google Cloud", Some("34.0.0.0/14"), None),
            prefix("europe-west1", "Google Cloud", Some("34.0.0.0/14"), None),
            prefix("europe-west1", "Google Cloud", None, Some("2001:4860::/32")),
            prefix(
                "europe-west1",
                "Google Cloud",
                Some("34.1.0.0/16"),
                Some("2001:4860:1::/32"),
            ),
            prefix("us-central1", "Google Cloud", Some("35.0.0.0/16"), None),
            prefix("europe-west1", "Private", Some("10.0.0.0/8"), None),
        ];
        let addresses = addresses_for(&prefixes, "europe-west1");
        assert_eq!(
            addresses.into_iter().collect::<Vec<_>>(),
            vec!["2001:4860::/32", "34.0.0.0/14"]
        );
    }
}
