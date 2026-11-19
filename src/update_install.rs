//! Install an authenticated release through the current package's update path.

#[cfg(any(target_os = "linux", windows))]
use lobbylocker_core::update;
use lobbylocker_core::{
    update::{AvailableUpdate, Platform},
    Result,
};
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct Target {
    pub(crate) platform: Platform,
    #[cfg(any(target_os = "linux", windows))]
    path: PathBuf,
    #[cfg(target_os = "linux")]
    identity: (u64, u64),
}

#[cfg(target_os = "linux")]
impl Target {
    pub(crate) fn detect() -> Option<Self> {
        use std::{fs::File, io::Read, os::unix::fs::MetadataExt};

        let mount = PathBuf::from(std::env::var_os("APPDIR")?);
        if !cfg!(target_arch = "x86_64")
            || std::env::current_exe().ok()? != mount.join("usr/bin/lobbylocker-app")
        {
            return None;
        }
        let path = PathBuf::from(std::env::var_os("APPIMAGE")?)
            .canonicalize()
            .ok()?;
        let mut file = File::open(&path).ok()?;
        let mut header = [0; 11];
        file.read_exact(&mut header).ok()?;
        if &header[..4] != b"\x7fELF" || &header[8..] != b"AI\x02" {
            return None;
        }
        let metadata = file.metadata().ok()?;
        if !metadata.is_file() {
            return None;
        }
        Some(Self {
            platform: Platform::AppImage,
            path,
            identity: (metadata.dev(), metadata.ino()),
        })
    }

    pub(crate) fn install(self, update: AvailableUpdate) -> Result<Option<PathBuf>> {
        self.replace_appimage(|staged| update::download(&update, staged))
            .map(Some)
    }

    fn replace_appimage(
        self,
        download: impl FnOnce(&mut tempfile::NamedTempFile) -> Result<()>,
    ) -> Result<PathBuf> {
        use std::{
            fs::File,
            os::unix::fs::{MetadataExt, PermissionsExt},
        };

        let original = File::open(&self.path).map_err(|e| e.to_string())?;
        original.try_lock().map_err(|e| e.to_string())?;
        let metadata = original.metadata().map_err(|e| e.to_string())?;
        if (metadata.dev(), metadata.ino()) != self.identity {
            return Err("This AppImage has changed. Reopen it before updating.".into());
        }

        let parent = self.path.parent().ok_or("Invalid AppImage path.")?;
        let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        download(&mut staged)?;
        let current = self.path.symlink_metadata().map_err(|e| e.to_string())?;
        if (current.dev(), current.ino()) != self.identity {
            return Err("This AppImage has changed. Reopen it before updating.".into());
        }
        staged
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(metadata.mode() & 0o777))
            .and_then(|()| staged.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        staged
            .persist(&self.path)
            .map_err(|e| e.error.to_string())?;
        Ok(self.path)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::{
        fs,
        io::Write,
        os::unix::fs::{MetadataExt, PermissionsExt},
    };

    #[test]
    fn appimage_updates_preserve_the_old_file_until_verification_succeeds() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("LobbyLocker.AppImage");
        fs::write(&path, b"old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        let metadata = path.metadata().unwrap();
        let target = Target {
            platform: Platform::AppImage,
            path: path.clone(),
            identity: (metadata.dev(), metadata.ino()),
        };
        assert!(target
            .clone()
            .replace_appimage(|staged| {
                staged.write_all(b"incomplete").unwrap();
                Err("Download failed verification.".into())
            })
            .is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old");

        target
            .clone()
            .replace_appimage(|staged| {
                staged.write_all(b"new").unwrap();
                Ok(())
            })
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o755);
        assert!(target
            .replace_appimage(|_| panic!("stale process must not download or overwrite"))
            .is_err());
    }
}

#[cfg(windows)]
impl Target {
    pub(crate) fn detect() -> Option<Self> {
        use std::{ffi::OsString, os::windows::ffi::OsStringExt, ptr};
        use windows_sys::Win32::{
            System::Com::CoTaskMemFree,
            UI::Shell::{FOLDERID_ProgramFilesX64, SHGetKnownFolderPath},
        };

        if !cfg!(target_arch = "x86_64") {
            return None;
        }
        // Windows resolves Program Files independently of the caller's environment.
        let mut raw = ptr::null_mut();
        let result = unsafe {
            SHGetKnownFolderPath(&FOLDERID_ProgramFilesX64, 0, ptr::null_mut(), &mut raw)
        };
        let program_files = if result >= 0 && !raw.is_null() {
            let len = (0..).take_while(|&i| unsafe { *raw.add(i) } != 0).count();
            let wide = unsafe { std::slice::from_raw_parts(raw, len) };
            Some(PathBuf::from(OsString::from_wide(wide)))
        } else {
            None
        };
        unsafe { CoTaskMemFree(raw.cast()) };

        let directory = program_files?.canonicalize().ok()?.join("LobbyLocker");
        let executable = std::env::current_exe().ok()?.canonicalize().ok()?;
        // The elevated app only executes staged files from its Program Files install.
        if executable != directory.join("lobbylocker-app.exe")
            || !directory.join("Uninstall.exe").is_file()
        {
            return None;
        }
        Some(Self {
            platform: Platform::Windows,
            path: directory,
        })
    }

    pub(crate) fn install(self, update: AvailableUpdate) -> Result<Option<PathBuf>> {
        use std::{os::windows::ffi::OsStrExt, process::Command, ptr};
        use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};

        if self.path.canonicalize().ok().as_ref() != Some(&self.path) {
            return Err("The LobbyLocker installation directory has changed.".into());
        }
        let mut staged = tempfile::Builder::new()
            .prefix(".lobbylocker-update-")
            .suffix(".exe")
            .tempfile_in(&self.path)
            .map_err(|e| e.to_string())?;
        update::download(&update, &mut staged)?;
        let (file, path) = staged.keep().map_err(|e| e.error.to_string())?;
        drop(file);
        let child = Command::new(&path)
            .current_dir(&self.path)
            .arg(format!("/UPDATEPID={}", std::process::id()))
            .spawn();
        if let Err(error) = child {
            let _ = std::fs::remove_file(&path);
            return Err(error.to_string());
        }
        // A running Windows installer cannot delete its own executable.
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        unsafe { MoveFileExW(wide.as_ptr(), ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) };
        Ok(None)
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
impl Target {
    pub(crate) fn detect() -> Option<Self> {
        None
    }

    pub(crate) fn install(self, _update: AvailableUpdate) -> Result<Option<PathBuf>> {
        Err("In-app updates are unavailable for this installation.".into())
    }
}
