//! Headless UI checks: the Rune view renders against real bundled game data,
//! tabs switch pages, and a block toggle persists through the core engine.
//! No display, no elevation, no firewall mutation.

use lobbylocker_app::state::{init_with, DataDirs, Tab};
use lobbylocker_app::view;
use rune::{Env, Runtime};
use std::path::PathBuf;
use std::time::Duration;

fn installed_game(id: &str, name: &str) -> lobbylocker_core::installed::InstalledGame {
    lobbylocker_core::installed::InstalledGame {
        id: id.into(),
        name: name.into(),
        launcher: "Steam".into(),
        directory: PathBuf::from("/preview/game"),
    }
}

fn test_runtime() -> (
    Runtime<lobbylocker_app::state::AppState>,
    tempfile::TempDir,
    PathBuf,
) {
    let config = tempfile::tempdir().unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dirs = DataDirs {
        games: root.join("data/games"),
        locales: root.join("data/locales"),
        config: config.path().to_path_buf(),
    };
    let state = init_with(dirs);
    let rt = Runtime::new(state, view::view, (1240.0, 860.0), Env::new(true, 1.0));
    (rt, config, root)
}

/// Runtime whose state already has cs2 in the library (state set before the
/// first frame, so the initial build mounts it).
fn cs2_runtime() -> (Runtime<lobbylocker_app::state::AppState>, tempfile::TempDir) {
    cs2_runtime_at((1240.0, 860.0))
}

fn cs2_runtime_at(
    size: (f32, f32),
) -> (Runtime<lobbylocker_app::state::AppState>, tempfile::TempDir) {
    let config = tempfile::tempdir().unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dirs = DataDirs {
        games: root.join("data/games"),
        locales: root.join("data/locales"),
        config: config.path().to_path_buf(),
    };
    let mut state = init_with(dirs);
    state
        .engine
        .set_library_games(vec!["cs2".to_string()], &[])
        .unwrap();
    state.status = state.engine.runtime();
    state.selected = "cs2".to_string();
    state.scan.games = vec![installed_game("cs2", "Counter-Strike 2")];
    state.scanned = true;
    state.allow_firewall = false;
    (
        Runtime::new(state, view::view, size, Env::new(true, 1.0)),
        config,
    )
}

#[test]
fn regions_page_shows_the_steam_banner() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    let pixels = rune::Pixels::new();
    pixels.set(4, 2, vec![180u8; 4 * 2 * 4]);
    rt.state.banners.insert("cs2".to_string(), pixels);
    rt.invalidate();
    for _ in 0..40 {
        rt.frame(1.0 / 60.0);
    }
    let (hero, _) = rt.presented("game-hero").expect("hero");
    let (banner, _) = rt.presented("game-banner").expect("banner");
    let (title, _) = rt.presented("game-name").expect("game name");
    assert!(
        hero[2] > 300.0 && hero[3] > 100.0,
        "the banner fills the game header, got {hero:?}"
    );
    assert!(
        banner[1] >= hero[1] - 1.0 && banner[1] + banner[3] <= hero[1] + hero[3] + 1.0,
        "the art is the backdrop of the hero, hero {hero:?} banner {banner:?}"
    );
    assert!(
        title[1] >= hero[1] && title[1] + title[3] <= hero[1] + hero[3] + 1.0,
        "the game name sits on the art, hero {hero:?} title {title:?}"
    );
}

#[test]
fn compact_picker_puts_the_game_banner_behind_its_name() {
    let (mut rt, _config) = cs2_runtime_at((420.0, 800.0));
    rt.state.compact = true;
    rt.frame(0.0);
    assert!(
        rt.find("compact-banner-backdrop").is_none(),
        "no picked game art yet"
    );

    let pixels = rune::Pixels::new();
    pixels.set(4, 2, vec![180u8; 4 * 2 * 4]);
    rt.state.banners.insert("cs2".to_string(), pixels);
    rt.invalidate();
    for _ in 0..40 {
        rt.frame(1.0 / 60.0);
    }

    let (backdrop, _) = rt
        .presented("compact-banner-backdrop")
        .expect("banner backdrop");
    let (trigger, _) = rt.presented("compact-game-trigger").expect("game trigger");
    assert!(
        backdrop[0] >= trigger[0] - 1.0
            && backdrop[1] >= trigger[1] - 1.0
            && backdrop[0] + backdrop[2] <= trigger[0] + trigger[2] + 1.0
            && backdrop[1] + backdrop[3] <= trigger[1] + trigger[3] + 1.0,
        "banner backdrop {backdrop:?} stays inside the trigger {trigger:?}"
    );
}

#[test]
fn regions_page_renders_bundled_games() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    assert!(rt.find("tab-regions").is_some(), "regions tab is mounted");
    assert!(
        rt.find("tab-connections").is_some(),
        "connections tab is mounted"
    );
    assert!(rt.find("tab-settings").is_some(), "settings tab is mounted");
    let games = lobbylocker_app::state::library_games(&rt.state.status);
    assert!(games.iter().any(|game| game.id == "cs2"));
    assert!(rt.find("game-cs2").is_some(), "sidebar lists cs2");
    assert!(
        rt.find("server-ams").is_some(),
        "cs2 server cards are mounted"
    );
}

#[test]
fn switching_tabs_changes_pages() {
    let (mut rt, _config, _root) = test_runtime();
    rt.frame(0.0);
    let (x, y) = rt.find("tab-settings").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(rt.state.tab, Tab::Settings);
    assert!(rt.find("settings-page").is_some(), "settings page is shown");
    let (x, y) = rt.find("tab-connections").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(rt.state.tab, Tab::Connections);
    assert!(
        rt.find("connections-page").is_some(),
        "connections page is shown"
    );
}

#[test]
fn the_sidebar_footer_marks_a_waiting_update_and_always_offers_a_way_out() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    for _ in 0..40 {
        rt.frame(1.0 / 60.0);
    }
    // No update: no dot, no update button.
    assert!(rt.find("update-dot").is_none(), "no dot without an update");
    assert!(
        rt.find("update-button").is_none(),
        "no update button without an update"
    );
    // The way out is always there.
    let (quit, _) = rt.presented("quit-button").expect("quit button");
    assert!(quit[2] > 0.0, "the quit button is drawn");
    let (version, _) = rt.presented("sidebar-version").expect("version");
    assert!(
        quit[1] > version[1],
        "the quit button sits under the version"
    );
    // A newer version marks itself.
    rt.state.update_notice = Some(lobbylocker_app::state::preview_update("1.0.0"));
    rt.invalidate();
    for _ in 0..40 {
        rt.frame(1.0 / 60.0);
    }
    assert!(rt.find("update-dot").is_some(), "the dot marks the update");
    assert!(
        rt.find("update-button").is_some(),
        "the update button appears"
    );
    // The footer opens a confirmation dialog and retains the notice on close.
    click_ui(&mut rt, "update-button");
    assert!(rt.find("software-update-dialog").is_some());
    assert!(rt.find("install-update").is_some());
    click_ui(&mut rt, "close-update");
    assert!(rt.find("software-update-dialog").is_none());
    assert!(rt.find("update-dot").is_some());
    assert!(rt.find("quit-button").is_some(), "the quit button stays");
}
#[test]
fn growing_a_narrow_window_returns_to_the_expanded_layout() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    rt.set_viewport((600.0, 650.0), 1.0);
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    assert!(rt.state.compact, "a narrow window uses the compact layout");
    assert!(
        rt.state.compact_auto,
        "the switch was the window's own doing"
    );
    rt.set_viewport((1000.0, 700.0), 1.0);
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    assert!(
        !rt.state.compact,
        "a window that grows back uses the expanded layout"
    );
    assert!(rt.find("sidebar").is_some(), "the sidebar is back");
    assert!(rt.find("tab-settings").is_some(), "the tabs are back");
}

