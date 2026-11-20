//! Library games: classification, sorting, and candidate discovery.

use lobbylocker_core::{engine::Runtime as EngineRuntime, installed, model::Game};
use std::collections::BTreeMap;

use crate::state::AppState;

/// Built-in games that ship with server data.
pub const BUILTIN_GAMES: [&str; 5] = ["cs2", "deadlock", "overwatch2", "dbd", "rocketleague"];

pub(crate) fn game_display_name(
    id: &str,
    name: &str,
    t: &lobbylocker_core::i18n::Translator,
) -> String {
    if id == "rocketleague" {
        t.format("{game} (Beta)", &[("game", name)])
    } else {
        name.to_owned()
    }
}

pub fn block_game_ids(status: &EngineRuntime) -> Vec<String> {
    let mut ids: Vec<String> = status.applied_game_ids.clone();
    ids.extend(
        status
            .snapshot
            .preferences
            .last_applied_game_ids
            .iter()
            .cloned(),
    );
    for key in &status.snapshot.preferences.blocked_regions {
        let id = key.split(':').next().unwrap_or("").to_string();
        if !id.is_empty() {
            ids.push(id);
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

/// Sort key for library games: built-ins first, then by name, then id.
fn library_sort_key(id: &str, name: &str) -> (usize, String, String) {
    (
        BUILTIN_GAMES
            .iter()
            .position(|g| *g == id)
            .unwrap_or(BUILTIN_GAMES.len()),
        name.to_lowercase(),
        id.to_string(),
    )
}

pub fn library_games(status: &EngineRuntime) -> Vec<Game> {
    let mut games = status.snapshot.games.clone();
    games.sort_by_key(|g| library_sort_key(&g.id, &g.name));
    games
}

pub struct LibraryCandidate {
    pub id: String,
    pub name: String,
}

pub fn library_candidates(s: &AppState) -> Vec<LibraryCandidate> {
    let status = &s.status;
    let forced = block_game_ids(status);
    let mut choices: BTreeMap<String, LibraryCandidate> = BTreeMap::new();
    for game in &status.snapshot.games {
        if installed::is_steam_tool(&game.id, &game.name) && !forced.contains(&game.id) {
            continue;
        }
        let launcher_game = BUILTIN_GAMES.contains(&game.id.as_str())
            || ["steam-", "epic-", "xbox-"]
                .iter()
                .any(|prefix| game.id.starts_with(prefix));
        if launcher_game && !s.scan.games.iter().any(|installed| installed.id == game.id) {
            continue;
        }
        choices.insert(
            game.id.clone(),
            LibraryCandidate {
                id: game.id.clone(),
                name: game.name.clone(),
            },
        );
    }
    for game in &s.scan.games {
        if !choices.contains_key(&game.id) && !installed::is_launcher_tool(game) {
            choices.insert(
                game.id.clone(),
                LibraryCandidate {
                    id: game.id.clone(),
                    name: game.name.clone(),
                },
            );
        }
    }
    let mut list: Vec<LibraryCandidate> = choices.into_values().collect();
    list.sort_by_key(|c| library_sort_key(&c.id, &c.name));
    list
}

/// Keep the displayed selection valid after installed games change. Do not
/// create configuration files while scanning; an unconfigured game is saved
/// only when the user selects it.
pub fn reconcile_library_selection(s: &mut AppState) {
    let candidates = library_candidates(s);
    if candidates.iter().any(|game| game.id == s.selected) {
        return;
    }
    s.selected = candidates
        .iter()
        .find(|candidate| {
            s.status
                .snapshot
                .games
                .iter()
                .any(|game| game.id == candidate.id)
        })
        .map(|game| game.id.clone())
        .unwrap_or_default();
}
