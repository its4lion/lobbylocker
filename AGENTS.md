# AGENTS.md

LobbyLocker: Tauri 2 + Rust core + Svelte 5 desktop app that blocks game server IPs via the OS firewall.
`README.md` is the product documentation; this file covers only what is easy to get wrong.

## Setup

- Rust 1.90+, Node 22.12+, pnpm 10.17.1 (pinned via `packageManager`).
- Linux build deps: `libgtk-3-dev libwebkit2gtk-4.1-dev librsvg2-dev pkgconf`. Core-only work needs none of these.
- **`apps/tauri/src-tauri/icons/` is gitignored.** A fresh clone cannot compile the Tauri crate until icons are generated:
  `pnpm --filter @lobbylocker/tauri icons` (from `src/lib/assets/brand/lobbylocker-icon.svg`).
  CI does this before every Rust job. `apps/tauri/src-tauri/gen/` is gitignored too.

## Commands

Verified working from the repo root:

```sh
pnpm check     # pnpm -r check (svelte-check: ui + tauri) THEN cargo check --workspace --all-targets
pnpm test      # pnpm --filter @lobbylocker/tauri test THEN cargo test --workspace
pnpm format    # prettier --write ... THEN cargo fmt --all
```

CI gates (all must pass; run them before claiming a change is done):

```sh
pnpm format:check                                                   # prettier --check + cargo fmt --all --check
pnpm -r check                                                       # svelte-check in packages/ui and apps/tauri
cargo clippy --workspace --all-targets -- -D warnings              # CI enforces clippy; there is no wrapper script
cargo test --workspace
pnpm --filter @lobbylocker/tauri test
```

`pnpm check` runs the JS half before the Rust half, so a Rust-only failure means the JS half already passed.

Fast inner loops:

```sh
cargo check -p lobbylocker-core --all-targets   # ~2s, no GTK/WebKit needed
cargo test  -p lobbylocker-core                # 108 unit + 6 uninstall-hook tests
cargo test -p lobbylocker-core inspection      # substring filter on test names
cargo test -p lobbylocker-core --test uninstall_hooks
pnpm --filter @lobbylocker/tauri exec vitest run src/lib/firewall.test.ts
```

Cargo quirks:

- `Cargo.toml` sets `default-members = ["apps/tauri/src-tauri/core"]`, so a bare `cargo build`/`cargo test` **silently skips the Tauri app**. Always use `--workspace` (or an explicit `-p`) to cover it.
- `cargo check --workspace` builds the Tauri crate and needs GTK3 + WebKitGTK 4.1 plus the generated icons.

CLI (no Node/pnpm needed):

```sh
cargo run -p lobbylocker-core --bin lobbylocker -- help
cargo run -p lobbylocker-core --bin lobbylocker -- preview   # nft script or Windows rule JSON; never elevates
```

## Architecture

- **`lobbylocker-core`** (`apps/tauri/src-tauri/core`) — all real logic, no Tauri types. Modules: `model` (validation, rule resolution), `engine` (orchestration + state), `store` (JSON persistence), `firewall` (rule generation + elevation), `inspection` (OS readback), `installed` (launcher scanning), `launcher_folders` (private), `icons`, `latency` (ICMP only), `catalogue` (network refresh), `i18n`, `connections`, and `windows_firewall` (Windows-only, private).
- **Tauri host crate** (`apps/tauri/src-tauri`, package `lobbylocker-tauri`) — thin layer: `main.rs` (setup + `invoke_handler` + `data_path`), `commands.rs` (IPC DTOs), `platform.rs` (Linux Wayland workaround).
- **Svelte app** (`apps/tauri/src`) — `App.svelte` + `lib/components/`. `lib/backend.svelte.ts` is the Svelte 5 rune-based state holder; `lib/*.ts` are pure logic modules: `action-bar`, `artwork`, `firewall`, `game`, `i18n`, `layout`, `ping`, `requests`, plus `types.ts` (wire shapes).
- `packages/ui/` — CLI-installed shadcn-svelte components. Never hand-write a replacement here; add with `pnpm --filter @lobbylocker/ui ui:add <name>`. The app imports `@lobbylocker/ui/components/<name>` and `@lobbylocker/ui/styles.css`; layout is Tailwind utilities in the components; `apps/tauri/src/app.css` holds only document-level element defaults.
- `data/games/*.json`, `data/locales/*.json` — bundled via `tauri.conf.json` `bundle.resources`. Adding a new data folder requires editing that list; dev builds fall back to the repo dirs (`data_path()` in `main.rs`).

