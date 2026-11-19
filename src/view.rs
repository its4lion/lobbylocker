//! App shell (Hex-style): a `SplitView` with a left `Sidebar` (games +
//! view switcher) and a main pane (Toolbar + page + footer bar).
//! Pages live in their own modules.

use crate::state::*;
use rune::{col, el, row, text, Cx, El, ScrollSpec};
use rune_kit::shell::{SplitView, Toolbar};
use rune_kit::{Button, IconButton, Kit};

const BRAND_LOCKUP_ON_DARK: &[u8] =
    include_bytes!("../packaging/brand/lobbylocker-lockup-on-dark.svg");
const BRAND_LOCKUP_ON_LIGHT: &[u8] =
    include_bytes!("../packaging/brand/lobbylocker-lockup-on-light.svg");

/// The lockup art is 9245×2749; draw it at the same aspect ratio.
const BRAND_LOCKUP_ASPECT: f32 = 9245.0 / 2749.0;

fn brand_lockup(s: &AppState, k: &Kit, scale: f32) -> El {
    let height = 24.0;
    let width = height * BRAND_LOCKUP_ASPECT;
    if let Some(cache) = &s.image_cache {
        let bytes = if s.dark {
            BRAND_LOCKUP_ON_DARK
        } else {
            BRAND_LOCKUP_ON_LIGHT
        };
        let image = cache.svg(std::sync::Arc::<[u8]>::from(bytes), width, scale.max(1.0));
        return el()
            .key("brand-lockup")
            .pixels(image.pixels())
            .size(width, height);
    }
    // Tests render before `setup` installs the image cache; keep a text mark
    // there without changing the real lockup.
    row()
        .key("brand-lockup")
        .h(height)
        .center_y()
        .gap(8.0)
        .child(
            el().size(24.0, 24.0)
                .radius(6.0)
                .fill(rune::tok::accent(0, s.dark))
                .center()
                .child(
                    text("L")
                        .font(rune::tok::type_::TITLE)
                        .color(rune::Color::WHITE),
                ),
        )
        .child(
            text("LobbyLocker")
                .font(rune::TextStyle::new(14.0, 600.0))
                .color(k.c.text_base),
        )
}

pub fn kit(s: &AppState) -> &'static Kit {
    rune_theme_opencode::kit(s.dark)
}

pub fn theme(s: &AppState) -> rune::tok::Theme {
    rune::tok::theme(s.dark)
}

pub fn tr(s: &AppState) -> lobbylocker_core::i18n::Translator {
    s.locales
        .translator(&s.status.snapshot.preferences.language)
}

fn tab_pills(s: &AppState, k: &Kit, t: &lobbylocker_core::i18n::Translator) -> El {
    let tab_button = |tab: Tab, key: &'static str, s: &AppState| {
        let active = s.tab == tab;
        let label = if tab == Tab::Regions {
            t.text("Regions")
        } else if tab == Tab::Connections {
            t.text("Connections")
        } else {
            t.text("Settings")
        };
        row()
            .key(format!("tab-{key}"))
            .h(28.0)
            .px(12.0)
            .center_y()
            .radius(8.0)
            .fill(if active {
                k.c.layer_02
            } else {
                rune::Color::TRANSPARENT
            })
            .child(
                text(label.to_string())
                    .font(rune::TextStyle::new(
                        13.0,
                        if active { 600.0 } else { 400.0 },
                    ))
                    .color(if active {
                        k.c.text_base
                    } else {
                        k.c.text_muted
                    }),
            )
            .hover(k.c.hover)
            .press(k.c.pressed)
            .on_press(move |state: &mut AppState| {
                state.tab = tab;
                state.error.clear();
            })
    };
    row()
        .key("tabs")
        .gap(2.0)
        .padding(3.0)
        .radius(10.0)
        .fill(k.c.layer_01)
        .child(tab_button(Tab::Regions, "regions", s))
        .child(tab_button(Tab::Connections, "connections", s))
        .child(tab_button(Tab::Settings, "settings", s))
}

