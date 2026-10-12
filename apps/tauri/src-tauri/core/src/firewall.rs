use crate::{
    inspection::{self, Inspection},
    model::{normalize_address, parse_ports, windows_program, FirewallRule, Protocol},
    Result,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use std::process::Command;
#[cfg(target_os = "linux")]
use std::{io::Write, process::Stdio};

pub const TABLE: &str = "lobbylocker_v1";
pub const OWNER: &str = "LobbyLocker managed firewall rules v1";
pub const WINDOWS_GROUP: &str = "LobbyLocker.Managed.v1";
pub(crate) const WINDOWS_NETSECURITY: &str = "Import-Module (Join-Path $PSHOME 'Modules\\NetSecurity\\NetSecurity.psd1') -ErrorAction Stop; ";

#[cfg(target_os = "windows")]
pub(crate) fn powershell_command() -> Result<Command> {
    use std::os::windows::ffi::OsStringExt;
    // Do not obtain firewall evidence from an executable or module found in a
    // user-writable PATH/current directory. Use the native system installation.
    let mut buffer = [0u16; 32768];
    let length = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    } as usize;
    if length == 0 || length >= buffer.len() {
        return Err("Could not locate system PowerShell.".into());
    }
    let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length]))
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    Ok(Command::new(path))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
pub enum Request {
    Apply { rules: Vec<FirewallRule> },
    Reset {},
    Verify {},
}

impl Request {
    fn validate(&self) -> Result<()> {
        if let Self::Apply { rules } = self {
            if rules.len() > 5000 {
                return Err("Too many firewall rules.".into());
            }
            let mut ids = std::collections::HashSet::new();
            for rule in rules {
                rule.endpoint.validate()?;
                if let Some(program) = &rule.program {
                    windows_program(program)?;
                }
                if !ids.insert(&rule.endpoint.id) {
                    return Err("Duplicate firewall rule ID.".into());
                }
            }
        }
        Ok(())
    }
}

pub fn nft_script(request: &Request, exists: bool) -> Result<String> {
    request.validate()?;
    if matches!(request, Request::Verify {}) {
        return Err("Verification cannot generate firewall changes.".into());
    }
    let mut script = String::new();
    if exists {
        script.push_str(&format!("delete table inet {TABLE}\n"));
    }
    let Request::Apply { rules } = request else {
        return Ok(script);
    };
    if rules.iter().any(|rule| rule.program.is_some()) {
        return Err("Executable-scoped firewall rules are supported on Windows only. Refusing to apply them system-wide.".into());
    }
    if rules.is_empty() {
        return Ok(script);
    }
    script.push_str(&format!("table inet {TABLE} {{\n comment \"{OWNER}\";\n chain outbound {{\n type filter hook output priority 10; policy accept;\n"));
    for rule in rules {
        let rule = &rule.endpoint;
        let address = normalize_address(&rule.address)?;
        let family = if address.contains(':') { "ip6" } else { "ip" };
        let ports = parse_ports(&rule.ports)?;
        let protocols: Vec<_> = match (&rule.protocol, ports) {
            (Protocol::Any, None) => vec![None],
            (Protocol::Any, Some(_)) => vec![Some("tcp"), Some("udp")],
            (Protocol::Tcp, _) => vec![Some("tcp")],
            (Protocol::Udp, _) => vec![Some("udp")],
        };
        for protocol in protocols {
            script.push_str(&format!(" {family} daddr {address}"));
            if let Some(protocol) = protocol {
                script.push_str(&format!(" meta l4proto {protocol}"));
                if let Some((start, end)) = ports {
                    script.push_str(&format!(" {protocol} dport {start}-{end}"));
                }
            }
            script.push_str(&format!(" counter drop comment \"ll:{}\"\n", rule.id));
        }
    }
    script.push_str(" }\n}\n");
    Ok(script)
}

