//! App state: everything the Rune view reads, plus backend state owned by the
//! core engine. Fast operations run in the handler; slow ones (firewall,
//! ping, scan) run on [`Latest`] workers and land through [`poll`].

use lobbylocker_core::{
    banners,
    connections::{self, Connection},
    engine::{Engine, Runtime as EngineRuntime},
    i18n::Catalogue,
    installed::{self, Scan},
    model::{Game, Preferences, Region},
    store::Store,
    Result,
};
use rune::{ImageCache, Latest, Pixels, TextEdit, Waker};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Regions,
    Connections,
    Settings,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConnectionFilter {
    #[default]
    All,
    Tcp,
    Udp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FwKind {
    Apply,
    Reset,
    Verify,
}

pub struct FwDone {
    pub kind: FwKind,
    pub result: Result<()>,
}

pub struct ScanDone(pub Result<Scan>);
pub struct PingDone(pub Result<()>);
pub struct ConnDone(pub Result<Vec<Connection>>);
pub struct FreshDone;

/// Steam banner art, decoded off the UI thread: width, height, RGBA bytes.
pub struct BannerDone {
    pub game_id: String,
    pub pixels: Option<(u32, u32, Vec<u8>)>,
}

pub struct DataDirs {
    pub games: PathBuf,
    pub locales: PathBuf,
    pub config: PathBuf,
}

pub fn data_dirs() -> DataDirs {
    let games = std::env::var_os("LOBBYLOCKER_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| exe_data_dirs("games").into_iter().next())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/games"));
    let locales = std::env::var_os("LOBBYLOCKER_LOCALES_DIR")
        .map(PathBuf::from)
        .or_else(|| exe_data_dirs("locales").into_iter().next())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/locales"));
    let config = Store::default_directory().unwrap_or_else(|_| {
        dirs::config_dir()
            .map(|p| p.join("lobbylocker"))
            .unwrap_or_else(|| PathBuf::from(".lobbylocker"))
    });
    DataDirs {
        games,
        locales,
        config,
    }
}

/// Installed layouts, in preference order: `<exe>/data/<folder>` (portable /
/// AppImage), then `/usr/share` or `/usr/lib` siblings (native packages).
fn exe_data_dirs(folder: &str) -> Vec<PathBuf> {
    let Some(exe) = std::env::current_exe().ok() else {
        return Vec::new();
    };
    let Some(dir) = exe.parent() else {
        return Vec::new();
    };
    [
        dir.join("data").join(folder),
        dir.join("../share/lobbylocker/data").join(folder),
        dir.join("../lib/lobbylocker/data").join(folder),
    ]
    .into_iter()
    .filter(|p| p.is_dir())
    .collect()
}

