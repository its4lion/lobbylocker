//! Unit tests for the icons module.
use super::*;
fn png() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(16, 16, image::Rgba([100, 180, 220, 255]));
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).unwrap();
    output.into_inner()
}
#[test]
fn local_sidecar_icon_works_without_reading_or_running_an_elf_file() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("game");
    std::fs::write(&executable, b"not executed").unwrap();
    std::fs::write(directory.path().join("game.png"), png()).unwrap();
    assert!(executable_icon(&executable)
        .unwrap()
        .starts_with("data:image/png;base64,"));
}
#[test]
fn malformed_and_oversized_images_or_executables_fall_back() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bad.exe");
    std::fs::write(&path, b"MZ invalid binary").unwrap();
    assert!(executable_icon(&path).is_none());
    assert!(png_uri(b"not an image").is_none());
    assert!(read_bounded(&path, 2).is_none());
    assert!(steam_icon("../evil", &[]).is_none());
    let image = image::RgbaImage::new(513, 1);
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).unwrap();
    assert!(png_uri(&output.into_inner()).is_none());
}
#[test]
fn ico_resources_are_converted_to_safe_png_and_output_is_bounded() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../icons/icon.ico");
    assert!(image_file(&path)
        .unwrap()
        .starts_with("data:image/png;base64,"));
    let mut output = LimitedOutput(vec![0; MAX_ICON_BYTES]);
    assert!(output.write_all(&[1]).is_err());
}

#[test]
fn extracts_an_embedded_icon_from_pe_bytes_without_execution() {
    // Minimal PE32 with RT_ICON/RT_GROUP_ICON resources and no runnable code.
    // Build the fixture in memory instead of bundling a third-party executable.
    fn word(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn dword(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let png = png();
    let group_offset = (160 + png.len() + 3) & !3;
    let section_size = (group_offset + 20 + 511) & !511;
    let mut bytes = vec![0u8; 512 + section_size];
    bytes[..2].copy_from_slice(b"MZ");
    dword(&mut bytes, 0x3c, 0x80);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    word(&mut bytes, 132, 0x14c);
    word(&mut bytes, 134, 1);
    word(&mut bytes, 148, 224);
    word(&mut bytes, 152, 0x10b);
    dword(&mut bytes, 184, 0x1000);
    dword(&mut bytes, 188, 512);
    dword(&mut bytes, 208, 0x2000);
    dword(&mut bytes, 212, 512);
    dword(&mut bytes, 244, 16);
    dword(&mut bytes, 264, 0x1000);
    dword(&mut bytes, 268, section_size as u32);
    bytes[376..384].copy_from_slice(b".rsrc\0\0\0");
    dword(&mut bytes, 384, section_size as u32);
    dword(&mut bytes, 388, 0x1000);
    dword(&mut bytes, 392, section_size as u32);
    dword(&mut bytes, 396, 512);
    let resource = &mut bytes[512..];
    word(resource, 14, 2);
    dword(resource, 16, 3);
    dword(resource, 20, 0x80000000 | 32);
    dword(resource, 24, 14);
    dword(resource, 28, 0x80000000 | 56);
    word(resource, 46, 1);
    dword(resource, 48, 1);
    dword(resource, 52, 0x80000000 | 80);
    word(resource, 70, 1);
    dword(resource, 72, 1);
    dword(resource, 76, 0x80000000 | 104);
    word(resource, 94, 1);
    dword(resource, 96, 1033);
    dword(resource, 100, 128);
    word(resource, 118, 1);
    dword(resource, 120, 1033);
    dword(resource, 124, 144);
    dword(resource, 128, 0x1000 + 160);
    dword(resource, 132, png.len() as u32);
    dword(resource, 144, 0x1000 + group_offset as u32);
    dword(resource, 148, 20);
    resource[160..160 + png.len()].copy_from_slice(&png);
    let group = &mut resource[group_offset..group_offset + 20];
    word(group, 2, 1);
    word(group, 4, 1);
    group[6] = 16;
    group[7] = 16;
    word(group, 10, 1);
    word(group, 12, 32);
    dword(group, 14, png.len() as u32);
    word(group, 18, 1);
    assert!(embedded_icon(&bytes)
        .unwrap()
        .starts_with("data:image/png;base64,"));
}
