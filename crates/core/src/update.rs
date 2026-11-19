//! The stable release manifest and signed packages used by the desktop updater.

use crate::Result;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use minisign_verify::{PublicKey, Signature};
use reqwest::blocking::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    time::Duration,
};

const RELEASES: &str = "https://github.com/its4lion/LobbyLocker/releases";
pub const MANIFEST_NAME: &str = "lobbylocker-update.json";
pub const PUBLIC_KEY: &str = include_str!("../../../packaging/update.pub");
const MAX_MANIFEST: u64 = 64 * 1024;
const MAX_PACKAGE: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum Platform {
    #[serde(rename = "linux-x86_64")]
    AppImage,
    #[serde(rename = "windows-x86_64")]
    Windows,
}

impl Platform {
    pub fn package_name(self, version: &Version) -> String {
        match self {
            Self::AppImage => format!("LobbyLocker-{version}-x86_64.AppImage"),
            Self::Windows => format!("LobbyLocker-{version}.exe"),
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct Manifest {
    pub version: Version,
    pub platforms: BTreeMap<Platform, String>,
}

#[derive(Clone)]
pub struct AvailableUpdate {
    version: Version,
    platform: Platform,
    signature: Box<Signature>,
}

impl AvailableUpdate {
    pub fn version(&self) -> &Version {
        &self.version
    }
}

fn client(timeout: Duration) -> Result<Client> {
    Client::builder()
        .user_agent(concat!("LobbyLocker/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())
}

pub fn check(platform: Platform) -> Result<Option<AvailableUpdate>> {
    let response = client(Duration::from_secs(20))?
        .get(format!("{RELEASES}/latest/download/{MANIFEST_NAME}"))
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    response
        .take(MAX_MANIFEST + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() as u64 > MAX_MANIFEST {
        return Err("Update manifest exceeds the size limit.".into());
    }
    let current = Version::parse(env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())?;
    candidate(&data, &current, platform)
}

fn candidate(
    data: &[u8],
    current: &Version,
    platform: Platform,
) -> Result<Option<AvailableUpdate>> {
    let manifest: Manifest = serde_json::from_slice(data).map_err(|e| e.to_string())?;
    if !manifest.version.pre.is_empty()
        || !manifest.version.build.is_empty()
        || manifest.version.cmp_precedence(current).is_le()
    {
        return Ok(None);
    }
    let encoded = manifest
        .platforms
        .get(&platform)
        .ok_or("The release has no update package for this platform.")?;
    let decoded = STANDARD.decode(encoded).map_err(|e| e.to_string())?;
    let signature = Signature::decode(std::str::from_utf8(&decoded).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(Some(AvailableUpdate {
        version: manifest.version,
        platform,
        signature: Box::new(signature),
    }))
}

/// The caller stages to a temporary file and only activates it after success.
pub fn download(update: &AvailableUpdate, file: &mut impl Write) -> Result<()> {
    let version = &update.version;
    let package = update.platform.package_name(version);
    let response = client(Duration::from_secs(15 * 60))?
        .get(format!("{RELEASES}/download/v{version}/{package}"))
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|e| e.to_string())?;
    let key = PublicKey::decode(PUBLIC_KEY).map_err(|e| e.to_string())?;
    copy_verified(update, &key, response, file, MAX_PACKAGE)
}

/// The trusted comment is authenticated by the signature along with the bytes.
pub fn signing_comment(platform: Platform, version: &Version) -> String {
    format!("file:{}\tversion:{version}", platform.package_name(version))
}

fn copy_verified(
    update: &AvailableUpdate,
    key: &PublicKey,
    mut source: impl Read,
    mut file: impl Write,
    limit: u64,
) -> Result<()> {
    if update.signature.trusted_comment() != signing_comment(update.platform, &update.version) {
        return Err("Update signature does not match the release version and package.".into());
    }
    let mut verifier = key
        .verify_stream(&update.signature)
        .map_err(|e| e.to_string())?;
    let mut buffer = [0; 64 * 1024];
    let mut total = 0;
    loop {
        let count = source.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > limit {
            return Err("Update package exceeds the size limit.".into());
        }
        verifier.update(&buffer[..count]);
        file.write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
    }
    verifier.finalize().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_newer_stable_versions_are_offered() {
        let current = Version::new(0, 0, 9);
        for version in ["0.0.8", "0.0.9", "0.0.10-rc.1", "0.0.10+build"] {
            let data = format!(r#"{{"version":"{version}","platforms":{{}}}}"#);
            assert!(candidate(data.as_bytes(), &current, Platform::AppImage)
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn requires_the_signed_bytes_version_and_platform() {
        let keys = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let public_key = PublicKey::from_base64(&keys.pk.to_base64()).unwrap();
        let platform = Platform::AppImage;
        let version = Version::new(0, 0, 10);
        let bytes = b"an update";
        let signature = minisign::sign(
            Some(&keys.pk),
            &keys.sk,
            &bytes[..],
            Some(&signing_comment(platform, &version)),
            None,
        )
        .unwrap();
        let manifest = Manifest {
            version,
            platforms: BTreeMap::from([(platform, STANDARD.encode(signature.into_string()))]),
        };
        let data = serde_json::to_vec(&manifest).unwrap();
        let mut update = candidate(&data, &Version::new(0, 0, 9), platform)
            .unwrap()
            .unwrap();
        let mut written = Vec::new();
        copy_verified(&update, &public_key, &bytes[..], &mut written, 100).unwrap();
        assert_eq!(&written, bytes);
        assert!(copy_verified(&update, &public_key, &bytes[..], Vec::new(), 1).is_err());
        assert!(copy_verified(&update, &public_key, &b"tampered"[..], Vec::new(), 100).is_err());
        update.platform = Platform::Windows;
        assert!(copy_verified(&update, &public_key, &bytes[..], Vec::new(), 100).is_err());
        update.platform = platform;
        update.version = Version::new(0, 0, 11);
        assert!(copy_verified(&update, &public_key, &bytes[..], Vec::new(), 100).is_err());
    }
}