pub fn view(s: &AppState, cx: &mut Cx<AppState>) -> El {
    let k = kit(s);
    let t = tr(s);
    let mut layer: Vec<El> = Vec::new();
    let sidebar_state = s.sidebar_state;
    let mut split = SplitView::new(main_pane(s, cx, k, &t, &mut layer))
        .toolbar(Toolbar::new().trailing(header_controls(s, k, &t, &mut layer)))
        .titlebar_inset(cx.titlebar_inset())
        .hide_sidebar_toggle();
    if !s.compact {
        split = split.sidebar(
            &sidebar_state,
            |state: &mut AppState| &mut state.sidebar_state,
            crate::sidebar::game_library(s, k, &t),
        );
    }
    let split = split.build(k);
    let mut overlays = layer;
    let brand = row()
        .key("brand-overlay")
        .pin(Some(cx.titlebar_inset() + 12.0), Some(0.0), None, None)
        .h(48.0)
        .center_y()
        .child(brand_lockup(s, k, cx.env().scale));
    overlays.push(brand);
    if !s.compact {
        overlays.push(
            row()
                .key("tabs-overlay")
                .pin(Some(0.0), Some(0.0), Some(0.0), None)
                .h(48.0)
                .center()
                .child(tab_pills(s, k, &t)),
        );
    }
    if s.reset_open {
        crate::dialogs::reset_confirm(s, k, &t, &mut overlays);
    }
    if s.custom_open {
        crate::dialogs::custom_game(s, k, &t, &mut overlays);
    }
    crate::updates::dialog(s, k, &t, &mut overlays);
    if let Some(game) = s
        .status
        .snapshot
        .games
        .iter()
        .find(|game| game.id == s.selected)
    {
        if let Some(area) = &s.adding_area {
            crate::forms::target_dialog(s, game, area, k, &t, &mut overlays);
        }
        if let Some(key) = &s.editing_region {
            if let Some(region) = game
                .regions
                .iter()
                .find(|region| *key == format!("{}:{}", game.id, region.id))
            {
                crate::forms::server_details_dialog(s, game, region, k, &t, &mut overlays);
            }
        }
    }
    // Every locale lays out left to right; the language only changes the text.
    // Setting it at the root also forces each empty field's placeholder to
    // paint from the left edge (Rune anchors an auto-detected RTL placeholder
    // past the field's left edge, which would clip it out of the box).
    rune_kit::root(split, overlays)
        .dir(rune::Direction::Ltr)
        .on_layout(|state: &mut AppState, rect| {
            observe_window_layout(state, rect);
        })
}

fn header_controls(
    s: &AppState,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) -> El {
    let compact = s.compact;
    let theme_button = IconButton::new(if s.dark {
        &crate::icons::MOON
    } else {
        &crate::icons::SUN
    })
    .variant(rune_kit::IconButtonVariant::Ghost)
    .label(t.text("Toggle theme"))
    .on_press(|state: &mut AppState| state.dark = !state.dark)
    .build(k);
    if compact {
        return row().gap(6.0).center_y().child(theme_button);
    }
    let locales: Vec<String> = s.locales.locales.iter().map(|l| l.name.clone()).collect();
    let codes: Vec<String> = s.locales.locales.iter().map(|l| l.code.clone()).collect();
    let current = codes
        .iter()
        .position(|c| c == &s.status.snapshot.preferences.language)
        .and_then(|i| locales.get(i).cloned())
        .unwrap_or_else(|| "English".to_string());
    let language = rune_kit::Select::new("language-header", &locales)
        .label(t.text("Language"))
        .width(140.0)
        .current(Some(&current))
        .open(s.language_open)
        .on_toggle(|state: &mut AppState| {
            state.language_open = !state.language_open;
            state.settings_language_open = false;
        })
        .on_close(|state: &mut AppState| state.language_open = false)
        .on_choose(
            move |state: &mut AppState, choice: &str, _tx: &mut rune::Tx| {
                state.language_open = false;
                if let Some(code) = state
                    .locales
                    .locales
                    .iter()
                    .find(|l| l.name == choice)
                    .map(|l| l.code.clone())
                {
                    match state.engine.set_language(&code) {
                        Ok(()) => {
                            state.error.clear();
                            reload(state);
                        }
                        Err(e) => state.error = e,
                    }
                }
            },
        )
        .build(k, layer);
    row()
        .gap(6.0)
        .center_y()
        .child(language)
        .child(theme_button)
}

