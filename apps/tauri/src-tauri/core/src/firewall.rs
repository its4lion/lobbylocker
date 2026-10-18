use crate::{
    inspection::{self, Inspection},
    model::{normalize_address, parse_ports, windows_program, FirewallRule, Protocol},
    Result,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::process::Command;
#[cfg(target_os = "linux")]
use std::{io::Write, process::Stdio};

pub const TABLE: &str = "lobbylocker_v1";
pub const OWNER: &str = "LobbyLocker managed firewall rules v1";
pub const WINDOWS_GROUP: &str = "LobbyLocker.Managed.v1";
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

/// A validated native Windows Firewall operation. No shell commands are generated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WindowsRuleSpec {
    pub name: String,
    pub source_id: String,
    pub program: String,
    pub address: String,
    pub protocol: i32,
    pub ports: Option<String>,
}

pub(crate) fn windows_rule_plan(request: &Request) -> Result<Vec<WindowsRuleSpec>> {
    request.validate()?;
    let rules = match request {
        Request::Apply { rules } => rules,
        Request::Reset {} => return Ok(Vec::new()),
        Request::Verify {} => return Err("Verification cannot generate firewall changes.".into()),
    };
    let generation = uuid::Uuid::new_v4().simple().to_string();
    let mut plan = Vec::new();
    let mut identifiers = std::collections::HashSet::new();
    for rule in rules {
        if !identifiers.insert(rule.endpoint.id.to_ascii_lowercase()) {
            return Err("Windows firewall rule IDs must be unique ignoring case.".into());
        }
        let program = windows_program(rule.program.as_deref().ok_or(
            "Windows firewall rules require a game executable. System-wide fallback is not allowed.",
        )?)?;
        let endpoint = &rule.endpoint;
        let address = normalize_address(&endpoint.address)?;
        let ports = parse_ports(&endpoint.ports)?.map(|(start, end)| format!("{start}-{end}"));
        let protocols: &[(i32, &str)] = match (&endpoint.protocol, &ports) {
            (Protocol::Any, None) => &[(256, "Any")],
            (Protocol::Any, Some(_)) => &[(6, "TCP"), (17, "UDP")],
            (Protocol::Tcp, _) => &[(6, "TCP")],
            (Protocol::Udp, _) => &[(17, "UDP")],
        };
        for (protocol, suffix) in protocols {
            plan.push(WindowsRuleSpec {
                name: format!("LobbyLocker.{generation}.{}.{suffix}", endpoint.id),
                source_id: endpoint.id.clone(),
                program: program.clone(),
                address: address.clone(),
                protocol: *protocol,
                ports: ports.clone(),
            });
        }
    }
    Ok(plan)
}

