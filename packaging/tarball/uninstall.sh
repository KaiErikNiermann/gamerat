#!/usr/bin/env bash
# Best-effort uninstall: removes the files install.sh dropped. Uses
# the same PREFIX as install.sh defaults to (/usr/local). Pass
# PREFIX=... if you installed somewhere else.
set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"

echo "Removing gamerat from ${PREFIX}…"

# Binaries.
rm -f "${PREFIX}/bin/gamerat-daemon" \
      "${PREFIX}/bin/gameratctl" \
      "${PREFIX}/bin/gamerat-gui"

# Asset bundle.
rm -rf "${PREFIX}/share/gamerat"
rm -f "${PREFIX}/share/applications/gamerat.desktop"
rm -f "${PREFIX}/share/icons/hicolor/512x512/apps/gamerat.png"
rm -f "${PREFIX}/share/dbus-1/interfaces/org.appulsauce.GameRat1.xml"
rm -f "${PREFIX}/lib/systemd/user/gamerat-daemon.service"
rm -rf "${PREFIX}/share/licenses/gamerat"

# Refresh launcher caches so the tile disappears immediately.
if [ "${PREFIX}" = "/usr" ] || [ "${PREFIX}" = "/usr/local" ]; then
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache --quiet --force \
            "${PREFIX}/share/icons/hicolor" 2>/dev/null || true
    fi
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database --quiet \
            "${PREFIX}/share/applications" 2>/dev/null || true
    fi
fi

cat <<EOF
Uninstalled. State files under \$XDG_CONFIG_HOME/gamerat/ and
\$XDG_STATE_HOME/gamerat/ are left in place — remove manually if
you want a clean slate.
EOF
