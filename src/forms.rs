//! Edit-mode forms: add a server
//! address to a region, and edit a region's name, ping target, and targets.

use crate::state::*;
use lobbylocker_core::model::{Endpoint, Game, Protocol, Region};
use rune::{col, row, text, El, ScrollSpec};
use rune_kit::{Button, Dialog, Kit, SegmentedControl, State, TextInput};

fn labeled_field(k: &Kit, label: &str, input: El) -> El {
    col()
        .w_full()
        .gap(3.0)
        .child(
            text(label.to_string())
                .font(rune::tok::type_::LABEL)
                .color(k.c.text_muted),
        )
        .child(input)
}

pub fn open_target_dialog(s: &mut AppState, area: &str) {
    s.target_addr = rune::TextEdit::new("");
    s.target_name = rune::TextEdit::new("");
    s.target_ports = rune::TextEdit::new("");
    s.target_protocol = "udp".into();
    s.editing_region = None;
    s.adding_area = Some(area.to_string());
    s.error.clear();
}

pub fn target_dialog(
    s: &AppState,
    game: &Game,
    area: &str,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) {
    let busy = s.busy.is_some();
    let game_id = game.id.clone();
    let area_owned = area.to_string();
    let protocol_idx = match s.target_protocol.as_str() {
        "udp" => 0,
        "tcp" => 1,
        _ => 2,
    };
    let segment_state = if busy { State::Disabled } else { State::Rest };
    let protocol = SegmentedControl::new(&["UDP", "TCP", t.text("Any")], Some(protocol_idx))
        .label(t.text("Protocol"))
        .width(None)
        .segment_state(0, segment_state)
        .segment_state(1, segment_state)
        .segment_state(2, segment_state)
        .on_pick(|state: &mut AppState, idx: usize, _tx: &mut rune::Tx| {
            state.target_protocol = match idx {
                0 => "udp",
                1 => "tcp",
                _ => "any",
            }
            .into();
        })
        .build(k);
    let mut body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(8.0)
        .child(
            text(t.format("Add a server to {area}.", &[("area", area)]))
                .font(rune::tok::type_::BODY)
                .color(k.c.text_muted)
                .w_full()
                .wrap(),
        )
        .child(labeled_field(
            k,
            t.text("Server IP or CIDR"),
            TextInput::new(&s.target_addr)
                .width(None)
                .label(t.text("Server IP or CIDR"))
                .placeholder(t.text("Server IP or CIDR"))
                .disabled(busy)
                .on_edit(|state: &mut AppState| &mut state.target_addr)
                .build(k)
                .key("target-address"),
        ))
        .child(labeled_field(
            k,
            t.text("Server name"),
            TextInput::new(&s.target_name)
                .width(None)
                .label(t.text("Server name"))
                .placeholder(t.text("Server name"))
                .disabled(busy)
                .on_edit(|state: &mut AppState| &mut state.target_name)
                .build(k)
                .key("target-name"),
        ))
        .child(labeled_field(k, t.text("Protocol"), protocol))
        .child(labeled_field(
            k,
            t.text("Ports (optional, e.g. 27015-27060)"),
            TextInput::new(&s.target_ports)
                .width(None)
                .label(t.text("Ports (optional, e.g. 27015-27060)"))
                .placeholder(t.text("Ports (optional, e.g. 27015-27060)"))
                .disabled(busy)
                .on_edit(|state: &mut AppState| &mut state.target_ports)
                .build(k)
                .key("target-ports"),
        ));
    if !s.error.is_empty() {
        body = body.child(
            text(s.error.clone())
                .key("target-error")
                .font(rune::tok::type_::LABEL)
                .color(k.c.danger.fg)
                .w_full()
                .wrap(),
        );
    }
    body = body.child(
        row()
            .w_full()
            .gap(8.0)
            .justify_end()
            .child(
                Button::new(t.text("Cancel"))
                    .variant(rune_kit::ButtonVariant::Ghost)
                    .on_press(|state: &mut AppState| {
                        state.adding_area = None;
                        state.error.clear();
                    })
                    .build(k)
                    .key("target-cancel"),
            )
            .child(
                Button::new(t.text("Add server"))
                    .variant(rune_kit::ButtonVariant::Submit)
                    .disabled(busy)
                    .on_press(move |state: &mut AppState| add_target(state, &game_id, &area_owned))
                    .build(k)
                    .key("target-save"),
            ),
    );
    Dialog::new("target", t.text("Add server"))
        .open(true)
        .fit()
        .width(460.0)
        .on_close(|state: &mut AppState| {
            state.adding_area = None;
            state.error.clear();
        })
        .child(body)
        .build(k, layer);
}

