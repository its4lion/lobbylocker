//! Exercise removal hooks with a fake cleanup executable, never the OS firewall.

#[cfg(target_os = "linux")]
const DEB_PRERM: &str = include_str!("../../linux/deb-prerm.sh");
#[cfg(target_os = "linux")]
const RPM_PREUN: &str = include_str!("../../linux/rpm-preun.sh");
const NSIS_HOOKS: &str = include_str!("../../windows/hooks.nsh");
const ARCH_HOOK: &str =
    include_str!("../../../../../packaging/arch/lobbylocker-reset-firewall.hook");

#[test]
fn native_packages_register_cleanup_before_removal() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
    let bundle = &config["bundle"];
    assert_eq!(
        bundle["linux"]["deb"]["preRemoveScript"],
        "linux/deb-prerm.sh"
    );
    assert_eq!(
        bundle["linux"]["rpm"]["preRemoveScript"],
        "linux/rpm-preun.sh"
    );
    assert_eq!(
        bundle["windows"]["nsis"]["installerHooks"],
        "windows/hooks.nsh"
    );
    // The uninstaller must be elevated so it can launch the admin-manifest app
    // for firewall cleanup; a per-user uninstaller gets ERROR_ELEVATION_REQUIRED.
    assert_eq!(bundle["windows"]["nsis"]["installMode"], "perMachine");
    for required in [
        "Operation = Remove",
        "Target = lobbylocker",
        "When = PreTransaction",
        "Exec = /usr/bin/lobbylocker --reset-firewall",
        "AbortOnFail",
    ] {
        assert!(ARCH_HOOK.lines().any(|line| line == required));
    }
    assert!(!ARCH_HOOK.contains("Operation = Upgrade"));
    assert!(
        include_str!("../../../../../packaging/arch/PKGBUILD.release")
            .contains("$pkgdir/usr/share/libalpm/hooks/lobbylocker-reset-firewall.hook")
    );
}

#[test]
fn windows_uninstall_checks_launch_errors_and_exit_status_but_skips_updates() {
    assert!(NSIS_HOOKS.contains("!macro NSIS_HOOK_PREUNINSTALL"));
    let guard = NSIS_HOOKS.find("${If} $UpdateMode != 1").unwrap();
    let clear = NSIS_HOOKS.find("ClearErrors").unwrap();
    let reset = NSIS_HOOKS
        .find("ExecWait '\"$INSTDIR\\${MAINBINARYNAME}.exe\" --reset-firewall' $0")
        .unwrap();
    let errors = NSIS_HOOKS.find("${If} ${Errors}").unwrap();
    let exit_status = NSIS_HOOKS.find("${OrIf} $0 != 0").unwrap();
    let failure = NSIS_HOOKS.find("SetErrorLevel 1").unwrap();
    let abort = NSIS_HOOKS.find("Abort").unwrap();
    assert!(guard < clear && clear < reset && reset < errors);
    assert!(errors < exit_status && exit_status < failure && failure < abort);
    assert!(!NSIS_HOOKS.contains("Delete "));
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{DEB_PRERM, RPM_PREUN};
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};

    fn run_hook(
        script: &str,
        args: &[&str],
        cleanup_exit: Option<i32>,
    ) -> (std::process::Output, Vec<String>) {
        let root = tempfile::tempdir().unwrap();
        let cleanup = root.path().join("cleanup helper");
        let log = root.path().join("reset.log");
        if let Some(code) = cleanup_exit {
            fs::write(
                &cleanup,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$LOBBYLOCKER_TEST_RESET_LOG\"\nexit {code}\n"
                ),
            )
            .unwrap();
            fs::set_permissions(&cleanup, fs::Permissions::from_mode(0o700)).unwrap();
        }
        // Replace only the fixed executable path; do not execute the installed
        // binary or depend on privileges, nftables, polkit, or a desktop session.
        assert!(script.contains("/usr/bin/lobbylocker"));
        let hook = root.path().join("hook.sh");
        fs::write(
            &hook,
            script.replace("/usr/bin/lobbylocker", &format!("'{}'", cleanup.display())),
        )
        .unwrap();
        let output = Command::new("/bin/sh")
            .arg(hook)
            .args(args)
            .env("LOBBYLOCKER_TEST_RESET_LOG", &log)
            .output()
            .unwrap();
        let calls = fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect();
        (output, calls)
    }

    #[test]
    fn removal_runs_reset_once_before_success() {
        for (script, args) in [(DEB_PRERM, vec!["remove"]), (RPM_PREUN, vec!["0"])] {
            let (output, calls) = run_hook(script, &args, Some(0));
            assert!(output.status.success());
            assert_eq!(calls, ["--reset-firewall"]);
        }
    }

    #[test]
    fn failed_cleanup_stops_removal() {
        for (script, args) in [(DEB_PRERM, vec!["remove"]), (RPM_PREUN, vec!["0"])] {
            let (output, calls) = run_hook(script, &args, Some(7));
            assert!(!output.status.success());
            assert_eq!(calls, ["--reset-firewall"]);
            assert!(String::from_utf8_lossy(&output.stderr).contains("Uninstall stopped"));
        }
    }

    #[test]
    fn missing_cleanup_tool_stops_removal_instead_of_silently_succeeding() {
        for (script, args) in [(DEB_PRERM, vec!["remove"]), (RPM_PREUN, vec!["0"])] {
            let (output, calls) = run_hook(script, &args, None);
            assert!(!output.status.success());
            assert!(calls.is_empty());
            assert!(String::from_utf8_lossy(&output.stderr).contains("executable is missing"));
        }
    }

    #[test]
    fn upgrades_and_other_maintenance_keep_applied_rules() {
        let cases = [
            (DEB_PRERM, vec!["upgrade", "0.0.2"]),
            (DEB_PRERM, vec!["failed-upgrade", "0.0.1", "0.0.2"]),
            (DEB_PRERM, vec!["deconfigure"]),
            (DEB_PRERM, vec![]),
            (RPM_PREUN, vec!["1"]),
            (RPM_PREUN, vec!["2"]),
            (RPM_PREUN, vec![]),
        ];
        for (script, args) in cases {
            // Even a missing cleanup tool must not break an upgrade.
            let (output, calls) = run_hook(script, &args, None);
            assert!(output.status.success());
            assert!(calls.is_empty());
        }
    }
}