#[test]
fn the_window_width_decides_the_layout_without_changing_blocks() {
    let (mut rt, config) = cs2_runtime();
    let saved_blocks = rt.state.status.snapshot.preferences.blocked_regions.clone();
    rt.frame(0.0);
    rt.set_viewport((1000.0, 700.0), 1.0);
    rt.frame(0.0);
    rt.state.tab = Tab::Settings;
    rt.state.editing = true;
    rt.invalidate();
    rt.frame(0.0);
    // Narrowing the window switches to the compact layout.
    rt.set_viewport((420.0, 800.0), 1.0);
    for _ in 0..12 {
        rt.frame(1.0 / 60.0);
    }
    assert!(rt.state.compact);
    assert_eq!(rt.state.tab, Tab::Regions);
    assert!(!rt.state.editing);
    assert_eq!(rt.state.expanded_size, (1000.0, 700.0));
    assert_eq!(
        std::fs::read_to_string(config.path().join("window-layout")).unwrap(),
        "compact"
    );

    assert!(rt.find("compact-game-trigger").is_some());
    assert!(rt.find("compact-reset").is_some());
    assert!(rt.find("compact-ping").is_some());
    assert!(rt.find("sidebar").is_none());
    assert!(rt.find("tabs").is_none());
    let (first, _) = rt.presented("server-ams").expect("server card");
    let (second, _) = rt.presented("server-lhr").expect("second server card");
    assert!(
        second[1] >= first[1] + first[3],
        "compact cards use one column"
    );
    assert_eq!(
        rt.state.status.snapshot.preferences.blocked_regions,
        saved_blocks
    );

    // Widening it again returns to the expanded layout.
    rt.set_viewport((1000.0, 700.0), 1.0);
    for _ in 0..12 {
        rt.frame(1.0 / 60.0);
    }
    assert!(!rt.state.compact);
    assert_eq!(
        std::fs::read_to_string(config.path().join("window-layout")).unwrap(),
        "expanded"
    );
    assert!(rt.find("sidebar").is_some());
    assert!(rt.find("tab-settings").is_some());
    assert_eq!(
        rt.state.status.snapshot.preferences.blocked_regions,
        saved_blocks
    );
}

#[test]
fn compact_mode_restores_on_start_and_manual_narrowing() {
    let config = tempfile::tempdir().unwrap();
    std::fs::write(config.path().join("window-layout"), "compact").unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let state = init_with(DataDirs {
        games: root.join("data/games"),
        locales: root.join("data/locales"),
        config: config.path().to_path_buf(),
    });
    assert!(state.compact && state.initial_layout_resize);
    let mut rt = Runtime::new(state, view::view, (1240.0, 860.0), Env::new(true, 1.0));
    rt.update(|state, tx| {
        lobbylocker_app::state::poll(state, 0.0, tx);
    });
    assert!(!rt.state.initial_layout_resize);
    rt.set_viewport((420.0, 800.0), 1.0);
    rt.frame(0.0);
    assert!(rt.find("compact-picker").is_some());

    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    rt.set_viewport((600.0, 650.0), 1.0);
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    assert!(
        rt.state.compact,
        "manually narrowing an expanded window switches layouts"
    );
    assert_eq!(rt.state.compact_size, (600.0, 650.0));
    assert!(rt.find("compact-picker").is_some());
}

#[test]
fn the_compact_layout_keeps_the_update_marker_and_the_way_out() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    rt.set_viewport((600.0, 650.0), 1.0);
    for _ in 0..40 {
        rt.frame(1.0 / 60.0);
    }
    assert!(
        rt.state.compact,
        "the narrow window uses the compact layout"
    );
    // The sidebar is gone, so the compact footer carries these.
    assert!(rt.find("sidebar").is_none(), "the sidebar stays hidden");
    assert!(
        rt.find("quit-button").is_some(),
        "the compact layout still offers a way out"
    );
    assert!(rt.find("update-dot").is_none(), "no dot without an update");
    rt.state.update_notice = Some(lobbylocker_app::state::preview_update("1.0.0"));
    rt.invalidate();
    for _ in 0..40 {
        rt.frame(1.0 / 60.0);
    }
    assert!(rt.find("update-dot").is_some(), "the dot marks the update");
    assert!(
        rt.find("update-button").is_some(),
        "the update button appears"
    );
    click_ui(&mut rt, "update-button");
    assert!(rt.find("software-update-dialog").is_some());
    click_ui(&mut rt, "close-update");
    assert!(rt.find("software-update-dialog").is_none());
    assert!(rt.find("update-dot").is_some());
    assert!(rt.find("quit-button").is_some(), "the way out stays");
}

#[test]
fn compact_game_picker_switches_games_and_reset_still_requires_confirmation() {
    let (mut rt, _config) = cs2_runtime_at((420.0, 800.0));
    let custom =
        lobbylocker_core::installed::empty_game("custom-example".into(), "Example game").unwrap();
    rt.state.engine.save_game(&custom).unwrap();
    rt.state.status = rt.state.engine.runtime();
    rt.state.compact = true;
    let pixels = rune::Pixels::new();
    pixels.set(4, 4, vec![255; 4 * 4 * 4]);
    rt.state.custom_icons.insert("cs2".into(), pixels);
    rt.frame(0.0);
    assert!(
        rt.state.busy.is_none(),
        "opening compact mode never starts ping"
    );
    assert!(rt.find("compact-selected-icon-cs2").is_some());
    click_ui(&mut rt, "compact-game-trigger");
    assert!(rt.find("compact-game-content").is_some());
    assert!(rt.find("compact-option-icon-cs2").is_some());
    assert!(rt.find("compact-option-icon-custom-example").is_some());
    click_ui(&mut rt, "compact-game-option-custom-example");
    assert_eq!(rt.state.selected, "custom-example");
    assert!(!rt.state.compact_game_open);
    assert!(rt.find("compact-selected-icon-custom-example").is_some());
    assert!(rt.state.compact);
    click_ui(&mut rt, "compact-reset");
    assert!(rt.state.reset_open);
    assert!(
        rt.state.busy.is_none(),
        "opening reset does not change the firewall"
    );
}

