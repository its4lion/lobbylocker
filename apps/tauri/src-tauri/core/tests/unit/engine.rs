//! Unit tests for the engine module.
use super::*;

fn fixture_rules(games: &[Game], preferences: &Preferences) -> Inspection {
    let rules = resolve_rules(games, preferences).unwrap();
    Inspection {
        rules: rules
            .iter()
            .flat_map(|rule| {
                inspection::expected_keys(std::slice::from_ref(rule))
                    .unwrap()
                    .into_iter()
                    .map(|key| inspection::ObservedRule {
                        key,
                        source_id: Some(rule.endpoint.id.clone()),
                    })
            })
            .collect(),
    }
}

fn selected_fixture(engine: &Engine) -> Inspection {
    let mut game = engine
        .runtime()
        .snapshot
        .games
        .into_iter()
        .find(|g| g.id == "deadlock")
        .unwrap();
    game.executable_path = Some(r"C:\Games\deadlock.exe".into());
    engine.save_game(&game).unwrap();
    let mut preferences = engine.runtime().snapshot.preferences;
    preferences.blocked_regions = vec!["deadlock:dxb".into()];
    engine.preferences(preferences).unwrap();
    let snapshot = engine.runtime().snapshot;
    fixture_rules(&snapshot.games, &snapshot.preferences)
}

#[test]
fn invalid_imported_programs_do_not_hide_new_edits_behind_the_same_error() {
    let mut rule = FirewallRule {
        endpoint: crate::model::Endpoint {
            id: "test".into(),
            address: "203.0.113.7".into(),
            protocol: crate::model::Protocol::Udp,
            ports: None,
            custom: false,
        },
        program: Some("old-linux-path".into()),
    };
    let baseline = Engine::pending_rules(&[rule.clone()]);
    assert!(matches!(baseline, PendingKeys::Invalid(_)));
    rule.endpoint.id = "renamed-source".into();
    assert_eq!(
        Engine::pending_rules(&[rule.clone(), rule.clone()]),
        baseline
    );
    rule.endpoint.address = "203.0.113.8".into();
    assert_ne!(Engine::pending_rules(&[rule.clone()]), baseline);
    rule.endpoint.address = "203.0.113.7".into();
    assert_eq!(Engine::pending_rules(&[rule.clone()]), baseline);
    rule.program = Some(r"C:\Games\example.exe".into());
    assert_ne!(Engine::pending_rules(&[rule]), baseline);
}

#[test]
fn unreadable_restart_is_not_a_pending_apply_and_edits_can_be_undone() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let store = Store::open(root.path().into(), &defaults).unwrap();
    let mut snapshot = store.snapshot().unwrap();
    snapshot.preferences.blocked_regions = vec!["deadlock:dxb".into()];
    store.save_preferences(&snapshot.preferences).unwrap();
    let engine = Engine::new(store).unwrap();
    let saved = std::fs::read(root.path().join("preferences.json")).unwrap();
    assert!(!engine.runtime().dirty);
    assert!(engine
        .accept_readback(Err("Authorization cancelled".into()))
        .is_err());
    let before = engine.runtime();
    assert!(!before.dirty);
    assert!(before.applied_count.is_none());
    assert!(before.region_states.is_empty());
    assert_eq!(
        std::fs::read(root.path().join("preferences.json")).unwrap(),
        saved
    );

    let mut preferences = before.snapshot.preferences.clone();
    preferences.blocked_regions.clear();
    engine.preferences(preferences).unwrap();
    assert!(engine.runtime().dirty);
    assert!(engine.runtime().applied_count.is_none());
    engine
        .preferences(before.snapshot.preferences.clone())
        .unwrap();
    assert!(!engine.runtime().dirty);

    let mut game = before
        .snapshot
        .games
        .iter()
        .find(|g| g.id == "deadlock")
        .unwrap()
        .clone();
    game.name = "Renamed".into();
    game.regions
        .iter_mut()
        .find(|r| r.id == "dxb")
        .unwrap()
        .probe_target = Some("203.0.113.5".into());
    engine.save_game(&game).unwrap();
    assert!(!engine.runtime().dirty);
    let original = game.clone();
    game.regions
        .iter_mut()
        .find(|r| r.id == "dxb")
        .unwrap()
        .endpoints[0]
        .address = "203.0.113.9".into();
    engine.save_game(&game).unwrap();
    assert!(engine.runtime().dirty);
    engine.save_game(&original).unwrap();
    assert!(!engine.runtime().dirty);
    engine.set_language("ar").unwrap();
    engine.set_library_games(vec![], &[]).unwrap();
    assert!(!engine.runtime().dirty);
}