pub fn powershell_script(request: &Request) -> Result<String> {
    request.validate()?;
    if matches!(request, Request::Verify {}) {
        return Err("Verification cannot generate firewall changes.".into());
    }
    let preamble = format!("$ErrorActionPreference='Stop'; {WINDOWS_NETSECURITY}$old=@(Get-NetFirewallRule -PolicyStore PersistentStore | Where-Object {{$_.Group -eq '{WINDOWS_GROUP}'}}); ");
    let Request::Apply { rules } = request else {
        let mut script = format!("{preamble}$old | Remove-NetFirewallRule -ErrorAction Stop; ");
        // Confirm both saved and effective rules are gone, including disabled
        // rules that are intentionally omitted from the UI's active-block list.
        for store in ["PersistentStore", "ActiveStore"] {
            script.push_str(&format!("if (@(Get-NetFirewallRule -PolicyStore {store} | Where-Object {{$_.Group -eq '{WINDOWS_GROUP}'}}).Count -ne 0) {{ throw 'LobbyLocker firewall rules remain after Reset.' }}; "));
        }
        return Ok(script);
    };
    let transaction = uuid::Uuid::new_v4().simple().to_string();
    // Validate every program before creating new rules or removing the old ones.
    let mut preflight = String::new();
    let mut programs = std::collections::HashSet::new();
    for rule in rules {
        let program = windows_program(rule.program.as_deref().ok_or("Windows firewall rules require a game executable. System-wide fallback is not allowed.")?)?;
        if programs.insert(program.clone()) {
            let literal = program.replace('\'', "''");
            preflight.push_str(&format!("if (-not (Test-Path -LiteralPath '{literal}' -PathType Leaf)) {{ throw 'Game executable is unavailable. Choose its current .exe before Apply.' }}; "));
        }
    }
    let mut script = format!("{preflight}{preamble}$new=@(); try {{ ");
    for rule in rules {
        let program = windows_program(rule.program.as_deref().ok_or("Missing game executable.")?)?
            .replace('\'', "''");
        let rule = &rule.endpoint;
        let address = normalize_address(&rule.address)?;
        let ports = parse_ports(&rule.ports)?;
        let protocols = match (&rule.protocol, ports) {
            (Protocol::Any, None) => vec!["Any"],
            (Protocol::Any, Some(_)) => vec!["TCP", "UDP"],
            (Protocol::Tcp, _) => vec!["TCP"],
            (Protocol::Udp, _) => vec!["UDP"],
        };
        for protocol in protocols {
            script.push_str(&format!("$new += New-NetFirewallRule -Name 'LobbyLocker.{transaction}.{}.{protocol}' -DisplayName 'LobbyLocker: {}' -Group '{WINDOWS_GROUP}' -Direction Outbound -Action Block -Enabled True -Profile Any -Program '{program}' -RemoteAddress '{address}' -Protocol {protocol}", rule.id, rule.id));
            if let Some((start, end)) = ports {
                script.push_str(&format!(" -RemotePort '{start}-{end}'"));
            }
            script.push_str(" -ErrorAction Stop; ");
        }
    }
    // Keep the previous generation until the complete new generation has been created.
    script.push_str("$old | Remove-NetFirewallRule -ErrorAction Stop; } catch { $new | Remove-NetFirewallRule -ErrorAction SilentlyContinue; throw; }");
    Ok(script)
}

#[cfg(target_os = "linux")]
fn owned_table_exists() -> Result<bool> {
    let tables = Command::new("/usr/bin/nft")
        .args(["-j", "list", "tables"])
        .output()
        .map_err(|e| format!("Install nftables: {e}"))?;
    if !tables.status.success() {
        return Err(String::from_utf8_lossy(&tables.stderr).into());
    }
    let json: serde_json::Value =
        serde_json::from_slice(&tables.stdout).map_err(|e| e.to_string())?;
    let exists = inspection::owned_table_present(&json)?;
    if !exists {
        return Ok(false);
    }
    let output = Command::new("/usr/bin/nft")
        .args(["-j", "list", "table", "inet", TABLE])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into());
    }
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    if !json["nftables"].as_array().is_some_and(|entries| {
        entries
            .iter()
            .any(|e| e["table"]["name"] == TABLE && e["table"]["comment"] == OWNER)
    }) {
        return Err("A firewall table with this name exists but does not belong to LobbyLocker. Refusing to change it.".into());
    }
    Ok(true)
}

