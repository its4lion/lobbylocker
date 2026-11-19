//! Sign the exact installers staged for a stable release candidate.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use lobbylocker_core::update::{signing_comment, Manifest, Platform, MANIFEST_NAME, PUBLIC_KEY};
use minisign::{PublicKey, SecretKeyBox};
use semver::Version;
use serde_json::json;
use std::{collections::BTreeMap, env, error::Error, fs::File, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let usage = "Usage: sign-update VERSION RELEASE-ASSETS-DIRECTORY";
    let version: Version = args.next().ok_or(usage)?.parse()?;
    let directory = args.next().ok_or(usage)?;
    if !version.pre.is_empty() || !version.build.is_empty() || args.next().is_some() {
        return Err(usage.into());
    }

    let secret = env::var("TAURI_SIGNING_PRIVATE_KEY")
        .map_err(|_| "The release-signing environment is missing TAURI_SIGNING_PRIVATE_KEY.")?;
    let decoded = STANDARD
        .decode(secret.trim())
        .map_err(|_| "Invalid base64 release signing key.")?;
    let key = SecretKeyBox::from_string(std::str::from_utf8(&decoded)?)
        .and_then(|secret| {
            secret
                .clone()
                .into_unencrypted_secret_key()
                .or_else(|_| secret.into_secret_key(Some(String::new())))
        })
        .map_err(|_| "Could not load the release signing key.")?;
    let public_key = PublicKey::from_box(minisign::PublicKeyBox::from_string(PUBLIC_KEY)?)?;

    let directory = Path::new(&directory);
    let mut platforms = BTreeMap::new();
    let mut legacy = BTreeMap::new();
    for platform in [Platform::AppImage, Platform::Windows] {
        let name = platform.package_name(&version);
        let file = File::open(directory.join(&name))?;
        let signature = minisign::sign(
            Some(&public_key),
            &key,
            file,
            Some(&signing_comment(platform, &version)),
            None,
        )?;
        let signature = STANDARD.encode(signature.into_string());
        legacy.insert(platform, json!({
            "url": format!("https://github.com/its4lion/LobbyLocker/releases/download/v{version}/{name}"),
            "signature": &signature,
        }));
        platforms.insert(platform, signature);
    }
    // The installed Tauri clients use platform entries containing both a URL and a signature.
    serde_json::to_writer_pretty(
        File::create(directory.join("latest.json"))?,
        &json!({ "version": &version, "platforms": legacy }),
    )?;
    let manifest = Manifest { version, platforms };
    serde_json::to_writer_pretty(File::create(directory.join(MANIFEST_NAME))?, &manifest)?;
    Ok(())
}