## Non-obvious conventions

Frontend logic must live in testable `.ts`, not in `.svelte`:

- There is **no jsdom, no @testing-library, no component mounting, no vitest config file**. Vitest runs in the default Node environment, so tests can only cover plain logic. That is why behavior is extracted into `lib/*.ts` helpers — when you add logic to a component, put it in a helper module and test it there instead of reaching for a DOM test harness.
- `lib/backend.test.ts` mocks `@tauri-apps/api/core` with `vi.hoisted` and stubs `invoke` + `isTauri`; reuse that pattern. Tests assert against wire-shaped `Status` objects from `lib/types.ts`.
- `lib/requests.ts` serializes every IPC call through a promise tail. Only the backend's busy-guard message is retried; never replay an admin cancellation or a mutation error.

Adding an IPC command requires three edits, not one:

1. `#[tauri::command]` in `commands.rs`, wrapped in `work(state, ...)` so the call runs on `spawn_blocking` behind the shared `busy` guard (rejects concurrent mutations instead of racing). The one deliberate exception is `ensure_fresh_data`: a slow background feed must not raise the busy flag or block Apply, so it bypasses `work` and swallows its own error.
2. Register it in the `generate_handler!` list in `main.rs`.
3. Call it from `lib/backend.svelte.ts`.

Data model (`core/src/model.rs`):

- `Game`, `Preferences`, `Region`, `Endpoint` all use `deny_unknown_fields`. Unknown fields are hard errors.
- `Game::from_json` / `Preferences::from_json` use `read_compatible` with a small legacy allowlist — Game: `rules`, `accent`; Preferences: `profiles`, `enabledRules`, `threshold`, `autoEnabled`, `monitorEnabled`. Those keys are dropped on load and never re-saved. Do not widen the allowlist.
- Game JSON filename must equal the game `id`, or `snapshot()` hard-fails.

Persistence rules in the `store` module:

- User game files under `~/.config/lobbylocker/games/` are **never overwritten** from defaults on upgrade, and load never rewrites existing files.
- `snapshot()` sorts games `cs2`, `deadlock`, `overwatch2` first, then custom ones.

i18n:

- Locale JSON keys are the **English source strings** (`"Apply": "..."`); dynamic messages use `{name}` placeholders substituted in one plain-text pass (`lib/i18n.ts`), so a substituted value cannot inject markup.
- Missing keys fall back to English. Game names, OS diagnostics, and imported source notes are deliberately untranslated.
- `en.json` and `ar.json` are **compiled into the binary** via `include_str!` *and* re-read from the locales dir at runtime; a same-code file in the dir wins. A locale's filename must equal its `code`.
- The locale file declares `direction: "ltr" | "rtl"`; the UI reads it from `app.locale?.direction` — do not hardcode LTR.
- Adding a UI string means adding the key to `data/locales/en.json` (and `ar.json` if localized); a new `<code>.json` needs an app restart to appear.

## Firewall invariants worth preserving