#[test]
fn compact_game_picker_fits_scrolls_and_dismisses_at_minimum_size() {
    let (mut rt, _config) = cs2_runtime_at((360.0, 560.0));
    rt.state.compact = true;
    for index in 0..12 {
        rt.state.scan.games.push(installed_game(
            &format!("test-game-{index}"),
            &format!("Example game {index:02}"),
        ));
    }
    rt.frame(0.0);
    click_ui(&mut rt, "compact-game-trigger");
    let (popup, _) = rt.presented("compact-game-content").expect("game menu");
    assert!(popup[0] >= 0.0 && popup[0] + popup[2] <= 361.0);
    assert!(popup[1] >= 0.0 && popup[1] + popup[3] <= 561.0);
    let (before, _) = rt.presented("compact-game-option-cs2").unwrap();
    rt.wheel(popup[0] + 100.0, popup[1] + 100.0, -350.0);
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    let (after, _) = rt.presented("compact-game-option-cs2").unwrap();
    assert!(after[1] < before[1], "the game menu scrolls");
    rt.key(rune::KeyEvent {
        key: rune::Key::Escape,
        mods: Default::default(),
    });
    rt.frame(0.0);
    assert!(!rt.state.compact_game_open);
    assert_eq!(rt.state.selected, "cs2");
}

#[test]
fn compact_game_menu_tracks_the_picker_width_while_resizing() {
    for (language, scale) in [("en", 1.0), ("ar", 1.25)] {
        let (mut rt, _config) = cs2_runtime_at((360.0, 560.0));
        rt.state.compact = true;
        rt.state.status.snapshot.preferences.language = language.into();
        rt.frame(0.0);
        click_ui(&mut rt, "compact-game-trigger");

        for width in [360.0, 500.0, 640.0, 420.0, 360.0] {
            rt.set_viewport((width, 560.0), scale);
            rt.frame(0.0);
            let (trigger, _) = rt.presented("compact-game-trigger").unwrap();
            let (menu, _) = rt.presented("compact-game-content").unwrap();
            assert!(
                (menu[2] - trigger[2]).abs() < 1.0,
                "{language} at {width}: menu {menu:?}, trigger {trigger:?}"
            );
            assert!(
                (menu[0] - trigger[0]).abs() < 1.0,
                "{language} at {width}: menu {menu:?}, trigger {trigger:?}"
            );
            assert!(menu[0] >= 0.0 && menu[0] + menu[2] <= width + 1.0);
        }
    }
}

#[test]
fn compact_game_picker_uses_game_ids_and_supports_keyboard_selection() {
    let (mut rt, _config) = cs2_runtime_at((420.0, 800.0));
    rt.state.compact = true;
    let first = lobbylocker_core::installed::empty_game("same-one".into(), "Same name").unwrap();
    let second = lobbylocker_core::installed::empty_game("same-two".into(), "Same name").unwrap();
    rt.state.engine.save_game(&first).unwrap();
    rt.state.engine.save_game(&second).unwrap();
    rt.state.status = rt.state.engine.runtime();
    rt.frame(0.0);
    click_ui(&mut rt, "compact-game-trigger");
    for _ in 0..3 {
        rt.key(rune::KeyEvent {
            key: rune::Key::Down,
            mods: Default::default(),
        });
        rt.frame(0.0);
    }
    rt.key(rune::KeyEvent {
        key: rune::Key::Enter,
        mods: Default::default(),
    });
    rt.frame(0.0);
    assert_eq!(rt.state.selected, "same-two");
    assert!(!rt.state.compact_game_open);
}

#[test]
fn compact_server_switch_persists_when_the_app_expands() {
    let (mut rt, _config) = cs2_runtime_at((420.0, 800.0));
    rt.state.compact = true;
    rt.frame(0.0);
    let control = rt
        .elements()
        .into_iter()
        .find(|element| {
            element.role == "switch" && element.label.as_deref() == Some("Block Amsterdam")
        })
        .expect("Amsterdam switch");
    let x = control.rect[0] + control.rect[2] / 2.0;
    let y = control.rect[1] + control.rect[3] / 2.0;
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    assert!(rt
        .state
        .status
        .snapshot
        .preferences
        .blocked_regions
        .contains(&"cs2:ams".into()));
    rt.set_viewport((1240.0, 860.0), 1.0);
    for _ in 0..12 {
        rt.frame(1.0 / 60.0);
    }
    assert!(!rt.state.compact);
    assert!(rt
        .state
        .status
        .snapshot
        .preferences
        .blocked_regions
        .contains(&"cs2:ams".into()));
}

#[test]
fn a_compact_window_follows_the_width_it_is_given() {
    let (mut rt, _config) = cs2_runtime_at((420.0, 800.0));
    rt.state.compact = true;
    rt.frame(0.0);
    rt.set_viewport((800.0, 700.0), 1.0);
    for _ in 0..12 {
        rt.frame(1.0 / 60.0);
    }
    // Wide again: the expanded layout takes over.
    assert!(!rt.state.compact);
    assert_eq!(rt.state.expanded_size, (800.0, 700.0));
    rt.set_viewport((1240.0, 860.0), 1.0);
    for _ in 0..12 {
        rt.frame(1.0 / 60.0);
    }
    assert!(!rt.state.compact);
    // Narrow again: back to the compact layout.
    rt.set_viewport((600.0, 700.0), 1.0);
    for _ in 0..12 {
        rt.frame(1.0 / 60.0);
    }
    assert!(rt.state.compact);
    assert!(!rt.state.layout_transition);
    assert_eq!(rt.state.compact_size, (600.0, 700.0));
}

fn connection(
    protocol: &str,
    ip: &str,
    port: u16,
    process: &str,
) -> lobbylocker_core::connections::Connection {
    lobbylocker_core::connections::Connection {
        protocol: protocol.into(),
        remote_ip: ip.into(),
        remote_port: port,
        process: process.into(),
        state: "ESTAB".into(),
    }
}

#[test]
fn connections_show_scan_states_and_filter_the_snapshot() {
    let (mut rt, _config) = cs2_runtime();
    rt.state.tab = Tab::Connections;
    rt.frame(0.0);
    assert!(rt.state.busy.is_none(), "opening the page does not scan");
    assert!(rt.find("connections-empty").is_some());
    assert!(rt
        .elements()
        .iter()
        .any(|e| e.text.as_deref() == Some("No scan yet")));

    rt.state.connections_scanned = true;
    rt.state.connections = vec![
        connection(
            "tcp",
            "203.0.113.1",
            27015,
            "users:((\"game\",pid=42,fd=1))",
        ),
        connection("udp", "2001:db8::5", 443, "browser"),
    ];
    rt.invalidate();
    rt.frame(0.0);
    assert!(rt
        .elements()
        .iter()
        .any(|e| e.text.as_deref() == Some("2 of 2 connections")));
    assert!(rt
        .elements()
        .iter()
        .any(|e| e.text.as_deref() == Some("game")));
    assert!(rt
        .elements()
        .iter()
        .any(|e| e.text.as_deref() == Some("[2001:db8::5]:443")));

    let (x, y) = rt.find("connections-filter-udp").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
    assert!(rt.find("connection-0").is_none());
    assert!(rt.find("connection-1").is_some());
    assert!(rt
        .elements()
        .iter()
        .any(|e| e.text.as_deref() == Some("1 of 2 connections")));

    rt.state.connections_search.text = "missing".into();
    rt.invalidate();
    rt.frame(0.0);
    assert!(rt.find("connections-empty").is_some());
    assert!(rt
        .elements()
        .iter()
        .any(|e| e.text.as_deref() == Some("No matching connections")));

    rt.state.connections_search.text.clear();
    rt.state.connections_error = "test failure".into();
    rt.invalidate();
    rt.frame(0.0);
    assert!(rt.find("connections-error").is_some());
    assert!(
        rt.find("connection-1").is_some(),
        "a failed refresh keeps the previous snapshot"
    );

    rt.state.connections_error.clear();
    rt.state.connections.clear();
    rt.invalidate();
    rt.frame(0.0);
    assert!(rt
        .elements()
        .iter()
        .any(|e| e.text.as_deref() == Some("No remote connections found")));
}

