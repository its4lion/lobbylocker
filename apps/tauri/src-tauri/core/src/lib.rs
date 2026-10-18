pub mod catalogue;
pub mod connections;
pub mod engine;
pub mod firewall;
pub mod i18n;
pub mod icons;
pub mod inspection;
pub mod installed;
pub mod latency;
mod launcher_folders;
pub mod model;
#[cfg(target_os = "windows")]
mod readback;
pub mod store;
#[cfg(target_os = "windows")]
mod windows_firewall;

pub type Result<T> = std::result::Result<T, String>;

/// Unix seconds, or 0 if the system clock predates the epoch.
pub(crate) fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
