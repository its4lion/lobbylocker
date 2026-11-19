//! Read, derive, and write the LobbyLocker release version.
//!
//! The release workflows share this tool so a version is always written to the
//! same set of metadata files. `Cargo.toml` is the source of truth; the
//! workspace version is copied into `Cargo.lock`.
//!
//! Commands:
//!   set <version>          Write <version> into every version metadata file.
//!   derive-stable          Print the next stable version for this tree.
//!
//! Options for `set`:
//!   --allow-prerelease     Accept X.Y.Z-<prerelease> versions.
//!   --dry-run              Validate everything without writing.
//!
//! Options for `derive-stable`:
//!   --tags-file <path>     File with one tag per line (default: stdin).
//!   --current <version>    Override the current version (default: Cargo.toml).

use std::env;
use std::fs;
use std::io::{self, Read};
use std::ops::Range;
use std::process::ExitCode;

type Failure = String;

const TOML_FILE: &str = "Cargo.toml";
const LOCK_FILE: &str = "Cargo.lock";
const LOCK_PACKAGES: [&str; 2] = ["lobbylocker-core", "lobbylocker-app"];

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> &'static str {
    "usage: bump-version set <version> [--allow-prerelease] [--dry-run]\n       \
     bump-version derive-stable [--tags-file <path>] [--current <version>]"
}

fn run(args: Vec<String>) -> Result<(), Failure> {
    let Some((command, rest)) = args.split_first() else {
        return Err(usage().to_string());
    };
    match command.as_str() {
        "set" => command_set(rest),
        "derive-stable" => command_derive_stable(rest),
        other => Err(format!("unknown command: {other}\n{}", usage())),
    }
}

/// A stable semantic version: three numeric parts, no leading zeros.
fn parse_stable(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = numeric(parts.next()?)?;
    let minor = numeric(parts.next()?)?;
    let patch = numeric(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn numeric(part: &str) -> Option<u64> {
    if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if part.len() > 1 && part.starts_with('0') {
        return None;
    }
    part.parse().ok()
}

/// A parsed semantic version: its base and optional pre-release part.
type ParsedVersion<'a> = ((u64, u64, u64), Option<&'a str>);

/// A semantic version with an optional pre-release part. Build metadata is
/// rejected: it is ignored by semver precedence, it cannot be used in a
/// release tag, and it would make the packaged version differ from the
/// application version.
fn parse_any(version: &str) -> Option<ParsedVersion<'_>> {
    if version.contains('+') {
        return None;
    }
    let (base, prerelease) = match version.split_once('-') {
        Some((base, prerelease)) => (base, Some(prerelease)),
        None => (version, None),
    };
    let base = parse_stable(base)?;
    if let Some(prerelease) = prerelease {
        let valid = !prerelease.is_empty()
            && prerelease.split('.').all(|identifier| {
                !identifier.is_empty()
                    && identifier
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            });
        if !valid {
            return None;
        }
    }
    Some((base, prerelease))
}

fn read_text(file: &str) -> Result<String, Failure> {
    fs::read_to_string(file).map_err(|error| format!("{file}: {error}"))
}

fn write_text(file: &str, text: &str) -> Result<(), Failure> {
    fs::write(file, text).map_err(|error| format!("{file}: {error}"))
}

fn read_current_version() -> Result<String, Failure> {
    let text = read_text(TOML_FILE)?;
    workspace_package_version(&text)
        .ok_or_else(|| format!("{TOML_FILE} has no [workspace.package] version"))
}

fn command_set(args: &[String]) -> Result<(), Failure> {
    let mut version: Option<&str> = None;
    let mut allow_prerelease = false;
    let mut dry_run = false;
    for arg in args {
        match arg.as_str() {
            "--allow-prerelease" => allow_prerelease = true,
            "--dry-run" => dry_run = true,
            other if other.starts_with("--") => return Err(format!("unknown option: {other}")),
            other => {
                if version.replace(other).is_some() {
                    return Err(format!("unexpected argument: {other}"));
                }
            }
        }
    }
    let version = version.ok_or_else(|| format!("missing version\n{}", usage()))?;
    let (base, prerelease) =
        parse_any(version).ok_or_else(|| format!("Invalid semantic version: {version}"))?;
    if prerelease.is_some() && !allow_prerelease {
        return Err(format!("Invalid stable semantic version: {version}"));
    }

    let current = read_current_version()?;
    let current_base = parse_stable(&current)
        .ok_or_else(|| format!("Cannot parse the current version: {current}"))?;
    if prerelease.is_some() {
        // A pre-release of the current base sorts below the stable release.
        if base <= current_base {
            return Err(format!(
                "Pre-release {version} must be newer than the current version {current}"
            ));
        }
    } else if base < current_base {
        return Err(format!(
            "Cannot lower the version from {current} to {version}"
        ));
    }

    let updates = apply_version(version)?;
    if dry_run {
        println!("Validated all version metadata for {version}");
        return Ok(());
    }
    // Write only after every file validates, so a failure never leaves a
    // half-bumped tree behind.
    for (file, text) in &updates {
        write_text(file, text)?;
    }
    println!("Updated all version metadata to {version}");
    Ok(())
}