#[test]
fn connections_fit_and_scroll_at_minimum_window_sizes() {
    for (size, compact) in [((720.0, 560.0), false), ((360.0, 560.0), true)] {
        let (mut rt, _config) = cs2_runtime_at(size);
        if compact {
            rt.state.sidebar_state = rt.state.sidebar_state.collapsed();
            rt.state.status.snapshot.preferences.language = "ar".into();
        }
        rt.state.tab = Tab::Connections;
        rt.state.connections_scanned = true;
        rt.state.connections = (0..40)
            .map(|i| connection("tcp", "203.0.113.1", 1000 + i, "game"))
            .collect();
        rt.frame(0.0);
        let (card, _) = rt.presented("connections-card").expect("results card");
        let (scan, _) = rt.presented("connections-scan").expect("scan button");
        let (search, _) = rt.presented("connections-search").expect("search field");
        let (first, _) = rt.presented("connection-0").expect("first result");
        assert!(scan[0] >= 0.0 && scan[0] + scan[2] <= size.0 + 1.0);
        assert!(search[0] >= card[0] - 1.0 && search[0] + search[2] <= card[0] + card[2] + 1.0);
        assert!(first[0] >= card[0] && first[0] + first[2] <= card[0] + card[2] + 1.0);
        rt.wheel(card[0] + card[2] / 2.0, card[1] + 100.0, -400.0);
        for _ in 0..6 {
            rt.frame(1.0 / 60.0);
        }
        let (after, _) = rt
            .presented("connection-0")
            .expect("first result remains mounted");
        assert!(after[1] < first[1] - 10.0, "results scroll within the card");
    }
}

#[test]
fn toggling_a_region_updates_saved_selection() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    let game = rt
        .state
        .status
        .snapshot
        .games
        .iter()
        .find(|g| g.id == "cs2")
        .cloned()
        .expect("cs2 bundled");
    let first = game
        .regions
        .iter()
        .find(|r| !r.endpoints.is_empty())
        .unwrap();
    assert!(!first.endpoints.is_empty());
    let key = format!("server-{}", first.id);
    let (x, y) = rt.find(&key).expect("a server card is mounted");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    // Clicking the card body does not toggle; the switch inside does. Drive
    // the underlying handler directly through state for determinism.
    let before = rt.state.status.snapshot.preferences.blocked_regions.len();
    lobbylocker_app::regions::toggle_region(&mut rt.state, "cs2", &first.id.clone());
    rt.frame(0.0);
    let after = rt.state.status.snapshot.preferences.blocked_regions.len();
    assert_eq!(after, before + 1, "region toggle persists one selection");
    // Toggling again removes it.
    lobbylocker_app::regions::toggle_region(&mut rt.state, "cs2", &first.id.clone());
    rt.frame(0.0);
    assert_eq!(
        rt.state.status.snapshot.preferences.blocked_regions.len(),
        before
    );
}

#[test]
fn vendored_lucide_icons_match_the_source_crate() {
    use lucide_static_svg::Icon;
    let cases = [
        ("Settings2", &lobbylocker_app::icons::GEAR),
        ("Shield", &lobbylocker_app::icons::SHIELD),
        ("Folder", &lobbylocker_app::icons::FOLDER),
        ("File", &lobbylocker_app::icons::FILE),
        ("Flame", &lobbylocker_app::icons::FLAME),
        ("Globe", &lobbylocker_app::icons::GLOBE),
        ("Sun", &lobbylocker_app::icons::SUN),
        ("Moon", &lobbylocker_app::icons::MOON),
        ("RotateCw", &lobbylocker_app::icons::REFRESH),
        ("Search", &lobbylocker_app::icons::SEARCH),
    ];
    for (want, vendored) in cases {
        assert_eq!(vendored.name, want, "name matches lucide `{want}`");
        assert!(
            vendored.body.contains(r#"stroke="currentColor""#),
            "body paints in the element colour"
        );
        assert_eq!(vendored.view, 24.0);
    }
    let shield = Icon::Shield.svg_str();
    let first_path = shield
        .split("<path")
        .nth(1)
        .map(|s| s.split('"').nth(1).unwrap_or(""))
        .unwrap_or("");
    assert!(!first_path.is_empty());
    assert!(
        lobbylocker_app::icons::SHIELD.body.contains(first_path),
        "vendored shield carries the catalog geometry"
    );
}

#[test]
fn area_heading_toggles_when_clicked_by_name() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    let (x, y) = rt.find("area-Europe-title").expect("Europe title");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    assert!(
        rt.state.collapsed_areas.contains(&"cs2:Europe".to_string()),
        "clicking the area name collapses it"
    );
    assert!(
        rt.find("server-ams").is_none(),
        "cards unmount when collapsed"
    );
    let (x, y) = rt.find("area-Europe-title").expect("Europe title");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    assert!(
        !rt.state.collapsed_areas.contains(&"cs2:Europe".to_string()),
        "clicking again expands"
    );
    assert!(rt.find("server-ams").is_some(), "cards remount");
}

fn click_ui(rt: &mut Runtime<lobbylocker_app::state::AppState>, key: &str) {
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("missing control {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    for _ in 0..12 {
        rt.frame(1.0 / 60.0);
    }
}

#[test]
fn edit_mode_opens_server_details_and_preserves_existing_targets() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    let original = rt
        .state
        .status
        .snapshot
        .games
        .iter()
        .find(|g| g.id == "cs2")
        .unwrap()
        .regions
        .iter()
        .find(|r| r.id == "ams")
        .unwrap()
        .clone();
    let blocked = rt.state.status.snapshot.preferences.blocked_regions.clone();
    click_ui(&mut rt, "edit-mode");
    assert!(rt.state.editing);
    assert!(rt.find("target-dialog").is_none());
    let (heading, _) = rt.presented("area-Europe-title").unwrap();
    let (add, _) = rt.presented("add-server-Europe").unwrap();
    assert!((heading[1] + heading[3] / 2.0 - add[1] - add[3] / 2.0).abs() < 1.0);

    click_ui(&mut rt, "edit-server-ams");
    assert_eq!(rt.state.editing_region.as_deref(), Some("cs2:ams"));
    assert!(rt.presented("server-details-dialog").is_some());
    assert_eq!(rt.state.region_name.text, original.name);
    rt.state.region_name.text = "Renamed server".into();
    rt.invalidate();
    rt.frame(0.0);
    click_ui(&mut rt, "server-save");
    assert!(rt.state.editing_region.is_none());
    let saved = rt
        .state
        .status
        .snapshot
        .games
        .iter()
        .find(|g| g.id == "cs2")
        .unwrap()
        .regions
        .iter()
        .find(|r| r.id == "ams")
        .unwrap();
    assert_eq!(saved.name, "Renamed server");
    assert_eq!(saved.endpoints, original.endpoints);
    assert_eq!(saved.probe_target, original.probe_target);
    assert_eq!(
        rt.state.status.snapshot.preferences.blocked_regions,
        blocked
    );
}

