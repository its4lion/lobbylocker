use crate::{
    model::{validate_id, Game, Preferences, Snapshot},
    Result,
};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub struct Store {
    pub directory: PathBuf,
    defaults: PathBuf,
}

/// Launcher-scoped ids that later gained a bundled game file. An upgraded
/// profile keeps the old placeholder forever, which would hide the bundled
/// data and keep asking the user for server addresses.
const RENAMED_LAUNCHER_GAMES: &[(&str, &str)] = &[("steam-381210", "dbd")];

/// Last background refresh per game, so opening a game page does not hit the
/// official feed more than once a day. Only timestamps are stored here; the
/// user's games stay in `games/`.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DataRefresh {
    checked_at: BTreeMap<String, u64>,
    /// Any attempt, so a dead feed is not retried on every page open.
    #[serde(default)]
    attempted_at: BTreeMap<String, u64>,
}

fn now_seconds() -> u64 {
    crate::now_seconds()
}

fn atomic_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().ok_or("Invalid configuration path.")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

impl Store {
    pub fn default_directory() -> Result<PathBuf> {
        dirs::config_dir()
            .map(|p| p.join("lobbylocker"))
            .ok_or("Could not find your configuration directory.".into())
    }

    pub fn open(directory: PathBuf, defaults: &Path) -> Result<Self> {
        let store = Self {
            directory,
            defaults: defaults.to_path_buf(),
        };
        fs::create_dir_all(store.directory.join("games")).map_err(|e| e.to_string())?;
        if defaults.is_dir() {
            for entry in fs::read_dir(defaults).map_err(|e| e.to_string())? {
                let path = entry.map_err(|e| e.to_string())?.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    let game = Game::from_json(&fs::read(&path).map_err(|e| e.to_string())?)
                        .map_err(|e| format!("Invalid {}: {e}", path.display()))?;
                    game.validate()?;
                    let target = store.game_path(&game.id)?;
                    // Never replace a user's saved game. Only an empty
                    // launcher-created placeholder is filled from defaults.
                    if target.exists() {
                        store.repair_empty_placeholder(&target, &game)?;
                    } else {
                        store.save_game(&game)?;
                    }
                }
            }
        }
        if !store.directory.join("preferences.json").exists() {
            store.save_preferences(&Preferences::default())?;
        }
        // A failed repair must not stop the app from opening; the old entry
        // simply remains until the next launch.
        let _ = store.migrate_renamed_launcher_games();
        Ok(store)
    }

    /// A placeholder created by launcher discovery has no targets, so seeding
    /// the bundled file cannot discard user input. Games with real targets are
    /// never rewritten.
    fn repair_empty_placeholder(&self, path: &Path, default: &Game) -> Result<()> {
        let saved = Game::from_json(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if !saved.regions.is_empty()
            || saved.note != crate::installed::EMPTY_GAME_NOTE
            || default.regions.is_empty()
        {
            return Ok(());
        }
        let mut game = default.clone();
        game.executable_path = saved.executable_path;
        self.save_game(&game)
    }

    /// Move an empty launcher placeholder to the bundled game that replaced it.
    /// A placeholder with user-added targets is left untouched.
    fn migrate_renamed_launcher_games(&self) -> Result<()> {
        let preferences_path = self.directory.join("preferences.json");
        let mut preferences =
            Preferences::from_json(&fs::read(&preferences_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let mut preferences_changed = false;
        for &(old_id, new_id) in RENAMED_LAUNCHER_GAMES {
            let old_path = self.game_path(old_id)?;
            let new_path = self.game_path(new_id)?;
            if !old_path.is_file() || !new_path.is_file() {
                continue;
            }
            let old = Game::from_json(&fs::read(&old_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            if old
                .regions
                .iter()
                .any(|region| !region.endpoints.is_empty())
            {
                continue;
            }
            let mut builtin = Game::from_json(&fs::read(&new_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            // Without bundled data there is nothing to move to.
            if builtin.regions.is_empty() {
                continue;
            }
            if builtin.executable_path.is_none() {
                builtin.executable_path = old.executable_path;
                builtin.validate()?;
                self.save_game(&builtin)?;
            }
            if preferences.library_game_ids.iter().any(|id| id == old_id) {
                preferences.library_game_ids = preferences
                    .library_game_ids
                    .iter()
                    .map(|id| {
                        if id == old_id {
                            new_id.to_owned()
                        } else {
                            id.clone()
                        }
                    })
                    .collect();
                preferences.library_game_ids.sort();
                preferences.library_game_ids.dedup();
                preferences_changed = true;
            }
            // Keep the bytes for recovery without letting the empty twin load.
            let mut archive = old_path.with_extension("json.migrated");
            let mut suffix = 1;
            while archive.exists() {
                archive = old_path.with_extension(format!("json.migrated.{suffix}"));
                suffix += 1;
            }
            fs::rename(&old_path, archive).map_err(|e| e.to_string())?;
        }
        if preferences_changed {
            preferences.validate()?;
            self.save_preferences(&preferences)?;
        }
        Ok(())
    }

    fn game_path(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.directory.join("games").join(format!("{id}.json")))
    }

    pub fn default_game(&self, id: &str) -> Result<Game> {
        validate_id(id)?;
        let path = self.defaults.join(format!("{id}.json"));
        let game = Game::from_json(
            &fs::read(&path)
                .map_err(|e| format!("Could not load game defaults {}: {e}", path.display()))?,
        )
        .map_err(|e| e.to_string())?;
        game.validate()?;
        if game.id != id {
            return Err("Default game filename does not match its ID.".into());
        }
        Ok(game)
    }

    pub fn save_game(&self, game: &Game) -> Result<()> {
        game.validate()?;
        atomic_json(&self.game_path(&game.id)?, game)
    }

    pub fn save_preferences(&self, preferences: &Preferences) -> Result<()> {
        preferences.validate()?;
        atomic_json(&self.directory.join("preferences.json"), preferences)
    }

    /// When this game's background refresh last ran, or `None` if never or unreadable.
    pub fn data_refresh_checked_at(&self, game_id: &str) -> Option<u64> {
        validate_id(game_id).ok()?;
        let raw = fs::read(self.directory.join("data-refresh.json")).ok()?;
        let checked = serde_json::from_slice::<DataRefresh>(&raw).ok()?;
        let at = *checked.checked_at.get(game_id)?;
        // A damaged or hand-edited file must not pin refreshes off forever.
        (at > 0 && at <= now_seconds()).then_some(at)
    }

    pub fn mark_data_refresh_checked(&self, game_id: &str) -> Result<()> {
        self.mark_data_refresh(game_id, true)
    }

    pub fn mark_data_refresh_attempted(&self, game_id: &str) -> Result<()> {
        self.mark_data_refresh(game_id, false)
    }

    /// Last attempt, successful or not.
    pub fn data_refresh_attempted_at(&self, game_id: &str) -> Option<u64> {
        validate_id(game_id).ok()?;
        let raw = fs::read(self.directory.join("data-refresh.json")).ok()?;
        let checked = serde_json::from_slice::<DataRefresh>(&raw).ok()?;
        let at = *checked.attempted_at.get(game_id)?;
        (at > 0 && at <= now_seconds()).then_some(at)
    }

    fn mark_data_refresh(&self, game_id: &str, succeeded: bool) -> Result<()> {
        validate_id(game_id)?;
        let path = self.directory.join("data-refresh.json");
        let mut refresh = fs::read(&path)
            .ok()
            .and_then(|raw| serde_json::from_slice::<DataRefresh>(&raw).ok())
            .unwrap_or(DataRefresh {
                checked_at: BTreeMap::new(),
                attempted_at: BTreeMap::new(),
            });
        let now = now_seconds();
        // A tampered file must not grow without bound.
        refresh.checked_at.retain(|_, at| *at > 0 && *at <= now);
        refresh.attempted_at.retain(|_, at| *at > 0 && *at <= now);
        refresh.attempted_at.insert(game_id.to_string(), now);
        if succeeded {
            refresh.checked_at.insert(game_id.to_string(), now);
        }
        atomic_json(&path, &refresh)
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let mut games = Vec::new();
        for entry in fs::read_dir(self.directory.join("games")).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let game = Game::from_json(&fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Invalid game file {}: {e}", path.display()))?;
            game.validate()?;
            if path.file_stem().and_then(|p| p.to_str()) != Some(game.id.as_str()) {
                return Err(format!(
                    "Game filename must match its ID: {}",
                    path.display()
                ));
            }
            games.push(game);
        }
        games.sort_by_key(|g| match g.id.as_str() {
            "cs2" => 0,
            "deadlock" => 1,
            "overwatch2" => 2,
            "dbd" => 3,
            _ => 4,
        });
        let preferences = Preferences::from_json(
            &fs::read(self.directory.join("preferences.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("Invalid preferences.json: {e}"))?;
        preferences.validate()?;
        Ok(Snapshot {
            games,
            preferences,
            config_dir: self.directory.display().to_string(),
            platform: std::env::consts::OS.into(),
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/store.rs"]
mod tests;
