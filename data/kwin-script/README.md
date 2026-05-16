# KWin Script bridge

KDE Plasma 6 doesn't advertise the wlr-foreign-toplevel-management or
plasma-window-management Wayland protocols to unprivileged clients, so
the gamerat daemon can't observe window focus directly. This script
runs *inside* KWin (where focus tracking is freely available) and
forwards each activation to the daemon over D-Bus.

The daemon's `IngestKwinFocus` D-Bus method receives the events and
pushes them through `KwinBackend`, which the dispatch loop polls
identically to any other backend. From the daemon's perspective, focus
tracking on KDE looks just like wlr-foreign-toplevel-management on
Sway / Hyprland.

## Layout

```
gamerat-focus/
├── metadata.json        # KPackage/KWin Script manifest
└── contents/
    └── code/
        └── main.js      # the script itself
```

Mirror this tree under `~/.local/share/kwin/scripts/` to install.

## Install

Recommended path (proper KPackage install — KWin picks it up cleanly):

```sh
# From the repo root:
kpackagetool6 -t KWin/Script --install data/kwin-script/gamerat-focus
```

Or for hand-tuned development, drop the tree under the user-scope
KWin scripts dir directly:

```sh
install -d ~/.local/share/kwin/scripts/
cp -r data/kwin-script/gamerat-focus ~/.local/share/kwin/scripts/
```

## Enable

**GUI** (most reliable on Plasma 6.6+):

> *System Settings → Window Management → KWin Scripts*, toggle
> **"gamerat focus bridge"** on. KWin both flips the kwinrc key and
> triggers the discovery / load / start sequence for you.

**Shell** path — needs the explicit `loadScript` + `start` calls
because `kwriteconfig + reconfigure` alone doesn't currently re-scan
new scripts on Plasma 6.6:

```sh
kwriteconfig6 --file kwinrc --group Plugins --key gamerat-focusEnabled true
qdbus6 org.kde.KWin /KWin reconfigure
qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript \
    ~/.local/share/kwin/scripts/gamerat-focus/contents/code/main.js \
    gamerat-focus
qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.start
```

Verify it's running:

```sh
qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.isScriptLoaded gamerat-focus
# → true
```

## Disable / uninstall

```sh
kwriteconfig6 --file kwinrc --group Plugins --key gamerat-focusEnabled false
qdbus org.kde.KWin /KWin reconfigure
rm -rf ~/.local/share/kwin/scripts/gamerat-focus
```

## What it does

Subscribes to `workspace.windowActivated` and emits one D-Bus call per
focus change:

```text
callDBus(
    "org.appulsauce.GameRat1",
    "/org/appulsauce/GameRat1",
    "org.appulsauce.GameRat1",
    "IngestKwinFocus",
    appId,    // window.resourceClass
    title     // window.caption
)
```

Plus one synthetic event at script-load time for whatever's currently
focused, so the daemon's state reflects reality without waiting for the
next focus change.

## Security

The script runs in KWin's standard JS sandbox. It makes one outgoing
D-Bus call (no method registration, no global state, no shell access).
The daemon receiving the calls is a user-session service on the
session bus — no root, no system bus.

## Troubleshooting

- **Script enabled but daemon sees no events:** run `gameratctl watch`
  in a terminal and switch windows. If nothing appears, check that
  `org.appulsauce.GameRat1` is on the session bus
  (`busctl --user list | grep appulsauce`).
- **Reload the script after editing `main.js`:** the KWin GUI doesn't
  pick changes up automatically. Disable, re-enable, and reconfigure
  (the three-command shell sequence above, with `true` again).
- **Where do `console.log` from the script go?**
  `journalctl --user -t kwin_wayland` (or `_dwayland`, depending on
  your Plasma version).
