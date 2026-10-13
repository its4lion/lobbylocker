//! Unit tests for the model module.
use super::*;

#[test]
fn windows_program_paths_are_literal_absolute_local_executables() {
    assert_eq!(
        windows_program("D:/Games/Example/game.EXE").unwrap(),
        r"D:\Games\Example\game.EXE"
    );
    assert!(windows_program(r"C:\Program Files\O'Brien\العربية.exe").is_ok());
    for invalid in [
        "",
        "game.exe",
        r"C:game.exe",
        r"\Games\game.exe",
        r"C:\Games\*.exe",
        r"%PROGRAMFILES%\game.exe",
        r"C:\Games\game.exe:stream",
        r"C:\Games\..\game.exe",
        r"C:\Games\game.cmd",
        r"\\server\share\game.exe",
        r"\\?\C:\Games\game.exe",
        "C:\\Games\\bad\nname.exe",
        "C:\\Games\\bad\0name.exe",
    ] {
        assert!(windows_program(invalid).is_err(), "{invalid:?}");
    }
}

#[test]
fn windows_resolution_never_falls_back_and_deduplicates_within_each_program() {
    let mut one = crate::installed::custom_game(
        "one".into(),
        "One",
        "Custom",
        "203.0.113.7",
        Protocol::Udp,
        None,
    )
    .unwrap();
    let mut two = one.clone();
    two.id = "two".into();
    two.name = "Two".into();
    let mut preferences = Preferences {
        blocked_regions: vec!["one:custom".into(), "two:custom".into()],
        ..Default::default()
    };
    assert!(resolve_rules_for_scope(
        &[one.clone(), two.clone()],
        &preferences,
        FirewallScope::Executable
    )
    .unwrap_err()
    .contains("One"));
    one.executable_path = Some(r"C:\Games\One\game.exe".into());
    two.executable_path = Some(r"D:\Games\Two\game.exe".into());
    let games = vec![one.clone(), two.clone()];
    let windows = resolve_rules_for_scope(&games, &preferences, FirewallScope::Executable).unwrap();
    assert_eq!(windows.len(), 2);
    assert_ne!(windows[0].program, windows[1].program);
    assert_eq!(
        resolve_rules_for_scope(&games, &preferences, FirewallScope::SystemWide)
            .unwrap()
            .len(),
        1
    );
    let targets = selected_targets_for_scope(&games, &preferences, FirewallScope::Executable);
    assert_eq!(targets[0].executable_path, one.executable_path);
    assert!(
        selected_targets_for_scope(&games, &preferences, FirewallScope::SystemWide)[0]
            .executable_path
            .is_none()
    );
    two.executable_path = Some(r"c:\games\ONE\GAME.exe".into());
    assert_eq!(
        resolve_rules_for_scope(&[one.clone(), two], &preferences, FirewallScope::Executable)
            .unwrap()
            .len(),
        1
    );
    preferences.blocked_regions.clear();
    one.executable_path = None;
    assert!(
        resolve_rules_for_scope(&[one], &preferences, FirewallScope::Executable)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn program_availability_is_checked_without_executing_it() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("game.exe");
    let game = crate::installed::custom_game(
        "one".into(),
        "One",
        "Custom",
        "203.0.113.7",
        Protocol::Udp,
        None,
    )
    .unwrap();
    let rule = FirewallRule {
        endpoint: game.regions[0].endpoints[0].clone(),
        program: Some(path.to_str().unwrap().into()),
    };
    assert!(validate_program_files(std::slice::from_ref(&rule)).is_err());
    std::fs::write(path, b"not executed").unwrap();
    validate_program_files(&[rule]).unwrap();
}

#[test]
fn removed_default_folders_are_optional_for_old_files_and_strictly_validated() {
    let mut preferences = Preferences::default();
    let mut json = serde_json::to_value(&preferences).unwrap();
    json.as_object_mut()
        .unwrap()
        .remove("excludedGameLibraryPaths");
    assert!(Preferences::from_json(&serde_json::to_vec(&json).unwrap())
        .unwrap()
        .excluded_game_library_paths
        .is_empty());
    preferences.excluded_game_library_paths = vec!["relative-folder".into()];
    assert!(preferences.validate().is_err());
    let root = tempfile::tempdir().unwrap();
    preferences.excluded_game_library_paths =
        vec![root.path().join("disconnected").to_str().unwrap().into()];
    assert!(preferences.validate().is_ok());
    preferences.excluded_game_library_paths = vec![root.path().to_str().unwrap().into(); 257];
    assert!(preferences.validate().is_err());
}

#[test]
fn applied_game_tracking_ignores_empty_and_unknown_regions_and_includes_shared_targets() {
    let one = crate::installed::custom_game(
        "one".into(),
        "One",
        "Custom",
        "203.0.113.8",
        Protocol::Udp,
        None,
    )
    .unwrap();
    let mut two = one.clone();
    two.id = "two".into();
    let mut empty = one.clone();
    empty.id = "empty".into();
    empty.regions[0].endpoints.clear();
    let games = vec![one, two, empty];
    let preferences = Preferences {
        blocked_regions: vec![
            "one:custom".into(),
            "two:custom".into(),
            "empty:custom".into(),
            "unknown:custom".into(),
        ],
        ..Default::default()
    };
    assert_eq!(selected_game_ids(&games, &preferences), vec!["one", "two"]);
    let targets = selected_targets(&games, &preferences);
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].game_name, "One");
    assert_eq!(targets[0].endpoints, targets[1].endpoints);
    assert_eq!(
        resolve_rules_for_scope(&games, &preferences, FirewallScope::SystemWide)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn library_selection_defaults_empty_and_is_bounded_and_validated() {
    let legacy = br#"{"schemaVersion":1,"blockedRegions":[],"threshold":100,"autoEnabled":false}"#;
    let preferences = Preferences::from_json(legacy).unwrap();
    assert!(preferences.library_game_ids.is_empty());
    let mut preferences = Preferences {
        library_game_ids: vec!["../escape".into()],
        ..Default::default()
    };
    assert!(preferences.validate().is_err());
    preferences.library_game_ids = vec!["cs2".into(); 501];
    assert!(preferences.validate().is_err());
}

#[test]
fn area_toggles_group_servers_without_touching_other_games() {
    let endpoint = Endpoint {
        id: "server".into(),
        address: "203.0.113.1".into(),
        protocol: Protocol::Udp,
        ports: None,
        custom: false,
    };
    let mut game = Game {
        schema_version: 1,
        id: "test-game".into(),
        name: "Test".into(),
        source: None,
        updated_at: None,
        executable_path: None,
        note: String::new(),
        regions: vec![Region {
            id: "eu-one".into(),
            name: "Europe one".into(),
            area: "Europe".into(),
            probe_target: None,
            probe_custom: false,
            provider_scope: None,
            endpoints: vec![endpoint],
        }],
    };
    let mut other = game.regions[0].clone();
    other.id = "eu-two".into();
    other.endpoints[0].id = "custom-server".into();
    other.endpoints[0].address = "203.0.113.2".into();
    other.endpoints[0].custom = true;
    game.regions.push(other.clone());
    other.id = "eu-empty".into();
    other.endpoints.clear();
    game.regions.push(other.clone());
    other.id = "middle-east".into();
    other.area = "Middle East".into();
    game.regions.push(other);
    let mut preferences = Preferences {
        blocked_regions: vec![
            "other-game:eu-one".into(),
            "test-game:middle-east".into(),
            "test-game:eu-empty".into(),
        ],
        ..Default::default()
    };
    assert_eq!(
        game.area_keys("Europe"),
        vec!["test-game:eu-one", "test-game:eu-two"]
    );
    preferences.set_area_blocked(&game, "Europe", true);
    preferences.set_area_blocked(&game, "Europe", true);
    assert_eq!(preferences.blocked_regions.len(), 4);
    assert!(!preferences
        .blocked_regions
        .contains(&"test-game:eu-empty".into()));
    let rules =
        resolve_rules_for_scope(&[game.clone()], &preferences, FirewallScope::SystemWide).unwrap();
    assert_eq!(rules.len(), 2);
    assert!(rules.iter().any(|r| r.endpoint.custom));
    preferences.set_area_blocked(&game, "Europe", false);
    assert_eq!(
        preferences.blocked_regions,
        vec!["other-game:eu-one", "test-game:middle-east"]
    );
    assert!(resolve_rules(&[game], &preferences).unwrap().is_empty());
}

#[test]
fn removed_latency_settings_load_only_from_disk_and_are_not_retained() {
    let mut json = serde_json::to_value(Preferences::default()).unwrap();
    json["autoEnabled"] = serde_json::json!(true);
    json["monitorEnabled"] = serde_json::json!(true);
    json["threshold"] = serde_json::json!(100);
    let loaded = Preferences::from_json(&serde_json::to_vec(&json).unwrap()).unwrap();
    loaded.validate().unwrap();
    let saved = serde_json::to_value(loaded).unwrap();
    for key in ["autoEnabled", "monitorEnabled", "threshold"] {
        assert!(saved.get(key).is_none());
    }
    assert!(serde_json::from_value::<Preferences>(json).is_err());
}

#[test]
fn compatibility_loaders_ignore_only_removed_fields_and_ipc_remains_strict() {
    let mut preferences = serde_json::to_value(Preferences::default()).unwrap();
    preferences["profiles"] = serde_json::json!([]);
    preferences["enabledRules"] = serde_json::json!(["cs2:legacy"]);
    let bytes = serde_json::to_vec(&preferences).unwrap();
    assert!(Preferences::from_json(&bytes).is_ok());
    assert!(serde_json::from_slice::<Preferences>(&bytes).is_err());
    preferences["unknown"] = serde_json::json!(true);
    assert!(Preferences::from_json(&serde_json::to_vec(&preferences).unwrap()).is_err());

    let game = crate::installed::custom_game(
        "custom-example".into(),
        "Example",
        "Custom",
        "203.0.113.7",
        Protocol::Udp,
        None,
    )
    .unwrap();
    let mut game = serde_json::to_value(game).unwrap();
    game["rules"] = serde_json::json!([]);
    let bytes = serde_json::to_vec(&game).unwrap();
    assert!(Game::from_json(&bytes).is_ok());
    assert!(serde_json::from_slice::<Game>(&bytes).is_err());
    game["unknown"] = serde_json::json!(true);
    assert!(Game::from_json(&serde_json::to_vec(&game).unwrap()).is_err());
}

#[test]
fn library_paths_are_bounded_and_disconnected_folders_remain_loadable() {
    let root = tempfile::tempdir().unwrap();
    let mut preferences = Preferences {
        game_library_paths: vec![root
            .path()
            .join("currently-disconnected")
            .to_str()
            .unwrap()
            .to_owned()],
        ..Default::default()
    };
    assert!(preferences.validate().is_ok());
    preferences.game_library_paths = vec!["relative-folder".into()];
    assert!(preferences.validate().is_err());
    preferences.game_library_paths = vec![root.path().to_str().unwrap().to_owned(); 33];
    assert!(preferences.validate().is_err());
}

#[test]
fn addresses_are_normalized_and_shell_input_rejected() {
    assert_eq!(
        normalize_address("203.0.113.9/24").unwrap(),
        "203.0.113.0/24"
    );
    assert_eq!(normalize_address("2001:db8::1").unwrap(), "2001:db8::1");
    for invalid in [
        "0.0.0.0/0",
        "::/0",
        "127.0.0.1",
        "203.0.113.1; reboot",
        "example.com",
        "10.0.0.0/1",
    ] {
        assert!(normalize_address(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn ports_are_strict() {
    assert_eq!(
        parse_ports(&Some("27015-27060".into())).unwrap(),
        Some((27015, 27060))
    );
    for invalid in ["0", "65536", "20-10", "1; whoami", "1-2-3"] {
        assert!(parse_ports(&Some(invalid.into())).is_err());
    }
}
