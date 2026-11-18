//! Settings: one scrolling page of clean, bordered section cards (no tabs).

use crate::icons;
use crate::state::*;
use rune::{col, el, row, text, Cx, El, ScrollSpec};
use rune_kit::{Button, Kit, Select, Switch};

/// Height of one settings row, used to cap how many rows a card shows.
const ROW_HEIGHT: f32 = 52.0;
/// Rows a card shows before it starts scrolling.
const VISIBLE_ROWS: usize = 6;

pub fn settings_page(
    s: &AppState,
    _cx: &mut Cx<AppState>,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) -> El {
    let mut page = col()
        .key("settings-page")
        .w_full()
        .max_w(760.0)
        .gap(20.0)
        .child(
            col().gap(2.0).child(
                text(t.text("Settings").to_string())
                    .key("settings-title")
                    .font(rune::TextStyle::new(20.0, 600.0))
                    .color(k.c.text_base),
            ),
        );
    page = page.child(general_section(s, k, t, layer));
    if windows_blocking(&s.status) {
        page = page.child(blocking_section(s, k, t, layer));
    }
    page = page.child(library_section(s, k, t));
    page = page.child(recovery_section(s, k, t));
    // The game files come last: they are the least used part of Settings.
    page = page.child(files_section(s, k, t));
    crate::view::scroll_content(col().w_full().items_center().child(page))
}

/// A section: icon + title + rule heading, optional muted subtitle, then a
/// bordered card whose rows are separated by hairlines.
///
/// A card with more rows than `max_rows` scrolls instead of growing.
struct SectionSpec<'a> {
    key: &'a str,
    icon: &'static rune::Icon,
    title: &'a str,
    subtitle: &'a str,
    action: Option<El>,
    max_rows: Option<usize>,
}

fn section(k: &Kit, spec: SectionSpec<'_>, rows: Vec<El>) -> El {
    let SectionSpec {
        key,
        icon,
        title,
        subtitle,
        action,
        max_rows,
    } = spec;
    let mut head = row()
        .w_full()
        .center_y()
        .gap(8.0)
        .child(rune::icon(icon, 15.0).color(k.c.text_muted))
        .child(
            text(title.to_string())
                .font(rune::TextStyle::new(13.0, 600.0))
                .color(k.c.text_base),
        )
        .child(el().h(1.0).grow(1.0).fill(k.c.border_muted).a11y_hidden());
    if let Some(action) = action {
        head = head.child(action);
    }
    let mut card = col()
        .key(format!("{key}-card"))
        .w_full()
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_muted)
        .radius(10.0)
        .clip_rounded(10.0);
    let separator = || el().h(1.0).w_full().fill(k.c.border_muted).a11y_hidden();
    match max_rows {
        Some(limit) if rows.len() > limit => {
            let mut list = col()
                .key(format!("{key}-rows"))
                .w_full()
                .min_h(0.0)
                .shrink()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                });
            for (i, item) in rows.into_iter().enumerate() {
                if i > 0 {
                    list = list.child(separator());
                }
                list = list.child(item);
            }
            card = card.child(
                col()
                    .key(format!("{key}-scroll"))
                    .w_full()
                    .max_h(limit as f32 * ROW_HEIGHT)
                    .min_h(0.0)
                    .shrink()
                    .child(k.scroll_bar(list).w_full().shrink().min_h(0.0)),
            );
        }
        _ => {
            for (i, item) in rows.into_iter().enumerate() {
                if i > 0 {
                    card = card.child(separator());
                }
                card = card.child(item);
            }
        }
    }
    let mut section = col().key(key).w_full().gap(8.0).child(head);
    if !subtitle.is_empty() {
        section = section.child(
            text(subtitle.to_string())
                .font(rune::TextStyle::new(12.0, 400.0))
                .color(k.c.text_muted)
                .wrap()
                .w_full(),
        );
    }
    section.child(card)
}

fn setting_row(
    k: &Kit,
    icon: &'static rune::Icon,
    title: &str,
    description: &str,
    control: El,
) -> El {
    row()
        .key(format!("row-{title}"))
        .w_full()
        .py(10.0)
        .px(12.0)
        .center_y()
        .gap(10.0)
        .radius(6.0)
        .child(
            el().key(format!("row-{title}-icon-cell"))
                .size(20.0, 20.0)
                .center()
                .child(rune::icon(icon, 16.0).color(k.c.icon_muted)),
        )
        .child(
            col()
                .key(format!("row-{title}-text"))
                .grow(1.0)
                .min_h(0.0)
                .child(
                    text(title.to_string())
                        .font(rune::TextStyle::new(13.0, 500.0))
                        .color(k.c.text_base),
                )
                .child(
                    text(description.to_string())
                        .font(rune::TextStyle::new(12.0, 400.0))
                        .color(k.c.text_muted),
                ),
        )
        .child(control)
}