#[test]
fn unreadable_retry_preserves_a_known_difference_but_never_active_status() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let engine = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    let report = selected_fixture(&engine);
    engine.record_inspection(Inspection::default()).unwrap();
    assert!(engine.runtime().dirty);
    engine
        .invalidate_inspection(&ReadbackFailure::other("Authorization cancelled"))
        .unwrap();
    assert!(engine.runtime().dirty);
    assert!(engine.runtime().applied_count.is_none());
    engine.record_inspection(report).unwrap();
    assert!(!engine.runtime().dirty);
    engine
        .invalidate_inspection(&ReadbackFailure::other("Read failed"))
        .unwrap();
    assert!(!engine.runtime().dirty);
}

#[test]
fn failed_or_incomplete_updates_show_observed_rules_instead_of_requested_success() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let engine = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    let mut report = selected_fixture(&engine);
    report.rules.pop();
    let saved = std::fs::read(root.path().join("preferences.json")).unwrap();
    let outcome = FirewallOutcome {
        operation_error: Some("Fixture mutation failure".into()),
        inspection: Ok(report.clone()),
    };
    assert!(engine
        .finish_apply(vec!["deadlock".into()], outcome)
        .is_err());
    let runtime = engine.runtime();
    assert_eq!(runtime.applied_count, Some(1));
    assert_eq!(runtime.region_states["deadlock:dxb"], RegionState::Partial);
    assert_eq!(runtime.verification, Verification::Different);
    assert_eq!(
        std::fs::read(root.path().join("preferences.json")).unwrap(),
        saved
    );
    let outcome = FirewallOutcome {
        operation_error: None,
        inspection: Ok(report),
    };
    assert!(engine
        .finish_apply(vec!["deadlock".into()], outcome)
        .is_err());
    assert_eq!(engine.runtime().applied_count, Some(1));
    let outcome = FirewallOutcome {
        operation_error: None,
        inspection: Err("Fixture read denied".into()),
    };
    assert!(engine
        .finish_apply(vec!["deadlock".into()], outcome)
        .is_err());
    let runtime = engine.runtime();
    assert!(runtime.applied_count.is_none());
    assert!(runtime.applied_targets.is_none());
    assert!(runtime.region_states.is_empty());
    assert!(runtime.inspected_at.is_none());
    assert_eq!(runtime.verification, Verification::Unverified);
    assert_eq!(
        runtime.snapshot.preferences.blocked_regions,
        vec!["deadlock:dxb"]
    );
}

#[test]
fn reset_clears_choices_only_when_readback_proves_no_owned_blocks_remain() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let engine = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    let report = selected_fixture(&engine);
    engine.save_recovery(vec!["deadlock".into()]).unwrap();
    let saved = std::fs::read(root.path().join("preferences.json")).unwrap();
    assert!(engine
        .finish_reset(FirewallOutcome {
            operation_error: None,
            inspection: Ok(report)
        })
        .is_err());
    assert_eq!(engine.runtime().applied_count, Some(2));
    assert_eq!(
        engine.runtime().region_states["deadlock:dxb"],
        RegionState::Blocked
    );
    assert_eq!(
        std::fs::read(root.path().join("preferences.json")).unwrap(),
        saved
    );
    assert!(engine
        .finish_reset(FirewallOutcome {
            operation_error: None,
            inspection: Err("Unreadable".into())
        })
        .is_err());
    assert_eq!(
        std::fs::read(root.path().join("preferences.json")).unwrap(),
        saved
    );
    engine
        .finish_reset(FirewallOutcome {
            operation_error: None,
            inspection: Ok(Inspection::default()),
        })
        .unwrap();
    assert_eq!(engine.runtime().applied_count, Some(0));
    assert!(engine
        .runtime()
        .snapshot
        .preferences
        .blocked_regions
        .is_empty());
    assert!(engine
        .runtime()
        .snapshot
        .preferences
        .last_applied_game_ids
        .is_empty());
    assert!(!engine.runtime().dirty);
}