/// Narrow interface shared by the native policy store and fault-injected tests.
#[cfg(any(target_os = "windows", test))]
pub(crate) trait WindowsRuleStore {
    fn managed_names(&mut self) -> Result<Vec<String>>;
    fn add(&mut self, rule: &WindowsRuleSpec) -> Result<()>;
    fn remove(&mut self, name: &str) -> Result<()>;
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn replace_windows_rules(
    store: &mut impl WindowsRuleStore,
    plan: &[WindowsRuleSpec],
) -> Result<()> {
    let old = store.managed_names()?;
    let mut expected: Vec<_> = plan.iter().map(|rule| rule.name.to_lowercase()).collect();
    expected.sort();
    if expected.windows(2).any(|names| names[0] == names[1])
        || old
            .iter()
            .any(|name| expected.binary_search(&name.to_lowercase()).is_ok())
    {
        return Err("Duplicate or colliding Windows firewall rule names.".into());
    }
    let mut created: Vec<&str> = Vec::new();
    for rule in plan {
        if let Err(error) = store.add(rule) {
            let mut cleanup_errors = Vec::new();
            for name in created.iter().rev() {
                if let Err(cleanup) = store.remove(name) {
                    cleanup_errors.push(cleanup);
                }
            }
            return Err(if cleanup_errors.is_empty() {
                error
            } else {
                format!(
                    "{error} Replacement-rule cleanup also failed: {}",
                    cleanup_errors.join("; ")
                )
            });
        }
        created.push(rule.name.as_str());
    }
    // Keep the old generation until every replacement exists. If removing old
    // rules fails, retain the complete new generation and report the actual OS
    // snapshot rather than rolling back protection after a partial deletion.
    for name in old {
        store.remove(&name).map_err(|error| {
            format!("Could not remove an old LobbyLocker rule: {error}. Firewall state must be verified before retrying.")
        })?;
    }
    let mut remaining: Vec<_> = store
        .managed_names()?
        .into_iter()
        .map(|name| name.to_lowercase())
        .collect();
    remaining.sort();
    if remaining != expected {
        return Err("LobbyLocker firewall rules do not match the completed operation.".into());
    }
    Ok(())
}

/// A read-only description of native Windows Firewall changes for the CLI.
pub fn windows_preview(request: &Request) -> Result<String> {
    let rules = windows_rule_plan(request)?;
    serde_json::to_string_pretty(&serde_json::json!({
        "action": if matches!(request, Request::Reset {}) { "reset" } else { "apply" },
        "group": WINDOWS_GROUP,
        "rules": rules,
    }))
    .map_err(|error| error.to_string())
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
        crate::windows_firewall::execute(request)
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
        windows_rule_plan(request)?;
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
        use std::os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        };
        use windows::Win32::{
            Foundation::RPC_E_CHANGED_MODE,
            System::Com::{
                CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
            },
        };
        use windows_sys::Win32::{
            Foundation::WAIT_OBJECT_0,
            System::Threading::{GetProcessId, WaitForSingleObject, INFINITE},
            UI::{
                Shell::{
                    ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC,
                    SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NO_CONSOLE, SHELLEXECUTEINFOW,
                },
                WindowsAndMessaging::SW_HIDE,
            },
        };
        struct Apartment(bool);
        impl Drop for Apartment {
            fn drop(&mut self) {
                if self.0 {
                    unsafe { CoUninitialize() };
                }
            }
        }
        let initialized =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
            return Err(format!(
                "Could not initialize administrator request: {initialized}"
            ));
        }
        let _apartment = Apartment(initialized.is_ok());
        // RunAs cannot redirect stdout. Keep the bounded, local named pipe and
        // authenticate its client against the process handle returned by Windows.
        let receiver = crate::readback::Receiver::new()?;
        let file: Vec<u16> = executable
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let verb: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();
        // Only our validated base64url payload and generated pipe token enter
        // the argument list; the executable path is passed separately.
        let arguments: Vec<u16> = format!(
            "--firewall-helper {payload} --readback-pipe {}",
            receiver.name(),
        )
        .encode_utf16()
        .chain(Some(0))
        .collect();
        let mut launch = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS
                | SEE_MASK_NOASYNC
                | SEE_MASK_NO_CONSOLE
                | SEE_MASK_FLAG_NO_UI,
            lpVerb: verb.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: arguments.as_ptr(),
            nShow: SW_HIDE,
            ..Default::default()
        };
        if unsafe { ShellExecuteExW(&mut launch) } == 0 {
            return Err(format!(
                "Firewall operation failed or UAC was cancelled: {}",
                std::io::Error::last_os_error()
            ));
        }
        if launch.hProcess.is_null() {
            return Err("Windows did not return a firewall helper process handle.".into());
        }
        let process = unsafe { OwnedHandle::from_raw_handle(launch.hProcess) };
        let pid = unsafe { GetProcessId(process.as_raw_handle()) };
        if pid == 0 {
            return Err(format!(
                "Could not identify firewall helper: {}",
                std::io::Error::last_os_error()
            ));
        }
        // UAC may take as long as the user needs. The helper's readback is
        // consumed concurrently, so FlushFileBuffers cannot deadlock this wait.
        if unsafe { WaitForSingleObject(process.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
            return Err(format!(
                "Could not wait for firewall helper: {}",
                std::io::Error::last_os_error()
            ));
        }
        let (client, bytes) = receiver.finish()?;
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
