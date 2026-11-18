//! Unit tests for the banner module.
use super::*;

#[test]
fn bundled_and_launcher_games_map_to_a_steam_app_id() {
    assert_eq!(steam_app_id("cs2"), Some(730));
    assert_eq!(steam_app_id("deadlock"), Some(1_422_450));
    assert_eq!(steam_app_id("overwatch2"), Some(2_357_570));
    assert_eq!(steam_app_id("dbd"), Some(381_210));
    assert_eq!(steam_app_id("steam-1091500"), Some(1_091_500));
    assert_eq!(steam_app_id("epic-abc"), None);
    assert_eq!(steam_app_id("custom-game"), None);
    assert_eq!(steam_app_id("steam-0"), None);
    assert_eq!(steam_app_id("steam-nope"), None);
    assert!(has_banner("cs2"));
    assert!(!has_banner("custom-game"));
}

#[test]
fn a_game_without_steam_art_has_no_local_files() {
    assert!(banner_paths("custom-game").is_empty());
    assert!(banner_paths("epic-abc").is_empty());
}

#[test]
fn steam_cached_art_is_found_under_its_hashed_folder() {
    let root = tempfile::tempdir().unwrap();
    let hashed = root.path().join("appcache/librarycache/730/abc123");
    fs::create_dir_all(&hashed).unwrap();
    fs::write(hashed.join("library_hero.jpg"), b"hero").unwrap();
    fs::write(hashed.join("logo.png"), b"logo").unwrap();
    let paths = client_art(root.path(), 730);
    assert_eq!(paths, vec![hashed.join("library_hero.jpg")]);
}

#[test]
fn older_flat_steam_art_is_found_too() {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("appcache/librarycache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("730_library_hero.jpg"), b"hero").unwrap();
    let paths = client_art(root.path(), 730);
    assert_eq!(paths, vec![cache.join("730_library_hero.jpg")]);
}

#[test]
fn the_users_own_grid_art_is_found_by_app_id() {
    let root = tempfile::tempdir().unwrap();
    let grid = root.path().join("userdata/12345678/config/grid");
    fs::create_dir_all(&grid).unwrap();
    fs::write(grid.join("730p.png"), b"custom").unwrap();
    fs::write(grid.join("730_library_600x900.jpg"), b"custom").unwrap();
    fs::write(grid.join("999p.png"), b"other").unwrap();
    fs::write(grid.join("730.txt"), b"not art").unwrap();
    let paths = custom_art(root.path(), 730);
    assert_eq!(paths.len(), 2);
    assert!(paths.iter().all(|path| path.starts_with(&grid)));
}

#[test]
fn a_banner_file_is_read_within_its_size_bound() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("hero.jpg");
    fs::write(&path, b"\xFF\xD8jpeg").unwrap();
    assert_eq!(read_banner(&path).unwrap(), b"\xFF\xD8jpeg");
    let empty = root.path().join("empty.jpg");
    fs::write(&empty, b"").unwrap();
    assert!(read_banner(&empty).is_err());
    assert!(read_banner(&root.path().join("missing.jpg")).is_err());
}