#[test]
fn add_server_dialog_creates_a_new_region_and_reports_validation_errors() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    let original = rt
        .state
        .status
        .snapshot
        .games
        .iter()
        .find(|g| g.id == "cs2")
        .unwrap()
        .clone();
    let blocked = rt.state.status.snapshot.preferences.blocked_regions.clone();
    click_ui(&mut rt, "edit-mode");
    click_ui(&mut rt, "add-server-Europe");
    assert!(
        rt.state.collapsed_areas.is_empty(),
        "adding does not collapse the area"
    );
    assert!(rt.presented("target-dialog").is_some());
    rt.state.target_addr.text = "not an IP".into();
    rt.invalidate();
    rt.frame(0.0);
    click_ui(&mut rt, "target-save");
    assert!(rt.find("target-error").is_some());
    assert!(rt.state.adding_area.is_some());

    rt.state.target_addr.text = "203.0.113.9".into();
    rt.state.target_name.text = "My test server".into();
    rt.state.target_ports.text = "27015".into();
    rt.invalidate();
    rt.frame(0.0);
    click_ui(&mut rt, "target-save");
    assert!(rt.state.adding_area.is_none());
    let saved = rt
        .state
        .status
        .snapshot
        .games
        .iter()
        .find(|g| g.id == "cs2")
        .unwrap();
    assert_eq!(
        &saved.regions[..original.regions.len()],
        original.regions.as_slice()
    );
    let added = saved.regions.last().unwrap();
    assert_eq!(
        (added.name.as_str(), added.area.as_str()),
        ("My test server", "Europe")
    );
    assert_eq!(added.endpoints[0].address, "203.0.113.9");
    assert_eq!(added.endpoints[0].ports.as_deref(), Some("27015"));
    let first_id = added.id.clone();
    assert_eq!(
        rt.state.status.snapshot.preferences.blocked_regions,
        blocked
    );
    lobbylocker_app::forms::open_target_dialog(&mut rt.state, "Europe");
    rt.state.target_addr.text = "203.0.113.10".into();
    rt.state.target_name.text = "My test server".into();
    lobbylocker_app::forms::add_target(&mut rt.state, "cs2", "Europe");
    let saved = rt
        .state
        .status
        .snapshot
        .games
        .iter()
        .find(|g| g.id == "cs2")
        .unwrap();
    assert_eq!(saved.regions.len(), original.regions.len() + 2);
    assert_ne!(saved.regions.last().unwrap().id, first_id);
}

#[test]
fn server_edit_dialogs_fit_minimum_window_sizes() {
    for (size, compact) in [((720.0, 560.0), false), ((360.0, 560.0), true)] {
        let (mut rt, _config) = cs2_runtime_at(size);
        if compact {
            rt.state.sidebar_state = rt.state.sidebar_state.collapsed();
            rt.state.status.snapshot.preferences.language = "ar".into();
        }
        rt.state.editing = true;
        rt.frame(0.0);
        let (edit, _) = rt.presented("edit-mode").expect("edit mode button");
        let (add, _) = rt
            .presented("add-server-Europe")
            .expect("add server button");
        let (card, _) = rt.presented("server-ams").expect("server card");
        let (title, _) = rt.presented("game-title").expect("game title");
        assert!(
            edit[0] + edit[2] <= size.0 + 1.0,
            "edit button fits at {size:?}: {edit:?}"
        );
        assert!(
            add[0] + add[2] <= size.0 + 1.0,
            "add button fits at {size:?}: {add:?}"
        );
        assert!(
            card[0] + card[2] <= size.0 + 1.0,
            "server card fits at {size:?}: {card:?}"
        );
        assert!(
            title[0] + title[2] <= size.0 + 1.0,
            "game title fits at {size:?}: {title:?}"
        );
        lobbylocker_app::forms::open_target_dialog(&mut rt.state, "Europe");
        rt.invalidate();
        for _ in 0..60 {
            rt.frame(1.0 / 60.0);
        }
        let (dialog, _) = rt.presented("target-dialog").expect("add dialog");
        let (field, _) = rt.presented("target-address").expect("address input");
        let (save, _) = rt.presented("target-save").expect("add button");
        assert!(dialog[0] >= 0.0 && dialog[0] + dialog[2] <= size.0);
        assert!(dialog[1] >= 0.0 && dialog[1] + dialog[3] <= size.1);
        assert!(field[0] >= dialog[0] && field[0] + field[2] <= dialog[0] + dialog[2]);
        assert!(save[1] + save[3] <= dialog[1] + dialog[3]);

        rt.state.adding_area = None;
        rt.state.editing_region = Some("cs2:ams".into());
        rt.invalidate();
        for _ in 0..60 {
            rt.frame(1.0 / 60.0);
        }
        let (dialog, _) = rt.presented("server-details-dialog").expect("edit dialog");
        let (save, _) = rt.presented("server-save").expect("save button");
        assert!(dialog[0] >= 0.0 && dialog[0] + dialog[2] <= size.0);
        assert!(dialog[1] >= 0.0 && dialog[1] + dialog[3] <= size.1);
        assert!(save[0] + save[2] <= dialog[0] + dialog[2]);
        assert!(save[1] + save[3] <= dialog[1] + dialog[3]);
    }
}

#[test]
fn server_cards_wrap_into_rows_instead_of_squeezing() {
    let config = tempfile::tempdir().unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dirs = DataDirs {
        games: root.join("data/games"),
        locales: root.join("data/locales"),
        config: config.path().to_path_buf(),
    };
    let mut state = init_with(dirs);
    state
        .engine
        .set_library_games(vec!["deadlock".to_string()], &[])
        .unwrap();
    state.status = state.engine.runtime();
    state.selected = "deadlock".to_string();
    state.scan.games = vec![installed_game("deadlock", "Deadlock")];
    state.scanned = true;
    state.allow_firewall = false;
    let mut rt = Runtime::new(state, view::view, (1240.0, 860.0), Env::new(true, 1.0));
    // Give Europe enough cards to need more than one row. `grow` is
    // `flex: g 1 0`, so a growing card would put every one on a single line;
    // this guards the fixed-width, wrapping grid.
    let game = rt
        .state
        .status
        .snapshot
        .games
        .iter_mut()
        .find(|g| g.id == "deadlock")
        .expect("deadlock bundled");
    let base = game
        .regions
        .iter()
        .find(|r| r.area == "Europe")
        .cloned()
        .expect("Europe region");
    for i in 0..7 {
        let mut region = base.clone();
        region.id = format!("wrap-test-{i}");
        region.name = format!("Wrap Test {i}");
        game.regions.push(region);
    }
    rt.frame(0.0);
    let mut rows = std::collections::BTreeSet::new();
    for i in 0..7 {
        let (rect, _) = rt
            .presented(&format!("server-wrap-test-{i}"))
            .expect("wrapped card is mounted");
        assert!(
            (288.0..=400.0).contains(&rect[2]),
            "card grows to fill its row but stays within 288..=400, got {}",
            rect[2]
        );
        rows.insert((rect[1] * 10.0).round() as i32);
    }
    assert!(
        rows.len() >= 2,
        "seven cards wrap onto at least two rows, got {rows:?}"
    );
}

