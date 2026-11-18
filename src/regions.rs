//! Regions page: per-game server
//! block switches, ping, refresh, and edit mode.

use crate::forms;
use crate::state::*;
use lobbylocker_core::model::Game;
use rune::{col, el, row, text, Cx, El, TextStyle};
use rune_kit::{Button, Kit, Switch};

pub fn regions_page(
    s: &AppState,
    cx: &mut Cx<AppState>,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) -> El {
    let games = library_games(&s.status);
    let visible = library_candidates(s);
    let game = games
        .iter()
        .find(|g| g.id == s.selected && visible.iter().any(|candidate| candidate.id == g.id));
    let mut body = col().key("regions-page").grow(1.0).min_h(0.0).gap(12.0);
    if let Some(game) = game {
        if !s.compact {
            body = body.child(game_header(s, game, k, t));
        }
        body = body
            .child(block_servers_header(s, game, k, t))
            .child(areas(s, cx, game, k, t, layer));
    } else {
        body = body.child(empty_state(s, k, t));
    }
    crate::view::scroll_content(body)
}

fn block_servers_header(
    s: &AppState,
    game: &Game,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
) -> El {
    if s.compact {
        let game_id = game.id.clone();
        return row()
            .key("block-servers-header")
            .w_full()
            .center_y()
            .justify_between()
            .gap(8.0)
            .child(
                text(t.text("Block servers"))
                    .font(rune::TextStyle::new(13.0, 500.0))
                    .color(k.c.text_base),
            )
            .child(
                Button::new(t.text("Check ping"))
                    .variant(rune_kit::ButtonVariant::Outline)
                    .disabled(s.busy.is_some())
                    .on_press(move |state: &mut AppState| {
                        if state.busy.is_none() {
                            state.busy = Some("ping".to_string());
                            request_ping(state, game_id.clone(), None, None);
                        }
                    })
                    .build(k)
                    .key("compact-ping"),
            );
    }
    let refreshable = BUILTIN_GAMES.contains(&game.id.as_str());
    let has_targets = game.regions.iter().any(|r| !r.endpoints.is_empty());
    let mut actions = row().gap(6.0).center_y();
    if refreshable {
        let label = if has_targets {
            "Refresh official list"
        } else {
            "Get server data"
        };
        let gid = game.id.clone();
        actions = actions.child(
            Button::new(t.text(label))
                .variant(rune_kit::ButtonVariant::Ghost)
                .on_press(move |state: &mut AppState| {
                    if state.busy.is_none() {
                        state.busy = Some("refresh".to_string());
                        match state.engine.refresh_game(&gid) {
                            Ok(()) => {
                                state.status = state.engine.runtime();
                                state.error.clear();
                            }
                            Err(e) => state.error = e,
                        }
                        state.busy = None;
                    }
                })
                .build(k),
        );
    }
    actions = actions.child(
        Button::new(t.text(if s.editing {
            "Done editing"
        } else {
            "Edit mode"
        }))
        .variant(rune_kit::ButtonVariant::Outline)
        .on_press(|state: &mut AppState| {
            state.editing = !state.editing;
            state.editing_region = None;
            state.adding_area = None;
        })
        .build(k)
        .key("edit-mode"),
    );
    row()
        .key("block-servers-header")
        .w_full()
        .flex_wrap()
        .justify_between()
        .center_y()
        .gap(8.0)
        .child(
            row().gap(4.0).center_y().child(
                text(t.text("Block servers"))
                    .font(rune::TextStyle::new(13.0, 500.0))
                    .color(k.c.text_base),
            ).child(
                rune::icon(&crate::icons::INFO, 14.0)
                    .key("block-servers-info")
                    .color(k.c.text_muted)
                    .help(t.text("Switch changes apply automatically. Blocks stay active after the app closes.")),
            ),
        )
        .child(actions)
}