pub struct AppState {
    pub engine: Arc<Engine>,
    pub locales: Catalogue,
    pub status: EngineRuntime,
    pub tab: Tab,
    pub selected: String,
    pub compact: bool,
    /// The compact layout was entered by narrowing the window, not by choice.
    pub compact_auto: bool,
    pub compact_game_open: bool,
    pub window_size: (f32, f32),
    pub expanded_size: (f32, f32),
    pub compact_size: (f32, f32),
    pub layout_transition: bool,
    pub layout_target: (f32, f32),
    pub layout_origin_width: f32,
    pub layout_observed: bool,
    pub initial_layout_resize: bool,
    pub layout_path: PathBuf,
    /// Left shell pane (games sidebar) state.
    pub sidebar_state: rune_kit::shell::PaneState,
    pub search: TextEdit,
    pub dark: bool,
    pub editing: bool,
    pub reset_open: bool,
    pub custom_open: bool,
    pub custom_exe: Option<String>,
    pub scan: Scan,
    pub scanned: bool,
    pub scan_busy: bool,
    pub connections: Vec<Connection>,
    pub connections_scanned: bool,
    pub connections_error: String,
    pub connections_search: TextEdit,
    pub connections_filter: ConnectionFilter,
    pub error: String,
    pub firewall_error: String,
    pub busy: Option<String>,
    /// Collapsible area headings, by `game:area`.
    pub collapsed_areas: Vec<String>,
    /// Edit-mode: region currently open for editing.
    pub editing_region: Option<String>,
    pub adding_area: Option<String>,
    pub fw_latest: Latest<FwDone>,
    pub scan_latest: Latest<ScanDone>,
    pub ping_latest: Latest<PingDone>,
    pub conn_latest: Latest<ConnDone>,
    pub fresh_latest: Latest<FreshDone>,
    pub banner_latest: Latest<BannerDone>,
    /// Target form inputs.
    pub target_addr: TextEdit,
    pub target_name: TextEdit,
    pub target_protocol: String,
    pub target_ports: TextEdit,
    /// Server-details inputs.
    pub probe_target: TextEdit,
    pub region_name: TextEdit,
    /// Header language picker.
    pub language_open: bool,
    /// Settings > General language picker, separate so both never open at once.
    pub settings_language_open: bool,
    pub scope_open: bool,
    /// A newer version to announce, when the app knows of one.
    pub update_notice: Option<UpdateNotice>,
    /// Switches saved before the pending apply, restored if it fails.
    pub pending_rollback: Option<Preferences>,
    /// Custom game form.
    pub custom_name_edit: TextEdit,
    pub custom_addr_edit: TextEdit,
    /// False disables firewall workers (tests, headless CI). Saved switches
    /// still persist; only elevation-driven apply/reset/verify are skipped.
    pub allow_firewall: bool,
    /// Decoded executable icons keyed by game id (empty in headless tests).
    pub custom_icons: BTreeMap<String, Pixels>,
    /// Steam banner art keyed by game id.
    pub banners: BTreeMap<String, Pixels>,
    /// SVG image cache for the brand mark; `None` until `setup` in tests.
    pub image_cache: Option<ImageCache>,
}

pub fn init() -> AppState {
    init_with(data_dirs())
}

pub fn init_with(dirs: DataDirs) -> AppState {
    let layout_path = dirs.config.join("window-layout");
    let compact =
        std::fs::read_to_string(&layout_path).is_ok_and(|value| value.trim() == "compact");
    let store = Store::open(dirs.config.clone(), &dirs.games)
        .unwrap_or_else(|e| panic!("Could not open LobbyLocker data: {e}"));
    let engine = Engine::new(store).unwrap_or_else(|e| panic!("Could not load games: {e}"));
    let locales =
        Catalogue::load(&dirs.locales).unwrap_or_else(|e| panic!("Could not load languages: {e}"));
    let status = engine.runtime();
    let selected = first_game(&status).unwrap_or_default();
    AppState {
        engine,
        locales,
        status,
        tab: Tab::Regions,
        selected,
        compact,
        // Starting from the saved layout: growing the window brings the
        // expanded layout back.
        // The window width alone decides the layout, so the automatic
        // switching is always on.
        compact_auto: true,
        compact_game_open: false,
        window_size: (EXPANDED_WIDTH, EXPANDED_HEIGHT),
        expanded_size: (EXPANDED_WIDTH, EXPANDED_HEIGHT),
        compact_size: (COMPACT_WIDTH, COMPACT_HEIGHT),
        layout_transition: compact,
        layout_target: (COMPACT_WIDTH, COMPACT_HEIGHT),
        layout_origin_width: EXPANDED_WIDTH,
        layout_observed: false,
        initial_layout_resize: compact,
        layout_path,
        sidebar_state: rune_kit::shell::PaneState::new(240.0),
        search: TextEdit::new(""),
        dark: true,
        editing: false,
        reset_open: false,
        custom_open: false,
        custom_exe: None,
        scan: Scan::default(),
        scanned: false,
        scan_busy: false,
        connections: Vec::new(),
        connections_scanned: false,
        connections_error: String::new(),
        connections_search: TextEdit::new(""),
        connections_filter: ConnectionFilter::All,
        error: String::new(),
        firewall_error: String::new(),
        busy: None,
        collapsed_areas: Vec::new(),
        editing_region: None,
        adding_area: None,
        fw_latest: Latest::default(),
        scan_latest: Latest::default(),
        ping_latest: Latest::default(),
        conn_latest: Latest::default(),
        fresh_latest: Latest::default(),
        banner_latest: Latest::default(),
        target_addr: TextEdit::new(""),
        target_name: TextEdit::new(""),
        target_protocol: "udp".to_string(),
        target_ports: TextEdit::new(""),
        probe_target: TextEdit::new(""),
        region_name: TextEdit::new(""),
        language_open: false,
        settings_language_open: false,
        scope_open: false,
        // The app has no updater, so a newer build is only ever announced by
        // an explicit override (or the preview scene).
        update_notice: std::env::var_os("LOBBYLOCKER_FAKE_UPDATE")
            .map(|version| preview_update(&version.to_string_lossy())),
        pending_rollback: None,
        custom_name_edit: TextEdit::new(""),
        custom_addr_edit: TextEdit::new(""),
        allow_firewall: std::env::var_os("LOBBYLOCKER_NO_FIREWALL").is_none(),
        custom_icons: BTreeMap::new(),
        banners: BTreeMap::new(),
        image_cache: None,
    }
}