#[test]
fn reverting_desired_edits_matches_the_same_os_snapshot_without_reapplying() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let engine = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    let report = selected_fixture(&engine);
    engine.record_inspection(report).unwrap();
    let before = engine.runtime();
    let mut preferences = before.snapshot.preferences.clone();
    preferences.blocked_regions.clear();
    engine.preferences(preferences).unwrap();
    assert!(engine.runtime().dirty);
    assert_eq!(
        engine.runtime().region_states["deadlock:dxb"],
        RegionState::Blocked
    );
    engine.preferences(before.snapshot.preferences).unwrap();
    let after = engine.runtime();
    assert!(!after.dirty);
    assert_eq!(after.applied_targets, before.applied_targets);
    assert_eq!(after.inspected_at, before.inspected_at);
    assert_eq!(after.verification, Verification::Matched);
}

#[test]
fn active_count_reports_native_rules_not_saved_target_or_game_counts() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let engine = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    let mut report = selected_fixture(&engine);
    report.rules.push(report.rules[0].clone());
    engine.record_inspection(report).unwrap();
    assert_eq!(engine.runtime().applied_count, Some(3));
    assert_eq!(
        engine.runtime().region_states["deadlock:dxb"],
        RegionState::Blocked
    );
    assert!(!engine.runtime().dirty);
    assert_eq!(
        engine.runtime().applied_targets.unwrap()[0].endpoints.len(),
        3
    );
}

#[test]
fn unverified_removals_keep_recovery_hints_until_absence_is_read_back() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let engine = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    engine.save_recovery(vec!["cs2".into()]).unwrap();
    assert!(engine
        .finish_apply(
            vec![],
            FirewallOutcome {
                operation_error: None,
                inspection: Err("Unreadable".into())
            }
        )
        .is_err());
    assert_eq!(
        engine.runtime().snapshot.preferences.last_applied_game_ids,
        vec!["cs2"]
    );
    assert!(engine.runtime().applied_count.is_none());
    engine
        .finish_apply(
            vec![],
            FirewallOutcome {
                operation_error: None,
                inspection: Ok(Inspection::default()),
            },
        )
        .unwrap();
    assert!(engine
        .runtime()
        .snapshot
        .preferences
        .last_applied_game_ids
        .is_empty());
    assert_eq!(engine.runtime().applied_count, Some(0));
}

#[test]
fn read_only_verification_restores_matching_blocks_without_saving_or_applying() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let engine = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    let report = selected_fixture(&engine);
    let before = std::fs::read(root.path().join("preferences.json")).unwrap();
    let restarted = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    assert!(!restarted.runtime().dirty);
    assert!(restarted.runtime().applied_count.is_none());
    restarted.record_inspection(report.clone()).unwrap();
    assert!(!restarted.runtime().dirty);
    assert_eq!(
        restarted.runtime().region_states["deadlock:dxb"],
        RegionState::Blocked
    );
    assert_eq!(restarted.runtime().applied_count, Some(2));
    engine.record_inspection(report.clone()).unwrap();
    let state = engine.runtime();
    assert_eq!(state.applied_count, Some(2));
    assert_eq!(state.applied_game_ids, vec!["deadlock"]);
    assert!(state
        .applied_targets
        .unwrap()
        .iter()
        .any(|target| target.region_id == "dxb"));
    assert!(!state.dirty);
    assert!(state.snapshot.preferences.last_applied_game_ids.is_empty());
    assert_eq!(
        std::fs::read(root.path().join("preferences.json")).unwrap(),
        before
    );
    engine.record_inspection(Inspection::default()).unwrap();
    assert_eq!(engine.runtime().applied_count, Some(0));
    assert!(engine.runtime().dirty);
    assert_eq!(
        engine.runtime().snapshot.preferences.blocked_regions,
        vec!["deadlock:dxb"]
    );
    let mut pending = engine.runtime().snapshot.preferences;
    pending.blocked_regions.clear();
    engine.preferences(pending).unwrap();
    let saved = std::fs::read(root.path().join("preferences.json")).unwrap();
    engine.record_inspection(report).unwrap();
    assert_eq!(engine.runtime().applied_count, Some(2));
    assert_eq!(
        engine.runtime().region_states["deadlock:dxb"],
        RegionState::Blocked
    );
    assert!(engine.runtime().dirty);
    assert_eq!(engine.runtime().verification, Verification::Different);
    assert_eq!(
        std::fs::read(root.path().join("preferences.json")).unwrap(),
        saved
    );
}

