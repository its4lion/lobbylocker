//! Sidebar game library (Hex-style): search header, avatar rows with
//! block-state subtitles, a scan action, and version footer.

use crate::state::*;
use rune::{col, el, row, text, El};
use rune_kit::shell::Sidebar;
use rune_kit::{Button, ControlSize, IconButton, IconButtonVariant, Kit};

pub fn game_library(s: &AppState, k: &Kit, t: &lobbylocker_core::i18n::Translator) -> El {
    let games = library_candidates(s);
    let query = s.search.text.trim().to_lowercase();
    let visible: Vec<_> = games
        .iter()
        .filter(|g| query.is_empty() || g.name.to_lowercase().contains(&query))
        .collect();

    let search = rune_kit::TextInput::new(&s.search)
        .placeholder(t.text("Search games…"))
        .leading_icon(&crate::icons::SEARCH)
        .leading_icon_size(14.0)
        .width(None)
        .on_edit(|state: &mut AppState| &mut state.search)
        .build(k)
        .key("search-games")
        .grow(1.0)
        .shrink()
        .min_w(0.0);

    let scan_label = t.text(if s.scan_busy {
        "Scanning games…"
    } else {
        "Scan games"
    });
    let scan = IconButton::new(&crate::icons::REFRESH)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .label(scan_label)
        .help(scan_label)
        .disabled(s.scan_busy)
        .on_press(|state: &mut AppState| {
            state.error.clear();
            request_scan(state);
        })
        .build(k)
        .key("scan-games");
    let search_row = row().w_full().center_y().gap(4.0).child(search).child(scan);

    let mut sidebar = Sidebar::new()
        .label(t.text("Games"))
        .row_gap(2.0)
        .header(search_row);

    if games.is_empty() {
        sidebar = sidebar.child(
            col().w_full().center().gap(8.0).child(
                text(
                    t.text(if s.scanned {
                        "No games found."
                    } else {
                        "Detecting games…"
                    })
                    .to_string(),
                )
                .font(rune::tok::type_::BODY)
                .color(k.c.text_muted),
            ),
        );
    } else if visible.is_empty() {
        sidebar = sidebar.child(
            text(t.text("No games match your search.").to_string())
                .font(rune::tok::type_::BODY)
                .color(k.c.text_muted),
        );
    } else {
        for game in &visible {
            let active = game.id == s.selected;
            let id = game.id.clone();
            // Only a blocked game carries a status line; an unblocked one
            // shows its name alone.
            let subtitle = (game_block_state(&game.id, &s.status) == Some("blocked"))
                .then(|| t.text("Blocked").to_string());
            let name = game_display_name(&game.id, &game.name, t);
            let label = match &subtitle {
                Some(subtitle) => format!("{name}, {subtitle}"),
                None => name.clone(),
            };
            let mut item = row()
                .key(format!("game-{}", game.id))
                .label(label)
                .w_full()
                .h(48.0)
                .px(8.0)
                .gap(10.0)
                .center_y()
                .radius(k.radius.md)
                .motion(k.motion.control)
                .focusable(true)
                .focus_selected(active)
                .child(game_icon(s, game, k, 32.0).a11y_hidden())
                .child(
                    col()
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0)
                        .child(
                            text(name)
                                .key(format!("game-name-{}", game.id))
                                .font(rune::TextStyle::new(13.0, 500.0))
                                .color(k.c.text_base)
                                .truncate()
                                .a11y_hidden(),
                        )
                        .children(subtitle.map(|subtitle| {
                            text(subtitle)
                                .key(format!("game-status-{}", game.id))
                                .font(k.ty.caption())
                                .color(k.c.text_faint)
                                .truncate()
                                .a11y_hidden()
                        })),
                );
            item = k.interactive(item);
            if active {
                item = item.fill(k.c.pressed);
            }
            sidebar = sidebar.child(
                item.on_press(move |state: &mut AppState| select_game(state, &id))
                    .role("listitem"),
            );
        }
    }

    let align = |content: El| row().w_full().child(content);
    let version = text(format!("LobbyLocker v{}", env!("CARGO_PKG_VERSION")))
        .key("sidebar-version")
        .font(k.ty.caption())
        .color(k.c.text_faint);
    // A dot marks that a newer build is waiting.
    let update_ready = s.update_notice.is_some();
    // The row sizes to its content, so the footer can align it to the
    // leading edge.
    let mut version_row = row().gap(6.0).center_y().child(version);
    if update_ready {
        version_row = version_row.child(
            el().key("update-dot")
                .size(8.0, 8.0)
                .radius(4.0)
                .fill(k.c.text_accent),
        );
    }
    let mut footer = col()
        .key("sidebar-footer-content")
        .w_full()
        .gap(4.0)
        .child(el().w_full().h(1.0).fill(k.c.border_muted).a11y_hidden())
        .child(align(version_row).px(8.0).pt(8.0));
    if let Some(update) = crate::updates::button(s, k, t) {
        footer = footer.child(align(update).px(8.0));
    }
    footer = footer.child(
        align(
            Button::new(t.text("Quit LobbyLocker"))
                .variant(rune_kit::ButtonVariant::Ghost)
                .on_press_tx(|_state: &mut AppState, tx: &mut rune::Tx| tx.quit())
                .build(k)
                .key("quit-button"),
        )
        .px(8.0),
    );

    sidebar.footer(footer).build(k)
}

pub fn select_game(s: &mut AppState, id: &str) {
    if !s.status.snapshot.games.iter().any(|game| game.id == id) {
        let Some(game) = s.scan.games.iter().find(|game| game.id == id) else {
            return;
        };
        let game = match lobbylocker_core::installed::empty_game(game.id.clone(), &game.name) {
            Ok(game) => game,
            Err(error) => {
                s.error = error;
                return;
            }
        };
        if let Err(error) = s.engine.save_game(&game) {
            s.error = error;
            return;
        }
        reload(s);
    }
    s.selected = id.to_string();
    s.tab = Tab::Regions;
    s.error.clear();
    s.editing = false;
    s.editing_region = None;
    s.adding_area = None;
    if !rune::is_headless() {
        request_fresh(s, id.to_string());
    }
}

/// Extracted executable icon when the runtime has decoded it, else a
/// rounded tile with the game's initial.
pub(crate) fn game_icon(s: &AppState, game: &LibraryCandidate, k: &Kit, size: f32) -> El {
    if let Some(pixels) = s.custom_icons.get(&game.id) {
        return el()
            .key(format!("icon-{}", game.id))
            .pixels_fit(pixels, rune::SurfaceFit::Fit)
            .size(size, size)
            .clip_rounded(6.0);
    }
    let initial = game.name.trim().chars().next().unwrap_or('?').to_string();
    el().key(format!("icon-{}", game.id))
        .size(size, size)
        .radius(6.0)
        .fill(k.c.layer_03)
        .center()
        .child(
            text(initial)
                .font(rune::tok::type_::LABEL)
                .color(k.c.text_base),
        )
}