#[cfg(target_os = "linux")]
fn run_nft(script: &str, check: bool) -> Result<()> {
    if script.is_empty() {
        return Ok(());
    }
    let mut command = Command::new("/usr/bin/nft");
    if check {
        command.arg("--check");
    }
    let mut child = command
        .args(["--file", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .ok_or("Could not open nft input.")?
        .write_all(script.as_bytes())
        .map_err(|e| e.to_string())?;
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
struct WindowsFirewallLock(windows_sys::Win32::Foundation::HANDLE);

#[cfg(target_os = "windows")]
impl Drop for WindowsFirewallLock {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Threading::ReleaseMutex(self.0);
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(target_os = "windows")]
fn acquire_windows_firewall_lock() -> Result<WindowsFirewallLock> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::{CreateMutexW, WaitForSingleObject},
    };

    let name: Vec<u16> = "Local\\LobbyLocker.Firewall.v1\0".encode_utf16().collect();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(format!(
            "Could not create the firewall operation lock: {}",
            std::io::Error::last_os_error()
        ));
    }
    match unsafe { WaitForSingleObject(handle, 0) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(WindowsFirewallLock(handle)),
        WAIT_TIMEOUT => {
            unsafe { CloseHandle(handle) };
            Err("Another LobbyLocker firewall operation is running.".into())
        }
        _ => {
            let error = std::io::Error::last_os_error();
            unsafe { CloseHandle(handle) };
            Err(format!(
                "Could not acquire the firewall operation lock: {error}"
            ))
        }
    }
}

fn execute_privileged(request: &Request) -> Result<()> {
    request.validate()?;
    if matches!(request, Request::Verify {}) {
        return Err("Verification cannot modify firewall rules.".into());
    }
    #[cfg(target_os = "linux")]
    {
        // This lock is global and root-owned, so separate app/CLI instances cannot race.
        use std::os::unix::fs::OpenOptionsExt;
        let lock_path = "/run/lobbylocker-firewall.lock";
        let lock = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(lock_path)
            .map_err(|e| format!("Could not acquire firewall lock (root required): {e}"))?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .map_err(|e| format!("Another firewall operation is running: {e}"))?;
        let _lock = lock;
        let script = nft_script(request, owned_table_exists()?)?;
        run_nft(&script, true)?;
        // A single nft batch replaces the table atomically.
        run_nft(&script, false)?;
        // Keep the operation lock until removal is confirmed. Package managers
        // must not lose the cleanup executable on a false-successful Reset.
        if matches!(request, Request::Reset {}) && owned_table_exists()? {
            return Err("LobbyLocker firewall rules remain after Reset.".into());
        }
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        // A named mutex serializes elevated helpers launched by separate app/CLI
        // processes. The Tauri command guard only covers one desktop process.
        let _lock = acquire_windows_firewall_lock()?;
        let script = powershell_script(request)?;
        let output = powershell_command()?
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into());
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    Err("Firewall changes are supported on Windows and Linux only.".into())
}