#[test]
fn last_applied_game_hints_survive_restart_and_do_not_claim_verified_firewall_state() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let engine = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    engine.save_recovery(vec!["cs2".into()]).unwrap();
    let mut preferences = engine.runtime().snapshot.preferences;
    preferences.blocked_regions.clear();
    preferences.last_applied_game_ids.clear();
    engine.preferences(preferences).unwrap();
    engine.set_library_games(vec![], &[]).unwrap();
    let reopened = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    let loaded = reopened.runtime();
    assert!(loaded.snapshot.preferences.library_game_ids.is_empty());
    assert!(loaded.snapshot.preferences.blocked_regions.is_empty());
    assert_eq!(
        loaded.snapshot.preferences.last_applied_game_ids,
        vec!["cs2"]
    );
    assert!(loaded.applied_count.is_none());
    assert!(loaded.applied_targets.is_none());
    assert!(loaded.applied_game_ids.is_empty());
    reopened.save_recovery(vec![]).unwrap();
    reopened.record_inspection(Inspection::default()).unwrap();
    assert_eq!(reopened.runtime().applied_targets, Some(vec![]));
    let cleared = Store::open(root.path().into(), &defaults)
        .unwrap()
        .snapshot()
        .unwrap();
    assert!(cleared.preferences.last_applied_game_ids.is_empty());
}

#[test]
fn curating_games_persists_without_replacing_files_or_changing_firewall_state() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let engine = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    {
        let mut state = engine.state.lock().unwrap();
        state.snapshot.preferences.blocked_regions = vec!["cs2:dxb".into()];
        state.applied_count = Some(1);
        state.applied_game_ids = vec!["cs2".into()];
        state.dirty = false;
        state.generation = 5;
        state
            .samples
            .insert("cs2:dxb".into(), sample(Some(50.0), 1000));
    }
    let original = std::fs::read(root.path().join("games/cs2.json")).unwrap();
    let before = engine.runtime();
    let detected = vec![crate::installed::InstalledGame {
        id: "steam-123".into(),
        name: "Example".into(),
        launcher: "Steam".into(),
        directory: root.path().into(),
    }];
    engine
        .set_library_games(
            vec!["cs2".into(), "steam-123".into(), "cs2".into()],
            &detected,
        )
        .unwrap();
    let after = engine.runtime();
    assert_eq!(
        after.snapshot.preferences.library_game_ids,
        vec!["cs2", "steam-123"]
    );
    assert!(after
        .snapshot
        .games
        .iter()
        .find(|game| game.id == "steam-123")
        .unwrap()
        .regions
        .is_empty());
    assert_eq!(
        before.snapshot.preferences.blocked_regions,
        after.snapshot.preferences.blocked_regions
    );
    assert_eq!(before.applied_count, after.applied_count);
    assert_eq!(before.applied_game_ids, after.applied_game_ids);
    assert_eq!(before.dirty, after.dirty);
    assert_eq!(before.generation, after.generation);
    assert_eq!(before.message, after.message);
    assert_eq!(after.samples["cs2:dxb"].latency_ms, Some(50.0));
    assert_eq!(
        std::fs::read(root.path().join("games/cs2.json")).unwrap(),
        original
    );
    let reopened = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    assert_eq!(
        reopened.runtime().snapshot.preferences.library_game_ids,
        vec!["cs2", "steam-123"]
    );
    assert!(reopened.runtime().applied_game_ids.is_empty());
    assert!(reopened.runtime().applied_count.is_none());
    engine.set_library_games(vec![], &[]).unwrap();
    assert!(engine
        .runtime()
        .snapshot
        .preferences
        .library_game_ids
        .is_empty());
    assert_eq!(engine.runtime().applied_game_ids, vec!["cs2"]);
    assert!(root.path().join("games/steam-123.json").exists());
    assert!(engine
        .set_library_games(vec!["unknown".into()], &[])
        .is_err());
    assert!(engine
        .runtime()
        .snapshot
        .preferences
        .library_game_ids
        .is_empty());
}

