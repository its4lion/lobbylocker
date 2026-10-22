//! Unit tests for the installed module.
use super::*;

#[test]
fn only_a_different_available_executable_requires_replacement_approval() {
    let root = tempfile::tempdir().unwrap();
    let current = root.path().join("current.exe");
    let selected = root.path().join("selected.exe");
    fs::write(&current, b"never executed").unwrap();
    fs::write(&selected, b"never executed").unwrap();
    let current = current.to_str().unwrap();
    let selected = selected.to_str().unwrap();
    assert!(requires_executable_replacement(Some(current), selected));
    assert!(!requires_executable_replacement(Some(current), current));
    assert!(!requires_executable_replacement(None, selected));
    assert!(!requires_executable_replacement(
        Some(root.path().to_str().unwrap()),
        selected
    ));
    fs::remove_file(current).unwrap();
    assert!(!requires_executable_replacement(Some(current), selected));
}

#[test]
fn overwatch_launcher_resolves_verified_retail_or_legacy_binary_without_running_it() {
    for launcher_name in ["Overwatch Launcher.exe", "OVERWATCH LAUNCHER.EXE"] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("Custom games/Overwatch");
        fs::create_dir_all(directory.join("_retail_")).unwrap();
        let launcher = directory.join(launcher_name);
        fs::write(&launcher, b"never executed").unwrap();
        fs::write(directory.join("Battle.net.exe"), b"not the game").unwrap();
        fs::write(directory.join("_retail_/Other.exe"), b"not the game").unwrap();
        assert!(resolve_executable(launcher.to_str().unwrap())
            .unwrap_err()
            .contains("Choose Overwatch.exe in the _retail_ folder"));

        let legacy = directory.join("Overwatch.exe");
        fs::write(&legacy, b"never executed").unwrap();
        let resolved = resolve_executable(launcher.to_str().unwrap()).unwrap();
        assert_eq!(
            fs::canonicalize(resolved).unwrap(),
            fs::canonicalize(&legacy).unwrap()
        );

        let retail = directory.join("_retail_/Overwatch.exe");
        fs::write(&retail, b"never executed").unwrap();
        let resolved = resolve_executable(launcher.to_str().unwrap()).unwrap();
        assert_eq!(
            fs::canonicalize(&resolved).unwrap(),
            fs::canonicalize(&retail).unwrap()
        );
        assert_eq!(resolve_executable(&resolved).unwrap(), resolved);
        #[cfg(target_os = "windows")]
        assert!(crate::model::windows_program(&resolved).is_ok());

        // A stale saved launcher can still identify a valid installation.
        fs::remove_file(&launcher).unwrap();
        assert_eq!(
            resolve_executable(launcher.to_str().unwrap()).unwrap(),
            resolved
        );
    }
    for path in [
        r"D:\Other install\game.exe",
        r"D:\Games\Overwatch\_retail_\Overwatch.exe",
        r"D:\Games\Battle.net.exe",
    ] {
        assert_eq!(resolve_executable(path).unwrap(), path);
    }
    assert!(resolve_executable("Overwatch Launcher.exe").is_err());
}

#[cfg(unix)]
#[test]
fn overwatch_launcher_cannot_resolve_an_external_binary_through_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("Overwatch");
    fs::create_dir_all(directory.join("_retail_")).unwrap();
    let outside = root.path().join("outside.exe");
    fs::write(&outside, b"never executed").unwrap();
    fs::write(directory.join("Overwatch Launcher.exe"), b"never executed").unwrap();
    std::os::unix::fs::symlink(&outside, directory.join("_retail_/Overwatch.exe")).unwrap();
    std::os::unix::fs::symlink(&outside, directory.join("Overwatch.exe")).unwrap();
    assert!(
        resolve_executable(directory.join("Overwatch Launcher.exe").to_str().unwrap()).is_err()
    );
    assert!(!is_battlenet_overwatch_executable(
        &directory.join("_retail_/Overwatch.exe")
    ));
    assert!(!is_battlenet_overwatch_executable(
        &directory.join("Overwatch.exe")
    ));
}

