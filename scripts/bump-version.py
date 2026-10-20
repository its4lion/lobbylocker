#!/usr/bin/env python3
"""Read, derive, and write the LobbyLocker release version.

The release workflows share this script so a version is always written to the
same set of metadata files.

Commands:
  set <version>          Write <version> into every version metadata file.
  derive-stable          Print the next stable version for this tree.

Options for `set`:
  --allow-prerelease     Accept X.Y.Z-<prerelease> versions.
  --dry-run              Validate everything without writing.

Options for `derive-stable`:
  --tags-file <path>     File with one tag per line (default: stdin).
  --current <version>    Override the current version (default: package.json).
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

STABLE_VERSION = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)")
# Build metadata is rejected: it is ignored by semver precedence, it cannot be
# used in a release tag, and it would make the packaged version differ from
# the application version.
ANY_VERSION = re.compile(
    r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
)

JSON_FILES = (
    "package.json",
    "apps/tauri/package.json",
    "packages/ui/package.json",
    "apps/tauri/src-tauri/tauri.conf.json",
)


class Failure(Exception):
    """A version problem that must stop the release."""


def read_current_version() -> str:
    return json.loads(read_text("package.json"))["version"]


def read_text(file: str) -> str:
    # newline="" keeps the checked-out line endings instead of translating them.
    with Path(file).open("r", encoding="utf-8", newline="") as handle:
        return handle.read()


def write_text(file: str, text: str) -> None:
    # newline="" keeps the checked-out line endings instead of translating them.
    with Path(file).open("w", encoding="utf-8", newline="") as handle:
        handle.write(text)


def apply_version(version: str) -> dict[str, str]:
    """Return the updated content of every version metadata file."""
    updates: dict[str, str] = {}

    def bump(file: str, pattern: str, expected_count: int = 1) -> str:
        text = read_text(file)
        updated, count = re.subn(pattern, lambda match: f'{match[1]}"{version}"', text)
        if count != expected_count:
            raise Failure(f"{file}: expected {expected_count} version entries, found {count}")
        updates[file] = updated
        return updated

    for file in JSON_FILES:
        updated = bump(file, r'(?m)^([ \t]*"version"[ \t]*:[ \t]*)"[^"\n]*"')
        actual = json.loads(updated)["version"]
        if actual != version:
            raise Failure(f"{file} has version {actual}, expected {version}")

    updated = bump("Cargo.toml", r'(?m)^(\[workspace\.package\]\r?\nversion = )"[^"\n]*"')
    actual = tomllib.loads(updated)["workspace"]["package"]["version"]
    if actual != version:
        raise Failure(f"Cargo.toml has version {actual}, expected {version}")

    updated = bump(
        "Cargo.lock",
        r'(?m)^(\[\[package\]\]\r?\nname = "(?:lobbylocker-core|lobbylocker-tauri)"\r?\nversion = )"[^"\n]*"',
        expected_count=2,
    )
    packages = tomllib.loads(updated)["package"]
    workspace = {
        package["name"]: package["version"]
        for package in packages
        if package["name"] in {"lobbylocker-core", "lobbylocker-tauri"}
    }
    expected = {"lobbylocker-core": version, "lobbylocker-tauri": version}
    if workspace != expected:
        raise Failure(f"Cargo.lock workspace versions are {workspace}, expected {expected}")

    return updates


def command_set(args: argparse.Namespace) -> int:
    version = args.version
    match = ANY_VERSION.fullmatch(version)
    if not match:
        raise Failure(f"Invalid semantic version: {version}")
    prerelease = match.group(4)
    if prerelease and not args.allow_prerelease:
        raise Failure(f"Invalid stable semantic version: {version}")

    current = read_current_version()
    current_match = STABLE_VERSION.fullmatch(current)
    if not current_match:
        raise Failure(f"Cannot parse the current version: {current}")
    current_base = tuple(int(part) for part in current_match.groups())
    base = tuple(int(part) for part in match.group(1, 2, 3))

    if prerelease:
        # A pre-release of the current base sorts below the stable release.
        if base <= current_base:
            raise Failure(
                f"Pre-release {version} must be newer than the current version {current}"
            )
    elif base < current_base:
        raise Failure(f"Cannot lower the version from {current} to {version}")

    updates = apply_version(version)
    if args.dry_run:
        print(f"Validated all version metadata for {version}")
        return 0

    # Preserve formatting and dependency versions; write only after every file validates.
    for file, text in updates.items():
        write_text(file, text)
    print(f"Updated all version metadata to {version}")
    return 0


def read_tags(tags_file: str | None) -> set[str]:
    if tags_file:
        text = Path(tags_file).read_text(encoding="utf-8")
    else:
        text = sys.stdin.read()
    tags = set()
    for line in text.splitlines():
        tag = line.strip()
        if not tag:
            continue
        if tag.startswith("refs/tags/"):
            tag = tag[len("refs/tags/") :]
        tags.add(tag.removesuffix("^{}"))
    return tags


def parse_stable(version: str) -> tuple[int, int, int] | None:
    match = STABLE_VERSION.fullmatch(version)
    if not match:
        return None
    return tuple(int(part) for part in match.groups())


def next_base(current: str, tags: set[str]) -> str:
    """Return the next stable version after the current one and every tag."""
    known = [parse_stable(current)]
    for tag in tags:
        if tag.startswith("v"):
            known.append(parse_stable(tag[1:]))
    versions = [version for version in known if version is not None]
    if not versions:
        raise Failure(f"Cannot determine the current version: {current}")
    major, minor, patch = max(versions)
    return f"{major}.{minor}.{patch + 1}"


def command_derive_stable(args: argparse.Namespace) -> int:
    current = args.current or read_current_version()
    tags = read_tags(args.tags_file)

    print(next_base(current, tags))
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    setter = subparsers.add_parser("set", help="write a version into every metadata file")
    setter.add_argument("version")
    setter.add_argument("--allow-prerelease", action="store_true")
    setter.add_argument("--dry-run", action="store_true")
    setter.set_defaults(handler=command_set)

    stable = subparsers.add_parser("derive-stable", help="print the next stable version")
    stable.add_argument("--tags-file")
    stable.add_argument("--current")
    stable.set_defaults(handler=command_derive_stable)

    args = parser.parse_args(argv)
    try:
        return args.handler(args)
    except Failure as error:
        print(str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