pub fn add_target(s: &mut AppState, game_id: &str, area: &str) {
    let addr = s.target_addr.text.trim().to_string();
    let name = s.target_name.text.trim().to_string();
    let ports = s.target_ports.text.trim().to_string();
    let protocol = match s.target_protocol.as_str() {
        "tcp" => Protocol::Tcp,
        "any" => Protocol::Any,
        _ => Protocol::Udp,
    };
    let mut game = match s
        .status
        .snapshot
        .games
        .iter()
        .find(|g| g.id == game_id)
        .cloned()
    {
        Some(g) => g,
        None => return,
    };
    let endpoint = Endpoint {
        id: format!("custom-{}", uuid::Uuid::new_v4()),
        address: addr,
        protocol,
        ports: if ports.is_empty() { None } else { Some(ports) },
        custom: true,
    };
    // Validate before touching the engine.
    if let Err(e) = lobbylocker_core::model::normalize_address(&endpoint.address) {
        s.error = e;
        return;
    }
    if let Err(e) = endpoint.validate() {
        s.error = e;
        return;
    }
    game.regions.push(Region {
        id: format!("custom-{}", uuid::Uuid::new_v4()),
        name: if name.is_empty() {
            "Custom server".to_string()
        } else {
            name
        },
        area: area.to_string(),
        probe_target: None,
        probe_custom: false,
        provider_scope: None,
        endpoints: vec![endpoint],
    });
    match s.engine.save_game(&game) {
        Ok(()) => {
            s.target_addr = rune::TextEdit::new("");
            s.target_name = rune::TextEdit::new("");
            s.target_ports = rune::TextEdit::new("");
            s.adding_area = None;
            s.error.clear();
            reload(s);
        }
        Err(e) => s.error = e,
    }
}

