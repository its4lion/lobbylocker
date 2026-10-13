<div align="center">
  <img
    src="apps/tauri/src/lib/assets/brand/lobbylocker-icon.svg"
    alt="LobbyLocker"
    width="88"
  />

  <h1>LobbyLocker</h1>

  <p><strong>Leave the lobby. Keep your latency.</strong><br />
  A lightweight game server region blocker for Linux and Windows.</p>

  <p>
    <a href="https://github.com/its4lion/LobbyLocker/releases"><strong>Download</strong></a> ·
    <a href="#how-to-use">Getting started</a> ·
    <a href="#important-behavior">How it works</a> ·
    <a href="#troubleshooting">Troubleshooting</a> ·
    <a href="#development">Contribute</a>
  </p>

  <p>
    <a href="https://github.com/its4lion/LobbyLocker/actions/workflows/tests.yml"><img src="https://github.com/its4lion/LobbyLocker/actions/workflows/tests.yml/badge.svg" alt="Tests" /></a>
    <a href="https://github.com/its4lion/LobbyLocker/actions/workflows/clippy.yml"><img src="https://github.com/its4lion/LobbyLocker/actions/workflows/clippy.yml/badge.svg" alt="Clippy" /></a>
    <a href="https://github.com/its4lion/LobbyLocker/releases"><img src="https://img.shields.io/github/v/release/its4lion/lobbylocker" alt="Release" /></a>
    <a href="https://img.shields.io/badge/Built_with-Rust-1ed760"><img src="https://img.shields.io/badge/Built_with-Rust-1ed760" alt="Built with Rust" /></a>
    <a href="https://img.shields.io/badge/Linux_%C2%B7_Windows-24292f"><img src="https://img.shields.io/badge/Linux_%C2%B7_Windows-24292f" alt="Linux and Windows" /></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-1ed760" alt="MIT license" /></a>
  </p>
</div>

LobbyLocker blocks the game server regions you would rather not play on. Pick
the regions you want to avoid and LobbyLocker manages its own operating-system
firewall rules — no game files touched, no code injected, nothing rerouted.

> [!WARNING]
> LobbyLocker is still in beta. Server addresses can change, Windows builds are
> unsigned, and blocking a shared IP range can affect other applications.

## Features

- Region blocking for Overwatch 2, Deadlock, and Counter-Strike 2
- Custom games, regions, IP addresses, subnets, protocols, and ports
- Installed-game discovery and custom library folders
- Firewall status read directly from the operating system
- Manual ping checks that never change selections or firewall rules
- English and Arabic interfaces with right-to-left support

## Download

