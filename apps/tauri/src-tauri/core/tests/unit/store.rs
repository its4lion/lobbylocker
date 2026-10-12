//! Unit tests for the store module.
use super::*;

#[test]
fn separate_game_files_and_no_overwrite() {
    let root = tempfile::tempdir().unwrap();
    let defaults = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let store = Store::open(root.path().into(), &defaults).unwrap();
    let mut snapshot = store.snapshot().unwrap();
    assert!(root.path().join("games/cs2.json").exists());
    assert!(root.path().join("games/deadlock.json").exists());
    snapshot.games[0].name = "My custom name".into();
    store.save_game(&snapshot.games[0]).unwrap();
    let reopened = Store::open(root.path().into(), &defaults).unwrap();
    assert_eq!(reopened.snapshot().unwrap().games[0].name, "My custom name");
    assert!(store.game_path("../escape").is_err());
}

#[test]
fn older_game_files_without_provider_mappings_are_not_silently_replaced() {
    let root = tempfile::tempdir().unwrap();
    let defaults = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let store = Store::open(root.path().into(), &defaults).unwrap();
    let mut legacy: serde_json::Value =
        serde_json::to_value(store.default_game("overwatch2").unwrap()).unwrap();
    for region in legacy["regions"].as_array_mut().unwrap() {
        region.as_object_mut().unwrap().remove("providerScope");
    }
    let path = root.path().join("games/overwatch2.json");
    fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    let reopened = Store::open(root.path().into(), &defaults).unwrap();
    let old = reopened
        .snapshot()
        .unwrap()
        .games
        .into_iter()
        .find(|g| g.id == "overwatch2")
        .unwrap();
    assert!(old.regions.iter().all(|r| r.provider_scope.is_none()));
    assert_eq!(fs::read(path).unwrap(), before);
    assert!(reopened
        .default_game("overwatch2")
        .unwrap()
        .regions
        .iter()
        .filter(|r| r.id.starts_with("gcp-"))
        .all(|r| r.provider_scope.is_some()));
    assert!(reopened
        .default_game("overwatch2")
        .unwrap()
        .regions
        .iter()
        .filter(|r| r.id.starts_with("blizzard-"))
        .all(|r| r.provider_scope.is_none()));
}

#[test]
fn removed_features_in_old_files_are_ignored_without_rewriting_or_activating_them() {
    let root = tempfile::tempdir().unwrap();
    let defaults = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let store = Store::open(root.path().into(), &defaults).unwrap();
    let game_path = root.path().join("games/cs2.json");
    let preferences_path = root.path().join("preferences.json");
    let mut game = serde_json::to_value(store.default_game("cs2").unwrap()).unwrap();
    game["rules"] = serde_json::json!([{
        "label": "Old independent target",
        "endpoint": {"id": "legacy", "address": "203.0.113.8", "protocol": "udp", "ports": null, "custom": true}
    }]);
    let mut preferences = serde_json::to_value(Preferences::default()).unwrap();
    preferences["enabledRules"] = serde_json::json!(["cs2:legacy"]);
    preferences["profiles"] = serde_json::json!([{"id": "legacy", "name": "Old profile"}]);
    preferences["autoEnabled"] = serde_json::json!(true);
    preferences["monitorEnabled"] = serde_json::json!(true);
    preferences["threshold"] = serde_json::json!(50);
    fs::write(&game_path, serde_json::to_vec(&game).unwrap()).unwrap();
    fs::write(&preferences_path, serde_json::to_vec(&preferences).unwrap()).unwrap();
    let game_before = fs::read(&game_path).unwrap();
    let preferences_before = fs::read(&preferences_path).unwrap();

    let reopened = Store::open(root.path().into(), &defaults).unwrap();
    let snapshot = reopened.snapshot().unwrap();
    assert!(
        crate::model::resolve_rules(&snapshot.games, &snapshot.preferences)
            .unwrap()
            .is_empty()
    );
    assert_eq!(fs::read(&game_path).unwrap(), game_before);
    assert_eq!(fs::read(&preferences_path).unwrap(), preferences_before);

    let game = snapshot.games.iter().find(|game| game.id == "cs2").unwrap();
    reopened.save_game(game).unwrap();
    reopened.save_preferences(&snapshot.preferences).unwrap();
    let saved_game: serde_json::Value =
        serde_json::from_slice(&fs::read(game_path).unwrap()).unwrap();
    let saved_preferences: serde_json::Value =
        serde_json::from_slice(&fs::read(preferences_path).unwrap()).unwrap();
    assert!(saved_game.get("rules").is_none());
    assert!(saved_preferences.get("profiles").is_none());
    assert!(saved_preferences.get("enabledRules").is_none());
}

#[test]
fn background_refresh_markers_are_per_game_and_never_pin_refreshes_off() {
    let root = tempfile::tempdir().unwrap();
    let defaults = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let store = Store::open(root.path().into(), &defaults).unwrap();
    let marker = root.path().join("data-refresh.json");
    assert_eq!(store.data_refresh_checked_at("cs2"), None);

    store.mark_data_refresh_checked("cs2").unwrap();
    let now = crate::now_seconds();
    let checked = store.data_refresh_checked_at("cs2").unwrap();
    assert!(checked <= now && checked + 2 >= now);
    // One game being fresh says nothing about another.
    assert_eq!(store.data_refresh_checked_at("deadlock"), None);

    // A hostile or hand-edited marker must not disable refreshes permanently.
    for bad in [
        r#"{"checkedAt":{"cs2":99999999999}}"#,
        r#"{"checkedAt":{"cs2":"yesterday"}}"#,
        r#"{"checkedAt":{"cs2":0}}"#,
        r#"{"checkedAt":{"../escape":1}}"#,
        r#"{"checkedAt":{"cs2":1},"extra":true}"#,
        "not json",
    ] {
        fs::write(&marker, bad).unwrap();
        assert_eq!(store.data_refresh_checked_at("cs2"), None, "{bad}");
    }
    // Invalid IDs never reach the marker file.
    assert!(store.mark_data_refresh_checked("../escape").is_err());
    assert!(store.data_refresh_checked_at("../escape").is_none());

    // A damaged marker is rebuilt, keeping only valid recent entries.
    fs::write(&marker, "not json").unwrap();
    store.mark_data_refresh_checked("deadlock").unwrap();
    assert_eq!(store.data_refresh_checked_at("cs2"), None);
    assert!(store.data_refresh_checked_at("deadlock").is_some());
}