#[test]
fn sidebar_search_and_scan_fit_the_sidebar_width() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    let (search, _) = rt.presented("search-games").expect("search field");
    let (scan, _) = rt.presented("scan-games").expect("scan button");
    let (row, _) = rt.presented("game-cs2").expect("a library row");
    assert!(
        search[2] > 100.0
            && search[0] + search[2] <= scan[0] + 1.0
            && scan[0] + scan[2] <= row[0] + row[2] + 1.0,
        "search and scan fit without overlapping: {search:?}, {scan:?}, {row:?}"
    );
}

#[test]
fn sidebar_lists_supported_and_detected_games_without_selection_setup() {
    use lobbylocker_core::installed::{empty_game, InstalledGame};

    let (mut rt, config) = cs2_runtime();
    let custom = empty_game("custom-existing".into(), "A custom game").unwrap();
    rt.state.engine.save_game(&custom).unwrap();
    rt.state.engine.set_library_games(vec![], &[]).unwrap();
    let mut preferences = rt.state.engine.runtime().snapshot.preferences;
    preferences.blocked_regions.push("cs2:ams".into());
    rt.state.engine.preferences(preferences).unwrap();
    rt.state.status = rt.state.engine.runtime();
    let before = rt.state.status.snapshot.preferences.blocked_regions.clone();
    rt.state.scan.games = vec![
        installed_game("deadlock", "Deadlock"),
        InstalledGame {
            id: "steam-123456789".into(),
            name: "A detected game".into(),
            launcher: "Steam".into(),
            directory: config.path().to_path_buf(),
        },
        InstalledGame {
            id: "steam-1493710".into(),
            name: "Proton Experimental".into(),
            launcher: "Steam".into(),
            directory: config.path().to_path_buf(),
        },
    ];
    rt.state.scanned = true;
    lobbylocker_app::state::reconcile_library_selection(&mut rt.state);
    assert_eq!(rt.state.status.snapshot.preferences.blocked_regions, before);
    rt.frame(0.0);

    let (supported, _) = rt
        .presented("game-deadlock")
        .expect("installed supported game");
    assert!(
        rt.find("game-cs2").is_none(),
        "uninstalled supported games stay hidden"
    );
    assert_eq!(rt.state.selected, "deadlock");
    let (custom, _) = rt.presented("game-custom-existing").expect("custom game");
    let (detected, _) = rt.presented("game-steam-123456789").expect("detected game");
    assert!(supported[1] < custom[1] && supported[1] < detected[1]);
    assert!(rt.find("game-steam-1493710").is_none());
    assert!(rt.find("manage-games").is_none());

    let (x, y) = rt.find("game-steam-123456789").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
    assert_eq!(rt.state.selected, "steam-123456789");
    let snapshot = rt.state.engine.runtime().snapshot;
    assert!(snapshot
        .games
        .iter()
        .any(|game| game.id == "steam-123456789" && game.regions.is_empty()));
    assert_eq!(snapshot.preferences.blocked_regions, before);
    assert!(snapshot.preferences.library_game_ids.is_empty());
}

#[test]
fn sidebar_scan_button_starts_a_scan_without_firewall_changes() {
    let (mut rt, _config) = cs2_runtime();
    let before = rt.state.status.snapshot.preferences.blocked_regions.clone();
    rt.frame(0.0);
    let (x, y) = rt.find("scan-games").expect("scan button");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
    assert!(rt.state.scan_busy);
    assert_eq!(rt.state.status.snapshot.preferences.blocked_regions, before);
}

#[test]
fn sidebar_game_artwork_is_centered_next_to_both_text_lines() {
    let (mut rt, _config) = cs2_runtime();
    let pixels = rune::Pixels::new();
    pixels.set(4, 4, vec![255; 4 * 4 * 4]);
    rt.state.custom_icons.insert("cs2".to_string(), pixels);
    // Only a blocked game shows the status line this test centers against.
    rt.state.status.applied_count = Some(1);
    rt.state.status.region_states.insert(
        "cs2:Europe".to_string(),
        lobbylocker_core::inspection::RegionState::Blocked,
    );
    rt.frame(0.0);

    let (item, _) = rt.presented("game-cs2").expect("game row");
    let (icon, _) = rt.presented("icon-cs2").expect("game icon");
    let (name, _) = rt.presented("game-name-cs2").expect("game name");
    let (status, _) = rt.presented("game-status-cs2").expect("game status");
    assert_eq!((icon[2], icon[3]), (32.0, 32.0));
    assert!((icon[1] + icon[3] / 2.0 - item[1] - item[3] / 2.0).abs() < 1.0);
    assert!(icon[0] + icon[2] <= name[0]);
    assert!(icon[0] + icon[2] <= status[0]);
    assert!(name[1] + name[3] <= status[1] + 1.0);

    let game = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("game-cs2"))
        .expect("accessible game row");
    assert_eq!(game.role, "listitem");
    assert!(game.label.as_deref().unwrap().contains("Counter-Strike 2"));
}

#[test]
fn sidebar_game_rows_remain_selectable_with_pointer_and_keyboard() {
    let (mut rt, _config) = cs2_runtime();
    let template = rt.state.status.snapshot.games[0].clone();
    for id in ["test-first", "test-second"] {
        let mut game = template.clone();
        game.id = id.to_string();
        game.name = id.to_string();
        rt.state.status.snapshot.games.push(game);
    }
    rt.state.status.snapshot.preferences.library_game_ids =
        vec!["test-first".into(), "test-second".into()];
    rt.state.selected = "test-second".into();
    rt.frame(0.0);

    let (x, y) = rt.find("game-test-first").expect("first game");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
    assert_eq!(rt.state.selected, "test-first");

    rt.key(rune::KeyEvent {
        key: rune::Key::Down,
        mods: Default::default(),
    });
    rt.frame(0.0);
    assert_eq!(rt.state.selected, "test-second");
}

