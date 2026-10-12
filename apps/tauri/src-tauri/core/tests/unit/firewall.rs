//! Regression coverage for firewall scripts, helper validation, and elevation.
use super::*;
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
    assert_eq!(error, "UAC cancelled");
    assert_eq!(calls.get(), 2);
}
#[test]
fn verification_never_prompts_for_parsing_errors_or_readable_rules() {
    for message in [
        "Could not parse address reported by Windows",
        "Invalid firewall inspection response",
        "Windows firewall is disabled or rules have unsupported filters",
        "Firewall inspection timed out",
        "Could not locate system PowerShell",
    ] {
        assert_eq!(
            inspect_with_fallback(
                Err(inspection::InspectionError::Other(message.into())),
                true,
                || panic!("A non-permission failure must not launch an elevated helper"),
            )
            .unwrap_err(),
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
        .unwrap_err(),
        "Read denied"
    );
}
#[test]
fn verify_requests_cannot_enter_a_mutating_script_or_execution_path() {
    let request = Request::Verify {};
    assert!(nft_script(&request, true).is_err());
    assert!(powershell_script(&request).is_err());
    assert!(execute_privileged(&request).is_err());
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
#[test]
fn reset_only_touches_our_table_and_group() {
    assert_eq!(
        nft_script(&Request::Reset {}, true).unwrap(),
        "delete table inet lobbylocker_v1\n"
    );
    assert_eq!(nft_script(&Request::Reset {}, false).unwrap(), "");
    let script = powershell_script(&Request::Reset {}).unwrap();
    assert!(script.contains(WINDOWS_GROUP));
    assert!(!script.contains("netsh advfirewall reset"));
}
#[test]
fn windows_reset_confirms_saved_and_effective_owned_rules_are_gone() {
    let script = powershell_script(&Request::Reset {}).unwrap();
    let removal = script
        .find("$old | Remove-NetFirewallRule -ErrorAction Stop;")
        .unwrap();
    for store in ["PersistentStore", "ActiveStore"] {
        let check = format!("if (@(Get-NetFirewallRule -PolicyStore {store} | Where-Object {{$_.Group -eq '{WINDOWS_GROUP}'}}).Count -ne 0)");
        assert!(script.find(&check).unwrap() > removal);
    }
    assert!(script.contains("$ErrorActionPreference='Stop'"));
    assert!(script.contains("throw 'LobbyLocker firewall rules remain after Reset.'"));
    assert!(!script.contains("SilentlyContinue"));
    assert!(!script.contains("Enabled -eq"));
    assert_eq!(script.matches("Remove-NetFirewallRule").count(), 1);
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
    let script = powershell_script(&Request::Apply {
        rules: vec![windows],
    })
    .unwrap();
    assert!(script.contains("-Protocol TCP"));
    assert!(script.contains("-Protocol UDP"));
}
#[test]
fn helper_revalidates_untrusted_input() {
    let mut bad = rule(Protocol::Udp, None);
    bad.endpoint.id = "'; Invoke-Expression evil; '".into();
    assert!(powershell_script(&Request::Apply { rules: vec![bad] }).is_err());
}

#[test]
fn windows_requires_program_scope_and_linux_refuses_to_widen_it() {
    let mut scoped = rule(Protocol::Udp, None);
    assert!(powershell_script(&Request::Apply {
        rules: vec![scoped.clone()]
    })
    .is_err());
    scoped.program = Some(r"D:\Games\Example\game.exe".into());
    let request = Request::Apply {
        rules: vec![scoped],
    };
    let script = powershell_script(&request).unwrap();
    assert!(script.contains(r"-Program 'D:\Games\Example\game.exe'"));
    assert!(script.contains("-Direction Outbound -Action Block"));
    assert!(
        script.find("Test-Path -LiteralPath").unwrap()
            < script.find("New-NetFirewallRule").unwrap()
    );
    assert!(
        script.find("New-NetFirewallRule").unwrap()
            < script.find("$old | Remove-NetFirewallRule").unwrap()
    );
    assert!(script.contains("catch { $new | Remove-NetFirewallRule"));
    assert!(nft_script(&request, true).is_err());
    let clear = powershell_script(&Request::Apply { rules: vec![] }).unwrap();
    assert!(clear.contains("$old | Remove-NetFirewallRule"));
    assert!(!clear.contains("New-NetFirewallRule"));
}

#[test]
fn windows_program_literals_are_quoted_not_evaluated() {
    let mut scoped = rule(Protocol::Tcp, Some("443"));
    scoped.program = Some(r"C:\Games\O'Brien; $(evil)\العربية.exe".into());
    let script = powershell_script(&Request::Apply {
        rules: vec![scoped],
    })
    .unwrap();
    assert!(script.contains(r"-Program 'C:\Games\O''Brien; $(evil)\العربية.exe'"));
    assert!(script.contains(r"Test-Path -LiteralPath 'C:\Games\O''Brien; $(evil)\العربية.exe'"));
    assert!(!script.contains("Invoke-Expression"));
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