/// Handle a tightly scoped, structured request before initializing the desktop runtime.
/// The helper never loads user config, invokes a shell on Linux, or accepts arbitrary commands.
pub fn helper_entry() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    let arg = args.next()?;
    if arg == "--reset-firewall" {
        if args.next().is_some() {
            eprintln!("Unexpected reset argument.");
            return Some(1);
        }
        #[cfg(target_os = "windows")]
        let result = elevate(&Request::Reset {});
        #[cfg(not(target_os = "windows"))]
        let result = execute_privileged(&Request::Reset {});
        return Some(match result {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        });
    }
    if arg != "--firewall-helper" {
        return None;
    }
    let result: Result<i32> = (|| {
        let encoded = args.next().ok_or("Missing firewall request.")?;
        if encoded.len() > 1_000_000 {
            return Err("Firewall request too large.".into());
        }
        let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|e| e.to_string())?;
        let request: Request = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        request.validate()?;
        #[cfg(target_os = "windows")]
        let pipe = match args.next() {
            Some(flag) if flag == "--readback-pipe" => {
                let name = args.next().ok_or("Missing readback pipe.")?;
                crate::readback::validate_name(&name)?;
                Some(name)
            }
            Some(_) => return Err("Unexpected helper argument.".into()),
            None => None,
        };
        if args.next().is_some() {
            return Err("Unexpected helper argument.".into());
        }
        // Read back even after a failed update: the OS, not the requested rule
        // list or process exit code, determines the UI's active blocks.
        let mut outcome = FirewallOutcome {
            operation_error: if matches!(request, Request::Verify {}) {
                None
            } else {
                execute_privileged(&request).err()
            },
            inspection: inspection::inspect(),
        };
        let mut bytes = serde_json::to_vec(&outcome).map_err(|e| e.to_string())?;
        if bytes.len() > inspection::MAX_READBACK_BYTES {
            outcome.inspection = Err("Firewall readback is too large.".into());
            bytes = serde_json::to_vec(&outcome).map_err(|e| e.to_string())?;
        }
        #[cfg(target_os = "windows")]
        if let Some(pipe) = pipe {
            crate::readback::send(&pipe, &bytes)?;
        } else {
            use std::io::Write;
            std::io::stdout()
                .write_all(&bytes)
                .map_err(|e| e.to_string())?;
        }
        #[cfg(not(target_os = "windows"))]
        {
            use std::io::Write;
            std::io::stdout()
                .write_all(&bytes)
                .map_err(|e| e.to_string())?;
        }
        Ok(if outcome.operation_error.is_some() {
            1
        } else {
            0
        })
    })();
    Some(match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    })
}

#[cfg(target_os = "linux")]
const EXECUTABLE_RESTART_REQUIRED: &str = "LobbyLocker's executable was replaced or is unavailable. Fully close LobbyLocker, reopen the app, and retry Apply or Reset. No firewall command was started.";

#[cfg(target_os = "linux")]
fn linux_elevation_executable(
    current: &std::path::Path,
    appimage: Option<&std::path::Path>,
    running: &std::fs::Metadata,
) -> Result<std::path::PathBuf> {
    use std::os::unix::fs::MetadataExt;
    // Never strip Linux's " (deleted)" suffix and silently elevate replacement
    // code. For native binaries, also check that the on-disk helper is still
    // the running build. An AppImage uses its outer image rather than this inode.
    let executable = appimage.unwrap_or(current);
    let metadata =
        std::fs::metadata(executable).map_err(|_| EXECUTABLE_RESTART_REQUIRED.to_owned())?;
    if !executable.is_absolute()
        || !metadata.is_file()
        || (appimage.is_none()
            && (metadata.dev() != running.dev() || metadata.ino() != running.ino()))
    {
        return Err(EXECUTABLE_RESTART_REQUIRED.into());
    }
    Ok(executable.to_owned())
}

pub fn elevate(request: &Request) -> Result<()> {
    let outcome = change_with_readback(request)?;
    if let Some(error) = outcome.operation_error {
        return Err(error);
    }
    outcome.inspection.map(|_| ()).map_err(|error| {
        format!("Firewall operation completed, but its result could not be read: {error}")
    })
}

