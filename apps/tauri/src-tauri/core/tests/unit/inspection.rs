//! Regression coverage for OS readback, address parsing, and block verification.
use super::*;
use serde_json::json;

fn expected(program: Option<&str>) -> Vec<FirewallRule> {
    vec![FirewallRule {
        endpoint: crate::model::Endpoint {
            id: "test-rule".into(),
            address: "185.25.183.163".into(),
            protocol: Protocol::Udp,
            ports: Some("27015-27060".into()),
            custom: false,
        },
        program: program.map(str::to_owned),
    }]
}
fn nft() -> Value {
    json!({"nftables":[
        {"metainfo":{"json_schema_version":1}},
        {"table":{"family":"inet","name":TABLE,"comment":OWNER,"handle":7}},
        {"chain":{"family":"inet","table":TABLE,"name":"outbound","type":"filter","hook":"output","prio":10,"policy":"accept"}},
        {"rule":{"family":"inet","table":TABLE,"chain":"outbound","comment":"ll:test-rule","expr":[
            {"match":{"op":"==","left":{"payload":{"protocol":"ip","field":"daddr"}},"right":"185.25.183.163"}},
            {"match":{"op":"==","left":{"meta":{"key":"l4proto"}},"right":"udp"}},
            {"match":{"op":"==","left":{"payload":{"protocol":"udp","field":"dport"}},"right":{"range":[27015,27060]}}},
            {"counter":{"packets":183,"bytes":9120}}, {"drop":null}
        ]}}
    ]})
}
fn windows() -> Value {
    json!({"enforced":true,"supported":true,"rules":[{"address":"185.25.183.163/32","protocol":"17","ports":"27015-27060","program":"C:\\Games\\game.exe"}]})
}
#[test]
fn windows_addresses_accept_dotted_masks_and_existing_ip_formats() {
    for (reported, canonical) in [
        ("64.224.26.17", "64.224.26.17/32"),
        ("64.224.27.17/23", "64.224.26.0/23"),
        ("64.224.27.17/255.255.254.0", "64.224.26.0/23"),
        ("64.224.26.17/255.255.255.0", "64.224.26.0/24"),
        ("64.224.26.17/255.255.255.255", "64.224.26.17/32"),
        ("64.224.26.17/255.0.0.0", "64.0.0.0/8"),
        (" 64.224.26.0/255.255.254.0 ", "64.224.26.0/23"),
        ("2001:db8::7", "2001:db8::7/128"),
        ("2001:db8::7/64", "2001:db8::/64"),
    ] {
        assert_eq!(
            windows_address_key(reported).unwrap(),
            canonical,
            "{reported}"
        );
    }
    for prefix in 8..=32 {
        let net = ipnet::Ipv4Net::new("64.224.27.17".parse().unwrap(), prefix).unwrap();
        let reported = format!("{}/{}", net.addr(), net.netmask());
        assert_eq!(
            windows_address_key(&reported).unwrap(),
            address_key(&net.to_string()).unwrap(),
            "{reported}"
        );
    }
    // OS formatting support must not loosen validation of imported/user targets.
    assert!(normalize_address("64.224.26.0/255.255.254.0").is_err());
}
#[test]
fn windows_addresses_reject_invalid_masks_keywords_and_unsafe_targets() {
    for address in [
        "",
        "Any",
        "LocalSubnet",
        "example.com/255.255.255.0",
        "64.224.26.0/255.0.255.0",
        "64.224.26.0/255.255.255.1",
        "64.224.26.0/256.255.255.0",
        "64.224.26.0/255.255.255.0/24",
        "64.224.26.0/33",
        "2001:db8::/255.255.255.0",
        "64.224.26.0/0.0.0.0",
        "64.224.26.0/254.0.0.0",
        "127.0.0.1/255.255.255.0",
        "224.0.0.1/255.255.255.0",
        "0.0.0.0/255.255.255.255",
    ] {
        let error = windows_address_key(address).unwrap_err();
        assert!(error.starts_with("Could not parse address reported by Windows"));
        assert!(
            error.contains(&format!("{address:?}")),
            "{address}: {error}"
        );
    }
}
#[test]
fn windows_dotted_mask_readbacks_match_saved_cidr_and_survive_helper_ipc() {
    let mut rules = expected(Some(r"C:\Games\game.exe"));
    rules[0].endpoint.address = "64.224.26.0/23".into();
    rules[0].endpoint.protocol = Protocol::Any;
    rules[0].endpoint.ports = None;
    let mut json = windows();
    json["rules"][0]["address"] = json!("64.224.26.0/255.255.254.0");
    json["rules"][0]["protocol"] = json!("Any");
    json["rules"][0]["ports"] = json!("Any");
    let report = windows_snapshot(&json).unwrap();
    let keys = expected_keys(&rules).unwrap();
    assert_eq!(compare(&report.keys(), &keys), Verification::Matched);
    let restored = Inspection::from_bytes(&serde_json::to_vec(&report).unwrap()).unwrap();
    assert_eq!(restored.keys(), keys);
    let mut game = game();
    game.regions[0].endpoints = vec![rules[0].endpoint.clone()];
    assert_eq!(
        region_states(&[game], &restored, FirewallScope::Executable)["example:custom"],
        RegionState::Blocked
    );
    json["rules"][0]["program"] = json!(r"C:\Games\launcher.exe");
    assert_eq!(
        compare(&windows_keys(&json).unwrap(), &keys),
        Verification::Different
    );
}
#[test]
fn windows_invalid_addresses_fail_the_entire_readback_instead_of_skipping_rules() {
    let mut json = windows();
    let mut invalid = json["rules"][0].clone();
    invalid["address"] = json!("64.224.26.0/255.0.255.0");
    json["rules"].as_array_mut().unwrap().push(invalid);
    assert!(windows_snapshot(&json).is_err());
    json["rules"][1]["address"] = json!("64.224.26.0/255.255.254.0");
    assert!(windows_snapshot(&json).is_ok());
    json["supported"] = json!(false);
    assert!(windows_snapshot(&json).is_err());
    json["supported"] = json!(true);
    json["enforced"] = json!(false);
    assert!(windows_snapshot(&json).is_err());
}
#[test]
fn windows_permission_errors_use_structured_categories_not_localized_messages() {
    let denied = serde_json::to_vec(&json!({
        "permissionDenied": true,
        "message": "تم رفض الوصول."
    }))
    .unwrap();
    assert_eq!(
        windows_query_error(&denied, b""),
        InspectionError::PermissionDenied("تم رفض الوصول.".into())
    );
    let other = br#"{"permissionDenied":false,"message":"Access is denied in a diagnostic, not its error category."}"#;
    assert!(matches!(
        windows_query_error(other, b""),
        InspectionError::Other(_)
    ));
    for malformed in [
        &b"not JSON"[..],
        &br#"{"permissionDenied":true}"#[..],
        &br#"{"permissionDenied":"true","message":"denied"}"#[..],
        &br#"{"permissionDenied":true,"message":"denied","extra":true}"#[..],
    ] {
        assert_eq!(
            windows_query_error(malformed, b"PowerShell query failed"),
            InspectionError::Other("PowerShell query failed".into())
        );
    }
}
#[test]
fn only_native_permission_failures_are_eligible_for_elevation() {
    for message in [
        "netlink: Error: cache initialization failed: Operation not permitted",
        "Permission denied (you must be root)",
    ] {
        assert!(matches!(
            nft_query_error(message.as_bytes()),
            InspectionError::PermissionDenied(_)
        ));
    }
    assert!(matches!(
        nft_query_error(b"syntax error, unexpected token"),
        InspectionError::Other(_)
    ));
    assert!(matches!(
        InspectionError::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        InspectionError::PermissionDenied(_)
    ));
    assert!(matches!(
        InspectionError::from(std::io::Error::from(std::io::ErrorKind::NotFound)),
        InspectionError::Other(_)
    ));
    assert!(matches!(
        InspectionError::from("Enter a valid IPv4, IPv6, or CIDR address."),
        InspectionError::Other(_)
    ));
}
#[cfg(target_os = "windows")]
#[test]
fn windows_inspection_script_parses_without_executing_firewall_commands() {
    let script = windows_inspection_script().replace('\'', "''");
    let check = format!("$ErrorActionPreference='Stop'; $tokens=$null; $errors=$null; $null=[System.Management.Automation.Language.Parser]::ParseInput('{script}',[ref]$tokens,[ref]$errors); if ($errors.Count) {{throw ($errors | Out-String)}}");
    let output = crate::firewall::powershell_command()
        .unwrap()
        .args(["-NoProfile", "-NonInteractive", "-Command", &check])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[cfg(target_os = "windows")]
#[test]
fn windows_inspection_script_preserves_permission_categories_without_querying_rules() {
    // Throw before importing NetSecurity or executing any firewall cmdlet.
    // This exercises Windows PowerShell's real catch/JSON/exit-code behavior
    // without requiring admin access or modifying OS rules in the test suite.
    for (failure, denied) in [
        ("throw [System.UnauthorizedAccessException]::new('Acceso denegado.'); ", true),
        ("throw [System.Exception]::new('Wrapper', [System.UnauthorizedAccessException]::new('Acceso denegado.')); ", true),
        ("throw [System.Runtime.InteropServices.COMException]::new('Acceso denegado.', -2147024891); ", true),
        ("throw [System.Runtime.InteropServices.COMException]::new('Acceso denegado.', -2147217405); ", true),
        ("throw [System.Management.Automation.ErrorRecord]::new([System.Exception]::new('Acceso denegado.'), 'TestDenied', [System.Management.Automation.ErrorCategory]::PermissionDenied, $null); ", true),
        ("throw [System.InvalidOperationException]::new('Access denied is text, not a permission category.'); ", false),
    ] {
        let script = windows_inspection_script().replace(WINDOWS_NETSECURITY, failure);
        let error = query(crate::firewall::powershell_command().unwrap().args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ]))
        .unwrap_err();
        assert_eq!(
            matches!(error, InspectionError::PermissionDenied(_)),
            denied,
            "{failure}: {error}"
        );
        assert!(
            error.to_string().contains("Acceso denegado")
                || error.to_string().contains("Wrapper")
                || error.to_string().contains("Access denied"),
            "{failure}: {error}"
        );
    }
}
fn game() -> Game {
    let mut game = crate::installed::custom_game(
        "example".into(),
        "Example",
        "Europe",
        "185.25.183.163",
        Protocol::Udp,
        Some("27015-27060".into()),
    )
    .unwrap();
    game.executable_path = Some(r"C:\Games\game.exe".into());
    game
}
#[test]
fn observed_blocks_are_independent_of_saved_choices_and_source_labels() {
    let game = game();
    let report = linux_snapshot(&nft()).unwrap();
    assert_eq!(
        region_states(
            std::slice::from_ref(&game),
            &report,
            FirewallScope::SystemWide
        )["example:custom"],
        RegionState::Blocked
    );
    let mut narrower = report.clone();
    narrower.rules[0].key.ports = Some((27015, 27020));
    assert_eq!(
        region_states(
            std::slice::from_ref(&game),
            &narrower,
            FirewallScope::SystemWide
        )["example:custom"],
        RegionState::Partial
    );
    narrower.rules[0].key.address = "203.0.113.7/32".into();
    narrower.rules[0].source_id = Some(format!("example-{}", game.regions[0].endpoints[0].id));
    assert_eq!(
        region_states(
            std::slice::from_ref(&game),
            &narrower,
            FirewallScope::SystemWide
        )["example:custom"],
        RegionState::None
    );
    let targets = observed_targets(
        std::slice::from_ref(&game),
        &narrower,
        FirewallScope::SystemWide,
        &BTreeSet::new(),
    );
    assert_eq!(targets[0].endpoints[0].address, "203.0.113.7/32");
    assert_eq!(
        targets[0].endpoints[0].ports.as_deref(),
        Some("27015-27020")
    );
    let empty = Inspection::default();
    assert_eq!(
        region_states(&[game], &empty, FirewallScope::SystemWide)["example:custom"],
        RegionState::None
    );
}
#[test]
fn program_scope_not_rule_name_determines_windows_coverage() {
    let mut game = game();
    let report = windows_snapshot(&windows()).unwrap();
    assert_eq!(
        region_states(
            std::slice::from_ref(&game),
            &report,
            FirewallScope::Executable
        )["example:custom"],
        RegionState::Blocked
    );
    game.executable_path = Some(r"D:\Games\other.exe".into());
    assert_eq!(
        region_states(
            std::slice::from_ref(&game),
            &report,
            FirewallScope::Executable
        )["example:custom"],
        RegionState::None
    );
    game.executable_path = None;
    assert_eq!(
        region_states(
            std::slice::from_ref(&game),
            &report,
            FirewallScope::Executable
        )["example:custom"],
        RegionState::Unknown
    );
    let mut legacy = windows();
    legacy["rules"][0]["program"] = json!("Any");
    assert_eq!(
        region_states(
            std::slice::from_ref(&game),
            &windows_snapshot(&legacy).unwrap(),
            FirewallScope::Executable
        )["example:custom"],
        RegionState::Blocked
    );
    assert_eq!(
        region_states(&[game], &Inspection::default(), FirewallScope::Executable)["example:custom"],
        RegionState::None
    );
}
#[test]
fn broader_ranges_and_partial_protocols_are_taken_from_the_os() {
    let mut game = game();
    let mut report = linux_snapshot(&nft()).unwrap();
    report.rules[0].key.address = "185.25.183.0/24".into();
    report.rules[0].key.ports = None;
    assert_eq!(
        region_states(
            std::slice::from_ref(&game),
            &report,
            FirewallScope::SystemWide
        )["example:custom"],
        RegionState::Blocked
    );
    game.regions[0].endpoints[0].protocol = Protocol::Any;
    assert_eq!(
        region_states(
            std::slice::from_ref(&game),
            &report,
            FirewallScope::SystemWide
        )["example:custom"],
        RegionState::Partial
    );
    let mut tcp = report.rules[0].clone();
    tcp.key.protocol = "tcp".into();
    report.rules.push(tcp);
    assert_eq!(
        region_states(
            std::slice::from_ref(&game),
            &report,
            FirewallScope::SystemWide
        )["example:custom"],
        RegionState::Blocked
    );
    game.regions[0].endpoints[0].ports = None;
    assert_eq!(
        region_states(&[game], &report, FirewallScope::SystemWide)["example:custom"],
        RegionState::Partial
    );
}
#[test]
fn unmapped_and_previous_executable_rules_are_not_replaced_with_catalogue_targets() {
    let game = game();
    let mut report = windows_snapshot(&windows()).unwrap();
    report.rules[0].source_id = Some(format!("example-{}", game.regions[0].endpoints[0].id));
    let mut old = report.rules[0].clone();
    old.key.program = Some(r"d:\games\old.exe".into());
    report.rules.push(old);
    let mut unmapped = report.rules[0].clone();
    unmapped.key.address = "203.0.113.0/24".into();
    unmapped.source_id = None;
    report.rules.push(unmapped);
    let targets = observed_targets(
        &[game],
        &report,
        FirewallScope::Executable,
        &BTreeSet::new(),
    );
    assert_eq!(targets.len(), 3);
    assert!(targets
        .iter()
        .any(|target| target.executable_path.as_deref() == Some(r"d:\games\old.exe")));
    assert!(
        targets
            .iter()
            .any(|target| target.game_id.is_empty()
                && target.endpoints[0].address == "203.0.113.0/24")
    );
}
#[test]
fn malformed_readbacks_and_malformed_ports_are_never_verified() {
    let mut report = linux_snapshot(&nft()).unwrap();
    report.rules[0].key.protocol = "icmp".into();
    assert!(report.validate().is_err());
    report.rules[0].key.protocol = "udp".into();
    report.rules[0].key.ports = Some((27060, 27015));
    assert!(report.validate().is_err());
    assert!(Inspection::from_bytes(br#"{"rules":[],"extra":"unexpected"}"#).is_err());
    assert!(Inspection::from_bytes(b"permission denied").is_err());
}
#[test]
fn multiple_os_rules_cover_address_and_port_ranges_only_when_no_gaps_remain() {
    let mut game = game();
    game.regions[0].endpoints[0].address = "185.25.183.0/24".into();
    let mut report = linux_snapshot(&nft()).unwrap();
    report.rules[0].key.address = "185.25.183.0/25".into();
    let mut other = report.rules[0].clone();
    other.key.address = "185.25.183.128/25".into();
    report.rules.push(other);
    assert_eq!(
        region_states(
            std::slice::from_ref(&game),
            &report,
            FirewallScope::SystemWide
        )["example:custom"],
        RegionState::Blocked
    );
    report.rules[0].key.ports = Some((27015, 27030));
    report.rules[1].key.ports = Some((27031, 27060));
    // Port and address coverage cannot be combined across different hosts.
    assert_eq!(
        region_states(
            std::slice::from_ref(&game),
            &report,
            FirewallScope::SystemWide
        )["example:custom"],
        RegionState::Partial
    );
    for rule in &mut report.rules {
        rule.key.address = "185.25.183.0/24".into();
    }
    assert_eq!(
        region_states(
            std::slice::from_ref(&game),
            &report,
            FirewallScope::SystemWide
        )["example:custom"],
        RegionState::Blocked
    );
    report.rules[1].key.ports = Some((27032, 27060));
    assert_eq!(
        region_states(&[game], &report, FirewallScope::SystemWide)["example:custom"],
        RegionState::Partial
    );
    assert!(!ranges_cover(vec![(0, u128::MAX - 1)], (0, u128::MAX)));
    assert!(ranges_cover(vec![(0, u128::MAX)], (0, u128::MAX)));
}
#[test]
fn absent_owned_tables_are_distinct_from_invalid_or_denied_reads() {
    assert!(!owned_table_present(&json!({"nftables":[]})).unwrap());
    assert!(!owned_table_present(
        &json!({"nftables":[{"table":{"family":"inet","name":"other"}}]})
    )
    .unwrap());
    assert!(
        owned_table_present(&json!({"nftables":[{"table":{"family":"inet","name":TABLE}}]}))
            .unwrap()
    );
    assert!(owned_table_present(&json!({"nftables":[{"error":"permission denied"}]})).is_err());
    assert!(owned_table_present(&json!({"nftables":[{"table":{"name":TABLE}}]})).is_err());
}
#[test]
fn exact_owned_nft_rules_match_after_restart_and_counters_do_not_affect_verification() {
    let keys = expected_keys(&expected(None)).unwrap();
    assert_eq!(
        compare(&linux_keys(&nft()).unwrap(), &keys),
        Verification::Matched
    );
    let mut changed = nft();
    changed["nftables"][3]["rule"]["expr"][0]["match"]["right"] = json!("185.25.183.179");
    assert_eq!(
        compare(&linux_keys(&changed).unwrap(), &keys),
        Verification::Different
    );
    let mut empty = nft();
    empty["nftables"].as_array_mut().unwrap().pop();
    assert_eq!(
        compare(&linux_keys(&empty).unwrap(), &keys),
        Verification::Empty
    );
}
#[test]
fn ownership_chain_shape_and_drop_action_are_not_trusted_from_names_alone() {
    let mut other_owner = nft();
    other_owner["nftables"][1]["table"]["comment"] = json!("Someone else");
    assert!(linux_keys(&other_owner).is_err());
    let mut dormant = nft();
    dormant["nftables"][1]["table"]["flags"] = json!(["dormant"]);
    assert!(linux_keys(&dormant).is_err());
    let mut wrong_hook = nft();
    wrong_hook["nftables"][2]["chain"]["hook"] = json!("input");
    assert!(linux_keys(&wrong_hook).is_err());
    let mut allowed = nft();
    allowed["nftables"][3]["rule"]["expr"][4] = json!({"accept":null});
    assert!(linux_keys(&allowed).is_err());
    let mut extra_match = nft();
    extra_match["nftables"][3]["rule"]["expr"][3] =
        json!({"match":{"op":"==","left":{"meta":{"key":"skuid"}},"right":1000}});
    assert!(linux_keys(&extra_match).is_err());
    let mut unexpected_chain = nft();
    unexpected_chain["nftables"]
        .as_array_mut()
        .unwrap()
        .push(json!({"chain":{"name":"extra"}}));
    assert!(linux_keys(&unexpected_chain).is_err());
    assert!(linux_keys(&json!({"nftables":[]})).is_err());
    assert!(linux_keys(&json!({"error":"permission denied"})).is_err());
}
#[test]
fn ipv4_ipv6_prefixes_and_single_ports_are_normalized() {
    let mut json = nft();
    json["nftables"][3]["rule"]["expr"][0]["match"]["right"] =
        json!({"prefix":{"addr":"185.25.183.0","len":24}});
    json["nftables"][3]["rule"]["expr"][2]["match"]["right"] = json!(27015);
    let mut rules = expected(None);
    rules[0].endpoint.address = "185.25.183.9/24".into();
    rules[0].endpoint.ports = Some("27015".into());
    assert_eq!(linux_keys(&json).unwrap(), expected_keys(&rules).unwrap());
    json["nftables"][3]["rule"]["expr"][0]["match"]["left"]["payload"]["protocol"] = json!("ip6");
    json["nftables"][3]["rule"]["expr"][0]["match"]["right"] =
        json!({"prefix":{"addr":"2001:db8::","len":64}});
    rules[0].endpoint.address = "2001:db8::7/64".into();
    assert_eq!(linux_keys(&json).unwrap(), expected_keys(&rules).unwrap());
}
#[test]
fn any_protocol_with_ports_requires_both_tcp_and_udp_blocks() {
    let mut rules = expected(None);
    rules[0].endpoint.protocol = Protocol::Any;
    let mut json = nft();
    let mut tcp = json["nftables"][3].clone();
    tcp["rule"]["expr"][1]["match"]["right"] = json!("tcp");
    tcp["rule"]["expr"][2]["match"]["left"]["payload"]["protocol"] = json!("tcp");
    assert_eq!(
        compare(&linux_keys(&json).unwrap(), &expected_keys(&rules).unwrap()),
        Verification::Different
    );
    json["nftables"].as_array_mut().unwrap().push(tcp);
    assert_eq!(
        compare(&linux_keys(&json).unwrap(), &expected_keys(&rules).unwrap()),
        Verification::Matched
    );
}
#[test]
fn windows_checks_effective_program_scope_and_legacy_global_rules_do_not_match() {
    let keys = expected_keys(&expected(Some(r"c:\games\GAME.exe"))).unwrap();
    assert_eq!(
        compare(&windows_keys(&windows()).unwrap(), &keys),
        Verification::Matched
    );
    let mut global = windows();
    global["rules"][0]["program"] = json!("Any");
    assert_eq!(
        compare(&windows_keys(&global).unwrap(), &keys),
        Verification::Different
    );
    let mut changed_program = windows();
    changed_program["rules"][0]["program"] = json!(r"D:\Games\other.exe");
    assert_eq!(
        compare(&windows_keys(&changed_program).unwrap(), &keys),
        Verification::Different
    );
    let mut disabled = windows();
    disabled["enforced"] = json!(false);
    assert!(windows_keys(&disabled).is_err());
    let mut constrained = windows();
    constrained["supported"] = json!(false);
    assert!(windows_keys(&constrained).is_err());
    assert_eq!(
        compare(
            &windows_keys(&json!({"enforced":true,"supported":true,"rules":[]})).unwrap(),
            &keys
        ),
        Verification::Empty
    );
}
#[test]
fn inspection_script_is_read_only_and_uses_active_store() {
    let script = windows_inspection_script();
    assert!(script.contains(WINDOWS_GROUP));
    assert!(script.contains("-PolicyStore ActiveStore"));
    assert!(script.contains("Get-NetFirewallApplicationFilter"));
    assert!(script.contains("Get-NetFirewallProfile"));
    assert!(script.contains("Get-Service -Name MpsSvc,BFE"));
    assert!(script.contains(WINDOWS_NETSECURITY));
    assert!(script.contains("[System.Management.Automation.ErrorCategory]::PermissionDenied"));
    assert!(script.contains("[System.UnauthorizedAccessException]"));
    assert!(script.contains("permissionDenied=$denied"));
    assert!(script.contains("exit 1"));
    for write in [
        "New-NetFirewallRule",
        "Remove-NetFirewallRule",
        "Set-NetFirewallRule",
        "Set-NetFirewallProfile",
        "Start-Process",
    ] {
        assert!(!script.contains(write));
    }
}