fn first_game(status: &EngineRuntime) -> Option<String> {
    let games = library_games(status);
    games.first().map(|g| g.id.clone())
}

/// A release to announce: the version, and whether it is newer than the one
/// running.
#[derive(Clone)]
pub struct UpdateNotice {
    pub version: String,
    /// A newer version than the one running.
    pub available: bool,
}

/// A notice for a newer version, for the preview scene and tests.
pub fn preview_update(version: &str) -> UpdateNotice {
    UpdateNotice {
        version: version.to_string(),
        available: true,
    }
}

/// Put a known update away: the marker and its button go with it.
pub fn dismiss_update(s: &mut AppState) {
    s.update_notice = None;
}

/// Decode bounded executable icons (PNG/JPEG/ICO) into pixel tiles.
/// Scanned launcher icons fill gaps left by unset executable paths.
pub fn setup(s: &mut AppState, waker: Waker) {
    s.fw_latest.set_waker(waker.clone());
    s.scan_latest.set_waker(waker.clone());
    s.ping_latest.set_waker(waker.clone());
    s.conn_latest.set_waker(waker.clone());
    s.fresh_latest.set_waker(waker.clone());
    s.banner_latest.set_waker(waker.clone());
    s.image_cache = Some(rune::ImageCache::new(waker));
    refresh_icons(s);
    if rune::is_headless() {
        return;
    }
    // Read existing privileges first; a denied read surfaces with Retry.
    request_verify(s, true);
    request_scan(s);
}

/// The old Tauri IPC host ran this on its worker thread; the Rune app
/// calls the same core entry points in process.
fn run_scan(engine: &Engine) -> Result<Scan> {
    let preferences = engine.runtime().snapshot.preferences;
    let roots: Vec<PathBuf> = preferences
        .game_library_paths
        .iter()
        .map(PathBuf::from)
        .collect();
    let excluded: Vec<PathBuf> = preferences
        .excluded_game_library_paths
        .iter()
        .map(PathBuf::from)
        .collect();
    let scan = installed::scan_with_settings(&roots, &excluded);
    if cfg!(target_os = "windows") {
        engine.detect_game_executables(&scan.games)?;
    }
    Ok(scan)
}