/// Mutations and readback share the same authentication. A successful command
/// alone never becomes a synthetic active-rules snapshot.
pub fn change_with_readback(request: &Request) -> Result<FirewallOutcome> {
    request.validate()?;
    if matches!(request, Request::Verify {}) {
        return Err("Use the read-only verification operation.".into());
    }
    #[cfg(target_os = "windows")]
    if let Request::Apply { rules } = request {
        // Validate scope and paths before requesting administrator credentials.
        powershell_script(request)?;
        crate::model::validate_program_files(rules)?;
    }
    #[cfg(target_os = "linux")]
    if let Request::Apply { rules } = request {
        if rules.iter().any(|rule| rule.program.is_some()) {
            return Err("Executable-scoped firewall rules are supported on Windows only.".into());
        }
    }
    elevated_readback(request)
}

/// No files, locks, config, or firewall state are written by a Verify request.
pub fn inspect_firewall(elevated: bool) -> Result<Inspection> {
    inspect_with_fallback(inspection::inspect_with_error(), elevated, || {
        elevated_readback(&Request::Verify {})?.inspection
    })
}

fn inspect_with_fallback(
    direct: inspection::InspectionResult<Inspection>,
    elevated: bool,
    authenticate: impl FnOnce() -> Result<Inspection>,
) -> Result<Inspection> {
    match direct {
        Ok(report) => Ok(report),
        Err(inspection::InspectionError::PermissionDenied(_)) if elevated => authenticate(),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FirewallOutcome {
    pub operation_error: Option<String>,
    pub inspection: Result<Inspection>,
}

impl FirewallOutcome {
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > inspection::MAX_READBACK_BYTES {
            return Err("Firewall readback is too large.".into());
        }
        let outcome: Self =
            serde_json::from_slice(bytes).map_err(|e| format!("Invalid firewall readback: {e}"))?;
        if let Ok(report) = &outcome.inspection {
            report.validate()?;
        }
        Ok(outcome)
    }
}

fn elevated_readback(request: &Request) -> Result<FirewallOutcome> {
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(request).map_err(|e| e.to_string())?);
    if payload.len() > 1_000_000 {
        return Err("Firewall request too large.".into());
    }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    {
        // AppImage elevation must use the outer image, not a transient mounted executable.
        let appimage = std::env::var_os("APPIMAGE").map(std::path::PathBuf::from);
        let running = std::fs::metadata("/proc/self/exe")
            .map_err(|_| EXECUTABLE_RESTART_REQUIRED.to_owned())?;
        let executable = linux_elevation_executable(&executable, appimage.as_deref(), &running)?;
        let output = Command::new("/usr/bin/pkexec")
            .arg(executable)
            .args(["--firewall-helper", &payload])
            .output()
            .map_err(|e| format!("Could not request admin access (install polkit): {e}"))?;
        if output.stdout.is_empty() {
            return Err(format!(
                "Firewall operation failed or authorization was cancelled: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        FirewallOutcome::from_bytes(&output.stdout)
    }
    #[cfg(target_os = "windows")]
    {
        let mut command = powershell_command()?;
        let path = executable.to_string_lossy().replace('\'', "''");
        // RunAs cannot redirect stdout. A first-instance, local named pipe
        // returns the actual rules without a privileged write to a user file.
        let receiver = crate::readback::Receiver::new()?;
        let name = receiver.name().to_owned();
        let script = format!("$ErrorActionPreference='Stop'; try {{ $p=Start-Process -FilePath '{path}' -ArgumentList '--firewall-helper','{payload}','--readback-pipe','{name}' -Verb RunAs -Wait -PassThru; [Console]::WriteLine($p.Id); exit $p.ExitCode }} catch {{ Write-Error $_; exit 1 }}");
        let output = command
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output();
        let received = receiver.finish();
        let output = output.map_err(|e| e.to_string())?;
        let pid = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u32>()
            .map_err(|_| {
                format!(
                    "Firewall operation failed or UAC was cancelled: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )
            })?;
        let (client, bytes) = received?;
        if client != pid {
            return Err("Firewall readback came from an unexpected process.".into());
        }
        FirewallOutcome::from_bytes(&bytes)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    Err("Firewall changes are supported on Windows and Linux only.".into())
}

#[cfg(test)]
#[path = "../tests/unit/firewall.rs"]
mod tests;
