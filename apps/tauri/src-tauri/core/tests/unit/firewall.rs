//! Regression coverage for firewall changes, helper validation, and elevation.
use super::*;
use std::collections::BTreeMap;
#[test]
fn verification_authenticates_once_only_for_permission_denied_when_allowed() {
    let calls = std::cell::Cell::new(0);
    let report = inspect_with_fallback(
        Err(inspection::InspectionError::PermissionDenied(
            "Read denied".into(),
        )),
        true,
        || {
            calls.set(calls.get() + 1);
            Ok(Inspection::default())
        },
    )
    .unwrap();
    assert!(report.rules.is_empty());
    assert_eq!(calls.get(), 1);
    let error = inspect_with_fallback(
        Err(inspection::InspectionError::PermissionDenied(
            "Read denied".into(),
        )),
        true,
        || {
            calls.set(calls.get() + 1);
            Err("UAC cancelled".into())
        },
    )
    .unwrap_err();
    assert_eq!(error.message, "UAC cancelled");
    assert_eq!(calls.get(), 2);
}
#[test]
fn verification_never_prompts_for_parsing_errors_or_readable_rules() {
    for message in [
        "Could not parse address reported by Windows",
        "Invalid firewall inspection response",
        "Windows firewall is disabled or rules have unsupported filters",
        "Firewall inspection timed out",
        "Could not initialize Windows Firewall",
    ] {
        assert_eq!(
            inspect_with_fallback(
                Err(inspection::InspectionError::Other(message.into())),
                true,
                || panic!("A non-permission failure must not launch an elevated helper"),
            )
            .unwrap_err()
            .message,
            message
        );
    }
    assert!(inspect_with_fallback(Ok(Inspection::default()), true, || {
        panic!("A successful read must not launch an elevated helper")
    })
    .is_ok());
    assert_eq!(
        inspect_with_fallback(
            Err(inspection::InspectionError::PermissionDenied(
                "Read denied".into()
            )),
            false,
            || panic!("A caller that disallows elevation must not authenticate"),
        )
        .unwrap_err()
        .message,
        "Read denied"
    );
}
#[test]
fn verify_requests_cannot_enter_a_mutating_script_or_execution_path() {
    let request = Request::Verify {};
    assert!(nft_script(&request, true).is_err());
    assert!(windows_rule_plan(&request).is_err());
    assert!(windows_preview(&request).is_err());
    assert!(change_with_readback(&request).is_err());
    assert!(elevate(&request).is_err());
    assert!(serde_json::from_str::<Request>(r#"{"action":"verify","rules":[]}"#).is_err());
    assert!(serde_json::from_str::<Request>(
        r#"{"action":"verify","command":"nft flush ruleset"}"#
    )
    .is_err());
}

#[test]
fn helper_outcomes_keep_readback_separate_from_mutation_success() {
    let outcome = FirewallOutcome {
        operation_error: Some("Update failed".into()),
        inspection: Ok(Inspection::default()),
    };
    let parsed = FirewallOutcome::from_bytes(&serde_json::to_vec(&outcome).unwrap()).unwrap();
    assert!(parsed.operation_error.is_some());
    assert!(parsed.inspection.unwrap().rules.is_empty());
    assert!(FirewallOutcome::from_bytes(b"success without readback").is_err());
    assert!(FirewallOutcome::from_bytes(
        br#"{"operationError":null,"inspection":{"Ok":{"rules":[]}},"extra":"evil"}"#
    )
    .is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn deleted_or_rebuilt_executables_require_restart_before_elevation() {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("lobbylocker-tauri");
    std::fs::write(&executable, b"running build").unwrap();
    let running = std::fs::metadata(&executable).unwrap();
    assert_eq!(
        linux_elevation_executable(&executable, None, &running).unwrap(),
        executable
    );
    let replacement = root.path().join("replacement");
    std::fs::write(&replacement, b"rebuilt app").unwrap();
    std::fs::rename(replacement, &executable).unwrap();
    assert_eq!(
        linux_elevation_executable(&executable, None, &running).unwrap_err(),
        EXECUTABLE_RESTART_REQUIRED
    );
    let deleted = root.path().join("lobbylocker-tauri (deleted)");
    // Even though a replacement exists at the original path, do not run it
    // as root from a stale process. No commands execute in these tests.
    assert_eq!(
        linux_elevation_executable(&deleted, None, &running).unwrap_err(),
        EXECUTABLE_RESTART_REQUIRED
    );
    assert!(linux_elevation_executable(root.path(), None, &running).is_err());
    assert!(
        linux_elevation_executable(std::path::Path::new("relative-app"), None, &running).is_err()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn appimage_uses_existing_outer_image_and_literal_deleted_names_remain_valid() {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("app (deleted)");
    std::fs::write(&executable, b"literal filename, not a deleted inode").unwrap();
    let running = std::fs::metadata(&executable).unwrap();
    assert_eq!(
        linux_elevation_executable(&executable, None, &running).unwrap(),
        executable
    );
    let outer = root.path().join("LobbyLocker.AppImage");
    std::fs::write(&outer, b"outer image").unwrap();
    let inner = root.path().join("unmounted/AppRun");
    assert_eq!(
        linux_elevation_executable(&inner, Some(&outer), &running).unwrap(),
        outer
    );
    assert!(linux_elevation_executable(
        &inner,
        Some(&root.path().join("missing.AppImage")),
        &running
    )
    .is_err());
    assert!(linux_elevation_executable(&executable, Some(root.path()), &running).is_err());
}
fn rule(protocol: Protocol, ports: Option<&str>) -> FirewallRule {
    FirewallRule {
        endpoint: crate::model::Endpoint {
            id: "test-rule".into(),
            address: "203.0.113.8/24".into(),
            protocol,
            ports: ports.map(str::to_string),
            custom: true,
        },
        program: None,
    }
}

fn scoped_rule(protocol: Protocol, ports: Option<&str>) -> FirewallRule {
    let mut rule = rule(protocol, ports);
    rule.program = Some(r"C:\Games\Example\game.exe".into());
    rule
}
#[test]
fn reset_only_touches_our_table_and_group() {
    assert_eq!(
        nft_script(&Request::Reset {}, true).unwrap(),
        "delete table inet lobbylocker_v1\n"
    );
    assert_eq!(nft_script(&Request::Reset {}, false).unwrap(), "");
    let preview: serde_json::Value =
        serde_json::from_str(&windows_preview(&Request::Reset {}).unwrap()).unwrap();
    assert_eq!(preview["action"], "reset");
    assert_eq!(preview["group"], WINDOWS_GROUP);
    assert_eq!(preview["rules"], serde_json::json!([]));
}
#[test]
fn any_protocol_with_ports_expands_to_tcp_and_udp() {
    let request = Request::Apply {
        rules: vec![rule(Protocol::Any, Some("27015-27060"))],
    };
    let script = nft_script(&request, true).unwrap();
    assert!(script.contains("ip daddr 203.0.113.0/24 meta l4proto tcp tcp dport 27015-27060"));
    assert!(script.contains("meta l4proto udp udp dport"));
    assert!(!script.contains("flush ruleset"));
    let mut windows = rule(Protocol::Any, Some("27015-27060"));
    windows.program = Some(r"C:\Games\Example\game.exe".into());
    let plan = windows_rule_plan(&Request::Apply {
        rules: vec![windows],
    })
    .unwrap();
    assert_eq!(plan.len(), 2);
    assert_eq!(
        plan.iter().map(|rule| rule.protocol).collect::<Vec<_>>(),
        [6, 17]
    );
    for rule in &plan {
        assert_eq!(rule.address, "203.0.113.0/24");
        assert_eq!(rule.ports.as_deref(), Some("27015-27060"));
        assert_eq!(rule.program, r"C:\Games\Example\game.exe");
        assert_eq!(rule.source_id, "test-rule");
    }
    assert_ne!(plan[0].name, plan[1].name);
}
#[test]
fn helper_revalidates_untrusted_input() {
    let mut bad = rule(Protocol::Udp, None);
    bad.program = Some(r"C:\Games\game.exe".into());
    bad.endpoint.id = "'; Invoke-Expression evil; '".into();
    assert!(windows_rule_plan(&Request::Apply { rules: vec![bad] }).is_err());
    let mut bad = scoped_rule(Protocol::Udp, None);
    bad.endpoint.address = "Any".into();
    assert!(windows_rule_plan(&Request::Apply { rules: vec![bad] }).is_err());
    let duplicate = scoped_rule(Protocol::Udp, None);
    assert!(windows_rule_plan(&Request::Apply {
        rules: vec![duplicate.clone(), duplicate],
    })
    .is_err());
}

#[test]
fn windows_rule_ids_cannot_collide_by_case_before_requesting_elevation() {
    let first = scoped_rule(Protocol::Udp, None);
    let mut second = first.clone();
    second.endpoint.id = first.endpoint.id.to_ascii_uppercase();
    assert!(windows_rule_plan(&Request::Apply {
        rules: vec![first, second]
    })
    .is_err());
}

#[test]
fn windows_requires_program_scope_and_linux_refuses_to_widen_it() {
    let mut scoped = rule(Protocol::Udp, None);
    assert!(windows_rule_plan(&Request::Apply {
        rules: vec![scoped.clone()]
    })
    .is_err());
    scoped.program = Some(r"D:\Games\Example\game.exe".into());
    let request = Request::Apply {
        rules: vec![scoped],
    };
    let plan = windows_rule_plan(&request).unwrap();
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].program, r"D:\Games\Example\game.exe");
    assert_eq!(plan[0].protocol, 17);
    assert!(nft_script(&request, true).is_err());
    assert!(windows_rule_plan(&Request::Apply { rules: vec![] })
        .unwrap()
        .is_empty());
}

#[test]
fn windows_program_paths_remain_literal_data_in_native_plan_and_preview() {
    let mut scoped = rule(Protocol::Tcp, Some("443"));
    let path = r"C:\Games\O'Brien; $(evil)\العربية.exe";
    scoped.program = Some(path.into());
    let request = Request::Apply {
        rules: vec![scoped],
    };
    let plan = windows_rule_plan(&request).unwrap();
    assert_eq!(plan[0].program, path);
    assert_eq!(plan[0].ports.as_deref(), Some("443-443"));
    let preview: serde_json::Value =
        serde_json::from_str(&windows_preview(&request).unwrap()).unwrap();
    assert_eq!(preview["action"], "apply");
    assert_eq!(preview["group"], WINDOWS_GROUP);
    assert_eq!(preview["rules"][0]["program"], path);
}

#[test]
fn windows_any_protocol_without_ports_stays_one_rule_and_replacements_have_unique_names() {
    let request = Request::Apply {
        rules: vec![scoped_rule(Protocol::Any, None)],
    };
    let first = windows_rule_plan(&request).unwrap();
    let second = windows_rule_plan(&request).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].protocol, 256);
    assert_eq!(first[0].ports, None);
    assert_ne!(first[0].name, second[0].name);
    assert_eq!(first[0].source_id, second[0].source_id);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StoredRule {
    group: String,
    enabled: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum StoreEvent {
    Add(String),
    Remove(String),
}

struct FakeWindowsStore {
    rules: BTreeMap<String, StoredRule>,
    events: Vec<StoreEvent>,
    add_calls: usize,
    fail_add_at: Option<usize>,
    fail_remove: Option<String>,
    ineffective_remove: Option<String>,
}

impl FakeWindowsStore {
    fn existing() -> Self {
        Self {
            rules: [
                ("old-enabled", WINDOWS_GROUP, true),
                ("old-disabled", WINDOWS_GROUP, false),
                ("foreign", "Another application", true),
                ("similar-group", "LobbyLocker.Managed.v1.other", true),
            ]
            .into_iter()
            .map(|(name, group, enabled)| {
                (
                    name.into(),
                    StoredRule {
                        group: group.into(),
                        enabled,
                    },
                )
            })
            .collect(),
            events: Vec::new(),
            add_calls: 0,
            fail_add_at: None,
            fail_remove: None,
            ineffective_remove: None,
        }
    }

    fn assert_foreign_unchanged(&self) {
        for (name, group) in [
            ("foreign", "Another application"),
            ("similar-group", "LobbyLocker.Managed.v1.other"),
        ] {
            assert_eq!(
                self.rules.get(name),
                Some(&StoredRule {
                    group: group.into(),
                    enabled: true,
                })
            );
        }
    }
}

impl WindowsRuleStore for FakeWindowsStore {
    fn managed_names(&mut self) -> Result<Vec<String>> {
        Ok(self
            .rules
            .iter()
            .filter(|(_, rule)| rule.group == WINDOWS_GROUP)
            .map(|(name, _)| name.clone())
            .collect())
    }

    fn add(&mut self, rule: &WindowsRuleSpec) -> Result<()> {
        self.events.push(StoreEvent::Add(rule.name.clone()));
        self.add_calls += 1;
        if self.fail_add_at == Some(self.add_calls) {
            return Err("Injected creation failure".into());
        }
        assert!(!self.rules.contains_key(&rule.name));
        self.rules.insert(
            rule.name.clone(),
            StoredRule {
                group: WINDOWS_GROUP.into(),
                enabled: true,
            },
        );
        Ok(())
    }

    fn remove(&mut self, name: &str) -> Result<()> {
        self.events.push(StoreEvent::Remove(name.into()));
        if self.fail_remove.as_deref() == Some(name) {
            return Err(format!("Injected removal failure: {name}"));
        }
        if self.ineffective_remove.as_deref() != Some(name) {
            self.rules.remove(name);
        }
        Ok(())
    }
}

fn replacement_plan() -> Vec<WindowsRuleSpec> {
    windows_rule_plan(&Request::Apply {
        rules: vec![scoped_rule(Protocol::Any, Some("27015-27060"))],
    })
    .unwrap()
}

#[test]
fn windows_replacement_creates_complete_generation_before_removing_old_rules() {
    let plan = replacement_plan();
    let mut store = FakeWindowsStore::existing();
    replace_windows_rules(&mut store, &plan).unwrap();
    assert_eq!(
        store.events,
        [
            StoreEvent::Add(plan[0].name.clone()),
            StoreEvent::Add(plan[1].name.clone()),
            StoreEvent::Remove("old-disabled".into()),
            StoreEvent::Remove("old-enabled".into()),
        ]
    );
    assert_eq!(store.managed_names().unwrap().len(), plan.len());
    for rule in &plan {
        assert!(store.rules[&rule.name].enabled);
    }
    store.assert_foreign_unchanged();
}

#[test]
fn windows_creation_failure_rolls_back_only_the_new_generation() {
    let plan = replacement_plan();
    let mut store = FakeWindowsStore::existing();
    let before = store.rules.clone();
    store.fail_add_at = Some(2);
    let error = replace_windows_rules(&mut store, &plan).unwrap_err();
    assert!(error.contains("creation failure"));
    assert_eq!(store.rules, before);
    assert_eq!(
        store.events,
        [
            StoreEvent::Add(plan[0].name.clone()),
            StoreEvent::Add(plan[1].name.clone()),
            StoreEvent::Remove(plan[0].name.clone()),
        ]
    );
}

#[test]
fn windows_failed_rollback_reports_both_failures_and_preserves_old_rules() {
    let plan = replacement_plan();
    let mut store = FakeWindowsStore::existing();
    store.fail_add_at = Some(2);
    store.fail_remove = Some(plan[0].name.clone());
    let error = replace_windows_rules(&mut store, &plan).unwrap_err();
    assert!(error.contains("creation failure"));
    assert!(error.contains("removal failure"));
    assert!(error.contains(&plan[0].name));
    assert!(store.rules.contains_key("old-enabled"));
    assert!(store.rules.contains_key("old-disabled"));
    assert!(store.rules.contains_key(&plan[0].name));
    assert!(!store.rules.contains_key(&plan[1].name));
    store.assert_foreign_unchanged();
}

#[test]
fn windows_old_rule_deletion_failure_retains_the_complete_new_generation() {
    let plan = replacement_plan();
    let mut store = FakeWindowsStore::existing();
    store.fail_remove = Some("old-enabled".into());
    let error = replace_windows_rules(&mut store, &plan).unwrap_err();
    assert!(error.contains("old-enabled"));
    assert!(store.rules.contains_key("old-enabled"));
    assert!(!store.rules.contains_key("old-disabled"));
    for rule in &plan {
        assert!(store.rules[&rule.name].enabled);
    }
    store.assert_foreign_unchanged();
}

#[test]
fn windows_reset_and_empty_apply_remove_owned_rules_including_disabled_rules() {
    for request in [Request::Reset {}, Request::Apply { rules: vec![] }] {
        let mut store = FakeWindowsStore::existing();
        let plan = windows_rule_plan(&request).unwrap();
        assert!(!store.rules["old-disabled"].enabled);
        replace_windows_rules(&mut store, &plan).unwrap();
        assert!(store.managed_names().unwrap().is_empty());
        assert_eq!(store.rules.len(), 2);
        store.assert_foreign_unchanged();
    }
}

#[test]
fn windows_successful_removal_calls_do_not_hide_remaining_owned_rules() {
    for request in [Request::Reset {}, Request::Apply { rules: vec![] }] {
        let mut store = FakeWindowsStore::existing();
        store.ineffective_remove = Some("old-disabled".into());
        let error =
            replace_windows_rules(&mut store, &windows_rule_plan(&request).unwrap()).unwrap_err();
        assert!(error.contains("do not match"));
        assert!(store.rules.contains_key("old-disabled"));
        assert!(!store.rules.contains_key("old-enabled"));
        store.assert_foreign_unchanged();
    }
}

#[test]
fn windows_duplicate_or_existing_rule_names_fail_before_mutation() {
    let mut duplicate = replacement_plan();
    duplicate[1] = duplicate[0].clone();
    let mut collision = replacement_plan();
    collision[0].name = "old-enabled".into();
    for plan in [duplicate, collision] {
        let mut store = FakeWindowsStore::existing();
        let before = store.rules.clone();
        assert!(replace_windows_rules(&mut store, &plan).is_err());
        assert!(store.events.is_empty());
        assert_eq!(store.rules, before);
    }
}

#[test]
fn privileged_wire_rejects_unknown_fields_and_unscoped_legacy_requests() {
    let mut scoped = rule(Protocol::Udp, None);
    scoped.program = Some(r"C:\Games\game.exe".into());
    let request = Request::Apply {
        rules: vec![scoped],
    };
    let mut json = serde_json::to_value(&request).unwrap();
    json["rules"][0]["command"] = serde_json::json!("evil");
    assert!(serde_json::from_value::<Request>(json).is_err());
    let legacy =
        serde_json::json!({"action":"apply", "rules":[rule(Protocol::Udp, None).endpoint]});
    assert!(serde_json::from_value::<Request>(legacy).is_err());
    assert!(serde_json::from_str::<Request>(r#"{"action":"reset","extra":"evil"}"#).is_err());
}
