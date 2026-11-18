//! Banner art: read local Steam/user art and decode it into raw RGBA.

use crate::state::BannerDone;
use lobbylocker_core::banners;

pub fn load_banner(game_id: String) -> BannerDone {
    let mut fallback: Option<(u32, u32, Vec<u8>)> = None;
    for path in banners::banner_paths(&game_id) {
        let Ok(bytes) = banners::read_banner(&path) else {
            continue;
        };
        let Ok(decoded) = decode_banner(&bytes) else {
            continue;
        };
        let (width, height, _) = &decoded;
        // A banner is wide; a tall poster is only used when nothing else fits.
        if *width as f32 >= *height as f32 * 1.4 {
            return BannerDone {
                game_id,
                pixels: Some(decoded),
            };
        }
        if fallback.is_none() {
            fallback = Some(decoded);
        }
    }
    BannerDone {
        game_id,
        pixels: fallback,
    }
}

/// Decode banner bytes into raw RGBA, scaled down to a texture the window can
/// hold. Returns width, height, and the pixel bytes.
pub fn decode_banner(bytes: &[u8]) -> lobbylocker_core::Result<(u32, u32, Vec<u8>)> {
    use image::{ImageReader, Limits};
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64_000_000);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| e.to_string())?;
    // The banner is drawn a few hundred points wide; a smaller texture is
    // enough and keeps the window's memory flat.
    let image = image.thumbnail(1600, 1600);
    let rgba = image.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    Ok((width, height, rgba.into_raw()))
}