fn main_pane(
    s: &AppState,
    cx: &mut Cx<AppState>,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) -> El {
    let content = match s.tab {
        Tab::Regions => crate::regions::regions_page(s, cx, k, t, layer),
        Tab::Connections => crate::connections::connections_page(s, cx, k, t),
        Tab::Settings => crate::settings::settings_page(s, cx, k, t, layer),
    };
    let mut pane = col().key("main-pane").w_full().h_full().min_h(0.0);
    if s.compact {
        pane = pane.child(compact_game_picker(s, cx, k, t, layer));
    }
    pane.child(content).child(footer(s, k, t))
}

fn compact_game_picker(
    s: &AppState,
    cx: &mut Cx<AppState>,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) -> El {
    let games = library_candidates(s);
    let disabled = games.is_empty() || s.busy.is_some();
    let selected = games.iter().find(|game| game.id == s.selected);
    let name = |game: &LibraryCandidate| {
        if games.iter().filter(|other| other.name == game.name).count() > 1 {
            format!("{} ({})", game.name, game.id)
        } else {
            game.name.clone()
        }
    };
    let current = selected.map(name);
    let banner = selected.and_then(|game| s.banners.get(&game.id));
    let on_banner = banner.is_some();
    let mut value = row().grow(1.0).shrink().min_w(0.0).gap(8.0).center_y();
    let label = text(current.as_deref().unwrap_or(t.text("Select a game")))
        .font(k.ty.ui_compact)
        .color(if on_banner {
            k.c.white
        } else if selected.is_some() {
            k.c.text_base
        } else {
            k.c.text_faint
        })
        .grow(1.0)
        .shrink()
        .truncate()
        .a11y_hidden();
    if let Some(game) = selected {
        let artwork = crate::sidebar::game_icon(s, game, k, 24.0)
            .key(format!("compact-selected-icon-{}", game.id))
            .a11y_hidden();
        value = value.child(artwork).child(label);
    } else {
        value = value.child(label);
    }
    let chevron = rune::icon(
        if s.compact_game_open {
            k.glyph.chevron_up
        } else {
            k.glyph.chevron_down
        },
        16.0,
    )
    .color(if on_banner {
        k.c.white.alpha(0.85)
    } else {
        k.c.icon_muted
    })
    .a11y_hidden();
    let trigger = row()
        .key("compact-game-trigger")
        .role("combobox")
        .label(t.text("Select a game"))
        .a11y_value(current.unwrap_or_default())
        .expanded(s.compact_game_open)
        .disabled(disabled)
        .focusable(!disabled)
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .h(36.0)
        .px(6.0)
        .gap(6.0)
        .center_y()
        .radius(k.radius.md)
        .fill(if on_banner {
            rune::Color::TRANSPARENT
        } else {
            k.c.layer_01
        })
        .hover(k.c.hover);
    let trigger = if let Some(pixels) = banner {
        trigger
            .clip_rounded(k.radius.md)
            .child(
                el().key("compact-banner-backdrop")
                    .absolute(0.0, 0.0)
                    .w_full()
                    .h_full()
                    .pixels_fit(pixels, rune::SurfaceFit::Fill),
            )
            .child(
                el().key("compact-banner-scrim")
                    .absolute(0.0, 0.0)
                    .w_full()
                    .h_full()
                    .fill(k.c.black.alpha(0.45)),
            )
    } else {
        trigger
    };
    let trigger = trigger.child(value).child(chevron);
    let picker = if disabled {
        trigger.layer_opacity(0.5)
    } else {
        trigger.on_press(|state: &mut AppState| state.compact_game_open = !state.compact_game_open)
    };

    if s.compact_game_open && !disabled {
        let mut rows = col()
            .key("compact-game-rows")
            .w_full()
            .padding(4.0)
            .items_stretch();
        for (index, game) in games.iter().enumerate() {
            let id = game.id.clone();
            let chosen = game.id == s.selected;
            let artwork = crate::sidebar::game_icon(s, game, k, 24.0)
                .key(format!("compact-option-icon-{}", game.id))
                .a11y_hidden();
            let label = text(name(game))
                .font(k.ty.ui_compact)
                .color(k.c.text_base)
                .grow(1.0)
                .shrink()
                .truncate()
                .a11y_hidden();
            let mark = rune::icon(k.glyph.check_small, 14.0)
                .color(k.c.icon_base)
                .opacity(if chosen { 1.0 } else { 0.0 })
                .a11y_hidden();
            let option = row()
                .key(format!("compact-game-option-{}", game.id))
                .role("option")
                .label(name(game))
                .position_in_set(index + 1, games.len())
                .focusable(true)
                .focus_selected(chosen)
                .focus_highlight(k.c.hover)
                .w_full()
                .h(36.0)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .radius(4.0);
            let option = option.child(artwork).child(label).child(mark);
            rows = rows.child(option.on_press(move |state: &mut AppState| {
                state.compact_game_open = false;
                crate::sidebar::select_game(state, &id);
            }));
        }
        layer.push(rune_kit::scrim(
            "compact-game-scrim",
            |state: &mut AppState| {
                state.compact_game_open = false;
            },
        ));
        let scroll = col()
            .key("compact-game-scroll")
            .w_full()
            .min_h(0.0)
            .shrink()
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(rows);
        let list = col()
            .key("compact-game-listbox")
            .role("listbox")
            .label(t.text("Select a game"))
            .focus_group()
            .max_h(260.0)
            .min_h(0.0)
            .shrink()
            .child(k.scroll_bar(scroll).w_full().shrink().min_h(0.0));
        // Follow the actual trigger width, including when a compact window is
        // manually resized while its menu is open.
        let width = cx
            .layout_with("compact-game-trigger", |rect| rect[2])
            .unwrap_or(1.0);
        let popup = col()
            .key("compact-game-content")
            .label(t.text("Select a game"))
            .focus_return("compact-game-trigger")
            .w(width)
            .min_h(0.0)
            .radius(6.0)
            .fill(k.c.layer_01)
            .child(list)
            .modal(true, |state: &mut AppState| state.compact_game_open = false);
        let popup = k
            .shadow_layers(&k.elevation.floating)
            .fold(popup, |el, shadow| el.box_shadow(shadow))
            .anchored_fit(
                "compact-game-trigger",
                rune::Side::Bottom,
                rune::Align::Start,
                2.0,
            )
            .instant_changes()
            .motion_intent(rune::MotionIntent::CUT);
        layer.push(popup);
    }
    let scan_label = t.text(if s.scan_busy {
        "Scanning games…"
    } else {
        "Scan games"
    });
    let scan = IconButton::new(&crate::icons::REFRESH)
        .variant(rune_kit::IconButtonVariant::Ghost)
        .label(scan_label)
        .help(scan_label)
        .disabled(s.scan_busy)
        .on_press(|state: &mut AppState| {
            state.error.clear();
            request_scan(state);
        })
        .build(k)
        .key("compact-scan-games");
    let picker_row = row()
        .key("compact-picker")
        .w_full()
        .px(16.0)
        .pt(12.0)
        .gap(8.0)
        .center_y();
    picker_row.child(picker).child(scan)
}

