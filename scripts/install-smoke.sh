#!/usr/bin/env bash
# Distro-agnostic install + smoke test for the per-distro CI jobs.
#
# Auto-detects the package file in CWD (.deb / .rpm / .pkg.tar.zst /
# .tar.gz), installs it with the right tool, then runs:
#
#   - Static checks: every binary present + responds to --version,
#     systemd unit syntactically valid, D-Bus XML well-formed,
#     .desktop file passes desktop-file-validate, icon file exists,
#     mouse + kwin-script bundles present at their expected paths.
#
#   - Live probe: spawn `gamerat-daemon --no-ratbagd` under a fresh
#     dbus-run-session and verify it claims org.appulsauce.GameRat1
#     on the session bus within ~3 seconds. Requires no hardware, no
#     ratbagd, no graphical session.
#
# Assumes the script is run as root inside a clean distro container.
# Adjust the package-manager calls if you want to run as a non-root
# user with sudo.
set -euo pipefail

# ─────────────────────────────────────────────────────────────────────
# 1. Locate the artifact and install it.
# ─────────────────────────────────────────────────────────────────────
shopt -s nullglob
DEB=( gamerat*.deb )
RPM=( gamerat*.rpm )
PKG=( gamerat-*.pkg.tar.zst )
TGZ=( gamerat-*-x86_64-linux.tar.gz )

if [ ${#DEB[@]} -gt 0 ]; then
    echo "::group::Installing .deb (${DEB[0]})"
    apt-get update -qq
    # apt-get install ./path.deb pulls the package's runtime deps.
    apt-get install -y --no-install-recommends "./${DEB[0]}"
    echo "::endgroup::"
elif [ ${#RPM[@]} -gt 0 ]; then
    echo "::group::Installing .rpm (${RPM[0]})"
    dnf install -y "./${RPM[0]}"
    echo "::endgroup::"
elif [ ${#PKG[@]} -gt 0 ]; then
    echo "::group::Installing .pkg.tar.zst (${PKG[0]})"
    pacman -Syu --noconfirm --needed
    pacman -U --noconfirm "./${PKG[0]}"
    echo "::endgroup::"
elif [ ${#TGZ[@]} -gt 0 ]; then
    echo "::group::Extracting + install.sh from ${TGZ[0]}"
    tar xzf "${TGZ[0]}"
    DIR="${TGZ[0]%-x86_64-linux.tar.gz}"
    (cd "${DIR}" && PREFIX=/usr ./install.sh)
    echo "::endgroup::"
else
    echo "FAIL: no gamerat package artifact found in $(pwd)" >&2
    ls -la >&2
    exit 1
fi

# ─────────────────────────────────────────────────────────────────────
# 2. Static checks. Each block fails-fast on the first miss.
# ─────────────────────────────────────────────────────────────────────
echo "::group::Binaries present + respond to --version"
for bin in gamerat-daemon gameratctl gamerat-gui; do
    command -v "$bin" >/dev/null || { echo "missing binary: $bin"; exit 1; }
done
gamerat-daemon --version
gameratctl --version
# gamerat-gui talks to webkit on launch but `--version` should
# short-circuit before any GUI bring-up. If Tauri's clap layer
# eventually changes, revisit; for now it prints the cargo-package
# version and exits 0.
gamerat-gui --version
echo "::endgroup::"

echo "::group::Asset validation"
# `systemd-analyze --user verify` needs a usable user manager (XDG
# runtime dir, dbus session, …) which bare CI containers don't
# provide. Pass the absolute unit path to the system-mode verifier
# instead — it does the same syntax + key-value validation against
# the unit-file grammar regardless of which manager would load it.
systemd-analyze verify /usr/lib/systemd/user/gamerat-daemon.service
xmllint --noout /usr/share/dbus-1/interfaces/org.appulsauce.GameRat1.xml
desktop-file-validate /usr/share/applications/gamerat.desktop
test -f /usr/share/icons/hicolor/512x512/apps/gamerat.png || \
    { echo "icon missing"; exit 1; }
test -d /usr/share/gamerat/mice || \
    { echo "mouse SVG bundle missing"; exit 1; }
test -f /usr/share/kwin/scripts/gamerat-focus/contents/code/main.js || \
    { echo "kwin-script bundle missing from /usr/share/kwin/scripts"; exit 1; }
test -f /usr/lib/udev/rules.d/60-gamerat-uinput.rules || \
    { echo "soft-input udev rule missing"; exit 1; }
test -f /usr/lib/modules-load.d/gamerat-uinput.conf || \
    { echo "soft-input modules-load drop-in missing"; exit 1; }
echo "::endgroup::"

# ─────────────────────────────────────────────────────────────────────
# 3. Session-bus claim probe.
#
# The daemon registers org.appulsauce.GameRat1 on the SESSION bus
# during startup; the ratbag client opens the SYSTEM bus separately.
# Running with --no-ratbagd skips the system-bus dance entirely, so
# the session-bus name claim is observable in a clean container.
# ─────────────────────────────────────────────────────────────────────
echo "::group::Session-bus name claim (--no-ratbagd)"
dbus-run-session -- bash -c '
    set -e
    gamerat-daemon --no-ratbagd &
    PID=$!
    trap "kill ${PID} 2>/dev/null || true" EXIT
    for i in 1 2 3 4 5 6; do
        if busctl --user list 2>/dev/null \
                | grep -q org.appulsauce.GameRat1; then
            echo "session-bus name claimed (poll iteration $i)"
            exit 0
        fi
        sleep 0.5
    done
    echo "FAIL: daemon did not claim org.appulsauce.GameRat1 within 3s" >&2
    busctl --user list 2>/dev/null | head -20 >&2 || true
    exit 1
'
echo "::endgroup::"

echo "smoke test OK"