Download a versioned build from [GitHub Releases](https://github.com/its4lion/LobbyLocker/releases).

| Platform                       | Package                |
| ------------------------------ | ---------------------- |
| Windows 11                     | NSIS `.exe` installer  |
| Ubuntu 24.04 LTS               | `.deb` package         |
| Arch Linux                     | `.pkg.tar.zst` package |
| Other compatible Linux systems | AppImage (best effort) |

> [!WARNING]
> `lobbylocker-bin` is not available in the AUR yet because new AUR account
> registration is currently disabled. Until it becomes available for `yay`,
> install the downloaded Arch package directly:

```sh
sudo pacman -U ./lobbylocker-0.0.1-1-x86_64.pkg.tar.zst
```

Windows SmartScreen may warn because Windows builds are not code-signed. Linux
requires `nftables`, `pkexec`, and a working desktop polkit agent.

Windows installers and AppImages check for signed versioned updates when the app
opens and ask before installing. Ubuntu and Arch packages continue to update
through their package files or package manager.

## How to use

1. Open LobbyLocker and choose a detected game or add a custom game.
2. Select the regions you do **not** want to use.
3. Approve the operating-system permission prompt.

Rules stay active after LobbyLocker closes. Use **Settings → Reset** to remove
all LobbyLocker rules.

## Important behavior

- **Windows:** rules apply only to the exact game executable you select.
- **Linux:** rules apply system-wide to the selected addresses.
- LobbyLocker changes only its own firewall objects: `LobbyLocker.Managed.v1`
  on Windows and `inet lobbylocker_v1` on Linux.
- LobbyLocker does not modify game files, inject code, launch games, or route
  traffic through another server.
- Ping and connection scans are manual and never alter firewall rules.

**What the rules match**

- **Overwatch 2:** the Google Cloud ranges its servers sit in, over UDP. Login,
  voice and downloads use TCP and are never blocked.
- **Deadlock and CS2:** Valve's Steam Datagram Relay POPs, from Valve's published
  configuration. The game server sits behind one.
- **Never ICMP.** A blocked region still answers ping. That is not a failed
  block.

**What can still go wrong**

- Steam relays can be re-routed, so an occasional unexpected server is normal.
- A party leader can pull you into a region you blocked.
- Addresses rotate. Refresh before assuming a block broke.
- A match already queued still completes.
- Test with a custom game; the Overwatch practice range runs on a fixed host.
- No anti-cheat, matchmaking-region, or ranked-match guarantees.

## Uninstalling

The Windows uninstaller and Ubuntu/Arch package removal automatically reset
LobbyLocker-owned firewall rules **before** removing the application. If cleanup
fails or cannot be confirmed, removal stops so the cleanup tool remains available.
Normal updates keep the applied rules. Unrelated firewall rules are never removed.

**AppImages cannot run cleanup when their file is deleted**, including removal by
Shelly. Use **Settings → Reset**, wait for a successful confirmation, then close
LobbyLocker and remove the AppImage. For automatic cleanup on uninstall, use the
Ubuntu or Arch package instead.

Saved preferences and custom game files are kept for reinstalling. Deleting app
files directly, forcing removal, or disabling package hooks bypasses automatic
firewall cleanup.

## Games and server data

LobbyLocker includes starter data for Overwatch 2, Deadlock, and CS2. Server
lists can be incomplete or outdated, so every game can be edited locally.

**Refresh official list** fetches Google's published cloud ranges for
Overwatch 2 and Valve's Steam Datagram Relay configuration for Deadlock and
CS2. Neither is an official list of game servers.

Each game is stored as a JSON file:

- Linux: `~/.config/lobbylocker/games/`
- Windows: `%APPDATA%\lobbylocker\games\`

Existing game files are never overwritten automatically during an upgrade.
Overwatch cloud ranges are provider-owned ranges, not an official Blizzard
server list. LobbyLocker is not affiliated with Blizzard, Valve, or Google.

## Troubleshooting

| Problem                                 | What to do                                                                       |
| --------------------------------------- | -------------------------------------------------------------------------------- |
| A game is missing                       | Use **Scan games**, add its library folder, or choose its executable.            |
| Firewall status cannot be read          | Approve the permission prompt, then click **Retry**.                             |
| A blocked region still connects         | **Refresh official list**, then queue a new match — not the one in progress.     |
| Ping still answers on a blocked region  | Expected. Ping is ICMP and is never blocked.                                     |
| Another app loses connectivity on Linux | Shared addresses are blocked system-wide; change the selection or use **Reset**. |
| Removing an AppImage                    | Use **Settings → Reset** first.                                                  |

If the UI cannot open, an installed binary can remove owned rules with:

```sh
lobbylocker --reset-firewall
```

On Linux this command needs root privileges (`sudo lobbylocker --reset-firewall`
for a native package, or `pkexec ./LobbyLocker.AppImage --reset-firewall` using
your actual AppImage path). Windows requests administrator approval.

## Development

LobbyLocker uses Tauri 2, Rust, Svelte 5, and TypeScript. Development requires
Rust 1.90+, Node.js 22.12+, pnpm 10.17.1, and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform.

```sh
pnpm install
pnpm dev
```

Quality checks:

```sh
pnpm format:check
pnpm check
pnpm test
cargo clippy --workspace --all-targets -- -D warnings
```

## License

[MIT](LICENSE)