- Owned objects only: Linux nftables table `inet lobbylocker_v1`, Windows group `LobbyLocker.Managed.v1`. Reset removes only those. Never flush the system ruleset or touch routes.
- Linux elevation is a structured base64url `Request` payload (`--firewall-helper`), size-capped and validated, run through `/usr/bin/pkexec`. No shell strings, no frontend-supplied paths. `helper_entry()` must stay the first call in both `main()`s so the helper path never initializes the webview.
- **Windows runs the app itself as administrator** (`requireAdministrator` manifest embedded by both crates' build scripts). Firewall mutation and readback happen **in-process** via native COM — there is no elevation helper, no named pipe, and no `ShellExecuteExW` path. Do not reintroduce a helper process or PowerShell.
- Windows uses native Firewall COM, ICMP, TCP-table, and registry APIs. Native COM settings describe local policy; readback checks Group Policy overrides and reports active rules as unverified on MDM-enrolled devices.
- Blocked / Not blocked labels come from the OS readback, never from saved switches. Linux has no per-program scoping; the helper refuses executable-scoped requests rather than silently widening to system-wide.
- Package uninstall hooks (`linux/deb-prerm.sh`, `linux/rpm-preun.sh`, `windows/hooks.nsh`, `packaging/arch/*.hook`) call `lobbylocker --reset-firewall`. They must run only on a real removal, never on upgrade, and must abort the uninstall if cleanup fails.
- `apps/tauri/scripts/fetch-cloud-ranges.mjs` regenerates bundled `providerScope` ranges. **Endpoint ids must stay identical to the ones the in-app refresh writes** (`{regionId}-{index}`), otherwise a refresh treats every bundled address as new.
- Windows-specific behavior in `inspection` and `windows_firewall` **still needs validation on Windows** — `cargo test -p lobbylocker-core` passes on Linux without proving it.

## Environment

- Linux Wayland: `platform.rs` sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` before GTK init (prevents the Error 71 Wayland crash) and never overwrites an explicit value. `WEBKIT_DISABLE_DMABUF_RENDERER=0` or `GDK_BACKEND=x11 pnpm dev` to test the original path.
- `write EPIPE` from Vite/esbuild after the native process exits is shutdown noise, not a frontend compile failure. Fix the first native startup error.
- Data dir overrides: `LOBBYLOCKER_DATA_DIR`, `LOBBYLOCKER_LOCALES_DIR` (see `data_path()` in `main.rs`).
- `pnpm dev` (Tauri) vs `pnpm --filter @lobbylocker/tauri web:dev`: the latter is browser-only with no backend and the UI says so rather than faking data.
- A `(deleted)` executable path on Linux means a rebuild replaced the binary under a running instance; restart the app instead of trying to make Apply work.
- Release versions are bumped by `scripts/bump-version.py` across `package.json`, `apps/tauri/package.json`, `packages/ui/package.json`, `tauri.conf.json`, `Cargo.toml`, and `Cargo.lock`. Do not hand-edit one of them alone.
- Releasing is two manual steps. `.github/workflows/pre-release.yml` builds a **release candidate** (signed AppImage/DEB/Arch/NSIS) with the next stable version and leaves it as a **draft**; nothing is committed to `main`. `.github/workflows/publish-updater.yml` then publishes that draft (committing the version bump first) and writes the signed `updater` feed — no rebuild. It also runs on `release: published`, so publishing the draft by hand still refreshes the feed.

## Gotchas

- `packages/ui/components.json` aliases `hooks` to `@lobbylocker/ui/hooks`, but `packages/ui/package.json` `exports` has no `./hooks` entry (and `packages/ui/src/hooks` does not exist). Adding a shadcn component that pulls in hooks will fail to resolve until the export is added.
- `pnpm format:web` covers `apps/tauri`, `packages/ui`, and `data/locales/*.json` — **not** `data/games/*.json`. Do not reformat bundled game defaults.
- Expanded layout has a 760×560 minimum and a 1240×860 size; collapsed defaults to 420×800 with a 360×560 minimum. `lib/layout.ts` changes the native minimum at runtime and restores each layout's previous size without overriding configured maximum sizes. Check layout regressions at both minimum sizes.
- Ping only ever runs from an explicit **Check ping** click. Never add a startup or timer-driven ping, and never let latency affect block selections.
