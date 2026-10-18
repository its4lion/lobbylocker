//! Read-only, bounded discovery from local launcher metadata. No game executables run.
use crate::{
    model::{normalize_address, Endpoint, Game, Protocol, Region},
    Result,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct InstalledGame {
    /// Existing supported game ID, or a stable launcher-specific custom game ID.
    pub id: String,
    pub name: String,
    pub launcher: String,
    pub directory: PathBuf,
}

#[derive(Clone, Default, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    pub games: Vec<InstalledGame>,
    pub warnings: Vec<String>,
    pub icons: BTreeMap<String, String>,
    /// Effective default, registered, and user-added roots, normalized once.
    pub library_paths: Vec<PathBuf>,
}

/// Steam ships these tools as installed apps, but they are not playable games.
pub fn is_launcher_tool(game: &InstalledGame) -> bool {
    if game.launcher != "Steam" {
        return false;
    }
    let name = game.name.to_lowercase();
    matches!(
        game.id.as_str(),
        "steam-1493710"
            | "steam-1070560"
            | "steam-1391110"
            | "steam-1628350"
            | "steam-228980"
            | "steam-250820"
    ) || name == "proton"
        || name.starts_with("proton ")
        || name.starts_with("steam linux runtime")
        || name.starts_with("steamworks common redistributables")
        || name == "steamvr"
}

pub fn empty_game(id: String, name: &str) -> Result<Game> {
    let game = Game {
        schema_version: 1, id, name: name.trim().into(),
        source: None, updated_at: None, executable_path: None,
        note: "User-configured game. Add server IPs in Advanced; installation metadata does not reveal server addresses. Windows uses the selected game executable; Linux rules apply system-wide.".into(),
        regions: vec![],
    };
    game.validate()?;
    Ok(game)
}

pub fn custom_game(
    id: String,
    name: &str,
    area: &str,
    address: &str,
    protocol: Protocol,
    ports: Option<String>,
) -> Result<Game> {
    let game = Game {
        schema_version: 1, id, name: name.trim().into(), source: None, updated_at: None, executable_path: None,
        note: "User-configured game. IPs are supplied by you, not a verified server catalogue. Windows uses the selected game executable; Linux rules apply system-wide. Installing a game does not reveal its server addresses.".into(),
        regions: vec![Region {
            id: "custom".into(), name: "My servers".into(), area: area.into(), probe_target: None, probe_custom: false, provider_scope: None,
            endpoints: vec![Endpoint { id: "custom-server".into(), address: normalize_address(address)?, protocol, ports, custom: true }],
        }],
    };
    game.validate()?;
    Ok(game)
}

fn text(path: &Path) -> Result<String> {
    if fs::metadata(path).map_err(|e| e.to_string())?.len() > 2_000_000 {
        return Err("Launcher metadata exceeds 2 MB.".into());
    }
    fs::read_to_string(path).map_err(|e| e.to_string())
}