#[test]
fn manual_samples_never_change_selections_or_firewall_state() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let store = Store::open(root.path().into(), &defaults).unwrap();
    let preferences = Preferences::default();
    store.save_preferences(&preferences).unwrap();
    let engine = Engine::new(store).unwrap();
    // No startup measurements or automatic selections.
    assert!(engine.runtime().samples.is_empty());
    {
        let mut state = engine.state.lock().unwrap();
        state.applied_count = Some(0);
        state.dirty = false;
    }
    for time in [1000, 2000, 3000] {
        engine
            .record_latency(0, &[sample(Some(150.0), time)])
            .unwrap();
    }
    let runtime = engine.runtime();
    assert!(runtime.snapshot.preferences.blocked_regions.is_empty());
    assert!(runtime.applied_targets.is_none());
    assert_eq!(runtime.applied_count, Some(0));
    assert!(!runtime.dirty);
    assert!(runtime.message.contains("No firewall rules changed"));
    assert_eq!(runtime.histories["cs2:dxb"].len(), 3);
}

#[test]
fn changed_configuration_discards_late_manual_results() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let engine = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    let generation = engine.runtime().generation;
    engine.preferences(Preferences::default()).unwrap();
    engine
        .record_latency(generation, &[sample(Some(150.0), 1000)])
        .unwrap();
    assert!(engine.runtime().samples.is_empty());
}

#[test]
fn changing_library_folders_preserves_known_firewall_state_and_measurements() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let engine = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    {
        let mut state = engine.state.lock().unwrap();
        state.snapshot.preferences.blocked_regions = vec!["cs2:dxb".into()];
        state.applied_count = Some(1);
        state.dirty = false;
        state.generation = 5;
        state
            .samples
            .insert("cs2:dxb".into(), sample(Some(50.0), 1000));
        state.histories.insert("cs2:dxb".into(), vec![50.0]);
    }
    let before = engine.runtime();
    engine
        .set_game_library_paths(vec![root
            .path()
            .join("extra-library")
            .to_str()
            .unwrap()
            .to_owned()])
        .unwrap();
    let after = engine.runtime();
    assert_eq!(
        before.snapshot.preferences.blocked_regions,
        after.snapshot.preferences.blocked_regions
    );
    assert_eq!(before.applied_count, after.applied_count);
    assert_eq!(before.dirty, after.dirty);
    assert_eq!(before.generation, after.generation);
    assert_eq!(before.histories, after.histories);
    assert_eq!(before.message, after.message);
    assert_eq!(after.samples["cs2:dxb"].latency_ms, Some(50.0));
}

#[test]
fn manual_checks_report_missing_targets_without_network_access() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let store = Store::open(root.path().into(), &defaults).unwrap();
    let mut game = store.default_game("overwatch2").unwrap();
    for region in &mut game.regions {
        region.probe_target = None;
    }
    store.save_game(&game).unwrap();
    let engine = Engine::new(store).unwrap();
    assert!(engine
        .measure_latency("overwatch2", None)
        .unwrap_err()
        .contains("No ping targets"));
    assert!(engine
        .measure_latency("overwatch2", Some("not-found"))
        .is_err());
    assert!(engine.measure_latency("not-found", None).is_err());
    assert!(engine.runtime().samples.is_empty());
    assert_eq!(engine.runtime().applied_count, None);
}