fn footer(s: &AppState, k: &Kit, t: &lobbylocker_core::i18n::Translator) -> El {
    let bar = action_bar(&s.status);
    let busy = s.busy.is_some();
    let mut bar_row = row()
        .key("footer")
        .w_full()
        .px(16.0)
        .py(10.0)
        .center_y()
        .flex_wrap()
        .justify_between()
        .gap(8.0);
    let show = busy || !s.error.is_empty() || !s.firewall_error.is_empty() || bar.visible;
    if show {
        bar_row = bar_row.child(
            text(footer_message(s, t))
                .key("footer-msg")
                .font(rune::tok::type_::LABEL)
                .color(k.c.text_muted)
                .wrap(),
        );
        if bar.apply && !busy {
            bar_row = bar_row.child(
                Button::new(t.format(
                    "pending_changes_count",
                    &[("count", &bar.count.to_string())],
                ))
                .variant(rune_kit::ButtonVariant::Submit)
                .on_press(|state: &mut AppState| {
                    state.busy = Some("apply".to_string());
                    crate::state::request_apply(state);
                })
                .build(k),
            );
        }
        let retryable =
            verification_issue_retryable(s.status.verification_issue.as_ref().map(|i| match i {
                lobbylocker_core::inspection::VerificationIssue::ManagedByDeviceManagement => {
                    "managedByDeviceManagement"
                }
                lobbylocker_core::inspection::VerificationIssue::FirewallUnavailable => {
                    "firewallUnavailable"
                }
                lobbylocker_core::inspection::VerificationIssue::UnsupportedFilters => {
                    "unsupportedFilters"
                }
            }));
        if !s.firewall_error.is_empty() && retryable && !busy {
            bar_row = bar_row.child(
                Button::new(t.text("Retry"))
                    .variant(rune_kit::ButtonVariant::Outline)
                    .on_press(|state: &mut AppState| {
                        state.busy = Some("read".to_string());
                        crate::state::request_verify(state, true);
                    })
                    .build(k),
            );
        }
    }
    if s.compact {
        let reset = Button::new(t.text("Reset"))
            .variant(rune_kit::ButtonVariant::Outline)
            .icon(&crate::icons::REFRESH)
            .disabled(busy)
            .on_press(|state: &mut AppState| state.reset_open = true)
            .build(k)
            .key("compact-reset");
        let version = text(format!("LobbyLocker v{}", env!("CARGO_PKG_VERSION")))
            .font(rune::tok::type_::LABEL)
            .color(k.c.text_faint);
        let mut footer = col().key("compact-footer").w_full();
        if show {
            footer = footer.child(bar_row);
        }
        let actions = row()
            .w_full()
            .px(16.0)
            .pb(12.0)
            .pt(8.0)
            .center_y()
            .justify_between()
            .gap(8.0);
        footer
            .child(actions.child(reset).child(version))
            // The sidebar is hidden in the compact layout, so the update marker
            // and the way out live here instead.
            .child(compact_release_row(s, k, t))
    } else {
        bar_row
    }
}

