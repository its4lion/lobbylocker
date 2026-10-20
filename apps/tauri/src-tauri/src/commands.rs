use lobbylocker_core::{
    connections::{self, Connection},
    engine::{Engine, Runtime},
    i18n::{Catalogue, Locale},
    inspection::{RegionState, Verification, VerificationIssue},
    installed::{self, Scan},
    latency::PingSample,
    model::{FirewallTarget, Game, Preferences, Protocol, Snapshot},
    Result,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

pub struct Backend {
    pub engine: Arc<Engine>,
    pub locales: Catalogue,
    pub busy: Arc<AtomicBool>,
    pub installed: Arc<Mutex<Scan>>,
}

/// Explicit wire DTO: do not expose the engine's internal policy or locks to IPC.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    snapshot: Snapshot,
    self_update_supported: bool,
    samples: BTreeMap<String, PingSample>,
    histories: BTreeMap<String, Vec<f64>>,
    applied_targets: Option<Vec<FirewallTarget>>,
    applied_count: Option<usize>,
    applied_game_ids: Vec<String>,
    region_states: BTreeMap<String, RegionState>,
    inspected_at: Option<u64>,
    verification: Verification,
    verification_error: String,
    verification_issue: Option<VerificationIssue>,
    dirty: bool,
    message: String,
}

impl From<Runtime> for Status {
    fn from(state: Runtime) -> Self {
        Self {
            snapshot: state.snapshot,
            self_update_supported: self_update_supported(),
            samples: state.samples,
            histories: state.histories,
            applied_targets: state.applied_targets,
            applied_count: state.applied_count,
            applied_game_ids: state.applied_game_ids,
            region_states: state.region_states,
            inspected_at: state.inspected_at,
            verification: state.verification,
            verification_error: state.verification_error,
            verification_issue: state.verification_issue,
            dirty: state.dirty,
            message: state.message,
        }
    }
}

fn self_update_supported() -> bool {
    cfg!(target_os = "windows")
        || (cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some())
}

// The guard also clears busy on early errors and unwind. All blocking core work stays
// off the webview thread; concurrent IPC mutations are rejected rather than racing.
struct Operation(Arc<AtomicBool>);
impl Operation {
    fn acquire(flag: Arc<AtomicBool>) -> Result<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "Another operation is running. Please wait.".to_owned())?;
        Ok(Self(flag))
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