/// Return the updated content of every version metadata file.
fn apply_version(version: &str) -> Result<Vec<(&'static str, String)>, Failure> {
    let mut updates = Vec::new();

    let toml = read_text(TOML_FILE)?;
    let (updated, count) = bump_cargo_toml(&toml, version);
    if count != 1 {
        return Err(format!(
            "{TOML_FILE}: expected 1 version entries, found {count}"
        ));
    }
    if workspace_package_version(&updated).as_deref() != Some(version) {
        return Err(format!(
            "{TOML_FILE} has version {:?}, expected {version}",
            workspace_package_version(&updated)
        ));
    }
    updates.push((TOML_FILE, updated));

    let lock = read_text(LOCK_FILE)?;
    let (updated, count) = bump_cargo_lock(&lock, version);
    if count != LOCK_PACKAGES.len() {
        return Err(format!(
            "{LOCK_FILE}: expected {} version entries, found {count}",
            LOCK_PACKAGES.len()
        ));
    }
    let found = lock_versions(&updated);
    for package in LOCK_PACKAGES {
        let actual = found
            .iter()
            .find(|(name, _)| name == package)
            .map(|(_, version)| version.as_str());
        if actual != Some(version) {
            return Err(format!(
                "{LOCK_FILE} has {package} at {actual:?}, expected {version}"
            ));
        }
    }
    updates.push((LOCK_FILE, updated));

    Ok(updates)
}

fn command_derive_stable(args: &[String]) -> Result<(), Failure> {
    let mut tags_file: Option<&str> = None;
    let mut current: Option<&str> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--tags-file" => {
                tags_file = Some(iter.next().ok_or("--tags-file needs a path")?);
            }
            "--current" => {
                current = Some(iter.next().ok_or("--current needs a version")?);
            }
            other => return Err(format!("unknown option: {other}\n{}", usage())),
        }
    }
    let current = match current {
        Some(version) => version.to_string(),
        None => read_current_version()?,
    };
    let tags = match tags_file {
        Some(path) => fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?,
        None => {
            let mut text = String::new();
            io::stdin()
                .read_to_string(&mut text)
                .map_err(|error| error.to_string())?;
            text
        }
    };

    let current_base = parse_stable(&current)
        .ok_or_else(|| format!("Cannot determine the current version: {current}"))?;
    let mut versions = vec![current_base];
    // Ignore tags that are not stable `vX.Y.Z` releases.
    for line in tags.lines() {
        let tag = line.trim();
        if tag.is_empty() {
            continue;
        }
        let tag = tag.strip_prefix("refs/tags/").unwrap_or(tag);
        let tag = tag.strip_suffix("^{}").unwrap_or(tag);
        if let Some(version) = tag.strip_prefix('v') {
            if let Some(version) = parse_stable(version) {
                versions.push(version);
            }
        }
    }
    let (major, minor, patch) = versions
        .into_iter()
        .max()
        .ok_or_else(|| format!("Cannot determine the current version: {current}"))?;
    println!("{major}.{minor}.{}", patch + 1);
    Ok(())
}

/// Replace the `version` line that follows `[workspace.package]`.
fn bump_cargo_toml(text: &str, version: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    let mut expect_version = false;
    for line in text.split_inclusive('\n') {
        let (body, ending) = split_line_ending(line);
        if expect_version {
            expect_version = false;
            if let Some(span) = toml_version_span(body) {
                count += 1;
                out.push_str(&replace_span(body, span, version));
                out.push_str(ending);
                continue;
            }
        } else if body == "[workspace.package]" {
            expect_version = true;
        }
        out.push_str(body);
        out.push_str(ending);
    }
    (out, count)
}

/// The span of the quoted value in a `version = "..."` line.
fn toml_version_span(body: &str) -> Option<Range<usize>> {
    let start = take(body, "version = \"")?;
    let close = body[start..].find('"')?;
    Some(start..start + close)
}

/// Read the `version` of `[workspace.package]`.
fn workspace_package_version(text: &str) -> Option<String> {
    let mut expect_version = false;
    for line in text.split_inclusive('\n') {
        let (body, _) = split_line_ending(line);
        if expect_version {
            if let Some(span) = toml_version_span(body) {
                return Some(body[span].to_string());
            }
            expect_version = false;
        } else if body == "[workspace.package]" {
            expect_version = true;
        }
    }
    None
}