pub fn poll(s: &mut AppState, _t: f64, tx: &mut rune::Tx) -> bool {
    let mut changed = false;
    if s.initial_layout_resize {
        s.initial_layout_resize = false;
        tx.resize_window_instant(s.compact_size);
        changed = true;
    }
    if let Some(done) = s.fw_latest.take() {
        changed = true;
        s.busy = None;
        let failed = done.result.is_err();
        match done.kind {
            FwKind::Verify => {
                if let Err(e) = done.result {
                    s.firewall_error = e;
                } else {
                    s.firewall_error.clear();
                }
            }
            FwKind::Apply => {
                if let Err(e) = done.result {
                    s.error = e.clone();
                    s.firewall_error = e;
                    rollback_if_failed(s);
                } else {
                    s.error.clear();
                    s.firewall_error.clear();
                    s.pending_rollback = None;
                }
            }
            FwKind::Reset => {
                if let Err(e) = done.result {
                    s.error = e.clone();
                    s.firewall_error = e;
                } else {
                    s.error.clear();
                    s.firewall_error.clear();
                }
            }
        }
        let _ = failed;
        s.status = s.engine.runtime();
    }
    if let Some(done) = s.scan_latest.take() {
        changed = true;
        s.scan_busy = false;
        if s.busy.as_deref() == Some("scan") {
            s.busy = None;
        }
        match done.0 {
            Ok(scan) => {
                s.scanned = true;
                s.scan = scan;
                s.status = s.engine.runtime();
                reconcile_library_selection(s);
            }
            Err(e) => s.error = e,
        }
    }
    if let Some(done) = s.ping_latest.take() {
        changed = true;
        if s.busy.as_deref() == Some("ping") {
            s.busy = None;
        }
        if let Err(e) = done.0 {
            s.error = e;
        }
        s.status = s.engine.runtime();
    }
    if let Some(done) = s.conn_latest.take() {
        changed = true;
        if s.busy.as_deref() == Some("connections") {
            s.busy = None;
        }
        match done.0 {
            Ok(conns) => {
                s.connections = conns;
                s.connections_scanned = true;
                s.connections_error.clear();
            }
            Err(e) => s.connections_error = e,
        }
    }
    if s.fresh_latest.take().is_some() {
        changed = true;
        s.status = s.engine.runtime();
    }
    if let Some(done) = s.banner_latest.take() {
        changed = true;
        if let Some((width, height, rgba)) = done.pixels {
            let pixels = Pixels::new();
            pixels.set(width, height, rgba);
            s.banners.insert(done.game_id, pixels);
        }
    }
    // Steam banner art for the game on screen. Fetched off the UI thread and
    // cached on disk, so switching back to a game costs nothing.
    if s.tab == Tab::Regions
        && !s.banners.contains_key(&s.selected)
        && banners::has_banner(&s.selected)
        && !rune::is_headless()
    {
        let game_id = s.selected.clone();
        s.banner_latest.request_if_changed(&game_id, load_banner);
    }
    if changed {
        refresh_icons(s);
        tx.instant();
    }
    changed
}

/// Banner bytes from the user's own machine: the art they installed, then
/// Steam's cached art. Decoded and scaled down to a texture the window can
/// hold.
fn load_banner(game_id: String) -> BannerDone {
    let mut fallback: Option<(u32, u32, Vec<u8>)> = None;
    for path in banners::banner_paths(&game_id) {
        let Ok(bytes) = banners::read_banner(&path) else {
            continue;
        };
        let Ok(decoded) = decode_banner(&bytes) else {
            continue;
        };
        let (width, height, _) = &decoded;
        // A banner is wide; a tall poster is only used when nothing else fits.
        if *width as f32 >= *height as f32 * 1.4 {
            return BannerDone {
                game_id,
                pixels: Some(decoded),
            };
        }
        if fallback.is_none() {
            fallback = Some(decoded);
        }
    }
    BannerDone {
        game_id,
        pixels: fallback,
    }
}

/// Decode banner bytes into raw RGBA, scaled down to a texture the window can
/// hold. Returns width, height, and the pixel bytes.
pub fn decode_banner(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    use image::{ImageReader, Limits};
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64_000_000);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| e.to_string())?;
    // The banner is drawn a few hundred points wide; a smaller texture is
    // enough and keeps the window's memory flat.
    let image = image.thumbnail(1600, 1600);
    let rgba = image.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    Ok((width, height, rgba.into_raw()))
}

// ---------- Library helpers (ported from lib/game.ts) ----------

pub const AREAS: [&str; 8] = [
    "Europe",
    "Middle East",
    "North America",
    "South America",
    "Asia",
    "Oceania",
    "Africa",
    "Custom",
];

pub fn areas_in_order(regions: &[Region]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for r in regions {
        if !seen.contains(&r.area) {
            seen.push(r.area.clone());
        }
    }
    seen.sort_by(|a, b| {
        let ai = AREAS.iter().position(|x| x == a).unwrap_or(AREAS.len());
        let bi = AREAS.iter().position(|x| x == b).unwrap_or(AREAS.len());
        ai.cmp(&bi).then_with(|| a.cmp(b))
    });
    seen
}

pub fn region_key(game_id: &str, region_id: &str) -> String {
    format!("{game_id}:{region_id}")
}

