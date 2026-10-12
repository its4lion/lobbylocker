//! Bounded, local-only icon extraction. Never executes games, shell commands, or URLs.
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{ImageFormat, ImageReader, Limits};
use std::{
    fs::File,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

const MAX_ICON_BYTES: usize = 2_000_000;
const MAX_EXECUTABLE_BYTES: usize = 128_000_000;

fn read_bounded(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= limit).then_some(bytes)
}

fn png_uri(bytes: &[u8]) -> Option<String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(512);
    limits.max_image_height = Some(512);
    limits.max_alloc = Some(16_000_000);
    reader.limits(limits);
    let image = reader.decode().ok()?.thumbnail(128, 128);
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(output.into_inner())
    ))
}

// Resource writers can report much more data than a normal icon. Enforce a hard
// output bound while reconstructing the ICO, before the image decoder sees it.
struct LimitedOutput(Vec<u8>);
impl Write for LimitedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_ICON_BYTES {
            return Err(std::io::Error::other("Icon resource is too large."));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn embedded_icon(bytes: &[u8]) -> Option<String> {
    use pelite::{pe32::Pe as _, pe64::Pe as _};
    let pe = pelite::PeFile::from_bytes(bytes).ok()?;
    let resources = match pe {
        pelite::PeFile::T32(file) => file.resources().ok()?,
        pelite::PeFile::T64(file) => file.resources().ok()?,
    };
    for (_, group) in resources.icons().take(8).flatten() {
        if group.entries().len() > 64 {
            continue;
        }
        let mut ico = LimitedOutput(Vec::new());
        if group.write(&mut ico).is_ok() {
            if let Some(icon) = png_uri(&ico.0) {
                return Some(icon);
            }
        }
    }
    None
}

fn image_file(path: &Path) -> Option<String> {
    let bytes = read_bounded(path, MAX_ICON_BYTES)?;
    png_uri(&bytes)
}

/// Embedded PE icons also work for Windows games installed via Proton on Linux.
/// Native ELF files have no standard embedded icon: try explicit nearby artwork.
pub fn executable_icon(executable: &Path) -> Option<String> {
    if executable
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    {
        if let Some(bytes) = read_bounded(executable, MAX_EXECUTABLE_BYTES) {
            if let Some(icon) = embedded_icon(&bytes) {
                return Some(icon);
            }
        }
    }
    for ext in ["png", "ico", "jpg"] {
        if let Some(icon) = image_file(&executable.with_extension(ext)) {
            return Some(icon);
        }
    }
    let parent = executable.parent()?;
    for filename in ["icon.png", "icon.ico", "logo.png"] {
        if let Some(icon) = image_file(&parent.join(filename)) {
            return Some(icon);
        }
    }
    None
}

pub fn steam_icon(app_id: &str, roots: &[PathBuf]) -> Option<String> {
    if app_id.is_empty() || !app_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if let Some(home) = dirs::home_dir() {
        for base in [
            home.join(".local/share/icons/hicolor"),
            home.join(".icons/hicolor"),
        ] {
            for size in ["256x256", "128x128", "64x64", "48x48", "32x32"] {
                if let Some(icon) = image_file(
                    &base
                        .join(size)
                        .join("apps")
                        .join(format!("steam_icon_{app_id}.png")),
                ) {
                    return Some(icon);
                }
            }
        }
    }
    for root in roots.iter().take(64) {
        for suffix in [
            format!("appcache/librarycache/{app_id}_icon.jpg"),
            format!("appcache/librarycache/{app_id}/icon.jpg"),
            format!("appcache/librarycache/{app_id}_logo.png"),
        ] {
            if let Some(icon) = image_file(&root.join(suffix)) {
                return Some(icon);
            }
        }
    }
    None
}

/// Fixed known paths first, then a shallow bounded search. No recursive game-tree
/// traversal or execution; missing/corrupt icons silently use the UI fallback.
pub fn installation_icon(directory: &Path, id: &str) -> Option<String> {
    let known = match id {
        "cs2" => Some("game/bin/win64/cs2.exe"),
        "deadlock" => Some("game/bin/win64/deadlock.exe"),
        "overwatch2" => Some("_retail_/Overwatch.exe"),
        _ => None,
    };
    if let Some(path) = known {
        if let Some(icon) = executable_icon(&directory.join(path)) {
            return Some(icon);
        }
    }
    for filename in ["icon.png", "icon.ico", "logo.png"] {
        if let Some(icon) = image_file(&directory.join(filename)) {
            return Some(icon);
        }
    }
    let executables = std::fs::read_dir(directory)
        .ok()?
        .take(32)
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        });
    for path in executables.take(4) {
        if let Some(icon) = executable_icon(&path) {
            return Some(icon);
        }
    }
    None
}

#[cfg(test)]
#[path = "../tests/unit/icons.rs"]
mod tests;