/// Replace the `version` line of each workspace package in `Cargo.lock`.
fn bump_cargo_lock(text: &str, version: &str) -> (String, usize) {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    let mut index = 0;
    while index < lines.len() {
        let (body, ending) = split_line_ending(lines[index]);
        if body == "[[package]]" && index + 2 < lines.len() {
            let (name_body, name_ending) = split_line_ending(lines[index + 1]);
            let (version_body, version_ending) = split_line_ending(lines[index + 2]);
            let workspace =
                lock_package_name(name_body).filter(|name| LOCK_PACKAGES.contains(name));
            if let (Some(_), Some(span)) = (workspace, toml_version_span(version_body)) {
                count += 1;
                out.push_str(body);
                out.push_str(ending);
                out.push_str(name_body);
                out.push_str(name_ending);
                out.push_str(&replace_span(version_body, span, version));
                out.push_str(version_ending);
                index += 3;
                continue;
            }
        }
        out.push_str(body);
        out.push_str(ending);
        index += 1;
    }
    (out, count)
}

/// Read `(name, version)` for every package in `Cargo.lock`.
fn lock_versions(text: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index + 2 < lines.len() {
        let (body, _) = split_line_ending(lines[index]);
        if body == "[[package]]" {
            let (name_body, _) = split_line_ending(lines[index + 1]);
            let (version_body, _) = split_line_ending(lines[index + 2]);
            if let (Some(name), Some(span)) = (
                lock_package_name(name_body),
                toml_version_span(version_body),
            ) {
                found.push((name.to_string(), version_body[span].to_string()));
            }
        }
        index += 1;
    }
    found
}

fn lock_package_name(body: &str) -> Option<&str> {
    body.strip_prefix("name = \"")?.strip_suffix('"')
}

fn split_line_ending(line: &str) -> (&str, &str) {
    if let Some(body) = line.strip_suffix("\r\n") {
        (body, "\r\n")
    } else if let Some(body) = line.strip_suffix('\n') {
        (body, "\n")
    } else {
        (line, "")
    }
}

fn take(text: &str, prefix: &str) -> Option<usize> {
    text.strip_prefix(prefix)
        .map(|rest| text.len() - rest.len())
}

fn replace_span(text: &str, span: Range<usize>, value: &str) -> String {
    format!("{}{}{}", &text[..span.start], value, &text[span.end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOML: &str = concat!(
        "[package]\n",
        "name = \"lobbylocker-app\"\n",
        "version.workspace = true\n",
        "\n",
        "[workspace.package]\n",
        "version = \"0.0.6\"\n",
        "edition = \"2021\"\n",
    );

    const LOCK: &str = concat!(
        "[[package]]\nname = \"lobbylocker-app\"\nversion = \"0.0.6\"\n\n",
        "[[package]]\nname = \"lobbylocker-core\"\nversion = \"0.0.6\"\n\n",
        "[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n",
    );

    #[test]
    fn stable_versions_reject_leading_zeros_and_short_forms() {
        assert_eq!(parse_stable("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_stable("0.0.6"), Some((0, 0, 6)));
        assert_eq!(parse_stable("01.2.3"), None);
        assert_eq!(parse_stable("1.2"), None);
        assert_eq!(parse_stable("1.2.3.4"), None);
        assert_eq!(parse_stable("1.2.3-rc.1"), None);
    }

    #[test]
    fn any_versions_accept_prereleases_but_not_build_metadata() {
        assert_eq!(parse_any("1.2.3"), Some(((1, 2, 3), None)));
        assert_eq!(parse_any("1.2.3-rc.1"), Some(((1, 2, 3), Some("rc.1"))));
        assert_eq!(parse_any("1.2.3+build.1"), None);
        assert_eq!(parse_any("1.2.3-"), None);
        assert_eq!(parse_any("1.2.3-rc..1"), None);
    }

    #[test]
    fn cargo_toml_bumps_only_the_workspace_version() {
        let (updated, count) = bump_cargo_toml(TOML, "9.9.9");
        assert_eq!(count, 1);
        assert_eq!(
            workspace_package_version(&updated).as_deref(),
            Some("9.9.9")
        );
        assert!(updated.contains("version.workspace = true"));
        assert!(updated.contains("edition = \"2021\""));
    }

    #[test]
    fn cargo_lock_bumps_both_workspace_packages() {
        let (updated, count) = bump_cargo_lock(LOCK, "9.9.9");
        assert_eq!(count, 2);
        assert_eq!(
            lock_versions(&updated),
            vec![
                ("lobbylocker-app".to_string(), "9.9.9".to_string()),
                ("lobbylocker-core".to_string(), "9.9.9".to_string()),
                ("serde".to_string(), "1.0.0".to_string()),
            ]
        );
    }

    #[test]
    fn line_endings_are_preserved() {
        let text = "[workspace.package]\r\nversion = \"0.0.6\"\r\n";
        let (updated, count) = bump_cargo_toml(text, "9.9.9");
        assert_eq!(count, 1);
        assert_eq!(updated, "[workspace.package]\r\nversion = \"9.9.9\"\r\n");
    }
}