/// The compact layout's release row: the update marker and the quit button.
fn compact_release_row(s: &AppState, k: &Kit, t: &lobbylocker_core::i18n::Translator) -> El {
    let quit = Button::new(t.text("Quit LobbyLocker"))
        .variant(rune_kit::ButtonVariant::Ghost)
        .on_press_tx(|_state: &mut AppState, tx: &mut rune::Tx| tx.quit())
        .build(k)
        .key("quit-button");
    let update = crate::updates::button(s, k, t).map(|button| {
        row()
            .gap(6.0)
            .center_y()
            .children(s.update_notice.as_ref().map(|_| {
                el().key("update-dot")
                    .size(8.0, 8.0)
                    .radius(4.0)
                    .fill(k.c.text_accent)
            }))
            .child(button)
    });
    let row = row()
        .w_full()
        .px(16.0)
        .pb(12.0)
        .gap(8.0)
        .center_y()
        .justify_between();
    let row = match update {
        Some(update) => row.child(update),
        None => row,
    };
    row.child(quit)
}

fn footer_message(s: &AppState, t: &lobbylocker_core::i18n::Translator) -> String {
    if let Some(label) = &s.busy {
        return match label.as_str() {
            "apply" => t
                .text("Applying… Check the administrator prompt.")
                .to_string(),
            "reset" => t
                .text("Resetting… Check the administrator prompt.")
                .to_string(),
            "read" => t.text("Reading firewall…").to_string(),
            "ping" => t.text("Checking ping…").to_string(),
            "scan" => t.text("Detecting games…").to_string(),
            "connections" => t.text("Scanning connections…").to_string(),
            "refresh" => t.text("Refreshing the official list…").to_string(),
            _ => t.text("Working…").to_string(),
        };
    }
    if !s.error.is_empty() {
        return s.error.clone();
    }
    if !s.firewall_error.is_empty() {
        return s.firewall_error.clone();
    }
    let bar = action_bar(&s.status);
    if bar.apply {
        return t
            .format(
                "pending_changes_count",
                &[("count", &bar.count.to_string())],
            )
            .to_string();
    }
    t.format("active_rules_count", &[("count", &bar.count.to_string())])
        .to_string()
}

pub fn scroll_content(body: El) -> El {
    body.scroll(ScrollSpec {
        stick_bottom: false,
    })
    .scroll_edge_fade(16.0)
    .grow(1.0)
    .min_h(0.0)
    .px(24.0)
    .pt(20.0)
    .pb(20.0)
}