pub fn server_details_dialog(
    s: &AppState,
    game: &Game,
    region: &Region,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) {
    let busy = s.busy.is_some();
    let mut endpoints = col()
        .key("server-endpoints")
        .w_full()
        .h(110.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .scroll_edge_fade(12.0)
        .border(1.0, k.c.border_muted)
        .radius(6.0);
    if region.endpoints.is_empty() {
        endpoints = endpoints.child(
            text(t.text("No addresses yet."))
                .font(rune::tok::type_::LABEL)
                .color(k.c.text_muted)
                .padding(10.0),
        );
    }
    for endpoint in &region.endpoints {
        let gid = game.id.clone();
        let rid = region.id.clone();
        let eid = endpoint.id.clone();
        let protocol = match endpoint.protocol {
            Protocol::Udp => "UDP",
            Protocol::Tcp => "TCP",
            Protocol::Any => t.text("Any"),
        };
        endpoints = endpoints.child(
            row()
                .w_full()
                .px(10.0)
                .py(4.0)
                .center_y()
                .gap(8.0)
                .child(
                    text(format!(
                        "{} · {} · {}",
                        endpoint.address,
                        protocol,
                        endpoint.ports.as_deref().unwrap_or(t.text("All ports"))
                    ))
                    .font(rune::tok::type_::LABEL)
                    .color(k.c.text_muted)
                    .grow(1.0)
                    .shrink()
                    .truncate()
                    .help(endpoint.address.clone()),
                )
                .child(
                    Button::new(t.text("Remove"))
                        .variant(rune_kit::ButtonVariant::Ghost)
                        .disabled(busy)
                        .on_press(move |state: &mut AppState| {
                            remove_endpoint(state, &gid, &rid, &eid)
                        })
                        .build(k),
                ),
        );
    }
    let mut body = col()
        .key("server-details")
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(8.0)
        .child(endpoints)
        .child(labeled_field(
            k,
            t.text("Server name"),
            TextInput::new(&s.region_name)
                .width(None)
                .label(t.text("Server name"))
                .placeholder(t.text("Server name"))
                .disabled(busy)
                .on_edit(|state: &mut AppState| &mut state.region_name)
                .build(k)
                .key("region-name"),
        ))
        .child(labeled_field(
            k,
            t.text("Ping target IP (optional)"),
            TextInput::new(&s.probe_target)
                .width(None)
                .label(t.text("Ping target IP (optional)"))
                .placeholder(t.text("Ping target IP (optional)"))
                .disabled(busy)
                .on_edit(|state: &mut AppState| &mut state.probe_target)
                .build(k)
                .key("probe-target"),
        ));
    if !s.error.is_empty() {
        body = body.child(
            text(s.error.clone())
                .key("server-details-error")
                .font(rune::tok::type_::LABEL)
                .color(k.c.danger.fg)
                .w_full()
                .wrap(),
        );
    }
    let gid = game.id.clone();
    let rid = region.id.clone();
    let delete_game = gid.clone();
    let delete_region_id = rid.clone();
    body = body.child(
        row()
            .w_full()
            .gap(8.0)
            .flex_wrap()
            .justify_end()
            .child(
                Button::new(t.text("Delete server"))
                    .variant(rune_kit::ButtonVariant::Danger)
                    .disabled(busy)
                    .on_press(move |state: &mut AppState| {
                        delete_region(state, &delete_game, &delete_region_id)
                    })
                    .build(k)
                    .key("server-delete"),
            )
            .child(
                Button::new(t.text("Cancel"))
                    .variant(rune_kit::ButtonVariant::Ghost)
                    .on_press(|state: &mut AppState| {
                        state.editing_region = None;
                        state.error.clear();
                    })
                    .build(k)
                    .key("server-cancel"),
            )
            .child(
                Button::new(t.text("Save"))
                    .variant(rune_kit::ButtonVariant::Submit)
                    .disabled(busy)
                    .on_press(move |state: &mut AppState| save_server_details(state, &gid, &rid))
                    .build(k)
                    .key("server-save"),
            ),
    );
    Dialog::new("server-details", t.text("Edit server"))
        .open(true)
        .fit()
        .width(500.0)
        .on_close(|state: &mut AppState| {
            state.editing_region = None;
            state.error.clear();
        })
        .child(body)
        .build(k, layer);
}

fn read_game(s: &AppState, game_id: &str) -> Option<Game> {
    s.status
        .snapshot
        .games
        .iter()
        .find(|g| g.id == game_id)
        .cloned()
}

fn remove_endpoint(s: &mut AppState, game_id: &str, region_id: &str, endpoint_id: &str) {
    let mut game = match read_game(s, game_id) {
        Some(g) => g,
        None => return,
    };
    for region in &mut game.regions {
        if region.id == region_id {
            region.endpoints.retain(|e| e.id != endpoint_id);
        }
    }
    match s.engine.save_game(&game) {
        Ok(()) => {
            s.error.clear();
            reload(s);
        }
        Err(e) => s.error = e,
    }
}

fn save_server_details(s: &mut AppState, game_id: &str, region_id: &str) {
    let mut game = match read_game(s, game_id) {
        Some(g) => g,
        None => return,
    };
    let probe = s.probe_target.text.trim().to_string();
    for region in &mut game.regions {
        if region.id == region_id {
            let name = s.region_name.text.trim().to_string();
            if !name.is_empty() {
                region.name = name;
            }
            if probe.is_empty() {
                region.probe_target = None;
                region.probe_custom = false;
            } else {
                match probe.parse::<std::net::IpAddr>() {
                    Ok(_) => {
                        region.probe_target = Some(probe.clone());
                        region.probe_custom = true;
                    }
                    Err(_) => {
                        s.error =
                            "Ping targets must be literal IP addresses, not subnets or hostnames."
                                .to_string();
                        return;
                    }
                }
            }
        }
    }
    match s.engine.save_game(&game) {
        Ok(()) => {
            s.editing_region = None;
            s.error.clear();
            reload(s);
        }
        Err(e) => s.error = e,
    }
}

fn delete_region(s: &mut AppState, game_id: &str, region_id: &str) {
    let mut game = match read_game(s, game_id) {
        Some(g) => g,
        None => return,
    };
    game.regions.retain(|r| r.id != region_id);
    let mut prefs = s.status.snapshot.preferences.clone();
    prefs
        .blocked_regions
        .retain(|key| key != &region_key(game_id, region_id));
    match s.engine.save_game(&game) {
        Ok(()) => {
            if let Err(e) = s.engine.preferences(prefs) {
                s.error = e;
            }
            s.editing_region = None;
            s.error.clear();
            reload(s);
        }
        Err(e) => s.error = e,
    }
}
