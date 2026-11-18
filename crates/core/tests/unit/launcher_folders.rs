//! Unit tests for the launcher_folders module.
use super::*;
#[test]
fn finds_xbox_epic_and_battlenet_in_multiple_local_libraries() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let xbox = first.path().join("XboxGames/Example/Content");
    fs::create_dir_all(&xbox).unwrap();
    fs::write(xbox.join("MicrosoftGame.config"), r#"<Game><Identity Name="Example.Game"/><ShellVisuals DefaultDisplayName="Xbox Example"/></Game>"#).unwrap();
    let epic = second.path().join("Epic Games/Epic Example/.egstore");
    fs::create_dir_all(&epic).unwrap();
    fs::write(epic.join("receipt.mancpn"), r#"{"AppName":"epic-example"}"#).unwrap();
    let battle = second.path().join("Overwatch/_retail_");
    fs::create_dir_all(&battle).unwrap();
    fs::write(battle.join("Overwatch.exe"), b"never executed").unwrap();
    let scan = scan(&[first.path().into(), second.path().into()]);
    assert_eq!(scan.games.len(), 3);
    assert!(scan
        .games
        .iter()
        .any(|game| game.launcher == "Xbox" && game.name == "Xbox Example"));
    assert!(scan.games.iter().any(|game| game.id == "epic-epic-example"));
    assert!(scan.games.iter().any(|game| game.id == "overwatch2"));
}
#[test]
fn ignores_random_folders_and_invalid_or_external_entity_metadata() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("Example");
    fs::create_dir_all(game.join(".egstore")).unwrap();
    fs::write(
        game.join(".egstore/receipt.mancpn"),
        r#"{"AppName":"../evil"}"#,
    )
    .unwrap();
    fs::write(game.join("MicrosoftGame.config"), r#"<!DOCTYPE Game [<!ENTITY external SYSTEM "file:///etc/passwd">]><Game><Identity Name="Example"/>&external;</Game>"#).unwrap();
    assert!(scan(&[root.path().into()]).games.is_empty());
}