fn game_header(s: &AppState, game: &Game, k: &Kit, t: &lobbylocker_core::i18n::Translator) -> El {
    let game_id = game.id.clone();
    let ping = |variant: rune_kit::ButtonVariant| {
        Button::new(t.text("Check ping"))
            .variant(variant)
            .on_press(move |state: &mut AppState| {
                if state.busy.is_none() {
                    state.busy = Some("ping".to_string());
                    request_ping(state, game_id.clone(), None, None);
                }
            })
            .build(k)
    };
    let title_block = |title_color: rune::Color, sub_color: rune::Color| {
        col()
            .key("game-title")
            .grow(1.0)
            .min_w(0.0)
            .gap(2.0)
            .child(
                text(game.name.clone())
                    .key("game-name")
                    .font(rune::TextStyle::new(20.0, 600.0))
                    .color(title_color),
            )
            .child(
                text(
                    t.text("Choose which servers to block, individually or by region.")
                        .to_string(),
                )
                .font(rune::tok::type_::BODY)
                .color(sub_color)
                .w_full()
                .wrap(),
            )
    };
    let Some(pixels) = s.banners.get(&game.id) else {
        return col().key("game-header").w_full().child(
            row()
                .key("game-header-row")
                .w_full()
                .justify_between()
                .gap(12.0)
                .child(title_block(k.c.text_base, k.c.text_muted))
                .child(ping(rune_kit::ButtonVariant::Outline)),
        );
    };
    // Banner art as a backdrop: the game name sits on the art.
    col().key("game-header").w_full().child(
        el().key("game-hero")
            .w_full()
            .h(180.0)
            .radius(10.0)
            .clip_rounded(10.0)
            .child(
                el().key("game-banner")
                    .w_full()
                    .h_full()
                    .pixels_fit(pixels, rune::SurfaceFit::Fill),
            )
            .child(
                el().key("game-hero-scrim")
                    .absolute(0.0, 0.0)
                    .w_full()
                    .h_full()
                    .gradient(rune::Color::TRANSPARENT, k.c.black.alpha(0.85)),
            )
            .child(
                row()
                    .key("game-hero-text")
                    .absolute(0.0, 0.0)
                    .w_full()
                    .h_full()
                    .padding(16.0)
                    .justify_between()
                    .items_end()
                    .gap(12.0)
                    .child(title_block(k.c.white, k.c.white.alpha(0.78)))
                    .child(ping(rune_kit::ButtonVariant::Contrast)),
            ),
    )
}

fn areas(
    s: &AppState,
    cx: &mut Cx<AppState>,
    game: &Game,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) -> El {
    let mut list = col().key("areas").w_full().gap(16.0);
    if game.regions.is_empty() {
        list = list.child(
            col().w_full().center().gap(12.0).child(
                text(t.text("Add your first server IP. Saving selections does not change existing firewall rules; use Apply when ready.").to_string())
                    .font(rune::tok::type_::BODY)
                    .color(k.c.text_muted).w_full().wrap(),
            ).child(
                Button::new(t.text("Add server"))
                    .variant(rune_kit::ButtonVariant::Outline)
                    .disabled(s.busy.is_some())
                    .on_press(|state: &mut AppState| forms::open_target_dialog(state, "Custom"))
                    .build(k).key("add-server-Custom"),
            ),
        );
        return list;
    }
    for area in areas_in_order(&game.regions) {
        list = list.child(area_section(s, cx, game, &area, k, t, layer));
    }
    list
}

fn area_section(
    s: &AppState,
    cx: &mut Cx<AppState>,
    game: &Game,
    area: &str,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) -> El {
    let prefs = &s.status.snapshot.preferences;
    let (keys, _selected, checked) = area_selection(game, area, prefs);
    let busy = s.busy.is_some();
    let collapsed = s.collapsed_areas.contains(&format!("{}:{area}", game.id));
    let mut section = col().key(format!("area-{area}")).gap(8.0);
    let collapse_key = format!("{}:{area}", game.id);
    let label = row()
        .center_y()
        .gap(6.0)
        .label(
            t.text(if collapsed {
                "Expand {region}"
            } else {
                "Collapse {region}"
            })
            .replace("{region}", area),
        )
        .hover(k.c.hover)
        .press(k.c.pressed)
        .on_press(move |state: &mut AppState| {
            let key = collapse_key.clone();
            if state.collapsed_areas.contains(&key) {
                state.collapsed_areas.retain(|x| x != &key);
            } else {
                state.collapsed_areas.push(key);
            }
        })
        .child(
            el().key(format!("area-{area}-chevron"))
                .size(16.0, 16.0)
                .child(
                    rune::icon(
                        if collapsed {
                            &crate::icons::CHEVRON_UP
                        } else {
                            &crate::icons::CHEVRON_DOWN
                        },
                        14.0,
                    )
                    .color(k.c.text_muted),
                ),
        )
        .child(
            text(area.to_string())
                .key(format!("area-{area}-title"))
                .font(rune::TextStyle::new(13.0, 500.0))
                .color(k.c.text_base),
        );
    let mut heading = row().w_full().center_y().gap(6.0).child(label).child(
        el().key(format!("area-{area}-rule"))
            .h(1.0)
            .grow(1.0)
            .fill(k.c.border_muted)
            .a11y_hidden(),
    );
    if s.editing {
        let area = area.to_string();
        let key = format!("add-server-{area}");
        heading = heading.child(
            Button::new(t.text("Add server"))
                .variant(rune_kit::ButtonVariant::Outline)
                .icon(&crate::icons::PLUS)
                .disabled(busy)
                .on_press(move |state: &mut AppState| forms::open_target_dialog(state, &area))
                .build(k)
                .key(key),
        );
    } else {
        let game_id = game.id.clone();
        let area_owned = area.to_string();
        let toggle = Switch::new(checked)
            .label(
                t.text("Block all servers in {region}")
                    .replace("{region}", area),
            )
            .hide_label()
            .disabled(busy || keys.is_empty())
            .on_toggle(move |state: &mut AppState| toggle_area(state, &game_id, &area_owned));
        heading = heading.child(toggle.build(k));
    }
    section = section.child(heading);
    if !collapsed {
        let regions: Vec<_> = game.regions.iter().filter(|r| r.area == area).collect();
        section = section.child(card_grid(s, cx, game, &regions, k, t, layer));
    }
    section
}

