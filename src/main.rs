//! LobbyLocker native app (Rune). Pure Rust: the core engine owns all logic;
//! this binary is only the Rune view over it.
//!
//! `cargo run -p lobbylocker-app` opens the window;
//! `-- --snap lobbylocker.png --scene cs2` renders it headless.

use lobbylocker_app::state::{self, AppState};
use lobbylocker_app::view;

fn scene_cs2(s: &mut AppState) {
    s.compact = false;
    s.initial_layout_resize = false;
    s.layout_transition = false;
    // Keep user-added games out of previews without touching saved config.
    s.status
        .snapshot
        .games
        .retain(|g| state::BUILTIN_GAMES.contains(&g.id.as_str()));
    s.scan.games = vec![lobbylocker_core::installed::InstalledGame {
        id: "cs2".into(),
        name: "Counter-Strike 2".into(),
        launcher: "Steam".into(),
        directory: std::path::PathBuf::from("/preview/cs2"),
    }];
    s.scanned = true;
    s.selected = "cs2".to_string();
    s.tab = state::Tab::Regions;
}

fn scene_blocked(s: &mut AppState) {
    scene_cs2(s);
    let game = match s.status.snapshot.games.iter().find(|g| g.id == "cs2") {
        Some(g) => g.clone(),
        None => return,
    };
    let mut prefs = s.status.snapshot.preferences.clone();
    for region in &game.regions {
        if !region.endpoints.is_empty() {
            prefs
                .blocked_regions
                .push(state::region_key(&game.id, &region.id));
        }
    }
    s.status.snapshot.preferences = prefs;
}

fn scene_settings(s: &mut AppState) {
    scene_cs2(s);
    s.tab = state::Tab::Settings;
}

fn scene_connections(s: &mut AppState) {
    scene_cs2(s);
    s.tab = state::Tab::Connections;
    s.connections_scanned = true;
    s.connections = [
        (
            "tcp",
            "203.0.113.12",
            27015,
            "users:((\"game\",pid=123,fd=4))",
            "ESTAB",
        ),
        (
            "udp",
            "198.51.100.8",
            443,
            "users:((\"Discord\",pid=42,fd=9))",
            "ESTAB",
        ),
        ("tcp", "2001:db8::5", 443, "browser", "ESTABLISHED"),
        ("tcp", "192.0.2.20", 443, "", "ESTAB"),
    ]
    .into_iter()
    .map(|(protocol, remote_ip, remote_port, process, state)| {
        lobbylocker_core::connections::Connection {
            protocol: protocol.into(),
            remote_ip: remote_ip.into(),
            remote_port,
            process: process.into(),
            state: state.into(),
        }
    })
    .collect();
}

fn scene_empty(s: &mut AppState) {
    s.compact = false;
    s.initial_layout_resize = false;
    s.layout_transition = false;
    s.status.snapshot.games.clear();
    s.scan.games.clear();
    s.selected.clear();
    s.scanned = true;
}

fn scene_reset(s: &mut AppState) {
    scene_cs2(s);
    s.reset_open = true;
}

fn scene_edit(s: &mut AppState) {
    scene_cs2(s);
    s.editing = true;
}

fn scene_compact(s: &mut AppState) {
    scene_cs2(s);
    s.compact = true;
}

fn scene_update(s: &mut AppState) {
    scene_cs2(s);
    s.update_notice = Some(state::preview_update("0.0.7"));
}

fn scene_edit_add(s: &mut AppState) {
    scene_edit(s);
    lobbylocker_app::forms::open_target_dialog(s, "Europe");
}

fn scene_edit_details(s: &mut AppState) {
    scene_edit(s);
    if let Some(region) = s
        .status
        .snapshot
        .games
        .iter()
        .find(|g| g.id == "cs2")
        .and_then(|g| g.regions.first())
    {
        s.editing_region = Some(format!("cs2:{}", region.id));
        s.region_name = rune::TextEdit::new(region.name.clone());
        s.probe_target = rune::TextEdit::new(region.probe_target.clone().unwrap_or_default());
    }
}

fn main() {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if let Some(code) = lobbylocker_core::firewall::helper_entry() {
        std::process::exit(code);
    }
    rune::set_app_id("lobbylocker");
    rune::run(rune::AppSpec {
        scenes: &[
            ("cs2", scene_cs2),
            ("blocked", scene_blocked),
            ("settings", scene_settings),
            ("connections", scene_connections),
            ("empty", scene_empty),
            ("reset", scene_reset),
            ("edit", scene_edit),
            ("edit-add", scene_edit_add),
            ("edit-details", scene_edit_details),
            ("compact", scene_compact),
            ("update", scene_update),
        ],
        setup: Some(state::setup),
        poll: Some(state::poll),
        min_size: Some((360.0, 560.0)),
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        ..rune::AppSpec::new("LobbyLocker", (1240.0, 860.0), state::init, view::view)
    });
}