#[test]
fn regions_page_scrolls_when_content_overflows() {
    let config = tempfile::tempdir().unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dirs = DataDirs {
        games: root.join("data/games"),
        locales: root.join("data/locales"),
        config: config.path().to_path_buf(),
    };
    let mut state = init_with(dirs);
    state
        .engine
        .set_library_games(vec!["deadlock".to_string()], &[])
        .unwrap();
    state.status = state.engine.runtime();
    state.selected = "deadlock".to_string();
    state.scan.games = vec![installed_game("deadlock", "Deadlock")];
    state.scanned = true;
    state.allow_firewall = false;
    let mut rt = Runtime::new(state, view::view, (1240.0, 860.0), Env::new(true, 1.0));
    let game = rt
        .state
        .status
        .snapshot
        .games
        .iter_mut()
        .find(|g| g.id == "deadlock")
        .expect("deadlock bundled");
    let base = game
        .regions
        .iter()
        .find(|r| r.area == "Europe")
        .cloned()
        .expect("Europe region");
    for i in 0..20 {
        let mut region = base.clone();
        region.id = format!("scroll-test-{i}");
        region.name = format!("Scroll Test {i}");
        game.regions.push(region);
    }
    rt.frame(0.0);
    let (_, y_before) = rt.find("server-scroll-test-0").expect("first card");
    rt.wheel(700.0, 400.0, -600.0);
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    let (_, y_after) = rt.find("server-scroll-test-0").expect("first card");
    assert!(
        y_after < y_before - 10.0,
        "wheel scrolls the regions viewport, before {y_before} after {y_after}"
    );
}

#[test]
fn settings_is_one_page_of_bordered_sections() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    rt.state.tab = Tab::Settings;
    rt.invalidate();
    rt.frame(0.0);
    assert!(
        rt.find("settings-nav").is_none(),
        "settings has no section nav or tabs"
    );
    let mut x: Option<f32> = None;
    let mut last_bottom = f32::NEG_INFINITY;
    for key in [
        "general-section-card",
        "library-section-card",
        "recovery-section-card",
        "files-section-card",
    ] {
        let (rect, _) = rt
            .presented(key)
            .unwrap_or_else(|| panic!("{key} is on the one settings page"));
        if let Some(first_x) = x {
            assert!(
                (rect[0] - first_x).abs() < 1.0,
                "{key} shares the single settings column"
            );
        } else {
            x = Some(rect[0]);
        }
        assert!(
            rect[1] >= last_bottom,
            "{key} stacks below the previous section"
        );
        last_bottom = rect[1] + rect[3];
    }
}

#[test]
fn header_and_settings_language_menus_open_independently() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    rt.state.tab = Tab::Settings;
    rt.invalidate();
    rt.frame(0.0);

    let settle = |rt: &mut Runtime<lobbylocker_app::state::AppState>| {
        for _ in 0..6 {
            rt.frame(1.0 / 60.0);
        }
    };

    rt.state.settings_language_open = true;
    rt.invalidate();
    settle(&mut rt);
    assert!(
        rt.find("language-content").is_some(),
        "the settings language menu opens"
    );
    assert!(
        rt.find("language-header-content").is_none(),
        "opening the settings menu leaves the header menu closed"
    );

    rt.state.settings_language_open = false;
    rt.state.language_open = true;
    rt.invalidate();
    settle(&mut rt);
    assert!(
        rt.find("language-header-content").is_some(),
        "the header language menu opens"
    );
    assert!(
        rt.find("language-content").is_none(),
        "opening the header menu leaves the settings menu closed"
    );
}

#[test]
fn the_game_files_list_skips_games_without_servers_or_an_executable() {
    use lobbylocker_core::installed::{empty_game, InstalledGame};

    let (mut rt, config) = cs2_runtime();
    // A launcher placeholder: detected, but never set up.
    let placeholder = empty_game("steam-123456789".into(), "A detected game").unwrap();
    rt.state.engine.save_game(&placeholder).unwrap();
    rt.state.scan.games.push(InstalledGame {
        id: "steam-123456789".into(),
        name: "A detected game".into(),
        launcher: "Steam".into(),
        directory: config.path().to_path_buf(),
    });
    rt.state.tab = Tab::Settings;
    rt.state.scanned = true;
    rt.invalidate();
    for _ in 0..40 {
        rt.frame(1.0 / 60.0);
    }
    assert!(
        rt.find("file-dot-steam-123456789").is_none(),
        "a game without servers or an executable is not listed"
    );
    // A game with a server list is listed.
    assert!(
        rt.find("file-dot-cs2").is_some(),
        "a game with a server list is listed"
    );
}

#[test]
fn long_settings_lists_scroll_instead_of_growing() {
    use lobbylocker_core::installed::custom_game;
    use lobbylocker_core::model::Protocol;

    let (mut rt, _config) = cs2_runtime();
    // Eight folders with servers: more than the list shows at once.
    let mut paths = Vec::new();
    for i in 0..8 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy().to_string();
        paths.push(path);
        let game = custom_game(
            format!("custom-{}", i),
            format!("Game {i}").as_str(),
            "Custom",
            "192.0.2.10",
            Protocol::Udp,
            None,
        )
        .unwrap();
        rt.state.engine.save_game(&game).unwrap();
    }
    let mut preferences = rt.state.engine.runtime().snapshot.preferences;
    preferences.game_library_paths = paths.clone();
    rt.state.engine.preferences(preferences).unwrap();
    rt.state.status = rt.state.engine.runtime();
    rt.state.tab = Tab::Settings;
    rt.invalidate();
    for _ in 0..40 {
        rt.frame(1.0 / 60.0);
    }

    // The library folders card stops growing and scrolls instead.
    let (folders, _) = rt
        .presented("library-section-scroll")
        .expect("the folders list scrolls");
    assert!(
        folders[3] <= 6.0 * 52.0 + 2.0,
        "the folders list shows about six rows, not all of them"
    );
    // The game files card does the same.
    let (files, _) = rt
        .presented("files-section-scroll")
        .expect("the files list scrolls");
    assert!(
        files[3] <= 6.0 * 52.0 + 2.0,
        "the files list shows about six rows, not all of them"
    );
    // Every folder still has a row with an open button.
    assert!(
        rt.find("open-folder").is_some(),
        "a folder can be opened from the list"
    );
}

#[test]
fn settings_lists_detected_and_custom_library_folders() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    rt.state.tab = Tab::Settings;
    let default = PathBuf::from("/tmp/lobbylocker-default-steam");
    let custom = PathBuf::from("/tmp/lobbylocker-custom-library");
    rt.state.scan.library_paths = vec![default.clone()];
    rt.state.status.snapshot.preferences.game_library_paths =
        vec![custom.to_string_lossy().into_owned()];
    rt.invalidate();
    rt.frame(0.0);
    assert!(
        rt.presented(&format!("row-{}", default.display()))
            .is_some(),
        "a detected default library is listed"
    );
    assert!(
        rt.presented(&format!("row-{}", custom.display())).is_some(),
        "a user-added library is listed"
    );
    assert!(
        rt.find("library-empty").is_none(),
        "detected folders replace the empty state"
    );
    rt.state
        .status
        .snapshot
        .preferences
        .excluded_game_library_paths = vec![default.to_string_lossy().into_owned()];
    rt.invalidate();
    rt.frame(0.0);
    assert!(
        rt.presented(&format!("row-{}", default.display()))
            .is_none(),
        "removing a default folder hides it from the list"
    );
}

