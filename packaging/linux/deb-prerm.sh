#!/bin/sh
set -eu

# Upgrades keep the currently applied policy. Only a real removal cleans up.
if [ "${1:-}" = "remove" ]; then
  if [ ! -x /usr/bin/lobbylocker ]; then
    echo "Uninstall stopped: the LobbyLocker firewall cleanup executable is missing. Reinstall the package and retry." >&2
    exit 1
  fi
  if ! /usr/bin/lobbylocker --reset-firewall; then
    echo "Uninstall stopped: LobbyLocker firewall cleanup failed. The application has been kept so you can retry." >&2
    exit 1
  fi
fi

exit 0
