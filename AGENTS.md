# AGENTS.md

LobbyLocker: Tauri 2 + Rust core + Svelte 5 desktop app that blocks game server IPs via the OS firewall.
`README.md` is long and mostly accurate product documentation; this file covers only what is easy to get wrong.

## Commands

Verified working from the repo root:

```sh
pnpm check                       # pnpm -r check (svelte-check: ui + tauri) THEN cargo check --workspace --all-targets
pnpm test                        # vitest run (tauri app) THEN cargo test --workspace
pnpm format                      # prettier --write ... THEN cargo fmt --all
```

Fast inner loops:

```sh
cargo check -p lobbylocker-core --all-targets   # ~8s, no GTK/WebKit needed
cargo test  -p lobbylocker-core
pnpm --filter @lobbylocker/tauri test
pnpm --filter @lobbylocker/tauri exec vitest run <test-file>
cargo test -p lobbylocker-core inspection      # single module
pnpm --filter @lobbylocker/tauri exec tauri build --debug --no-bundle   # compile check without bundling
```

Order matters: `pnpm check` runs the JS half before the Rust half, so a Rust-only failure means the JS half already passed.

Cargo quirks:

- `Cargo.toml` sets `default-members = ["apps/tauri/src-tauri/core"]`, so a bare `cargo build`/`cargo test` **silently skips the Tauri app**. Always use `--workspace` (or an explicit `-p`) to cover it.
- `cargo check --workspace` builds the Tauri crate and needs GTK3 + WebKitGTK 4.1 (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `librsvg2-dev`, `pkgconf`). Core-only work needs neither.
- No clippy script exists. `cargo fmt --all` is the only Rust style gate.

CLI (no Node/pnpm needed):

```sh
cargo run -p lobbylocker-core --bin lobbylocker -- help
cargo run -p lobbylocker-core --bin lobbylocker -- preview   # previews nft rules or native Windows rule data; never elevates
```

## Architecture

- **`lobbylocker-core`** (inside the Tauri crate dir) — all real logic, no Tauri types. Modules, whatever file layout is in use: `model` (validation, rule resolution), `engine` (orchestration + state), `store` (JSON persistence), `firewall` (elevation/helper), `inspection` (OS readback), `installed` (launcher scanning), `icons`, `latency` (ICMP only), `catalogue` (network refresh), `i18n`, `connections`, `readback` (Windows-only).
- **Tauri host crate** — thin layer: entry point (setup + `invoke_handler`), commands (IPC DTOs), platform (Linux Wayland workaround).
- **Svelte app** — `App.svelte` + components, plus **pure logic modules** for action-bar, firewall, game, artwork, and i18n.
- `packages/ui/` — CLI-installed shadcn-svelte components. Never hand-write a replacement component here; add with `pnpm --filter @lobbylocker/ui ui:add <name>`. App imports `@lobbylocker/ui/components/<name>` and `@lobbylocker/ui/styles.css`; app layout is Tailwind utilities in the components; `apps/tauri/src/app.css` holds only document-level element defaults.
- `data/games/*.json`, `data/locales/*.json` — bundled via `tauri.conf.json` `bundle.resources`. Adding a new data folder requires editing that list; dev builds fall back to the repo dirs.

## Non-obvious conventions

Frontend logic must live in testable `.ts`, not in `.svelte`:

- There is **no jsdom, no @testing-library, no component mounting, no vitest config file**. Vitest runs in the default Node environment, so tests can only cover plain logic. That is why the repo extracts behavior into `lib/*.ts` helpers — when you add logic to a component, put the logic in a helper module and test it there instead of reaching for a DOM test harness.
- `lib/backend.test.ts` mocks `@tauri-apps/api/core` with `vi.hoisted` and stubs `invoke` + `isTauri`; reuse that pattern. Tests assert against wire-shaped `Status` objects from `lib/types.ts`.

Adding an IPC command requires three edits, not one:

