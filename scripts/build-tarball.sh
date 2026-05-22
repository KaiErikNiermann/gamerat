#!/usr/bin/env bash
# Assemble a generic Linux x86_64 binary tarball from the workspace's
# release outputs. Mirrors the install layout the .deb / .rpm / .pkg
# native packages produce, so users on unsupported distros can extract
# anywhere and get the same result.
#
# Usage:
#   scripts/build-tarball.sh 0.1.0
#
# Assumes `cargo build --release` has already produced:
#   target/release/gamerat-daemon
#   target/release/gameratctl
#   target/release/gamerat-gui
# (release.yml's build-tarball job does this first; locally, run the
# `cargo build` commands from the plan's pre-CI verification section.)
set -euo pipefail

VERSION="${1:?usage: $0 <version>}"
VERSION="${VERSION#v}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

# Stage layout mirrors $PREFIX-rooted install paths so the tarball's
# install.sh can copy with a single `cp -a "${STAGE}/." "${PREFIX}/"`
# regardless of which prefix the user chose.
STAGE_PARENT="$(mktemp -d)"
STAGE="${STAGE_PARENT}/gamerat-${VERSION}"
mkdir -p \
    "${STAGE}/bin" \
    "${STAGE}/share/gamerat/mice" \
    "${STAGE}/share/kwin/scripts" \
    "${STAGE}/share/applications" \
    "${STAGE}/share/icons/hicolor/512x512/apps" \
    "${STAGE}/share/dbus-1/interfaces" \
    "${STAGE}/lib/systemd/user" \
    "${STAGE}/share/licenses/gamerat"

install -m 755 target/release/gamerat-daemon "${STAGE}/bin/gamerat-daemon"
install -m 755 target/release/gameratctl     "${STAGE}/bin/gameratctl"
install -m 755 target/release/gamerat-gui    "${STAGE}/bin/gamerat-gui"

cp -a data/mice/.                      "${STAGE}/share/gamerat/mice/"
# KWin-scanned location (relative to the tarball's install PREFIX) so
# Plasma discovers the focus bridge and the daemon can load it directly.
cp -a data/kwin-script/gamerat-focus   "${STAGE}/share/kwin/scripts/"

install -m 644 data/dbus/org.appulsauce.GameRat1.xml \
    "${STAGE}/share/dbus-1/interfaces/org.appulsauce.GameRat1.xml"
install -m 644 packaging/arch/gamerat.desktop \
    "${STAGE}/share/applications/gamerat.desktop"
install -m 644 crates/gamerat-gui/src-tauri/icons/icon.png \
    "${STAGE}/share/icons/hicolor/512x512/apps/gamerat.png"
install -m 644 packaging/systemd/gamerat-daemon.service \
    "${STAGE}/lib/systemd/user/gamerat-daemon.service"
install -m 644 LICENSE \
    "${STAGE}/share/licenses/gamerat/LICENSE"

install -m 755 packaging/tarball/install.sh   "${STAGE}/install.sh"
install -m 755 packaging/tarball/uninstall.sh "${STAGE}/uninstall.sh"

OUT="${ROOT}/gamerat-${VERSION}-x86_64-linux.tar.gz"
tar -C "${STAGE_PARENT}" -czf "${OUT}" "gamerat-${VERSION}"
rm -rf "${STAGE_PARENT}"

echo "built ${OUT}"
ls -lh "${OUT}"
