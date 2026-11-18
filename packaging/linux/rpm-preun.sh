#!/bin/sh
set -eu

# RPM passes 0 for final removal and 1 for an upgrade.
if [ "${1:-1}" -eq 0 ]; then
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
