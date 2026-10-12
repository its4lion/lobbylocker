//! Unit tests for the catalogue module.
use super::*;

fn template() -> Game {
    let defaults = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../data/games/overwatch2.json");
    let mut game: Game = serde_json::from_slice(&std::fs::read(defaults).unwrap()).unwrap();
    game.regions
        .retain(|region| region.id == "gcp-europe-north1");
    game
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
    assert_eq!(game.regions[0].endpoints.len(), 1);
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