// Valve's quoted key/value format, including braces, comments and escaped paths.
#[derive(Debug)]
enum Value {
    Text(String),
    Object(BTreeMap<String, Value>),
}
fn vdf(input: &str) -> Result<BTreeMap<String, Value>> {
    let mut tokens = Vec::new();
    let mut chars = input.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => (),
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '{' | '}' => tokens.push(c.to_string()),
            '"' => {
                let mut s = String::new();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    if c == '"' {
                        closed = true;
                        break;
                    }
                    if c == '\\' {
                        let next = chars.next().ok_or("Incomplete VDF escape.")?;
                        if next != '\\' && next != '"' {
                            s.push('\\');
                        }
                        s.push(next);
                    } else {
                        s.push(c);
                    }
                }
                if !closed {
                    return Err("Unclosed VDF string.".into());
                }
                tokens.push(s);
            }
            _ => return Err("Unexpected launcher metadata token.".into()),
        }
        if tokens.len() > 100_000 {
            return Err("Too many metadata tokens.".into());
        }
    }
    fn object(
        tokens: &[String],
        index: &mut usize,
        nested: bool,
        depth: usize,
    ) -> Result<BTreeMap<String, Value>> {
        if depth > 16 {
            return Err("Launcher metadata is too deeply nested.".into());
        }
        let mut map = BTreeMap::new();
        while let Some(key) = tokens.get(*index) {
            *index += 1;
            if key == "}" {
                return if nested {
                    Ok(map)
                } else {
                    Err("Unexpected closing brace.".into())
                };
            }
            if key == "{" {
                return Err("Missing VDF key.".into());
            }
            let value = tokens.get(*index).ok_or("Missing VDF value.")?;
            *index += 1;
            let value = if value == "{" {
                Value::Object(object(tokens, index, true, depth + 1)?)
            } else if value == "}" {
                return Err("Missing VDF value.".into());
            } else {
                Value::Text(value.clone())
            };
            map.insert(key.to_ascii_lowercase(), value);
        }
        if nested {
            return Err("Unclosed VDF object.".into());
        }
        Ok(map)
    }
    object(&tokens, &mut 0, false, 0)
}
fn string<'a>(map: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a str> {
    match map.get(key)? {
        Value::Text(s) => Some(s),
        _ => None,
    }
}
fn child<'a>(map: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a BTreeMap<String, Value>> {
    match map.get(key)? {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

fn steam_libraries(roots: &[PathBuf], warnings: &mut Vec<String>) -> BTreeSet<PathBuf> {
    // Canonical roots avoid reading the same library through launcher symlinks.
    let roots: BTreeSet<_> = roots
        .iter()
        .filter(|p| p.join("steamapps").is_dir())
        .filter_map(|p| fs::canonicalize(p).ok())
        .collect();
    let mut libraries = roots.clone();
    for root in roots {
        let path = root.join("steamapps/libraryfolders.vdf");
        if !path.is_file() {
            continue;
        }
        match text(&path).and_then(|s| vdf(&s)) {
            Ok(map) => {
                if let Some(folders) = child(&map, "libraryfolders") {
                    for (key, value) in folders {
                        if !key.bytes().all(|c| c.is_ascii_digit()) {
                            continue;
                        }
                        let path = match value {
                            Value::Text(s) => Some(s.as_str()),
                            Value::Object(m) => string(m, "path"),
                        };
                        if let Some(path) = path.filter(|p| !p.is_empty()) {
                            let path = PathBuf::from(path);
                            if let Ok(path) = fs::canonicalize(&path) {
                                libraries.insert(path);
                            } else {
                                warnings
                                    .push(format!("Steam library not found: {}", path.display()));
                                if path.is_absolute() {
                                    libraries.insert(path);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => warnings.push(format!("Could not read {}: {e}", path.display())),
        }
    }
    libraries
}

pub fn scan_steam(roots: &[PathBuf]) -> Scan {
    scan_steam_filtered(roots, &[])
}

fn scan_steam_filtered(roots: &[PathBuf], excluded: &[PathBuf]) -> Scan {
    let mut scan = Scan::default();
    for root in steam_libraries(roots, &mut scan.warnings)
        .into_iter()
        .filter(|root| !excluded_path(root, excluded))
        .take(64)
    {
        scan.library_paths.push(root.clone());
        let apps = root.join("steamapps");
        let entries = match fs::read_dir(&apps) {
            Ok(entries) => entries,
            Err(e) => {
                scan.warnings
                    .push(format!("Could not read {}: {e}", apps.display()));
                continue;
            }
        };
        for entry in entries.take(10_000).flatten() {
            let path = entry.path();
            let filename = entry.file_name().to_string_lossy().into_owned();
            if !filename.starts_with("appmanifest_") || !filename.ends_with(".acf") {
                continue;
            }
            let result = text(&path).and_then(|s| {
                let map = vdf(&s)?;
                let app = child(&map, "appstate").ok_or("Missing AppState.")?;
                let app_id = string(app, "appid").ok_or("Missing Steam app ID.")?;
                if app_id.is_empty()
                    || !app_id.bytes().all(|c| c.is_ascii_digit())
                    || app_id.len() > 20
                {
                    return Err("Invalid Steam app ID.".into());
                }
                let name = string(app, "name")
                    .filter(|s| !s.trim().is_empty() && s.len() <= 120)
                    .ok_or("Missing or invalid game name.")?;
                let install = string(app, "installdir").ok_or("Missing installation directory.")?;
                // Never use an untrusted manifest to follow paths outside steamapps/common.
                if install.is_empty()
                    || install == "."
                    || install == ".."
                    || install.contains(['/', '\\'])
                {
                    return Err("Invalid Steam installation directory.".into());
                }
                let directory = apps.join("common").join(install);
                if !directory.is_dir() {
                    return Ok(None);
                }
                let installed = string(app, "stateflags")
                    .and_then(|v| v.parse::<u32>().ok())
                    .is_some_and(|v| v & 4 != 0);
                if !installed {
                    return Ok(None);
                }
                let id = match app_id {
                    "730" => "cs2".into(),
                    "1422450" => "deadlock".into(),
                    "2357570" => "overwatch2".into(),
                    _ => format!("steam-{app_id}"),
                };
                Ok(Some(InstalledGame {
                    id,
                    name: name.into(),
                    launcher: "Steam".into(),
                    directory,
                }))
            });
            match result {
                Ok(Some(game)) if is_launcher_tool(&game) => (),
                Ok(Some(game)) => {
                    if let Some(icon) = crate::icons::steam_icon(
                        filename
                            .trim_start_matches("appmanifest_")
                            .trim_end_matches(".acf"),
                        roots,
                    )
                    .or_else(|| crate::icons::installation_icon(&game.directory, &game.id))
                    {
                        scan.icons.insert(game.id.clone(), icon);
                    }
                    scan.games.push(game);
                }
                Ok(None) => (),
                Err(e) => scan
                    .warnings
                    .push(format!("Skipped {}: {e}", path.display())),
            }
        }
    }
    finish(&mut scan);
    scan
}

fn finish(scan: &mut Scan) {
    scan.games.sort_by(|a, b| a.id.cmp(&b.id));
    scan.games.dedup_by(|a, b| a.id == b.id);
    scan.games.sort_by_key(|g| g.name.to_lowercase());
    scan.warnings.truncate(100);
}

pub fn scan() -> Scan {
    scan_with_roots(&[])
}

pub fn scan_with_roots(extra_roots: &[PathBuf]) -> Scan {
    scan_with_settings(extra_roots, &[])
}

pub fn normalized_library_path(path: &Path) -> PathBuf {
    let path = if path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("steamapps"))
    {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

pub(crate) fn excluded_path(path: &Path, excluded: &[PathBuf]) -> bool {
    let normalized = normalized_library_path(path);
    excluded.iter().any(|root| normalized.starts_with(root))
}

fn default_library_paths() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        for suffix in [
            ".steam/steam",
            ".local/share/Steam",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ] {
            roots.push(home.join(suffix));
        }
    }
    for key in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(base) = std::env::var_os(key) {
            let base = PathBuf::from(base);
            roots.extend([
                base.join("Steam"),
                base.join("Epic Games"),
                base.join("Overwatch"),
            ]);
        }
    }
    #[cfg(target_os = "windows")]
    if let Some(path) = registered_steam_path() {
        roots.push(path);
    }
    if let Some(drive) = std::env::var_os("SystemDrive") {
        roots.push(PathBuf::from(format!(
            "{}\\XboxGames",
            drive.to_string_lossy()
        )));
    }
    if let Some(base) = std::env::var_os("ProgramData") {
        roots.push(PathBuf::from(base).join("Epic/EpicGamesLauncher/Data/Manifests"));
    }
    roots
}

#[cfg(target_os = "windows")]
fn registered_steam_path() -> Option<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows::{
        core::w,
        Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ},
    };

    // A bounded UTF-16 buffer preserves registered paths without launching reg.exe.
    let mut value = vec![0u16; 32_768];
    let mut length = (value.len() * std::mem::size_of::<u16>()) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Valve\Steam"),
            w!("SteamPath"),
            RRF_RT_REG_SZ,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut length),
        )
    };
    if result.is_err() || length as usize > value.len() * 2 || !length.is_multiple_of(2) {
        return None;
    }
    let value = &value[..length as usize / 2];
    let end = value
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(value.len());
    (end > 0).then(|| PathBuf::from(OsString::from_wide(&value[..end])))
}

