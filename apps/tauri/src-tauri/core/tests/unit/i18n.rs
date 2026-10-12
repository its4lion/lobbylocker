//! Unit tests for the i18n module.
use super::*;
#[test]
fn loads_new_language_from_json_and_falls_back_for_missing_keys() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("fr.json"),
        r#"{"code":"fr","name":"Français","direction":"ltr","messages":{"Games":"Jeux"}}"#,
    )
    .unwrap();
    let catalogue = Catalogue::load(root.path()).unwrap();
    let translator = catalogue.translator("fr");
    assert_eq!(
        (
            translator.text("Games"),
            translator.text("Add game"),
            translator.direction()
        ),
        ("Jeux", "Add game", "ltr")
    );
}
#[test]
fn arabic_has_rtl_direction_and_translated_regions() {
    let root = tempfile::tempdir().unwrap();
    let catalogue = Catalogue::load(root.path()).unwrap();
    let translator = catalogue.translator("ar");
    assert_eq!(
        (translator.direction(), translator.text("Europe")),
        ("rtl", "أوروبا")
    );
}
#[test]
fn invalid_locale_is_skipped_without_breaking_startup() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("broken.json"),
        r#"{"code":"broken","name":"Bad","direction":"invalid","messages":{}}"#,
    )
    .unwrap();
    assert_eq!(Catalogue::load(root.path()).unwrap().warnings.len(), 1);
}
#[test]
fn placeholders_are_not_recursively_expanded() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("test.json"), r#"{"code":"test","name":"Test","direction":"ltr","messages":{"template":"{name}: {count}"}}"#).unwrap();
    let catalogue = Catalogue::load(root.path()).unwrap();
    assert_eq!(
        catalogue
            .translator("test")
            .format("template", &[("name", "{count}"), ("count", "5")]),
        "{count}: 5"
    );
}
