# AGENTS.md

LobbyLocker: pure-Rust native desktop app (Rune + `lobbylocker-core`) that
blocks game server IPs via the OS firewall.
`README.md` is the product documentation; this file covers only what is easy to get wrong.

## Setup

- Rust 1.99.1+. No Node/pnpm — the monorepo is Cargo only.
- Linux runtime: a Vulkan or OpenGL driver for the Rune window plus
  `nftables`, `pkexec`, and a desktop polkit agent for firewall changes.
- Rune is pinned by git rev in the workspace `Cargo.toml`:
  `rune = { git = "https://github.com/anomalyco/rune", rev = "..." }`
  for both `rune` and `rune-kit`. Bump the rev to follow `dev`.

## Commands

Verified working from the repo root:

```sh
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

CI gates (run them before claiming a change is done):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Fast inner loops:

```sh
cargo check -p lobbylocker-core --all-targets   # ~2s, no GPU needed
cargo test  -p lobbylocker-core                # 126 unit + 6 uninstall-hook tests
cargo test -p lobbylocker-core inspection      # substring filter on test names
cargo test -p lobbylocker-core --test uninstall_hooks
cargo test -p lobbylocker-app                 # headless Rune view tests
```

Cargo quirks:

- `Cargo.toml` sets `default-members = ["."] (the root package)`, so a bare `cargo build`
  / `cargo run` builds the app. Always use `--workspace` (or an explicit `-p`)
  to cover the core.
- Linux builds need no GTK/WebKit. Headless runs need no display at all.

CLI (no GUI needed):

```sh
cargo run -p lobbylocker-core --bin lobbylocker -- help
cargo run -p lobbylocker-core --bin lobbylocker -- preview   # nft script or Windows rule JSON; never elevates
```

## Architecture

- **`lobbylocker-core`** (`crates/core`) — all real logic, no UI types.
  Modules: `model` (validation, rule resolution), `engine` (orchestration +
  state), `store` (JSON persistence), `firewall` (rule generation +
  elevation), `inspection` (OS readback), `installed` (launcher scanning),
  `launcher_folders` (private), `icons`, `banners` (local art lookup),
  `latency` (ICMP only), `catalogue`
  (network refresh), `i18n`, `connections`, and `windows_firewall`
  (Windows-only, private). Bins: `lobbylocker` (CLI + Linux `--firewall-helper` /
  `--reset-firewall` entry).
- **Rune app** (repo root package `lobbylocker-app`, bin `lobbylocker-app`)
  — `src/lib.rs` exports the modules; `src/main.rs` is a thin entry point
  (`helper_entry()` first, then `rune::run(AppSpec)`). State in `state.rs`
  (`AppState`, `Latest` workers, `poll`/`setup`); views in `view.rs`,
  `sidebar.rs`, `regions.rs`, `forms.rs`, `settings.rs`, `connections.rs`,
  `dialogs.rs`. `tests/ui.rs` drives `rune::Runtime` headlessly.
- `data/games/*.json`, `data/locales/*.json` — resolved at runtime: env
  override (`LOBBYLOCKER_DATA_DIR`, `LOBBYLOCKER_LOCALES_DIR`) → executable
  layout → repo `data/` in dev (`data_dirs()` in `state.rs`).

## Non-obvious conventions

- UI logic lives in the app crate's modules, not inline in view fns.
  Tests can only build views in `Runtime` (no DOM), so behavior is factored
  into testable helpers plus `tests/ui.rs` scene/interaction tests. Follow
  the `Runtime::new(state, view, size, env)` pattern from Rune's `hello`.
- Slow work (firewall, ping, scan) runs on `rune::Latest` workers and lands
  through `state::poll`. Handlers must never block on pkexec/ping.
  `LOBBYLOCKER_NO_FIREWALL` disables elevation workers (tests, CI).
- Adding a data dir requires updating `data_dirs()` — packaged data must be
  staged so the executable layout lookup finds it.
- Banner art is **local only**: `crates/core/src/banners.rs` looks in the
  Steam client's own cache (`appcache/librarycache/<app id>/…/library_hero.jpg`)
  and in the art the user installed themselves
  (`userdata/*/config/grid/<app id>*.png|jpg`). Nothing is downloaded and no
  URL is ever fetched for art. The app decodes it on a `Latest` worker and
  draws it as the game header's backdrop; a game without local art keeps the
  plain title header.

Data model (`crates/core/src/model.rs`):

- `Game`, `Preferences`, `Region`, `Endpoint` all use `deny_unknown_fields`.
  Unknown fields are hard errors.
- `Game::from_json` / `Preferences::from_json` use `read_compatible` with a
  small legacy allowlist — Game: `rules`, `accent`; Preferences: `profiles`,
  `enabledRules`, `threshold`, `autoEnabled`, `monitorEnabled`. Those keys are
  dropped on load and never re-saved. Do not widen the allowlist.
- Game JSON filename must equal the game `id`, or `snapshot()` hard-fails.

Persistence rules in the `store` module:

- User game files under `~/.config/lobbylocker/games/` are **never overwritten**
  from defaults on upgrade, and load never rewrites existing files.
- Old builds saved every detected Steam game as an empty placeholder
  (`steam-<appid>.json`, no regions). Those files stay on disk, so views must
  filter games without a server list and without an executable out
  (Settings → Game files does this).
- `snapshot()` sorts games `cs2`, `deadlock`, `overwatch2` first, then custom ones.

i18n:

- Locale JSON keys are the **English source strings** (`"Apply": "..."`); dynamic
  messages use `{name}` placeholders substituted in one plain-text pass
  (`core/src/i18n.rs`), so a substituted value cannot inject markup.
- Missing keys fall back to English. Game names, OS diagnostics, and imported
  source notes are deliberately untranslated.
- `en.json` and `ar.json` are **compiled into the binary** via `include_str!`
  *and* re-read from the locales dir at runtime; a same-code file in the dir wins.
  A locale's filename must equal its `code`.
- The locale file declares `direction: "ltr" | "rtl"`; the UI reads it from
  the selected locale metadata — do not hardcode LTR.
- Adding a UI string means adding the key to `data/locales/en.json`
  (and `ar.json` if localized); a new `<code>.json` needs an app restart to appear.

## Firewall invariants worth preserving

- Owned objects only: Linux nftables table `inet lobbylocker_v1`, Windows group
  `LobbyLocker.Managed.v1`. Reset removes only those. Never flush the system ruleset
  or touch routes.
- Linux elevation is a structured base64url `Request` payload (`--firewall-helper`),
  size-capped and validated, run through `/usr/bin/pkexec`. No shell strings,
  no frontend-supplied paths. `helper_entry()` must stay the first call in both
  `main()`s so the helper path never initializes the window.
- **Windows runs the app itself as administrator** (`requireAdministrator` manifest
  embedded by the core crate's build script). Firewall mutation and readback happen
  **in-process** via native COM — there is no elevation helper, no named pipe,
  and no `ShellExecuteExW` path. Do not reintroduce a helper process or PowerShell.
- Windows uses native Firewall COM, ICMP, TCP-table, and registry APIs. Native COM
  settings describe local policy; readback checks Group Policy overrides and
  reports active rules as unverified on MDM-enrolled devices.
- Blocked / Not blocked labels come from the OS readback, never from saved switches.
  Linux has no per-program scoping; the helper refuses executable-scoped requests
  rather than silently widening to system-wide.
- Package uninstall hooks (`packaging/linux/deb-prerm.sh`,
  `packaging/linux/rpm-preun.sh`, `packaging/windows/hooks.nsh`,
  `packaging/arch/*.hook`) call `lobbylocker --reset-firewall`. They must run
  only on a real removal, never on upgrade, and must abort the uninstall if cleanup
  fails.
- `scripts/fetch-cloud-ranges.mjs` regenerates bundled `providerScope` ranges.
  **Endpoint ids must stay identical to the ones the in-app refresh writes**
  (`{regionId}-{index}`), otherwise a refresh treats every bundled address as new.
- Windows-specific behavior in `inspection` and `windows_firewall` **still needs
  validation on Windows** — `cargo test` passes on Linux without proving it.

## Environment

- Linux Wayland crashes from the old WebKit path no longer apply, but window
  placement (always-on-top, exact positions) is a no-op on Wayland — do not
  depend on it.
- `write EPIPE` from Vite/esbuild after the native process exits was shutdown
  noise from the old Tauri build. Pure Rust has no such step.
- Data dir overrides: `LOBBYLOCKER_DATA_DIR`, `LOBBYLOCKER_LOCALES_DIR`
  (see `data_dirs()` in `src/state.rs`).
- `cargo run -p lobbylocker-app` vs `cargo run -p lobbylocker-core --bin
  lobbylocker`: the former opens the window; the latter is the headless CLI
  (`--reset-firewall` cleanup helper used by package hooks).
- A `(deleted)` executable path on Linux means a rebuild replaced the binary
  under a running instance; restart the app instead of trying to make Apply work.
- Release versions are bumped by `scripts/bump-version.py` across `package.json`,
  `Cargo.toml`, and `Cargo.lock`. Do not hand-edit one of them alone.
- Releasing is two manual steps. `.github/workflows/pre-release.yml` builds a
  **release candidate** (signed staged packages: Arch, DEB, AppImage, NSIS) with
  the next stable version and leaves it as a **draft**; nothing is committed to
  `main`. `.github/workflows/publish-updater.yml` then publishes that draft
  (committing the version bump first) — no rebuild.

## Gotchas

- Rune pins by git rev, so `cargo` vendors the full Rune workspace into
  `~/.cargo/git/checkouts/`. First fetch is slow; CI caches it.
- Rune's `--snap`/`--tree` need an env with a software Vulkan/ICD stack in CI
  (gpu init). The `tests` workflow's `headless` job runs on `ubuntu-24.04`
  where llvmpipe provides it.
- `--snap` scenes are deterministic: `cs2`, `blocked`, `settings`,
  `connections`, `empty`, `reset`, `edit`, `compact`, `update`. Scene fns must
  stay UI-only — never write config or firewall state.
- There is **no updater** in the app. A newer build is announced by a dot and
  an "Update" button in the sidebar footer (next to the version), and the
  "Quit LobbyLocker" button lives in the same footer. `LOBBYLOCKER_FAKE_UPDATE=<version>`
  or `--scene update` previews that state; the button only puts the notice away.
- `rune::is_headless()` guards startup elevation and fresh-data refresh; a
  plain `cargo test` is NOT headless (no `--snap` args), so tests must set
  `allow_firewall = false` before toggling blocks.
- Expanded layout is 1240×860; compact 420×800 with a 360×560 minimum.
  `state.rs` resizes via `Tx::resize_window` only on startup (saved layout).
  Check layout regressions at both minimum sizes. A window dragged below 720
  points wide switches to the compact layout, and growing it back returns to
  the expanded one; there is no manual toggle any more.
- Ping only ever runs from an explicit **Check ping** click. Never add a
  startup or timer-driven ping, and never let latency affect block selections.
