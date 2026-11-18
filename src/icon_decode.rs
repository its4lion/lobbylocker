//! Decoding executable icons (PNG/JPEG/ICO) into pixel tiles for the UI.

use crate::state::AppState;
use lobbylocker_core::icons;
use rune::Pixels;

pub fn refresh_icons(s: &mut AppState) {
    let mut wanted: Vec<(String, String)> = Vec::new();
    for game in s.status.snapshot.games.iter().take(100) {
        if let Some(path) = &game.executable_path {
            if let Some(data_url) = icons::executable_icon(std::path::Path::new(path)) {
                wanted.push((game.id.clone(), data_url));
                continue;
            }
        }
        if let Some(data_url) = s.scan.icons.get(&game.id) {
            wanted.push((game.id.clone(), data_url.clone()));
        }
    }
    for (id, data_url) in s.scan.icons.iter().take(100) {
        if !wanted.iter().any(|(existing, _)| existing == id) {
            wanted.push((id.clone(), data_url.clone()));
        }
    }
    s.custom_icons
        .retain(|id, _| wanted.iter().any(|(w, _)| w == id));
    for (id, data_url) in wanted {
        if s.custom_icons.contains_key(&id) {
            continue;
        }
        let Some(pixels) = data_url_pixels(&data_url) else {
            continue;
        };
        s.custom_icons.insert(id, pixels);
    }
}

fn data_url_pixels(data_url: &str) -> Option<Pixels> {
    use base64::Engine;
    let (_, b64) = data_url.split_once(',')?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    let pixels = Pixels::new();
    pixels.set(w, h, img.into_raw());
    Some(pixels)
}