1. `#[tauri::command]` in the commands module, wrapped in `work(state, ...)` so the call runs on `spawn_blocking` behind the shared `busy` guard (rejects concurrent mutations instead of racing).
2. Register it in the `generate_handler!` list in the host entry point.
3. Call it from the frontend backend helper.

Firewall invariants worth preserving:

- Owned objects only: Linux nftables table `inet lobbylocker_v1`, Windows group `LobbyLocker.Managed.v1`. Reset removes only those. Never flush the system ruleset or touch routes.
- Elevation is a structured base64url `Request` payload (`--firewall-helper`), size-capped and validated. No shell strings, no frontend-supplied paths. `helper_entry()` must stay the first call in both `main()`s so the helper path never initializes the webview.
- Windows UAC cannot redirect stdout, so readback goes over a bounded local named pipe validated against the launched helper's PID (the `readback` module).
- Windows uses native Firewall COM, ICMP, TCP-table, and registry APIs. The elevated helper is launched with `ShellExecuteExW`; do not reintroduce PowerShell or console subprocesses for background work.
- Blocked / Not blocked labels come from the OS readback, never from saved switches. Linux has no per-program scoping; the helper refuses executable-scoped requests rather than silently widening to system-wide.
- Native COM firewall settings describe local policy. Readback checks Group Policy overrides and reports active rules as unverified on MDM-enrolled devices because effective MDM policy is not established by those APIs.
- Windows-specific behavior in the inspection and firewall modules **still needs validation on Windows** — `cargo test -p lobbylocker-core` passes on Linux without proving Windows behavior.

Persistence rules in the `store` module:

- User game files under `~/.config/lobbylocker/games/` are **never overwritten** from defaults on upgrade, and load never rewrites existing files.
- Legacy `profiles`, `enabledRules`, and game-level `rules` are intentionally ignored on load and omitted from saves. Do not reintroduce them.
- Game JSON filename must equal the game `id`, or `snapshot()` hard-fails.

i18n:

- Locale JSON keys are the **English source strings** (`"Apply": "..."`); dynamic messages use `{name}` placeholders substituted in one plain-text pass (frontend i18n helper), so a substituted value cannot inject markup.
- Missing keys fall back to English. Game names, OS diagnostics, and imported source notes are deliberately untranslated.
- Adding a UI string means adding the key to `data/locales/en.json` (and `ar.json` if localized); a new `<code>.json` needs an app restart to appear.

## Environment

- Linux Wayland: the platform module sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` before GTK init (prevents the Error 71 Wayland crash) and never overwrites an explicit value. `WEBKIT_DISABLE_DMABUF_RENDERER=0` or `GDK_BACKEND=x11 pnpm dev` to test the original path.
- `write EPIPE` from Vite/esbuild after the native process exits is shutdown noise, not a frontend compile failure. Fix the first native startup error.
- Data dir overrides: `LOBBYLOCKER_DATA_DIR`, `LOBBYLOCKER_LOCALES_DIR` (see `data_path()` in the host entry point).
- `pnpm dev` (Tauri) vs `pnpm --filter @lobbylocker/tauri web:dev`: the latter is browser-only with no backend and the UI says so rather than faking data.
- A `(deleted)` executable path on Linux means a rebuild replaced the binary under a running instance; restart the app instead of trying to make Apply work.

## Gotchas

- `packages/ui/components.json` aliases `hooks` to `@lobbylocker/ui/hooks`, but `packages/ui/package.json` `exports` has no `./hooks` entry (and `packages/ui/src/hooks` does not exist). Adding a shadcn component that pulls in hooks will fail to resolve until the export is added.
- `pnpm format:web` covers `apps/tauri`, `packages/ui`, and `data/locales/*.json` — **not** `data/games/*.json`. Do not reformat bundled game defaults.
- Expanded layout has a 760×560 minimum; collapsed layout defaults to 420×800 with a 360×560 minimum. `lib/layout.ts` changes the native minimum at runtime and restores each layout's previous size without overriding configured maximum sizes. Check layout regressions at both minimum sizes.
- Ping only ever runs from an explicit **Check ping** click. Never add a startup or timer-driven ping, and never let latency affect block selections.