#[test]
fn supported_games_resolve_only_their_actual_installed_executables() {
    let root = tempfile::tempdir().unwrap();
    for (id, relative) in [
        ("cs2", "game/bin/win64/cs2.exe"),
        ("deadlock", "game/bin/win64/deadlock.exe"),
        ("overwatch2", "_retail_/Overwatch.exe"),
        ("overwatch2", "Overwatch.exe"),
    ] {
        let directory = tempfile::tempdir_in(root.path()).unwrap();
        let game = InstalledGame {
            id: id.into(),
            name: id.into(),
            launcher: "Steam".into(),
            directory: directory.path().into(),
        };
        fs::write(directory.path().join("Launcher.exe"), b"never run").unwrap();
        assert!(game_executable(&game).is_none());
        let executable = directory.path().join(relative);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"never run").unwrap();
        let detected = game_executable(&game).unwrap();
        assert_eq!(
            fs::canonicalize(&detected).unwrap(),
            fs::canonicalize(executable).unwrap()
        );
        #[cfg(target_os = "windows")]
        assert!(crate::model::windows_program(&detected).is_ok());
        assert!(game_executable(&InstalledGame {
            id: "steam-unknown".into(),
            ..game
        })
        .is_none());
    }
}

#[test]
fn missing_or_incomplete_installs_do_not_guess_an_executable() {
    let root = tempfile::tempdir().unwrap();
    let game = InstalledGame {
        id: "cs2".into(),
        name: "Counter-Strike 2".into(),
        launcher: "Steam".into(),
        directory: root.path().into(),
    };
    fs::write(root.path().join("cs2.exe"), b"not the known game binary").unwrap();
    assert!(game_executable(&game).is_none());
    fs::create_dir_all(root.path().join("game/bin/win64/cs2.exe")).unwrap();
    assert!(game_executable(&game).is_none());
}

#[cfg(unix)]
#[test]
fn detected_executables_cannot_escape_the_installation_through_a_symlink() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("Overwatch");
    fs::create_dir_all(&directory).unwrap();
    let outside = root.path().join("outside.exe");
    fs::write(&outside, b"never run").unwrap();
    std::os::unix::fs::symlink(&outside, directory.join("Overwatch.exe")).unwrap();
    let game = InstalledGame {
        id: "overwatch2".into(),
        name: "Overwatch 2".into(),
        launcher: "Steam".into(),
        directory,
    };
    assert!(game_executable(&game).is_none());
}