pub fn toggle_keys(values: &[String], keys: &[String], enabled: bool) -> Vec<String> {
    let remaining: Vec<String> = values
        .iter()
        .filter(|v| !keys.contains(v))
        .cloned()
        .collect();
    if !enabled {
        return remaining;
    }
    let mut out = remaining;
    for k in keys {
        if !out.contains(k) {
            out.push(k.clone());
        }
    }
    out
}

pub fn area_selection(game: &Game, area: &str, prefs: &Preferences) -> (Vec<String>, usize, bool) {
    let keys: Vec<String> = game
        .regions
        .iter()
        .filter(|r| r.area == area && !r.endpoints.is_empty())
        .map(|r| region_key(&game.id, &r.id))
        .collect();
    let selected = keys
        .iter()
        .filter(|k| prefs.blocked_regions.contains(k))
        .count();
    let checked = !keys.is_empty() && selected == keys.len();
    (keys, selected, checked)
}

pub fn region_checked(game: &Game, region: &Region, prefs: &Preferences) -> bool {
    let key = region_key(&game.id, &region.id);
    !region.endpoints.is_empty() && prefs.blocked_regions.contains(&key)
}

pub fn region_available(region: &Region) -> bool {
    !region.endpoints.is_empty()
}

/// Block badge per game (ported from gameBlockState).
pub fn game_block_state(game_id: &str, status: &EngineRuntime) -> Option<&'static str> {
    status.applied_count?;
    let mut has_blocked = false;
    let mut has_any = false;
    let mut all_none = true;
    for (key, state) in &status.region_states {
        if key.starts_with(&format!("{game_id}:")) {
            has_any = true;
            if matches!(state, lobbylocker_core::inspection::RegionState::Blocked)
                || matches!(state, lobbylocker_core::inspection::RegionState::Partial)
            {
                has_blocked = true;
            }
            if !matches!(state, lobbylocker_core::inspection::RegionState::None) {
                all_none = false;
            }
        }
    }
    if has_blocked {
        return Some("blocked");
    }
    if has_any && all_none {
        return Some("none");
    }
    if status.applied_count == Some(0) {
        return Some("none");
    }
    None
}

// ---------- Firewall helpers (ported from lib/firewall.ts) ----------

pub fn windows_blocking(status: &EngineRuntime) -> bool {
    status.snapshot.platform == "windows"
}

pub fn executable_blocking(status: &EngineRuntime) -> bool {
    use lobbylocker_core::model::FirewallScope;
    windows_blocking(status)
        && status.snapshot.preferences.windows_blocking_scope == FirewallScope::Executable
}

pub fn scope_warning(status: &EngineRuntime) -> &'static str {
    if executable_blocking(status) {
        "Applies only to each selected game executable on Windows. Rules remain active after closing LobbyLocker."
    } else {
        "Applies system-wide. Shared ranges can affect other apps; rules remain active after closing LobbyLocker."
    }
}

/// Saved targets that still need an executable (ported savedTargets +
/// missingPrograms).
pub fn missing_programs(status: &EngineRuntime) -> Vec<String> {
    if !executable_blocking(status) {
        return Vec::new();
    }
    let selected: std::collections::HashSet<String> = status
        .snapshot
        .preferences
        .blocked_regions
        .iter()
        .cloned()
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut missing = Vec::new();
    for game in &status.snapshot.games {
        let has_pick = game
            .regions
            .iter()
            .any(|r| !r.endpoints.is_empty() && selected.contains(&region_key(&game.id, &r.id)));
        if has_pick
            && game
                .executable_path
                .as_deref()
                .is_none_or(|p| p.trim().is_empty())
            && seen.insert(game.id.clone())
        {
            missing.push(game.name.clone());
        }
    }
    missing
}

pub fn verification_issue_retryable(issue: Option<&str>) -> bool {
    issue != Some("managedByDeviceManagement")
}

// ---------- Ping helpers (ported from lib/ping.ts) ----------

pub fn ping_description(sample: &lobbylocker_core::latency::PingSample) -> String {
    match sample.latency_ms {
        Some(ms) => format!("{} ms", ms.round()),
        None => "No reply".to_string(),
    }
}