fn card_grid(
    s: &AppState,
    cx: &mut Cx<AppState>,
    game: &Game,
    regions: &[&lobbylocker_core::model::Region],
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) -> El {
    if s.compact {
        let mut grid = col().key("card-grid").w_full().gap(8.0);
        for region in regions {
            grid = grid.child(server_card(s, cx, game, region, k, t, layer).w_full());
        }
        return grid;
    }
    // Flex-wrap grid: cards wrap to new rows as width allows, then grow to
    // fill each row (up to 400pt) instead of leaving a gap on the right.
    // `.min_w` is what makes wrapping work with `grow`: grow sets flex-basis
    // 0, so without a minimum every card would "fit" on one line.
    let mut grid = row().key("card-grid").w_full().flex_wrap().gap(8.0);
    for region in regions {
        grid = grid.child(
            server_card(s, cx, game, region, k, t, layer)
                .w(288.0)
                .min_w(240.0)
                .max_w(400.0)
                .grow(1.0),
        );
    }
    grid
}

pub fn toggle_area(s: &mut AppState, game_id: &str, area: &str) {
    let game = match s.status.snapshot.games.iter().find(|g| g.id == game_id) {
        Some(g) => g.clone(),
        None => return,
    };
    let (keys, _, checked) = area_selection(&game, area, &s.status.snapshot.preferences);
    let previous = s.status.snapshot.preferences.clone();
    let mut prefs = previous.clone();
    prefs.blocked_regions = toggle_keys(&prefs.blocked_regions, &keys, !checked);
    if save_preferences(s, prefs) {
        write_blocks(s, Some(previous));
    }
}

pub fn toggle_region(s: &mut AppState, game_id: &str, region_id: &str) {
    let game = match s.status.snapshot.games.iter().find(|g| g.id == game_id) {
        Some(g) => g.clone(),
        None => return,
    };
    let region = match game.regions.iter().find(|r| r.id == region_id) {
        Some(r) => r.clone(),
        None => return,
    };
    let key = region_key(game_id, region_id);
    let checked = region_checked(&game, &region, &s.status.snapshot.preferences);
    let previous = s.status.snapshot.preferences.clone();
    let mut prefs = previous.clone();
    prefs.blocked_regions = toggle_keys(&prefs.blocked_regions, &[key], !checked);
    if save_preferences(s, prefs) {
        write_blocks(s, Some(previous));
    }
}

/// The ported readback's label for a server card ("Blocked" / "Not blocked"
/// / edge cases), from the locale. Server cards always carry it; the sidebar
/// only shows a line when the game is blocked.
fn block_description(
    s: &AppState,
    game: &Game,
    region: &lobbylocker_core::model::Region,
    t: &lobbylocker_core::i18n::Translator,
) -> String {
    if region.endpoints.is_empty() {
        return t.text("No targets configured").to_string();
    }
    if s.status.applied_count.is_none() {
        return t.text("Status unavailable").to_string();
    }
    if s.status.applied_count == Some(0) {
        return t.text("Not blocked").to_string();
    }
    match s
        .status
        .region_states
        .get(&region_key(&game.id, &region.id))
    {
        Some(
            lobbylocker_core::inspection::RegionState::Blocked
            | lobbylocker_core::inspection::RegionState::Partial,
        ) => t.text("Blocked").to_string(),
        Some(lobbylocker_core::inspection::RegionState::None) => t.text("Not blocked").to_string(),
        _ => t.text("Status unavailable").to_string(),
    }
}