#[test]
fn lists_default_registered_and_added_folders_once_and_keeps_removals_effective() {
    let root = tempfile::tempdir().unwrap();
    let steam = root.path().join("Steam");
    let external = root.path().join("ExternalLibrary");
    let absent = root.path().join("DisconnectedLibrary");
    manifest(&steam, "730", "Counter-Strike 2", "cs2", 4);
    manifest(&external, "1422450", "Deadlock", "deadlock", 4);
    fs::write(steam.join("steamapps/libraryfolders.vdf"), format!(r#""libraryfolders" {{ "0" {{ "path" "{}" }} "1" {{ "path" "{}" }} "2" {{ "path" "{}" }} }}"#, steam.display(), external.display(), absent.display())).unwrap();
    let defaults = vec![steam.clone(), steam.clone()];
    let scan = scan_library_roots(&defaults, &[steam.join("steamapps")], &[]);
    assert_eq!(scan.library_paths.len(), 3);
    assert!(scan
        .library_paths
        .contains(&fs::canonicalize(&steam).unwrap()));
    assert!(scan
        .library_paths
        .contains(&fs::canonicalize(&external).unwrap()));
    assert!(scan.library_paths.contains(&absent));
    assert_eq!(scan.games.len(), 2);
    let scan = scan_library_roots(&defaults, &[], &[external.clone(), absent.clone()]);
    assert_eq!(scan.library_paths, vec![fs::canonicalize(&steam).unwrap()]);
    assert_eq!(
        scan.games
            .iter()
            .map(|game| game.id.as_str())
            .collect::<Vec<_>>(),
        vec!["cs2"]
    );
    let scan = scan_library_roots(&defaults, &[], std::slice::from_ref(&steam));
    assert!(!scan
        .library_paths
        .contains(&fs::canonicalize(&steam).unwrap()));
    assert!(scan.games.iter().all(|game| game.id != "cs2"));
    assert!(scan.games.iter().any(|game| game.id == "deadlock"));
    assert!(steam.join("steamapps/appmanifest_730.acf").exists());
    let restored = scan_library_roots(&defaults, &[external], &[]);
    assert_eq!(restored.games.len(), 2);
    let json = serde_json::to_value(restored).unwrap();
    assert!(json["libraryPaths"].is_array());
    assert!(json.get("library_paths").is_none());
}

#[cfg(unix)]
#[test]
fn default_folder_aliases_are_deduplicated_and_cannot_bypass_removal() {
    let root = tempfile::tempdir().unwrap();
    let steam = root.path().join("Steam");
    let alias = root.path().join("Alias");
    manifest(&steam, "730", "Counter-Strike 2", "cs2", 4);
    std::os::unix::fs::symlink(&steam, &alias).unwrap();
    let roots = vec![steam.clone(), alias.clone()];
    let scan = scan_library_roots(&roots, std::slice::from_ref(&alias), &[]);
    assert_eq!(scan.library_paths, vec![fs::canonicalize(&steam).unwrap()]);
    let scan = scan_library_roots(&roots, &[alias], &[steam]);
    assert!(scan.library_paths.is_empty());
    assert!(scan.games.is_empty());
}

#[test]
fn removed_launcher_children_are_not_rediscovered_through_a_parent_folder() {
    let root = tempfile::tempdir().unwrap();
    let xbox = root.path().join("XboxGames");
    let config = xbox.join("Example/Content");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("MicrosoftGame.config"), r#"<Game><Identity Name="Example.Game"/><ShellVisuals DefaultDisplayName="Xbox Example"/></Game>"#).unwrap();
    assert_eq!(
        scan_library_roots(std::slice::from_ref(&xbox), &[root.path().into()], &[])
            .games
            .len(),
        1
    );
    let scan = scan_library_roots(
        std::slice::from_ref(&xbox),
        &[root.path().into()],
        std::slice::from_ref(&xbox),
    );
    assert!(scan.games.is_empty());
    assert!(config.join("MicrosoftGame.config").exists());
}

#[test]
fn launcher_tools_are_not_scanned_as_games() {
    let root = tempfile::tempdir().unwrap();
    let apps = root.path().join("steamapps");
    fs::create_dir_all(apps.join("common/Proton Experimental")).unwrap();
    fs::write(apps.join("appmanifest_1493710.acf"), r#""AppState" { "appid" "1493710" "name" "Proton Experimental" "installdir" "Proton Experimental" "StateFlags" "4" }"#).unwrap();
    assert!(scan_steam(&[root.path().into()]).games.is_empty());
    let game = InstalledGame {
        id: "steam-123".into(),
        name: "My Game".into(),
        launcher: "Steam".into(),
        directory: root.path().into(),
    };
    assert!(!is_launcher_tool(&game));
    assert!(is_launcher_tool(&InstalledGame {
        name: "Steam Linux Runtime (sniper)".into(),
        ..game.clone()
    }));
    assert!(!is_launcher_tool(&InstalledGame {
        launcher: "Epic".into(),
        name: "Proton".into(),
        ..game
    }));
}
fn manifest(root: &Path, app: &str, name: &str, install: &str, state: u32) {
    fs::create_dir_all(root.join("steamapps/common").join(install)).unwrap();
    fs::write(root.join(format!("steamapps/appmanifest_{app}.acf")), format!(r#""AppState" {{ "appid" "{app}" "name" "{name}" "installdir" "{install}" "StateFlags" "{state}" }}"#)).unwrap();
}
#[test]
fn steam_scans_external_libraries_and_maps_supported_and_custom_games() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    manifest(root.path(), "730", "Counter-Strike 2", "cs2", 4);
    manifest(external.path(), "999", "An unsupported game", "unknown", 4);
    manifest(root.path(), "111", "Not fully installed", "pending", 2);
    fs::write(
        root.path().join("steamapps/libraryfolders.vdf"),
        format!(
            r#"// launcher data
            "libraryfolders" {{ "0" {{ "path" "{}" }} "1" {{ "path" "{}" }} }}"#,
            root.path().display(),
            external.path().display()
        ),
    )
    .unwrap();
    let scan = scan_steam(&[root.path().into(), root.path().into()]);
    assert!(scan.warnings.is_empty());
    assert_eq!(scan.games.len(), 2);
    assert!(scan.games.iter().any(|g| g.id == "cs2"));
    assert!(scan.games.iter().any(|g| g.id == "steam-999"));
}
#[test]
fn follows_multiple_library_drives_and_shows_duplicate_installs_once() {
    let root = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let third = tempfile::tempdir().unwrap();
    manifest(root.path(), "730", "Counter-Strike 2", "cs2", 4);
    manifest(second.path(), "730", "Counter-Strike 2", "cs2", 4);
    manifest(second.path(), "1422450", "Deadlock", "deadlock", 4);
    manifest(third.path(), "900000003", "Another game", "another", 4);
    fs::write(root.path().join("steamapps/libraryfolders.vdf"), format!(r#""libraryfolders" {{ "0" {{ "path" "{}" }} "1" {{ "path" "{}" }} "2" {{ "path" "{}" }} }}"#, root.path().display(), second.path().display(), third.path().display())).unwrap();
    let scan = scan_steam(&[root.path().into(), second.path().into()]);
    assert!(scan.warnings.is_empty());
    assert_eq!(scan.games.len(), 3);
    assert_eq!(scan.games.iter().filter(|game| game.id == "cs2").count(), 1);
    assert!(scan.games.iter().any(|game| game.id == "deadlock"));
    assert!(scan.games.iter().any(|game| game.id == "steam-900000003"));
}
#[test]
fn non_steam_folders_do_not_produce_false_steam_scan_warnings() {
    let root = tempfile::tempdir().unwrap();
    let scan = scan_steam(&[root.path().into()]);
    assert!(scan.games.is_empty());
    assert!(scan.warnings.is_empty());
}
#[test]
fn corrupt_manifests_do_not_break_the_scan_or_follow_traversal() {
    let root = tempfile::tempdir().unwrap();
    manifest(root.path(), "1422450", "Deadlock", "Deadlock", 4);
    fs::write(
        root.path().join("steamapps/appmanifest_bad.acf"),
        r#""AppState" { "appid" "111" "name" "Bad" "installdir" "../outside" "StateFlags" "4" }"#,
    )
    .unwrap();
    fs::write(
        root.path().join("steamapps/appmanifest_broken.acf"),
        "not VDF",
    )
    .unwrap();
    let scan = scan_steam(&[root.path().into()]);
    assert_eq!(scan.games.len(), 1);
    assert_eq!(scan.games[0].id, "deadlock");
    assert_eq!(scan.warnings.len(), 2);
}
#[test]
fn scanned_icons_are_local_and_corrupt_artwork_does_not_hide_games() {
    let root = tempfile::tempdir().unwrap();
    manifest(root.path(), "900000001", "Local icon game", "with-icon", 4);
    manifest(
        root.path(),
        "900000002",
        "Broken icon game",
        "broken-icon",
        4,
    );
    let image = image::RgbaImage::from_pixel(16, 16, image::Rgba([120, 160, 200, 255]));
    image
        .save(root.path().join("steamapps/common/with-icon/icon.png"))
        .unwrap();
    fs::write(
        root.path().join("steamapps/common/broken-icon/icon.png"),
        b"not an image",
    )
    .unwrap();
    let scan = scan_steam(&[root.path().into()]);
    assert_eq!(scan.games.len(), 2);
    assert!(scan.warnings.is_empty());
    assert!(scan.icons["steam-900000001"].starts_with("data:image/png;base64,"));
    assert!(!scan.icons.contains_key("steam-900000002"));
}
#[test]
fn vdf_paths_and_invalid_structure() {
    let map = vdf(r#""folders" { "path" "C:\\Games\\Steam" }"#).unwrap();
    assert_eq!(
        string(child(&map, "folders").unwrap(), "path"),
        Some(r"C:\Games\Steam")
    );
    for invalid in [r#""x" { "y" "z""#, r#""x" "unterminated"#, "}", r#""x" }"#] {
        assert!(vdf(invalid).is_err());
    }
}
#[test]
fn epic_only_lists_present_complete_installs() {
    let root = tempfile::tempdir().unwrap();
    let mut json = serde_json::json!({"AppName":"some-game", "DisplayName":"Some Game", "InstallLocation":root.path()});
    assert_eq!(
        epic_manifest(&json.to_string()).unwrap().unwrap().id,
        "epic-some-game"
    );
    json["bIsIncompleteInstall"] = serde_json::json!(true);
    assert!(epic_manifest(&json.to_string()).unwrap().is_none());
}
#[test]
fn custom_games_validate_normalize_and_never_guess_ping_targets() {
    let game = custom_game(
        "steam-999".into(),
        " My Game ",
        "Europe",
        "203.0.113.7/24",
        Protocol::Udp,
        Some("27015".into()),
    )
    .unwrap();
    assert_eq!(game.name, "My Game");
    assert_eq!(game.regions[0].endpoints[0].address, "203.0.113.0/24");
    assert_eq!(game.regions[0].probe_target, None);
    assert!(custom_game(
        "bad/id".into(),
        "Game",
        "Europe",
        "203.0.113.1",
        Protocol::Udp,
        None
    )
    .is_err());
    assert!(custom_game(
        "game".into(),
        "",
        "Europe",
        "0.0.0.0/0",
        Protocol::Udp,
        None
    )
    .is_err());
}