pub fn scan_with_settings(extra_roots: &[PathBuf], excluded: &[PathBuf]) -> Scan {
    scan_library_roots(&default_library_paths(), extra_roots, excluded)
}

fn scan_library_roots(
    default_roots: &[PathBuf],
    extra_roots: &[PathBuf],
    excluded: &[PathBuf],
) -> Scan {
    let excluded: Vec<_> = excluded
        .iter()
        .take(256)
        .map(|path| normalized_library_path(path))
        .collect();
    let bases: BTreeSet<_> = default_roots
        .iter()
        .filter(|root| root.is_dir())
        .chain(extra_roots.iter().take(32))
        .map(|root| normalized_library_path(root))
        .collect();
    let mut roots = Vec::new();
    for root in &bases {
        roots.extend([root.clone(), root.join("Steam"), root.join("SteamLibrary")]);
    }
    let mut scan = scan_steam_filtered(&roots, &excluded);
    for root in extra_roots.iter().take(32) {
        if !root.is_dir() {
            scan.warnings
                .push(format!("Game library not found: {}", root.display()));
        }
    }
    let local_roots: Vec<_> = bases
        .into_iter()
        .filter(|root| !excluded_path(root, &excluded))
        .take(64)
        .collect();
    scan.library_paths.extend(local_roots.iter().cloned());
    // Epic's Windows .item manifests explicitly supply the install location.
    for root in &local_roots {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.take(10_000).flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "item") {
                    continue;
                }
                let result = text(&path).and_then(|s| epic_manifest(&s));
                match result {
                    Ok(Some(game)) => scan.games.push(game),
                    Ok(None) => (),
                    Err(e) => scan
                        .warnings
                        .push(format!("Skipped {}: {e}", path.display())),
                }
            }
        }
    }
    // Prefer the launcher's authoritative display name over a folder receipt.
    scan.games
        .extend(crate::launcher_folders::scan_with_exclusions(&local_roots, &excluded).games);
    scan.games
        .retain(|game| !excluded_path(&game.directory, &excluded));
    let games: BTreeSet<_> = scan.games.iter().map(|game| &game.id).collect();
    scan.icons.retain(|id, _| games.contains(id));
    scan.library_paths.sort();
    scan.library_paths.dedup();
    finish(&mut scan);
    for game in &scan.games {
        if !scan.icons.contains_key(&game.id) {
            if let Some(icon) = crate::icons::installation_icon(&game.directory, &game.id) {
                scan.icons.insert(game.id.clone(), icon);
            }
        }
    }
    scan
}

fn epic_manifest(input: &str) -> Result<Option<InstalledGame>> {
    let json: serde_json::Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
    if json["bIsIncompleteInstall"].as_bool() == Some(true) {
        return Ok(None);
    }
    let app = json["AppName"].as_str().ok_or("Missing Epic app ID.")?;
    if app.is_empty()
        || app.len() > 80
        || !app
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("Invalid Epic app ID.".into());
    }
    let name = json["DisplayName"]
        .as_str()
        .filter(|n| !n.trim().is_empty() && n.len() <= 120)
        .ok_or("Missing Epic game name.")?;
    let directory = PathBuf::from(
        json["InstallLocation"]
            .as_str()
            .filter(|p| !p.is_empty())
            .ok_or("Missing Epic install location.")?,
    );
    if !directory.is_dir() {
        return Ok(None);
    }
    Ok(Some(InstalledGame {
        id: format!("epic-{app}"),
        name: name.into(),
        launcher: "Epic Games".into(),
        directory,
    }))
}

#[cfg(test)]
#[path = "../tests/unit/installed.rs"]
mod tests;