async fn work<T: Send + 'static>(
    state: tauri::State<'_, Backend>,
    action: impl FnOnce(&Engine) -> Result<T> + Send + 'static,
) -> Result<T> {
    let guard = Operation::acquire(Arc::clone(&state.busy))?;
    let engine = Arc::clone(&state.engine);
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        action(&engine)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_status(state: tauri::State<'_, Backend>) -> Status {
    state.engine.runtime().into()
}

#[tauri::command]
pub async fn verify_firewall(state: tauri::State<'_, Backend>, elevated: bool) -> Result<Status> {
    work(state, move |engine| {
        let result = engine.verify_firewall(elevated);
        // Non-elevated callers can inspect the failure in the returned status.
        // The UI allows authentication on launch and reports cancellation as an error.
        if elevated {
            result?;
        }
        Ok(engine.runtime().into())
    })
    .await
}

#[derive(Serialize)]
pub struct Languages {
    locales: Vec<Locale>,
    warnings: Vec<String>,
}

#[tauri::command]
pub fn get_languages(state: tauri::State<'_, Backend>) -> Languages {
    Languages {
        locales: state
            .locales
            .locales
            .iter()
            .map(|l| l.as_ref().clone())
            .collect(),
        warnings: state.locales.warnings.clone(),
    }
}

#[tauri::command]
pub async fn set_preferences(
    state: tauri::State<'_, Backend>,
    preferences: Preferences,
) -> Result<()> {
    work(state, move |engine| engine.preferences(preferences)).await
}

#[tauri::command]
pub async fn set_language(state: tauri::State<'_, Backend>, code: String) -> Result<()> {
    if !state.locales.locales.iter().any(|l| l.code == code) {
        return Err("Language is not available.".into());
    }
    work(state, move |engine| engine.set_language(&code)).await
}

#[tauri::command]
pub async fn save_game(state: tauri::State<'_, Backend>, game: Game) -> Result<()> {
    work(state, move |engine| engine.save_game(&game)).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewGame {
    pub candidate_id: Option<String>,
    pub name: String,
    pub area: String,
    pub address: String,
    pub protocol: Protocol,
    pub ports: Option<String>,
    pub executable_path: Option<String>,
}

fn create(engine: &Engine, input: NewGame) -> Result<String> {
    let id = input
        .candidate_id
        .unwrap_or_else(|| format!("custom-{}", uuid::Uuid::new_v4()));
    if engine
        .runtime()
        .snapshot
        .games
        .iter()
        .any(|game| game.id == id)
    {
        return Err(
            "A game with this ID is already configured. Add targets in Advanced instead.".into(),
        );
    }
    if let Some(path) = &input.executable_path {
        validate_executable(path)?;
    }
    let mut game = installed::custom_game(
        id.clone(),
        &input.name,
        &input.area,
        &input.address,
        input.protocol,
        input.ports,
    )?;
    game.executable_path = input.executable_path;
    engine.save_game(&game)?;
    let mut ids = engine.runtime().snapshot.preferences.library_game_ids;
    ids.push(id.clone());
    engine.set_library_games(ids, &[])?;
    Ok(id)
}

#[tauri::command]
pub async fn create_game(state: tauri::State<'_, Backend>, input: NewGame) -> Result<String> {
    work(state, move |engine| create(engine, input)).await
}

#[tauri::command]
pub async fn refresh_game(state: tauri::State<'_, Backend>, game_id: String) -> Result<()> {
    work(state, move |engine| engine.refresh_game(&game_id)).await
}

/// Silent background refresh of one game's official server feed.
///
/// Deliberately not routed through `work`: a slow or unreachable feed must not
/// raise the shared busy flag or reject a concurrent Apply. The engine still
/// serializes it internally, so it cannot interleave with other mutations.
#[tauri::command]
pub async fn ensure_fresh_data(state: tauri::State<'_, Backend>, game_id: String) -> Result<()> {
    let engine = Arc::clone(&state.engine);
    // Refresh failures are the user's network, not an operation error: report
    // success either way and let the manual button surface real problems.
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let _ = engine.ensure_fresh_data(&game_id);
    })
    .await;
    Ok(())
}

#[tauri::command]
pub async fn check_ping(
    state: tauri::State<'_, Backend>,
    game_id: String,
    region_id: Option<String>,
    area: Option<String>,
) -> Result<()> {
    work(state, move |engine| {
        if area.is_some() && region_id.is_some() {
            return Err("Choose either a server region or one server location.".into());
        }
        if let Some(area) = area {
            engine.measure_area_latency(&game_id, &area)
        } else {
            engine.measure_latency(&game_id, region_id.as_deref())
        }
    })
    .await
}

#[tauri::command]
pub async fn apply_rules(state: tauri::State<'_, Backend>) -> Result<()> {
    work(state, Engine::apply).await
}

#[tauri::command]
pub async fn reset_rules(state: tauri::State<'_, Backend>) -> Result<()> {
    work(state, Engine::reset).await
}

#[tauri::command]
pub async fn scan_games(state: tauri::State<'_, Backend>) -> Result<Scan> {
    let cache = Arc::clone(&state.installed);
    work(state, move |engine| cached_scan(engine, &cache)).await
}

fn cached_scan(engine: &Engine, cache: &Mutex<Scan>) -> Result<Scan> {
    let scan = scan_libraries(engine);
    if cfg!(target_os = "windows") {
        engine.detect_game_executables(&scan.games)?;
    }
    *cache.lock().map_err(|e| e.to_string())? = scan.clone();
    Ok(scan)
}

#[tauri::command]
pub async fn set_library_games(state: tauri::State<'_, Backend>, ids: Vec<String>) -> Result<()> {
    let cache = Arc::clone(&state.installed);
    work(state, move |engine| {
        let detected = cache.lock().map_err(|e| e.to_string())?;
        engine.set_library_games(ids, &detected.games)
    })
    .await
}