// ---------- Action bar (ported from lib/action-bar.ts) ----------

pub struct ActionBar {
    pub visible: bool,
    pub apply: bool,
    pub count: usize,
}

pub fn action_bar(status: &EngineRuntime) -> ActionBar {
    let visible = status.dirty || (status.applied_count.unwrap_or(0) > 0);
    let count = if status.dirty {
        status.snapshot.preferences.blocked_regions.len()
    } else {
        status.applied_count.unwrap_or(0)
    };
    ActionBar {
        visible,
        apply: status.dirty,
        count,
    }
}

// ---------- Backend operations ----------

pub fn request_verify(s: &AppState, elevated: bool) {
    let engine = s.engine.clone();
    s.fw_latest.request((), move |()| FwDone {
        kind: FwKind::Verify,
        result: engine.verify_firewall(elevated),
    });
}

pub fn request_apply(s: &AppState) {
    let engine = s.engine.clone();
    s.fw_latest.request((), move |()| FwDone {
        kind: FwKind::Apply,
        result: engine.apply(),
    });
}

pub fn request_reset(s: &AppState) {
    let engine = s.engine.clone();
    s.fw_latest.request((), move |()| FwDone {
        kind: FwKind::Reset,
        result: engine.reset(),
    });
}

pub fn request_ping(
    s: &AppState,
    game_id: String,
    region_id: Option<String>,
    area: Option<String>,
) {
    let engine = s.engine.clone();
    s.ping_latest.request((), move |()| {
        PingDone(match (area, region_id) {
            (Some(area), _) => engine.measure_area_latency(&game_id, &area),
            (_, region) => engine.measure_latency(&game_id, region.as_deref()),
        })
    });
}

pub fn request_scan(s: &mut AppState) {
    s.scan_busy = true;
    let engine = s.engine.clone();
    s.scan_latest
        .request((), move |()| ScanDone(run_scan(&engine)));
}

pub fn request_connections(s: &AppState) {
    s.conn_latest
        .request((), move |()| ConnDone(connections::scan()));
}

pub fn request_fresh(s: &AppState, game_id: String) {
    let engine = s.engine.clone();
    s.fresh_latest.request((), move |()| {
        let _ = engine.ensure_fresh_data(&game_id);
        FreshDone
    });
}

pub fn reload(s: &mut AppState) {
    s.status = s.engine.runtime();
}

/// Toggle/save preferences synchronously (file I/O only, no elevation).
/// Returns true when the block set changed.
pub fn save_preferences(s: &mut AppState, prefs: Preferences) -> bool {
    let before = s.status.snapshot.preferences.blocked_regions.clone();
    match s.engine.preferences(prefs) {
        Ok(()) => {
            s.error.clear();
            reload(s);
            s.status.snapshot.preferences.blocked_regions != before
        }
        Err(e) => {
            s.error = e;
            false
        }
    }
}

/// Auto-apply after a toggle, rolling back the saved switches when the
/// firewall write is rejected — same contract as the old frontend.
/// `rollback` holds the switches from before the toggle.
pub fn write_blocks(s: &mut AppState, rollback: Option<Preferences>) {
    if !s.allow_firewall || s.busy.is_some() || !s.status.dirty {
        return;
    }
    if !missing_programs(&s.status).is_empty() {
        return;
    }
    s.busy = Some("apply".to_string());
    s.pending_rollback = rollback;
    request_apply(s);
}

fn rollback_if_failed(s: &mut AppState) {
    if let Some(prefs) = s.pending_rollback.take() {
        if let Err(e) = s.engine.preferences(prefs) {
            s.error = e;
        }
        reload(s);
    }
}

// Re-exported so existing `crate::state::*` imports keep working after the
// layout code moved to its own module.
pub use crate::layout::{
    observe_window_layout, set_compact, COMPACT_HEIGHT, COMPACT_WIDTH, COMPACT_WIDTH_THRESHOLD,
    EXPANDED_HEIGHT, EXPANDED_WIDTH,
};

// Re-exported so existing `crate::state::*` imports keep working after the
// library code moved to its own module.
pub use crate::library::*;

// Re-exported so existing `crate::state::*` imports keep working.
pub use crate::icon_decode::refresh_icons;
