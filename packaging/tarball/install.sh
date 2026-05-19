#!/usr/bin/env bash
# Generic install script bundled inside the binary tarball. Runs
# entirely in userspace; assumes /usr/local is writable to the
# caller (use sudo if not).
#
# Usage:
#   ./install.sh                 # PREFIX=/usr/local (default)
#   PREFIX=/opt/gamerat ./install.sh
#   PREFIX="${HOME}/.local" ./install.sh    # per-user install
set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"
SRCDIR="$(cd "$(dirname "$0")" && pwd)"

echo "Installing gamerat into ${PREFIX}…"

mkdir -p "${PREFIX}"
# Each top-level directory in the tarball matches an FHS subdir
# (bin/, share/, lib/), so the install is a single recursive copy.
# `cp -a` preserves the executable bits set by build-tarball.sh.
for dir in bin share lib; do
    if [ -d "${SRCDIR}/${dir}" ]; then
        cp -a "${SRCDIR}/${dir}" "${PREFIX}/"
    fi
done

# Refresh caches so launchers pick the tile up immediately. Only
# meaningful when installing under /usr or /usr/local; per-user
# installs to ~/.local need the matching --local cache (kept out of
# scope for simplicity, users can run gtk-update-icon-cache manually
# if they want it).
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
Installed.

Next steps:
  1. Make sure ratbagd is running:   systemctl --system start ratbagd
  2. Enable the gamerat daemon:      systemctl --user enable --now gamerat-daemon
  3. Launch the GUI from your menu, or run:  ${PREFIX}/bin/gamerat-gui

If \$PREFIX wasn't a standard system path, you may need to:
  - Add ${PREFIX}/bin to PATH
  - Copy lib/systemd/user/gamerat-daemon.service to ~/.config/systemd/user/
EOF
