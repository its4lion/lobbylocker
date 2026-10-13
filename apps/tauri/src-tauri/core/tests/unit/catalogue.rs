//! Unit tests for the catalogue module.
use super::*;

fn bundled_overwatch() -> Game {
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../data/games/overwatch2.json");
    Game::from_json(&std::fs::read(defaults).unwrap()).unwrap()
}

fn template() -> Game {
    let mut game = bundled_overwatch();
    game.regions
        .retain(|region| region.id == "gcp-europe-north1");
    game.regions[0].probe_target = None;
    game
}

#[test]
fn every_bundled_overwatch_server_has_an_explicit_ping_target() {
    let game = bundled_overwatch();
    game.validate().unwrap();
    assert_eq!(game.regions.len(), 11);
    assert!(game
        .regions
        .iter()
        .all(|region| region.probe_target.is_some()));
}

#[test]
fn bundled_probes_fill_missing_targets_but_preserve_edits_and_disabling() {
    let mut defaults = template();
    defaults.regions[0].probe_target = Some("203.0.113.1".into());
    let mut saved = template();
    let endpoints = saved.regions[0].endpoints.clone();

    merge_defaults(&mut saved, defaults.clone());
    assert_eq!(
        saved.regions[0].probe_target,
        defaults.regions[0].probe_target
    );
    assert_eq!(saved.regions[0].endpoints, endpoints);

    saved.regions[0].probe_target = Some("203.0.113.9".into());
    merge_defaults(&mut saved, defaults.clone());
    assert_eq!(
        saved.regions[0].probe_target.as_deref(),
        Some("203.0.113.9")
    );

    saved.regions[0].probe_target = None;
    saved.regions[0].probe_custom = true;
    merge_defaults(&mut saved, defaults);
    assert_eq!(saved.regions[0].probe_target, None);
    assert!(saved.regions[0].probe_custom);
}

#[test]
fn google_refresh_filters_scopes_and_normalizes_both_families() {
    let mut game = template();
    let custom = Endpoint {
        id: "custom-target".into(),
        address: "198.51.100.7".into(),
        protocol: Protocol::Tcp,
        ports: Some("443".into()),
        custom: true,
    };
    game.regions[0].endpoints.push(custom.clone());
    game.regions[0].probe_target = Some("198.51.100.7".into());
    let before = game.regions[0].endpoints.len();
    let body = br#"{"creationTime":"2026-10-03","prefixes":[
            {"ipv4Prefix":"203.0.113.8/24","service":"Google Cloud","scope":"europe-north1"},
            {"ipv4Prefix":"203.0.113.0/24","service":"Google Cloud","scope":"europe-north1"},
            {"ipv6Prefix":"2001:db8::/48","service":"Google Cloud","scope":"europe-north1"},
            {"ipv4Prefix":"198.51.100.0/24","service":"Google Cloud","scope":"global"}
        ]}"#;
    let updated = update_google_regions(&game, body).unwrap();
    assert_eq!(updated.regions[0].endpoints.len(), 3);
    assert!(updated.regions[0].endpoints.contains(&custom));
    assert_eq!(
        updated.regions[0].probe_target,
        game.regions[0].probe_target
    );
    assert_eq!(updated.source.as_deref(), Some(GOOGLE_CLOUD_URL));
    assert_eq!(game.regions[0].endpoints.len(), before);
}

#[test]
fn google_refresh_does_not_invent_ping_targets() {
    let body = br#"{"prefixes":[{"ipv4Prefix":"203.0.113.0/24","service":"Google Cloud","scope":"europe-north1"}]}"#;
    let updated = update_google_regions(&template(), body).unwrap();
    assert_eq!(updated.regions[0].probe_target, None);
}

#[test]
fn google_refresh_preserves_unmanaged_regions() {
    let mut game = template();
    let manual_region = Region {
        id: "manual-region".into(),
        name: "Manually maintained server".into(),
        area: "Custom".into(),
        probe_target: Some("198.51.100.7".into()),
        probe_custom: true,
        provider_scope: None,
        endpoints: vec![Endpoint {
            id: "manual-server".into(),
            address: "198.51.100.7".into(),
            protocol: Protocol::Udp,
            ports: Some("27015".into()),
            custom: true,
        }],
    };
    game.regions.push(manual_region.clone());
    let body = br#"{"prefixes":[{"ipv4Prefix":"203.0.113.0/24","service":"Google Cloud","scope":"europe-north1"}]}"#;
    let updated = update_google_regions(&game, body).unwrap();
    assert_eq!(updated.regions[1], manual_region);
}

#[test]
fn invalid_and_incomplete_provider_updates_are_rejected() {
    for body in [
            br#"{"prefixes":[]}"#.as_slice(),
            br#"{"prefixes":[{"ipv4Prefix":"0.0.0.0/0","service":"Google Cloud","scope":"europe-north1"}]}"#.as_slice(),
            br#"{"prefixes":[{"ipv4Prefix":"203.0.113.1; reboot","service":"Google Cloud","scope":"europe-north1"}]}"#.as_slice(),
            br#"{"prefixes":[{"ipv6Prefix":"203.0.113.0/24","service":"Google Cloud","scope":"europe-north1"}]}"#.as_slice(),
        ] {
            assert!(update_google_regions(&template(), body).is_err());
        }
    let mut game = template();
    game.regions[0].provider_scope = Some("global".into());
    assert!(game.validate().is_err());
}