fn scan_libraries(engine: &Engine) -> Scan {
    let preferences = engine.runtime().snapshot.preferences;
    let roots = preferences
        .game_library_paths
        .iter()
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    let excluded = preferences
        .excluded_game_library_paths
        .iter()
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    installed::scan_with_settings(&roots, &excluded)
}

fn add_game_library(engine: &Engine, folder: &std::path::Path) -> Result<()> {
    let folder = if folder
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("steamapps"))
    {
        folder.parent().ok_or("Choose a game library folder.")?
    } else {
        folder
    };
    if !folder.is_absolute() || !folder.is_dir() {
        return Err("Choose an existing game library folder.".into());
    }
    let folder = std::fs::canonicalize(folder).map_err(|e| e.to_string())?;
    let path = folder
        .to_str()
        .ok_or("The library path is not valid Unicode.")?
        .to_owned();
    let preferences = engine.runtime().snapshot.preferences;
    let mut paths = preferences.game_library_paths;
    let mut excluded = preferences.excluded_game_library_paths;
    // Explicitly choosing a removed folder restores it (including ancestor exclusions).
    excluded.retain(|existing| {
        !folder.starts_with(installed::normalized_library_path(std::path::Path::new(
            existing,
        )))
    });
    if !paths
        .iter()
        .any(|existing| std::fs::canonicalize(existing).is_ok_and(|existing| existing == folder))
    {
        paths.push(path);
    }
    engine.set_game_library_settings(paths, excluded)?;
    Ok(())
}

fn remove_game_library_path(engine: &Engine, path: &str) -> Result<()> {
    let folder = installed::normalized_library_path(std::path::Path::new(path));
    if !folder.is_absolute() {
        return Err("Game library paths must be absolute local folders.".into());
    }
    let preferences = engine.runtime().snapshot.preferences;
    let mut paths = preferences.game_library_paths;
    paths.retain(|existing| {
        !installed::normalized_library_path(std::path::Path::new(existing)).starts_with(&folder)
    });
    let mut excluded = preferences.excluded_game_library_paths;
    if !excluded.iter().any(|existing| {
        installed::normalized_library_path(std::path::Path::new(existing)) == folder
    }) {
        excluded.push(
            folder
                .to_str()
                .ok_or("The library path is not valid Unicode.")?
                .to_owned(),
        );
    }
    engine.set_game_library_settings(paths, excluded)
}

#[tauri::command]
pub async fn select_game_library(state: tauri::State<'_, Backend>) -> Result<Option<Scan>> {
    let cache = Arc::clone(&state.installed);
    work(state, move |engine| {
        let Some(folder) = rfd::FileDialog::new().pick_folder() else {
            return Ok(None);
        };
        add_game_library(engine, &folder)?;
        Ok(Some(cached_scan(engine, &cache)?))
    })
    .await
}

#[tauri::command]
pub async fn remove_game_library(state: tauri::State<'_, Backend>, path: String) -> Result<Scan> {
    let cache = Arc::clone(&state.installed);
    work(state, move |engine| {
        remove_game_library_path(engine, &path)?;
        cached_scan(engine, &cache)
    })
    .await
}

fn validate_executable(path: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    lobbylocker_core::model::windows_program(path)?;
    let path = std::path::Path::new(path);
    if !path.is_absolute() || !path.is_file() {
        return Err("Choose an existing game executable file using Browse.".into());
    }
    Ok(())
}

fn save_game_executable(engine: &Engine, game_id: &str, path: &str) -> Result<()> {
    validate_executable(path)?;
    let mut game = engine
        .runtime()
        .snapshot
        .games
        .into_iter()
        .find(|game| game.id == game_id)
        .ok_or("Game not found.")?;
    game.executable_path = Some(path.to_owned());
    engine.save_game(&game)
}