fn server_card(
    s: &AppState,
    _cx: &mut Cx<AppState>,
    game: &Game,
    region: &lobbylocker_core::model::Region,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    _layer: &mut Vec<El>,
) -> El {
    let prefs = &s.status.snapshot.preferences;
    let checked = region_checked(game, region, prefs);
    let busy = s.busy.is_some();
    let mut card = row()
        .key(format!("server-{}", region.id))
        .w_full()
        .px(12.0)
        .py(10.0)
        .center_y()
        .gap(10.0)
        .radius(10.0)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_muted);
    let sample = s.status.samples.get(&region_key(&game.id, &region.id));
    let state_text = block_description(s, game, region, t);
    // min_w(0) lets the column shrink below the name's intrinsic width so
    // truncate() ellipsizes instead of pushing the switch out of the card.
    let mut info = col().grow(1.0).min_w(0.0).min_h(0.0).gap(2.0).child(
        text(region.name.clone())
            .font(TextStyle::new(13.0, 500.0))
            .color(k.c.text_base)
            .truncate(),
    );
    // The status label and the ping result share a row under the name.
    let mut sub = row().gap(8.0).center_y().child(
        text(state_text)
            .font(rune::tok::type_::LABEL)
            .color(k.c.text_muted),
    );
    if let Some(sample) = sample {
        sub = sub.child(
            text(ping_description(sample, t))
                .font(rune::tok::type_::LABEL)
                .color(if sample.latency_ms.is_some() {
                    k.c.text_base
                } else {
                    k.c.text_muted
                }),
        );
    }
    info = info.child(sub);
    card = card.child(info);
    if s.editing {
        let gid = game.id.clone();
        let rid = region.id.clone();
        card = card.child(
            Button::new(t.text("Edit"))
                .variant(rune_kit::ButtonVariant::Outline)
                .disabled(busy)
                .on_press(move |state: &mut AppState| {
                    state.editing_region = Some(format!("{gid}:{rid}"));
                    state.adding_area = None;
                    state.error.clear();
                    state.region_name = rune::TextEdit::new(
                        state
                            .status
                            .snapshot
                            .games
                            .iter()
                            .find(|g| g.id == gid)
                            .and_then(|g| g.regions.iter().find(|r| r.id == rid))
                            .map(|r| r.name.clone())
                            .unwrap_or_default(),
                    );
                    state.probe_target = rune::TextEdit::new(
                        state
                            .status
                            .snapshot
                            .games
                            .iter()
                            .find(|g| g.id == gid)
                            .and_then(|g| g.regions.iter().find(|r| r.id == rid))
                            .and_then(|r| r.probe_target.clone())
                            .unwrap_or_default(),
                    );
                    let _ = &gid;
                })
                .build(k)
                .key(format!("edit-server-{}", region.id)),
        );
    } else {
        let gid = game.id.clone();
        let rid = region.id.clone();
        card = card.child(
            Switch::new(checked)
                .label(format!("Block {region_name}", region_name = region.name))
                .hide_label()
                .disabled(busy || !region_available(region))
                .on_toggle(move |state: &mut AppState| toggle_region(state, &gid, &rid))
                .build(k),
        );
    }
    card
}

fn empty_state(s: &AppState, k: &Kit, t: &lobbylocker_core::i18n::Translator) -> El {
    if !library_candidates(s).is_empty() {
        return col().w_full().grow(1.0).min_h(0.0).center().child(
            text(t.text("Select a game from the sidebar.").to_string())
                .font(rune::tok::type_::BODY)
                .color(k.c.text_muted),
        );
    }
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .center()
        .gap(12.0)
        .child(
            text(
                t.text(if s.scanned {
                    "Your game library is empty"
                } else {
                    "Detecting games…"
                })
                .to_string(),
            )
            .font(rune::tok::type_::TITLE)
            .color(k.c.text_base),
        )
        .child(
            text(
                t.text("Scan for installed games or import a game in Settings.")
                    .to_string(),
            )
            .font(rune::tok::type_::BODY)
            .color(k.c.text_muted),
        )
        .child(
            Button::new(t.text("Scan games"))
                .variant(rune_kit::ButtonVariant::Outline)
                .disabled(s.scan_busy)
                .on_press(|state: &mut AppState| {
                    state.error.clear();
                    request_scan(state);
                })
                .build(k),
        )
}
