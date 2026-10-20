//! Unit tests for the commands module.
use super::*;

#[test]
fn attaching_an_executable_preserves_game_targets_selections_and_applied_state() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/games");
    let engine = Engine::new(
        lobbylocker_core::store::Store::open(root.path().join("config"), &defaults).unwrap(),
    )
    .unwrap();
    let executable = root.path().join("actual-game.exe");
    std::fs::write(&executable, b"not executed").unwrap();
    let mut preferences = engine.runtime().snapshot.preferences;
    preferences.blocked_regions = vec!["cs2:dxb".into()];
    engine.preferences(preferences).unwrap();
    let before = engine.runtime();
    save_game_executable(&engine, "cs2", executable.to_str().unwrap()).unwrap();
    let after = engine.runtime();
    let game = after
        .snapshot
        .games
        .iter()
        .find(|game| game.id == "cs2")
        .unwrap();
    assert_eq!(game.executable_path.as_deref(), executable.to_str());
    assert_eq!(
        game.regions,
        before
            .snapshot
            .games
            .iter()
            .find(|game| game.id == "cs2")
            .unwrap()
            .regions
    );
    assert_eq!(after.snapshot.preferences, before.snapshot.preferences);
    assert_eq!(after.applied_targets, before.applied_targets);
    assert_eq!(after.applied_count, before.applied_count);
    assert!(after.dirty);
    assert!(save_game_executable(&engine, "missing-game", executable.to_str().unwrap()).is_err());
    assert!(save_game_executable(
        &engine,
        "cs2",
        root.path().join("missing.exe").to_str().unwrap()
    )
    .is_err());
}

#[test]
fn new_games_persist_separately_without_blocking_pinging_or_replacing_existing_games() {
    let directory = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/games");
    let engine = Engine::new(
        lobbylocker_core::store::Store::open(directory.path().into(), &defaults).unwrap(),
    )
    .unwrap();
    let input = || NewGame {
        candidate_id: Some("steam-12345".into()),
        name: "Custom game".into(),
        area: "Europe".into(),
        address: "203.0.113.8/24".into(),
        protocol: Protocol::Udp,
        ports: None,
        executable_path: None,
    };
    assert_eq!(create(&engine, input()).unwrap(), "steam-12345");
    let runtime = engine.runtime();
    let game = runtime
        .snapshot
        .games
        .iter()
        .find(|g| g.id == "steam-12345")
        .unwrap();
    assert_eq!(game.regions[0].endpoints[0].address, "203.0.113.0/24");
    assert!(game.regions[0].probe_target.is_none());
    assert!(runtime.snapshot.preferences.blocked_regions.is_empty());
    assert_eq!(
        runtime.snapshot.preferences.library_game_ids,
        vec!["steam-12345"]
    );
    assert!(runtime.samples.is_empty());
    assert!(runtime.applied_count.is_none());
    assert!(directory.path().join("games/steam-12345.json").exists());
    assert!(create(&engine, input()).is_err());
    let mut invalid = input();
    invalid.candidate_id = Some("../escape".into());
    assert!(create(&engine, invalid).is_err());
}

#[test]
fn operation_guard_rejects_parallel_mutations_and_recovers_on_drop() {
    let flag = Arc::new(AtomicBool::new(false));
    let guard = Operation::acquire(Arc::clone(&flag)).unwrap();
    assert!(Operation::acquire(Arc::clone(&flag)).is_err());
    drop(guard);
    assert!(Operation::acquire(flag).is_ok());
}

#[test]
fn additional_library_folders_persist_once_without_firewall_or_probe_changes() {
    let directory = tempfile::tempdir().unwrap();
    let library = directory.path().join("XboxGames");
    std::fs::create_dir_all(&library).unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/games");
    let engine = Engine::new(
        lobbylocker_core::store::Store::open(directory.path().join("config"), &defaults).unwrap(),
    )
    .unwrap();
    let before = engine.runtime();
    add_game_library(&engine, &library).unwrap();
    add_game_library(&engine, &library).unwrap();
    let after = engine.runtime();
    assert_eq!(after.snapshot.preferences.game_library_paths.len(), 1);
    assert_eq!(
        before.snapshot.preferences.blocked_regions,
        after.snapshot.preferences.blocked_regions
    );
    assert_eq!(before.dirty, after.dirty);
    assert_eq!(before.generation, after.generation);
    assert_eq!(before.applied_count, after.applied_count);
    assert_eq!(before.message, after.message);
    assert!(after.samples.is_empty());
    assert!(add_game_library(&engine, std::path::Path::new("relative-folder")).is_err());
    assert!(add_game_library(&engine, &directory.path().join("missing")).is_err());
    let reopened =
        lobbylocker_core::store::Store::open(directory.path().join("config"), &defaults).unwrap();
    assert_eq!(
        reopened.snapshot().unwrap().preferences.game_library_paths,
        after.snapshot.preferences.game_library_paths
    );
}

