//! Dialogs: reset confirmation and custom game creation.

use crate::state::*;
use rune::{col, row, text, text_input, El, TextStyle};
use rune_kit::{Button, Dialog, Kit};

pub fn reset_confirm(
    s: &AppState,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) {
    let _ = s;
    Dialog::new("reset", t.text("Reset"))
        .open(true)
        .fit()
        .width(440.0)
        .on_close(|state: &mut AppState| state.reset_open = false)
        .child(
            col()
                .w_full()
                .px(16.0)
                .pb(16.0)
                .gap(16.0)
                .child(
                    text(
                        t.text("Remove only LobbyLocker-owned firewall rules? Other firewall rules are untouched.")
                            .to_string(),
                    )
                    .key("reset-description")
                    .font(rune::tok::type_::BODY)
                    .color(k.c.text_muted)
                    .w_full()
                    .wrap(),
                )
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .justify_end()
                        .child(
                            Button::new(t.text("Cancel"))
                                .variant(rune_kit::ButtonVariant::Ghost)
                                .on_press(|state: &mut AppState| state.reset_open = false)
                                .build(k),
                        )
                        .child(
                            Button::new(t.text("Reset"))
                                .variant(rune_kit::ButtonVariant::Danger)
                                .on_press(|state: &mut AppState| {
                                    state.reset_open = false;
                                    state.busy = Some("reset".to_string());
                                    request_reset(state);
                                })
                                .build(k),
                        ),
                ),
        )
        .build(k, layer);
}

pub fn custom_game(
    s: &AppState,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) {
    Dialog::new("custom", t.text("Add custom game"))
        .open(true)
        .fit()
        .width(460.0)
        .on_close(|state: &mut AppState| state.custom_open = false)
        .child(
            col()
                .w_full()
                .padding(16.0)
                .gap(10.0)
                .child(
                    text(
                        t.text("Name the game and add its first server address.")
                            .to_string(),
                    )
                    .font(rune::tok::type_::BODY)
                    .color(k.c.text_muted)
                    .w_full()
                    .wrap(),
                )
                .child(
                    text_input(&s.custom_name_edit, TextStyle::new(13.0, 400.0))
                        .key("custom-name")
                        .single_line()
                        .placeholder(t.text("Game name").to_string(), k.c.text_faint)
                        .on_edit(|state: &mut AppState| &mut state.custom_name_edit)
                        .grow(1.0),
                )
                .child(
                    text_input(&s.custom_addr_edit, TextStyle::new(13.0, 400.0))
                        .key("custom-addr")
                        .single_line()
                        .placeholder(t.text("Server IP or CIDR").to_string(), k.c.text_faint)
                        .on_edit(|state: &mut AppState| &mut state.custom_addr_edit)
                        .grow(1.0),
                )
                .child(
                    Button::new(t.text("Choose executable (optional)"))
                        .variant(rune_kit::ButtonVariant::Outline)
                        .on_press_tx(|_state: &mut AppState, tx: &mut rune::Tx| {
                            let opts = rune::FileDialogOptions {
                                title: "Choose game executable".to_string(),
                                directory: None,
                                file_name: None,
                                filters: vec![(
                                    "Game executable".to_string(),
                                    vec!["exe".to_string()],
                                )],
                            };
                            tx.open_file(opts, |state: &mut AppState, result, _tx| match result {
                                Ok(Some(path)) => {
                                    if let Some(p) = path.to_str() {
                                        state.custom_exe = Some(p.to_string());
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => state.error = e.to_string(),
                            });
                        })
                        .build(k),
                )
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .justify_end()
                        .child(
                            Button::new(t.text("Cancel"))
                                .variant(rune_kit::ButtonVariant::Ghost)
                                .on_press(|state: &mut AppState| state.custom_open = false)
                                .build(k),
                        )
                        .child(
                            Button::new(t.text("Create game"))
                                .variant(rune_kit::ButtonVariant::Submit)
                                .on_press(|state: &mut AppState| create_custom_game(state))
                                .build(k),
                        ),
                ),
        )
        .build(k, layer);
}

fn create_custom_game(s: &mut AppState) {
    let name = s.custom_name_edit.text.trim().to_string();
    let address = s.custom_addr_edit.text.trim().to_string();
    if name.is_empty() || address.is_empty() {
        s.error = "Enter a game name and a server address.".to_string();
        return;
    }
    let id = format!("custom-{}", uuid::Uuid::new_v4());
    if s.status.snapshot.games.iter().any(|g| g.id == id) {
        s.error = "A game with this ID is already configured. Add targets in Advanced instead."
            .to_string();
        return;
    }
    if let Some(path) = &s.custom_exe {
        if let Err(e) = validate_executable(path) {
            s.error = e;
            return;
        }
    }
    let mut game = match lobbylocker_core::installed::custom_game(
        id.clone(),
        &name,
        "Custom",
        &address,
        lobbylocker_core::model::Protocol::Udp,
        None,
    ) {
        Ok(g) => g,
        Err(e) => {
            s.error = e;
            return;
        }
    };
    game.executable_path = s.custom_exe.clone();
    if let Err(e) = s.engine.save_game(&game) {
        s.error = e;
        return;
    }
    s.custom_open = false;
    s.selected = id;
    s.error.clear();
    reload(s);
}

pub fn validate_executable(path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    if p.is_absolute() && p.is_file() {
        Ok(())
    } else {
        Err("Choose an existing game executable file.".to_string())
    }
}