#[test]
fn explicit_overwatch_refresh_migration_adds_bundled_ranges_and_preserves_saved_data() {
    let defaults_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../data/games/overwatch2.json");
    let defaults: Game = serde_json::from_slice(&std::fs::read(defaults_path).unwrap()).unwrap();
    let mut saved = defaults.clone();
    saved
        .regions
        .retain(|region| region.id == "gcp-europe-north1" || region.id == "blizzard-ams1");
    saved
        .regions
        .iter_mut()
        .find(|region| region.id == "gcp-europe-north1")
        .unwrap()
        .provider_scope = None;
    let amsterdam = saved
        .regions
        .iter_mut()
        .find(|region| region.id == "blizzard-ams1")
        .unwrap();
    amsterdam.endpoints[0].protocol = crate::model::Protocol::Udp;
    amsterdam.endpoints.push(crate::model::Endpoint {
        id: "my-ams-server".into(),
        address: "203.0.113.8".into(),
        protocol: crate::model::Protocol::Udp,
        ports: None,
        custom: true,
    });
    saved.regions.push(crate::model::Region {
        id: "my-server".into(),
        name: "My server".into(),
        area: "Custom".into(),
        probe_target: Some("203.0.113.7".into()),
        probe_custom: true,
        provider_scope: None,
        endpoints: vec![crate::model::Endpoint {
            id: "mine".into(),
            address: "203.0.113.7".into(),
            protocol: crate::model::Protocol::Udp,
            ports: None,
            custom: true,
        }],
    });

    catalogue::merge_defaults(&mut saved, defaults);

    assert_eq!(
        saved
            .regions
            .iter()
            .find(|region| region.id == "gcp-europe-north1")
            .unwrap()
            .provider_scope
            .as_deref(),
        Some("europe-north1")
    );
    assert_eq!(
        saved
            .regions
            .iter()
            .filter(|region| region.id.starts_with("blizzard-"))
            .count(),
        5
    );
    let amsterdam = saved
        .regions
        .iter()
        .find(|region| region.id == "blizzard-ams1")
        .unwrap();
    assert!(amsterdam
        .endpoints
        .iter()
        .any(|endpoint| endpoint.address == "64.224.26.0/23"
            && endpoint.protocol == crate::model::Protocol::Any
            && !endpoint.custom));
    assert!(amsterdam
        .endpoints
        .iter()
        .any(|endpoint| endpoint.id == "my-ams-server" && endpoint.custom));
    assert!(saved.regions.iter().any(|region| region.id == "my-server"));
}

fn sample(ms: Option<f64>, time: u64) -> PingSample {
    PingSample {
        region_key: "cs2:dxb".into(),
        target: "203.0.113.1".into(),
        latency_ms: ms,
        error: None,
        measured_at: time,
    }
}
#[test]
fn applied_targets_are_immutable_across_pending_edits() {
    let root = tempfile::tempdir().unwrap();
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let engine = Engine::new(Store::open(root.path().into(), &defaults).unwrap()).unwrap();
    let mut game = crate::installed::custom_game(
        "example".into(),
        "Example",
        "Custom",
        "203.0.113.7",
        crate::model::Protocol::Udp,
        None,
    )
    .unwrap();
    game.executable_path = Some(r"C:\Games\example.exe".into());
    engine.save_game(&game).unwrap();
    let mut preferences = engine.runtime().snapshot.preferences;
    preferences.blocked_regions = vec!["example:custom".into()];
    engine.preferences(preferences).unwrap();
    let before = engine.runtime();
    let report = fixture_rules(&before.snapshot.games, &before.snapshot.preferences);
    engine.record_inspection(report).unwrap();
    let targets = engine.runtime().applied_targets;
    game.regions[0].endpoints[0].address = "203.0.113.9".into();
    engine.save_game(&game).unwrap();
    let mut preferences = engine.runtime().snapshot.preferences;
    preferences.blocked_regions.clear();
    engine.preferences(preferences).unwrap();
    let after = engine.runtime();
    assert!(after.dirty);
    assert_eq!(after.applied_targets, targets);
    assert_eq!(after.applied_count, Some(1));
    assert!(
        crate::model::selected_targets(&after.snapshot.games, &after.snapshot.preferences)
            .is_empty()
    );
}

