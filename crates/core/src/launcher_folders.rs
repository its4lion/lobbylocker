//! Recognizable installation receipts only. Bounded shallow discovery, never
//! recursive drive scans, executable launches, package queries, or network calls.
use crate::installed::{InstalledGame, Scan};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

fn text(path: &Path) -> Option<String> {
    if !fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let file = fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > 2_000_000 {
        return None;
    }
    let mut text = String::new();
    file.take(2_000_001).read_to_string(&mut text).ok()?;
    (text.len() <= 2_000_000).then_some(text)
}

fn folder_name(directory: &Path) -> Option<String> {
    let name = directory.file_name()?.to_str()?;
    (!name.trim().is_empty() && name.len() <= 120).then(|| name.to_owned())
}

fn xbox(directory: &Path) -> Option<InstalledGame> {
    let config = text(&directory.join("Content/MicrosoftGame.config"))
        .or_else(|| text(&directory.join("MicrosoftGame.config")))?;
    // roxmltree rejects DTDs by default; never expand external entities.
    let xml = roxmltree::Document::parse_with_options(
        &config,
        roxmltree::ParsingOptions {
            nodes_limit: 10_000,
            ..Default::default()
        },
    )
    .ok()?;
    let identity = xml
        .descendants()
        .find(|node| node.has_tag_name("Identity"))?
        .attribute("Name")?;
    if identity.is_empty()
        || identity.len() > 80
        || !identity
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return None;
    }
    let label = xml
        .descendants()
        .find(|node| node.has_tag_name("ShellVisuals"))
        .and_then(|node| node.attribute("DefaultDisplayName"))
        .filter(|name| {
            !name.trim().is_empty() && name.len() <= 120 && !name.starts_with("ms-resource:")
        });
    Some(InstalledGame {
        id: format!("xbox-{}", identity.replace('.', "_")),
        name: label
            .map(str::to_owned)
            .or_else(|| folder_name(directory))?,
        launcher: "Xbox".into(),
        directory: directory.to_owned(),
    })
}

fn epic(directory: &Path) -> Option<InstalledGame> {
    let receipts = fs::read_dir(directory.join(".egstore")).ok()?;
    for receipt in receipts.take(64).flatten() {
        let path = receipt.path();
        if path.extension().is_none_or(|ext| ext != "mancpn") {
            continue;
        }
        let Some(json) =
            text(&path).and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        else {
            continue;
        };
        let Some(app) = json["AppName"].as_str().filter(|id| {
            !id.is_empty()
                && id.len() <= 80
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        }) else {
            continue;
        };
        return Some(InstalledGame {
            id: format!("epic-{app}"),
            name: folder_name(directory)?,
            launcher: "Epic".into(),
            directory: directory.to_owned(),
        });
    }
    None
}

#[cfg(test)]
fn scan(roots: &[PathBuf]) -> Scan {
    scan_with_exclusions(roots, &[])
}

pub(crate) fn scan_with_exclusions(roots: &[PathBuf], excluded: &[PathBuf]) -> Scan {
    let mut scan = Scan::default();
    let mut candidates = BTreeSet::new();
    for root in roots.iter().take(64) {
        for base in [
            root.clone(),
            root.join("XboxGames"),
            root.join("Epic Games"),
        ] {
            let Ok(base) = fs::canonicalize(base) else {
                continue;
            };
            if crate::installed::excluded_path(&base, excluded) {
                continue;
            }
            candidates.insert(base.clone());
            if let Ok(entries) = fs::read_dir(base) {
                for entry in entries.take(256).flatten() {
                    if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                        if let Ok(path) = fs::canonicalize(entry.path()) {
                            candidates.insert(path);
                        }
                    }
                }
            }
        }
    }
    for directory in candidates {
        if crate::installed::excluded_path(&directory, excluded) {
            continue;
        }
        let game = if directory.join("_retail_/Overwatch.exe").is_file() {
            Some(InstalledGame {
                id: "overwatch2".into(),
                name: "Overwatch 2".into(),
                launcher: "Battle.net".into(),
                directory: directory.clone(),
            })
        } else {
            xbox(&directory).or_else(|| epic(&directory))
        };
        if let Some(game) = game {
            scan.games.push(game);
        }
    }
    scan
}

#[cfg(test)]
#[path = "../tests/unit/launcher_folders.rs"]
mod tests;