#[test]
fn removing_and_restoring_default_or_added_folders_is_persistent_and_read_only() {
    let root = tempfile::tempdir().unwrap();
    let library = root.path().join("Steam");
    std::fs::create_dir_all(library.join("steamapps")).unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/games");
    let config = root.path().join("config");
    let engine =
        Engine::new(lobbylocker_core::store::Store::open(config.clone(), &defaults).unwrap())
            .unwrap();
    let before = engine.runtime();
    // A default is removable even though it is not in user-added paths.
    remove_game_library_path(&engine, library.to_str().unwrap()).unwrap();
    let removed = engine.runtime();
    assert!(removed.snapshot.preferences.game_library_paths.is_empty());
    assert_eq!(
        removed.snapshot.preferences.excluded_game_library_paths,
        vec![std::fs::canonicalize(&library).unwrap().to_str().unwrap()]
    );
    let reopened =
        Engine::new(lobbylocker_core::store::Store::open(config, &defaults).unwrap()).unwrap();
    assert_eq!(
        reopened
            .runtime()
            .snapshot
            .preferences
            .excluded_game_library_paths,
        removed.snapshot.preferences.excluded_game_library_paths
    );
    assert_eq!(
        before.snapshot.preferences.blocked_regions,
        removed.snapshot.preferences.blocked_regions
    );
    assert_eq!(before.dirty, removed.dirty);
    assert_eq!(before.applied_count, removed.applied_count);
    assert_eq!(before.applied_game_ids, removed.applied_game_ids);
    assert_eq!(before.generation, removed.generation);
    assert_eq!(before.message, removed.message);
    assert!(removed.samples.is_empty());
    assert!(library.join("steamapps").is_dir());
    add_game_library(&reopened, &library.join("steamapps")).unwrap();
    add_game_library(&reopened, &library).unwrap();
    assert!(reopened
        .runtime()
        .snapshot
        .preferences
        .excluded_game_library_paths
        .is_empty());
    assert_eq!(
        reopened
            .runtime()
            .snapshot
            .preferences
            .game_library_paths
            .len(),
        1
    );
    remove_game_library_path(&reopened, library.to_str().unwrap()).unwrap();
    assert!(reopened
        .runtime()
        .snapshot
        .preferences
        .game_library_paths
        .is_empty());
    assert_eq!(
        reopened
            .runtime()
            .snapshot
            .preferences
            .excluded_game_library_paths
            .len(),
        1
    );
    assert!(remove_game_library_path(&reopened, "relative-folder").is_err());
}

#[test]
fn selected_executable_is_validated_and_persisted_without_launching_or_applying() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("my-game.exe");
    std::fs::write(&executable, b"not an executable; never run this file").unwrap();
    let path = executable.to_str().unwrap().to_owned();
    assert!(validate_executable(&path).is_ok());
    assert!(validate_executable("relative-game.exe").is_err());
    assert!(validate_executable(directory.path().to_str().unwrap()).is_err());
    assert!(validate_executable(directory.path().join("missing.exe").to_str().unwrap()).is_err());
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/games");
    let store =
        lobbylocker_core::store::Store::open(directory.path().join("config"), &defaults).unwrap();
    let engine = Engine::new(store).unwrap();
    let id = create(
        &engine,
        NewGame {
            candidate_id: None,
            name: "My game".into(),
            area: "Custom".into(),
            address: "203.0.113.8".into(),
            protocol: Protocol::Udp,
            ports: None,
            executable_path: Some(path.clone()),
        },
    )
    .unwrap();
    let runtime = engine.runtime();
    let game = runtime
        .snapshot
        .games
        .iter()
        .find(|game| game.id == id)
        .unwrap();
    assert_eq!(game.executable_path.as_deref(), Some(path.as_str()));
    assert!(runtime.snapshot.preferences.blocked_regions.is_empty());
    assert!(runtime.samples.is_empty());
    assert!(runtime.applied_count.is_none());
    let saved: Game = serde_json::from_slice(
        &std::fs::read(directory.path().join(format!("config/games/{id}.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(saved.executable_path, Some(path));
}

#[test]
fn wire_status_has_no_private_policy_and_uses_camel_case() {
    let value = serde_json::to_value(Status {
        snapshot: Snapshot {
            games: vec![],
            preferences: Preferences::default(),
            config_dir: "test".into(),
            platform: "linux".into(),
        },
        self_update_supported: false,
        samples: BTreeMap::new(),
        histories: BTreeMap::new(),
        applied_targets: None,
        applied_count: None,
        applied_game_ids: vec![],
        region_states: BTreeMap::new(),
        inspected_at: None,
        verification: Verification::Unverified,
        verification_error: String::new(),
        verification_issue: None,
        dirty: true,
        message: "test".into(),
    })
    .unwrap();
    assert!(value.get("policy").is_none());
    assert_eq!(value["selfUpdateSupported"], false);
    assert!(value.get("autoRegions").is_none());
    assert!(value["appliedTargets"].is_null());
    assert!(value["snapshot"]["preferences"]
        .get("autoEnabled")
        .is_none());
    assert!(value["snapshot"]["preferences"].get("threshold").is_none());
    assert!(value.get("appliedCount").is_some());
    assert!(value["appliedGameIds"].is_array());
    assert!(value["regionStates"].is_object());
    assert!(value["inspectedAt"].is_null());
    assert_eq!(value["verification"], "unverified");
    assert!(value.get("inspection").is_none());
    assert!(value["snapshot"]["preferences"]["libraryGameIds"].is_array());
    assert!(value["snapshot"]["preferences"]["lastAppliedGameIds"].is_array());
    assert!(value["snapshot"]["preferences"]["language"].is_string());
    assert!(value["snapshot"]["preferences"].get("profiles").is_none());
    assert!(value["snapshot"]["preferences"]
        .get("enabledRules")
        .is_none());
}