#[test]
fn background_refresh_is_limited_to_feed_games_and_never_fetches_on_every_visit() {
    let root = tempfile::tempdir().unwrap();
    let defaults =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let store = Store::open(root.path().into(), &defaults).unwrap();
    let engine = Engine::new(store).unwrap();
    let store = Store::open(root.path().into(), &defaults).unwrap();

    // No official feed: nothing is recorded, so a later manual refresh is
    // unaffected and no request is attempted.
    assert!(engine.ensure_fresh_data("not-a-game").is_ok());
    assert_eq!(store.data_refresh_checked_at("not-a-game"), None);

    // A fresh marker avoids network access.
    store.mark_data_refresh_checked("cs2").unwrap();
    let before = store.snapshot().unwrap();
    assert!(engine.ensure_fresh_data("cs2").is_ok());
    let after = store.snapshot().unwrap();
    assert_eq!(after.games, before.games);
    assert_eq!(after.preferences, before.preferences);
}

#[test]
fn bundled_ping_targets_reach_saved_games_during_the_feed_cooldown() {
    let root = tempfile::tempdir().unwrap();
    let defaults =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let store = Store::open(root.path().into(), &defaults).unwrap();
    let mut saved = store.default_game("overwatch2").unwrap();
    for region in &mut saved.regions {
        region.probe_target = None;
    }
    store.save_game(&saved).unwrap();
    store.mark_data_refresh_checked("overwatch2").unwrap();
    let checked = store.data_refresh_checked_at("overwatch2");
    let engine = Engine::new(store).unwrap();
    let before = engine.runtime();

    engine.ensure_fresh_data("overwatch2").unwrap();

    let after = engine.runtime();
    let game = after
        .snapshot
        .games
        .iter()
        .find(|game| game.id == "overwatch2")
        .unwrap();
    assert!(game
        .regions
        .iter()
        .all(|region| region.probe_target.is_some()));
    for (region, original) in game.regions.iter().zip(&saved.regions) {
        assert_eq!(region.endpoints, original.endpoints);
    }
    assert_eq!(after.snapshot.preferences, before.snapshot.preferences);
    assert_eq!(after.dirty, before.dirty);
    assert_eq!(after.applied_count, before.applied_count);
    assert!(after.samples.is_empty());
    assert!(after.histories.is_empty());
    let store = Store::open(root.path().into(), &defaults).unwrap();
    assert_eq!(store.data_refresh_checked_at("overwatch2"), checked);
    assert_eq!(store.snapshot().unwrap().games, after.snapshot.games);
}

#[test]
fn revisiting_valve_games_keeps_the_refreshed_list_during_cooldown() {
    let root = tempfile::tempdir().unwrap();
    let defaults =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let store = Store::open(root.path().into(), &defaults).unwrap();
    for id in ["deadlock", "cs2"] {
        let mut game = store.default_game(id).unwrap();
        game.regions[0].name = "Updated Amsterdam".into();
        game.regions[0].endpoints[0].address = "203.0.113.9".into();
        let mut chicago = game.regions[0].clone();
        chicago.id = "ord".into();
        chicago.name = "Chicago".into();
        chicago.area = "North America".into();
        for (i, endpoint) in chicago.endpoints.iter_mut().enumerate() {
            endpoint.id = format!("ord-{i}");
        }
        game.regions.push(chicago);
        game.updated_at = Some(format!("unix:{}", crate::now_seconds()));
        store.save_game(&game).unwrap();
        store.mark_data_refresh_checked(id).unwrap();
    }
    let engine = Engine::new(store).unwrap();
    let before = engine.runtime();

    for _ in 0..2 {
        for id in ["deadlock", "cs2"] {
            engine.ensure_fresh_data(id).unwrap();
        }
    }

    let after = engine.runtime();
    assert!(
        after.snapshot.games == before.snapshot.games,
        "Returning to Valve games must keep the refreshed relay lists"
    );
    assert_eq!(after.snapshot.preferences, before.snapshot.preferences);
    assert_eq!(after.generation, before.generation);
    let store = Store::open(root.path().into(), &defaults).unwrap();
    assert!(store.snapshot().unwrap().games == before.snapshot.games);
}
