/*
 * gamerat-focus — KWin Script bridge.
 *
 * KWin (Plasma 6) doesn't advertise wlr-foreign-toplevel-management
 * or plasma-window-management to unprivileged Wayland clients, so the
 * gamerat daemon can't observe window focus from outside the compositor.
 * This script runs inside KWin, subscribes to the workspace activation
 * signal, and forwards each focus change to the daemon over D-Bus.
 *
 * License: GPL-2.0-or-later. See data/kwin-script/README.md for install
 * + enable instructions.
 */

const DAEMON_SERVICE   = "org.appulsauce.GameRat1";
const DAEMON_PATH      = "/org/appulsauce/GameRat1";
const DAEMON_INTERFACE = "org.appulsauce.GameRat1";
const METHOD           = "IngestKwinFocus";

function ingest(window) {
    if (!window) {
        // No window has focus (e.g. showing desktop). Not useful for
        // rule matching — skip.
        print("gamerat-focus: activated(null) -> skip");
        return;
    }

    // `resourceClass` is the Wayland app_id (or the X11 WM_CLASS
    // instance for Xwayland clients). Some Plasma versions report it
    // as a QByteArray, so coerce defensively to a JS string.
    const appId = "" + (window.resourceClass || window.resourceName || "");
    const title = "" + (window.caption || "");

    print("gamerat-focus: activated app=" + appId + " title=" + title);

    if (appId === "") {
        // No identifier means no rule can possibly match — don't burn
        // a D-Bus call.
        return;
    }

    callDBus(DAEMON_SERVICE, DAEMON_PATH, DAEMON_INTERFACE, METHOD, appId, title);
}

print("gamerat-focus: script loaded, connecting workspace.windowActivated");
workspace.windowActivated.connect(ingest);

// Emit one event for whatever's currently focused so the daemon's
// `Status.focused_app_id` reflects reality immediately — without it,
// the daemon stays "no focus seen yet" until the user actually
// switches windows.
if (workspace.activeWindow) {
    print("gamerat-focus: firing initial event for active window");
    ingest(workspace.activeWindow);
} else {
    print("gamerat-focus: no active window at script-load time");
}
