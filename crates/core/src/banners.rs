//! Banner art for games, taken from the user's own machine.
//!
//! Sources, best first: art the user installed themselves (Steam keeps
//! SteamGridDB posters in the client's `userdata/*/config/grid` folder), then
//! Steam's own cached art next to the client
//! (`appcache/librarycache/<app id>/…/library_hero.jpg`). Nothing is
//! downloaded and no user-supplied URL is ever fetched: a game without local
//! art simply has no banner.

use crate::Result;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Largest image accepted from disk (bytes).
pub const MAX_BANNER_BYTES: usize = 4_000_000;

/// Steam app id behind a game id, when the game maps to a Steam store entry.
/// Launcher-scoped ids (`steam-730`) carry the app id; the bundled games
/// carry it in a fixed table that matches the launcher scan.
pub fn steam_app_id(game_id: &str) -> Option<u32> {
    match game_id {
        "cs2" => Some(730),
        "deadlock" => Some(1_422_450),
        "overwatch2" => Some(2_357_570),
        "dbd" => Some(381_210),
        "rocketleague" => Some(252_950),
        other => other
            .strip_prefix("steam-")
            .and_then(|id| id.parse::<u32>().ok())
            .filter(|id| *id > 0),
    }
}

/// True when a game can have Steam art at all.
pub fn has_banner(game_id: &str) -> bool {
    steam_app_id(game_id).is_some()
}

/// Files that may hold banner art for a game, best first: the user's own art,
/// then Steam's cached art.
pub fn banner_paths(game_id: &str) -> Vec<PathBuf> {
    let Some(app_id) = steam_app_id(game_id) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for root in crate::installed::steam_client_roots() {
        paths.extend(custom_art(&root, app_id));
        paths.extend(client_art(&root, app_id));
    }
    paths
}

/// Read a banner file, bounded so a huge file cannot exhaust memory.
pub fn read_banner(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_BANNER_BYTES as u64 {
        return Err("Banner file is too large.".into());
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.is_empty() || bytes.len() > MAX_BANNER_BYTES {
        return Err("Banner file is empty.".into());
    }
    Ok(bytes)
}

/// Art the user installed themselves: SteamGridDB posters and the like live
/// in `userdata/<account>/config/grid`, named after the app id.
fn custom_art(root: &Path, app_id: u32) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let userdata = root.join("userdata");
    let Ok(accounts) = fs::read_dir(&userdata) else {
        return paths;
    };
    let prefix = app_id.to_string();
    for account in accounts.flatten() {
        let grid = account.path().join("config").join("grid");
        let Ok(files) = fs::read_dir(&grid) else {
            continue;
        };
        for file in files.flatten() {
            let name = file.file_name().to_string_lossy().to_lowercase();
            if !name.starts_with(&prefix) {
                continue;
            }
            if !(name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg")) {
                continue;
            }
            paths.push(file.path());
        }
    }
    paths
}

/// Steam's own cached art for the app.
fn client_art(root: &Path, app_id: u32) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let cache = root.join("appcache").join("librarycache");
    let app_dir = cache.join(app_id.to_string());
    if app_dir.is_dir() {
        for name in [
            "library_hero.jpg",
            "library_header.jpg",
            "library_capsule.jpg",
        ] {
            if let Some(path) = find_named(&app_dir, name, 3) {
                paths.push(path);
            }
        }
    }
    // Older clients kept the art flat, named after the app id.
    for name in [
        format!("{app_id}_library_hero.jpg"),
        format!("{app_id}_header.jpg"),
    ] {
        let path = cache.join(name);
        if path.is_file() {
            paths.push(path);
        }
    }
    paths
}

/// The first file called `name` under `dir`, up to `depth` levels down.
fn find_named(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_named(&path, name, depth - 1) {
                return Some(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some(name) {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
#[path = "../tests/unit/banners.rs"]
mod tests;