/// Attach a program to an existing game without replacing its regions or targets.
#[tauri::command]
pub async fn set_game_executable(state: tauri::State<'_, Backend>, game_id: String) -> Result<()> {
    work(state, move |engine| {
        if !engine
            .runtime()
            .snapshot
            .games
            .iter()
            .any(|game| game.id == game_id)
        {
            return Err("Game not found.".into());
        }
        let dialog = rfd::FileDialog::new();
        #[cfg(target_os = "windows")]
        let dialog = dialog.add_filter("Game executable", &["exe"]);
        let Some(path) = dialog.pick_file() else {
            return Ok(());
        };
        let path = path
            .to_str()
            .ok_or("The executable path is not valid Unicode.")?;
        save_game_executable(engine, &game_id, path)
    })
    .await
}

#[derive(Serialize)]
pub struct ExecutableSelection {
    path: String,
    icon: Option<String>,
}

/// Read-only path selection and bounded icon-resource extraction. Never launches the game.
#[tauri::command]
pub async fn select_game_executable(
    state: tauri::State<'_, Backend>,
) -> Result<Option<ExecutableSelection>> {
    work(state, |_| {
        let dialog = rfd::FileDialog::new();
        #[cfg(target_os = "windows")]
        let dialog = dialog.add_filter("Game executable", &["exe"]);
        let Some(path) = dialog.pick_file() else {
            return Ok(None);
        };
        let path = path
            .to_str()
            .ok_or("The executable path is not valid Unicode.")?
            .to_owned();
        validate_executable(&path)?;
        let icon = lobbylocker_core::icons::executable_icon(std::path::Path::new(&path));
        Ok(Some(ExecutableSelection { path, icon }))
    })
    .await
}

#[tauri::command]
pub async fn get_custom_icons(
    state: tauri::State<'_, Backend>,
) -> Result<BTreeMap<String, String>> {
    work(state, |engine| {
        let mut icons = BTreeMap::new();
        for game in engine
            .runtime()
            .snapshot
            .games
            .iter()
            .filter(|game| game.executable_path.is_some())
            .take(100)
        {
            if let Some(path) = &game.executable_path {
                if let Some(icon) =
                    lobbylocker_core::icons::executable_icon(std::path::Path::new(path))
                {
                    icons.insert(game.id.clone(), icon);
                }
            }
        }
        Ok(icons)
    })
    .await
}

#[tauri::command]
pub async fn discover_connections(state: tauri::State<'_, Backend>) -> Result<Vec<Connection>> {
    work(state, |_| connections::scan()).await
}

#[tauri::command]
pub async fn import_game(state: tauri::State<'_, Backend>) -> Result<Option<String>> {
    work(state, |engine| {
        let Some(path) = rfd::FileDialog::new().add_filter("Game JSON", &["json"]).pick_file() else {
            return Ok(None);
        };
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 5_000_000 {
            return Err("Game JSON exceeds 5 MB.".into());
        }
        let game = Game::from_json(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        game.validate()?;
        if engine.runtime().snapshot.games.iter().any(|g| g.id == game.id)
            && rfd::MessageDialog::new()
                .set_title("LobbyLocker")
                .set_description("Replace the existing game file? Custom targets in that file will be replaced. Firewall rules are unchanged until Apply.")
                .set_buttons(rfd::MessageButtons::OkCancel)
                .show() != rfd::MessageDialogResult::Ok
        {
            return Ok(None);
        }
        engine.save_game(&game)?;
        let mut ids = engine.runtime().snapshot.preferences.library_game_ids;
        ids.push(game.id.clone());
        engine.set_library_games(ids, &[])?;
        Ok(Some(game.id))
    }).await
}

#[tauri::command]
pub async fn export_game(state: tauri::State<'_, Backend>, game_id: String) -> Result<()> {
    work(state, move |engine| {
        let game = engine
            .runtime()
            .snapshot
            .games
            .into_iter()
            .find(|g| g.id == game_id)
            .ok_or("Game not found.")?;
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Game JSON", &["json"])
            .set_file_name(format!("{}.json", game.id))
            .save_file()
        else {
            return Ok(());
        };
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&game).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    })
    .await
}

#[cfg(test)]
#[path = "../tests/unit/commands.rs"]
mod tests;