fn general_section(
    s: &AppState,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) -> El {
    let locales: Vec<String> = s.locales.locales.iter().map(|l| l.name.clone()).collect();
    let codes: Vec<String> = s.locales.locales.iter().map(|l| l.code.clone()).collect();
    let current = codes
        .iter()
        .position(|c| c == &s.status.snapshot.preferences.language)
        .and_then(|i| locales.get(i).cloned())
        .unwrap_or_else(|| "English".to_string());
    let rows = vec![
        setting_row(
            k,
            &icons::GLOBE,
            t.text("Language"),
            t.text("Interface language. Game names stay in English."),
            Select::new("language", &locales)
                .width(180.0)
                .current(Some(&current))
                .open(s.settings_language_open)
                .on_toggle(|state: &mut AppState| {
                    state.settings_language_open = !state.settings_language_open;
                    state.language_open = false;
                })
                .on_close(|state: &mut AppState| state.settings_language_open = false)
                .on_choose(
                    move |state: &mut AppState, choice: &str, _tx: &mut rune::Tx| {
                        state.settings_language_open = false;
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
                .build(k, layer),
        ),
        setting_row(
            k,
            if s.dark { &icons::MOON } else { &icons::SUN },
            t.text("Appearance"),
            t.text("Dark or light theme for the window."),
            Switch::new(s.dark)
                .label(t.text("Dark mode"))
                .hide_label()
                .on_toggle(|state: &mut AppState| state.dark = !state.dark)
                .build(k),
        ),
    ];
    section(
        k,
        SectionSpec {
            key: "general-section",
            icon: &icons::GEAR,
            title: t.text("General"),
            subtitle: "",
            action: None,
            max_rows: None,
        },
        rows,
    )
}

fn blocking_section(
    s: &AppState,
    k: &Kit,
    t: &lobbylocker_core::i18n::Translator,
    layer: &mut Vec<El>,
) -> El {
    use lobbylocker_core::model::FirewallScope;
    let scope = s.status.snapshot.preferences.windows_blocking_scope;
    let labels = vec![
        t.text("Selected games").to_string(),
        t.text("All apps on this PC").to_string(),
    ];
    let current = if scope == FirewallScope::Executable {
        labels[0].clone()
    } else {
        labels[1].clone()
    };
    let rows = vec![setting_row(
        k,
        &icons::SHIELD,
        t.text("Apply to"),
        t.text("Which apps the firewall rules cover."),
        Select::new("blocking-scope", &labels)
            .width(180.0)
            .current(Some(&current))
            .open(s.scope_open)
            .on_toggle(|state: &mut AppState| {
                state.scope_open = !state.scope_open;
            })
            .on_close(|state: &mut AppState| state.scope_open = false)
            .on_choose(
                move |state: &mut AppState, choice: &str, _tx: &mut rune::Tx| {
                    state.scope_open = false;
                    let next = if choice == labels[0] {
                        FirewallScope::Executable
                    } else {
                        FirewallScope::SystemWide
                    };
                    let mut prefs = state.status.snapshot.preferences.clone();
                    prefs.windows_blocking_scope = next;
                    let _ = save_preferences(state, prefs);
                },
            )
            .build(k, layer),
    )];
    section(
        k,
        SectionSpec {
            key: "blocking-section",
            icon: &icons::SHIELD,
            title: t.text("Blocking"),
            subtitle: scope_warning(&s.status),
            action: None,
            max_rows: None,
        },
        rows,
    )
}

fn excluded_library_path(path: &std::path::Path, excluded: &[std::path::PathBuf]) -> bool {
    let normalized = lobbylocker_core::installed::normalized_library_path(path);
    excluded.iter().any(|root| {
        normalized.starts_with(lobbylocker_core::installed::normalized_library_path(root))
    })
}

fn same_library_path(a: &str, b: &str) -> bool {
    use lobbylocker_core::installed::normalized_library_path;
    normalized_library_path(std::path::Path::new(a))
        == normalized_library_path(std::path::Path::new(b))
}

fn push_library_root(
    roots: &mut Vec<(String, bool)>,
    seen: &mut Vec<std::path::PathBuf>,
    path: String,
    user_added: bool,
) {
    let normalized =
        lobbylocker_core::installed::normalized_library_path(std::path::Path::new(&path));
    if seen.contains(&normalized) {
        return;
    }
    seen.push(normalized);
    roots.push((path, user_added));
}

fn library_section(s: &AppState, k: &Kit, t: &lobbylocker_core::i18n::Translator) -> El {
    let preferences = &s.status.snapshot.preferences;
    let custom = preferences.game_library_paths.clone();
    let excluded: Vec<std::path::PathBuf> = preferences
        .excluded_game_library_paths
        .iter()
        .map(std::path::PathBuf::from)
        .collect();
    let mut roots: Vec<(String, bool)> = Vec::new();
    let mut seen: Vec<std::path::PathBuf> = Vec::new();
    for root in &s.scan.library_paths {
        if excluded_library_path(root, &excluded) {
            continue;
        }
        let user_added = custom.iter().any(|path| {
            lobbylocker_core::installed::normalized_library_path(std::path::Path::new(path))
                == *root
        });
        push_library_root(
            &mut roots,
            &mut seen,
            root.to_string_lossy().into_owned(),
            user_added,
        );
    }
    for path in &custom {
        if excluded_library_path(std::path::Path::new(path), &excluded) {
            continue;
        }
        push_library_root(&mut roots, &mut seen, path.clone(), true);
    }
    let mut rows: Vec<El> = Vec::new();
    for (path, user_added) in &roots {
        let title = path.clone();
        let p = path.clone();
        let user_added = *user_added;
        let open_path = path.clone();
        let open = Button::new(t.text("Open folder"))
            .variant(rune_kit::ButtonVariant::Ghost)
            .on_press(move |state: &mut AppState| {
                match lobbylocker_core::installed::open_folder(std::path::Path::new(&open_path)) {
                    Ok(()) => state.error.clear(),
                    Err(e) => state.error = e,
                }
            })
            .build(k)
            .key("open-folder");
        let remove = Button::new(t.text("Remove"))
            .variant(rune_kit::ButtonVariant::Ghost)
            .on_press(move |state: &mut AppState| {
                let pref = state.status.snapshot.preferences.clone();
                let mut paths = pref.game_library_paths.clone();
                paths.retain(|x| !same_library_path(x, &p));
                let mut excluded = pref.excluded_game_library_paths.clone();
                if !excluded.iter().any(|x| same_library_path(x, &p)) {
                    excluded.push(p.clone());
                }
                match state.engine.set_game_library_settings(paths, excluded) {
                    Ok(()) => {
                        reload(state);
                        request_scan(state);
                    }
                    Err(e) => state.error = e,
                }
            })
            .build(k);
        rows.push(setting_row(
            k,
            &icons::FOLDER,
            &title,
            if user_added {
                t.text("Added by you")
            } else {
                t.text("Detected automatically")
            },
            row().gap(6.0).child(open).child(remove),
        ));
    }
    if rows.is_empty() {
        rows.push(setting_row(
            k,
            &icons::FOLDER,
            t.text("No library folders found"),
            t.text("Choose a game library folder to detect installed games."),
            el().key("library-empty").w(8.0),
        ));
    }
    let action = Button::new(t.text("Add folder"))
        .variant(rune_kit::ButtonVariant::Outline)
        .on_press_tx(|_state: &mut AppState, tx: &mut rune::Tx| {
            let opts = rune::FileDialogOptions {
                title: "Choose game library folder".to_string(),
                directory: None,
                file_name: None,
                filters: Vec::new(),
            };
            tx.open_folder(opts, |state: &mut AppState, result, _tx| match result {
                Ok(Some(path)) => {
                    let prefer = state.status.snapshot.preferences.clone();
                    let mut paths = prefer.game_library_paths.clone();
                    let mut excluded = prefer.excluded_game_library_paths.clone();
                    let p = path.to_string_lossy().to_string();
                    if !paths.iter().any(|x| same_library_path(x, &p)) {
                        paths.push(p.clone());
                    }
                    excluded.retain(|x| !same_library_path(x, &p));
                    match state.engine.set_game_library_settings(paths, excluded) {
                        Ok(()) => {
                            reload(state);
                            request_scan(state);
                        }
                        Err(e) => state.error = e,
                    }
                }
                Ok(None) => {}
                Err(e) => state.error = e.to_string(),
            });
        })
        .build(k);
    section(
        k,
        SectionSpec {
            key: "library-section",
            icon: &icons::FOLDER,
            title: t.text("Game library folders"),
            subtitle: t.text("Extra folders scanned for installed games."),
            action: Some(action),
            max_rows: Some(VISIBLE_ROWS),
        },
        rows,
    )
}

fn files_section(s: &AppState, k: &Kit, t: &lobbylocker_core::i18n::Translator) -> El {
    let mut rows: Vec<El> = Vec::new();
    let visible = library_candidates(s);
    let games: Vec<_> = library_games(&s.status)
        .into_iter()
        .filter(|game| visible.iter().any(|candidate| candidate.id == game.id))
        // Games the launcher scan found but nobody set up have neither a
        // server list nor an executable: there is nothing to show for them.
        .filter(|game| game.executable_path.is_some() || !game.regions.is_empty())
        .collect();
    for game in &games {
        let path = game
            .executable_path
            .clone()
            .unwrap_or_else(|| t.text("Executable not found").to_string());
        rows.push(setting_row(
            k,
            &icons::FILE,
            &game.name,
            &path,
            rune::el().key(format!("file-dot-{}", game.id)).w(8.0),
        ));
    }
    if rows.is_empty() {
        rows.push(setting_row(
            k,
            &icons::FILE,
            t.text("No games yet"),
            t.text("Add a game to the library to see its files here."),
            el().key("files-empty").w(8.0),
        ));
    }
    let action = Button::new(t.text("Import game file"))
        .variant(rune_kit::ButtonVariant::Outline)
        .on_press_tx(|_state: &mut AppState, tx: &mut rune::Tx| {
            let opts = rune::FileDialogOptions {
                title: "Import game JSON".to_string(),
                directory: None,
                file_name: None,
                filters: vec![("Game JSON".to_string(), vec!["json".to_string()])],
            };
            tx.open_file(opts, |state: &mut AppState, result, _tx| match result {
                Ok(Some(path)) => import_game(state, &path),
                Ok(None) => {}
                Err(e) => state.error = e.to_string(),
            });
        })
        .build(k);
    section(
        k,
        SectionSpec {
            key: "files-section",
            icon: &icons::FILE,
            title: t.text("Game files"),
            subtitle: t.text("Executables LobbyLocker uses for firewall rules."),
            action: Some(action),
            max_rows: Some(VISIBLE_ROWS),
        },
        rows,
    )
}

fn recovery_section(s: &AppState, k: &Kit, t: &lobbylocker_core::i18n::Translator) -> El {
    let active = s.status.applied_count.unwrap_or(0) > 0;
    let desc = if active {
        t.format(
            "active_rules_count",
            &[("count", &s.status.applied_count.unwrap_or(0).to_string())],
        )
    } else {
        t.text("No active LobbyLocker blocks remain. Other firewall rules are untouched.")
            .to_string()
    };
    let rows = vec![
        setting_row(
            k,
            &icons::SHIELD,
            t.text("Status"),
            &desc,
            Button::new(t.text("Verify firewall"))
                .variant(rune_kit::ButtonVariant::Outline)
                .on_press(|state: &mut AppState| {
                    state.busy = Some("read".to_string());
                    request_verify(state, true);
                })
                .build(k),
        ),
        setting_row(
            k,
            &icons::FLAME,
            t.text("Firewall cleanup"),
            t.text("Remove only LobbyLocker-owned firewall rules."),
            Button::new(t.text("Reset"))
                .variant(rune_kit::ButtonVariant::Danger)
                .on_press(|state: &mut AppState| state.reset_open = true)
                .build(k),
        ),
    ];
    section(
        k,
        SectionSpec {
            key: "recovery-section",
            icon: &icons::FLAME,
            title: t.text("Firewall"),
            subtitle: "",
            action: None,
            max_rows: None,
        },
        rows,
    )
}

fn import_game(s: &mut AppState, path: &std::path::Path) {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            s.error = e.to_string();
            return;
        }
    };
    if metadata.len() > 5_000_000 {
        s.error = "Game JSON exceeds 5 MB.".to_string();
        return;
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            s.error = e.to_string();
            return;
        }
    };
    let game = match lobbylocker_core::model::Game::from_json(&bytes) {
        Ok(g) => g,
        Err(e) => {
            s.error = e;
            return;
        }
    };
    if let Err(e) = game.validate() {
        s.error = e;
        return;
    }
    if s.status.snapshot.games.iter().any(|g| g.id == game.id) && !s.error_is_replace_ok() {
        s.error = "Replace the existing game file? Custom targets in that file will be replaced. Firewall rules are unchanged until Apply.".to_string();
        return;
    }
    match s.engine.save_game(&game) {
        Ok(()) => {
            s.selected = game.id;
            s.error.clear();
            reload(s);
        }
        Err(e) => s.error = e,
    }
}

impl AppState {
    fn error_is_replace_ok(&self) -> bool {
        false
    }
}