#[test]
fn removing_a_detected_folder_removes_its_saved_alias() {
    let (mut rt, config) = cs2_runtime();
    let folder = config.path().join("library");
    std::fs::create_dir(&folder).unwrap();
    let alias = folder.join(".").to_string_lossy().into_owned();
    rt.state
        .engine
        .set_game_library_settings(vec![alias], vec![])
        .unwrap();
    rt.state.status = rt.state.engine.runtime();
    rt.state.scan.library_paths = vec![folder.clone()];
    rt.state.tab = Tab::Settings;
    rt.frame(0.0);

    let remove = rt
        .elements()
        .into_iter()
        .find(|e| e.label.as_deref() == Some("Remove"))
        .expect("library remove button");
    let x = remove.rect[0] + remove.rect[2] / 2.0;
    let y = remove.rect[1] + remove.rect[3] / 2.0;
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);

    let prefs = rt.state.engine.runtime().snapshot.preferences;
    assert!(
        prefs.game_library_paths.is_empty(),
        "saved alias is removed"
    );
    assert_eq!(
        prefs.excluded_game_library_paths,
        vec![folder.to_string_lossy()]
    );
}

#[test]
fn settings_does_not_duplicate_the_sidebar_footer() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    rt.state.tab = Tab::Settings;
    rt.frame(0.0);
    assert!(
        rt.find("sidebar-version").is_some(),
        "version stays in the sidebar footer"
    );
    assert!(
        rt.find("settings-version").is_none(),
        "settings nav no longer repeats the version and About row"
    );
}

#[test]
fn sidebar_footer_stays_left_to_right_in_every_language() {
    for (size, language) in [((720.0, 560.0), "en"), ((360.0, 560.0), "ar")] {
        let (mut rt, _config) = cs2_runtime_at(size);
        rt.state.status.snapshot.preferences.language = language.to_string();
        rt.frame(0.0);
        let (footer, _) = rt.presented("sidebar-footer-content").expect("footer");
        let (version, _) = rt.presented("sidebar-version").expect("version");
        assert!(rt.find("about").is_none());
        assert!(rt.find("manage-games").is_none());
        assert!(
            (version[0] - footer[0] - 8.0).abs() < 1.0,
            "{language}: version sits at the leading edge: {version:?} in {footer:?}"
        );
        assert!(version[0] >= footer[0] && version[0] + version[2] <= footer[0] + footer[2]);
    }
}

#[test]
fn dialogs_size_to_their_content() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    let settle = |rt: &mut Runtime<lobbylocker_app::state::AppState>| {
        for _ in 0..60 {
            rt.frame(1.0 / 60.0);
        }
    };
    rt.state.reset_open = true;
    rt.invalidate();
    rt.frame(0.0);
    settle(&mut rt);
    let (reset, _) = rt.presented("reset-dialog").expect("reset dialog");
    assert!(
        reset[3] < 200.0,
        "confirmation fits its content instead of the 368pt panel, got {}",
        reset[3]
    );
    let (description, _) = rt
        .presented("reset-description")
        .expect("reset description");
    let cancel = rt
        .elements()
        .into_iter()
        .find(|e| e.label.as_deref() == Some("Cancel"))
        .expect("cancel button");
    assert!(
        description[0] >= reset[0]
            && description[0] + description[2] <= reset[0] + reset[2]
            && description[3] > 20.0,
        "description wraps inside the dialog: {description:?} in {reset:?}"
    );
    assert!(
        cancel.rect[1] >= description[1] + description[3],
        "actions sit below the description"
    );
    rt.state.reset_open = false;
    rt.state.custom_open = true;
    rt.invalidate();
    rt.frame(0.0);
    settle(&mut rt);
    let (custom, _) = rt.presented("custom-dialog").expect("custom dialog");
    assert!(
        custom[3] < 340.0,
        "custom dialog fits its content, got {}",
        custom[3]
    );
}

#[test]
fn reset_confirmation_fits_in_compact_window() {
    let (mut rt, _config) = cs2_runtime_at((360.0, 560.0));
    rt.state.reset_open = true;
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    let (dialog, _) = rt.presented("reset-dialog").expect("reset dialog");
    let (description, _) = rt.presented("reset-description").expect("description");
    assert!(dialog[0] >= 0.0 && dialog[0] + dialog[2] <= 360.0);
    assert!(
        description[0] >= dialog[0] && description[0] + description[2] <= dialog[0] + dialog[2]
    );
}

/// Long region names (from the refreshed list) must ellipsize inside a
/// minimum-width card instead of pushing the switch out past the border.
#[test]
fn server_card_switch_stays_inside_the_card_with_long_names() {
    let (mut rt, _config) = cs2_runtime_at((784.0, 860.0));
    let game = rt
        .state
        .status
        .snapshot
        .games
        .iter_mut()
        .find(|g| g.id == "cs2")
        .unwrap();
    for region in game.regions.iter_mut() {
        region.name = format!("Datapacket {} (Germany)", region.name);
    }
    rt.frame(0.0);
    let els = rt.elements();
    let mut checked = 0;
    for e in &els {
        let Some(key) = e.key.as_deref() else {
            continue;
        };
        if !key.starts_with("server-") {
            continue;
        }
        let card = e.rect;
        for s in &els {
            if s.key.as_deref() != Some("control") {
                continue;
            }
            let r = s.rect;
            let cy = r[1] + r[3] / 2.0;
            if cy < card[1] || cy > card[1] + card[3] || r[0] < card[0] || r[0] > card[0] + card[2]
            {
                continue;
            }
            checked += 1;
            let past = r[0] + r[2] - (card[0] + card[2]);
            assert!(
                past <= -11.5,
                "{key}: switch extends {past:.1} past the card border (card {card:?}, switch {r:?})"
            );
        }
    }
    assert!(
        checked >= 8,
        "expected switches in the presented cards, got {checked}"
    );
}

/// Clicking the official-list refresh button must not block the frame loop:
/// the work goes to a worker and `busy` is set until the worker lands.
#[test]
fn refresh_button_hands_work_to_a_worker() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    let btn = rt
        .elements()
        .into_iter()
        .find(|e| e.label.as_deref() == Some("Refresh official list"))
        .expect("the refresh button is mounted");
    let x = btn.rect[0] + btn.rect[2] / 2.0;
    let y = btn.rect[1] + btn.rect[3] / 2.0;
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
    assert_eq!(
        rt.state.busy.as_deref(),
        Some("refresh"),
        "the click marks the refresh as busy"
    );
    assert!(
        rt.state.refresh_latest.is_pending(),
        "the refresh runs off the UI thread"
    );
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("Refreshing the official list…")),
        "the footer reports the refresh as running"
    );
    // Keep the suite offline: drop the worker's answer before it can land.
    rt.state.refresh_latest.cancel();
}

/// A refresh that fails fast (an unknown game) clears `busy` and surfaces
/// the error through `poll`, so the UI stays responsive in both cases.
#[test]
fn refresh_worker_error_clears_busy() {
    let (mut rt, _config) = cs2_runtime();
    rt.frame(0.0);
    rt.state.busy = Some("refresh".to_string());
    lobbylocker_app::state::request_refresh(&rt.state, "missing-game".to_string());
    for _ in 0..100 {
        rt.update(|state, tx| {
            lobbylocker_app::state::poll(state, 0.0, tx);
        });
        if rt.state.busy.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        rt.state.busy.is_none(),
        "the worker landing clears the busy state"
    );
    assert!(
        rt.state.error.contains("Game not found"),
        "the error surfaces, got {:?}",
        rt.state.error
    );
}
