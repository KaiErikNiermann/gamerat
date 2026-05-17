#!/usr/bin/env bash
# Install + enable the gamerat KWin Script bridge.
#
# Plasma 6 doesn't expose wlr-foreign-toplevel-management to
# unprivileged clients, so the daemon can't observe window focus
# directly. This script lives inside KWin and forwards each activation
# over D-Bus. See data/kwin-script/README.md for the long version.
#
# Usage: scripts/install-kwin-script.sh

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
src="$repo_root/data/kwin-script/gamerat-focus"
dest_dir="$HOME/.local/share/kwin/scripts"
dest="$dest_dir/gamerat-focus"

if [[ ! -d "$src" ]]; then
    echo "error: source tree missing: $src" >&2
    exit 1
fi

if [[ -d "$dest" ]]; then
    echo "→ updating existing install at $dest"
else
    echo "→ installing to $dest"
fi
install -d "$dest_dir"
cp -r "$src" "$dest"

# Flip the kwinrc switch on. kwriteconfig6 is shipped by kconfig.
echo "→ kwriteconfig6 --file kwinrc --group Plugins --key gamerat-focusEnabled true"
kwriteconfig6 --file kwinrc --group Plugins --key gamerat-focusEnabled true

# Two-step reload: reconfigure + re-load the script. KWin caches the
# script registry so kwriteconfig alone isn't sufficient on 6.6+.
echo "→ qdbus6 reconfigure"
qdbus6 org.kde.KWin /KWin reconfigure || true

echo "→ qdbus6 loadScript + start"
qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript \
    "$dest/contents/code/main.js" gamerat-focus || true
qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.start || true

echo
echo "Verifying:"
qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.isScriptLoaded gamerat-focus || true

cat <<'EOF'

Done. Next steps:
  1. Start the daemon:    cargo run -p gamerat-daemon --release
  2. Open the GUI and switch windows — the StatusCard "Focused app"
     line should update in real time, and the Dev panel (visible in
     `pnpm tauri dev`) should show focus-changed events arriving.

If "Focused app" still doesn't update, check:
  - busctl --user list | grep appulsauce        (is the daemon on the bus?)
  - journalctl --user -t kwin_wayland | tail    (KWin script logs)
EOF
