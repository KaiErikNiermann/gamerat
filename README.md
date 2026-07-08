<div align="center">
  <img src="assets/banner.svg" alt="gamerat" width="296" />
</div>

Per-application configuration daemon for gaming mice. Layered on
[ratbagd](https://github.com/libratbag/libratbag), it watches the
focused window and rewrites the device's active profile, DPI, and
button bindings to whatever the active game (or none) calls for.

The daemon is hardware-agnostic via libratbag; gamerat adds:

- abstract profiles stored on disk, decoupled from the device's
  built-in slot count
- focus-driven auto-switching with a fallback "Desktop" baseline
- a Tauri GUI for binding editing and profile management
- a CLI (`gameratctl`) for scripting and headless setups
- a soft-macro pipeline that supplements firmware-level bindings
  via uinput

## Requirements

- Linux with libratbag and ratbagd installed
- A ratbagd-supported mouse — see the [libratbag device
  list](https://github.com/libratbag/libratbag/tree/master/data/devices)
- For the GUI: webkit2gtk-4.1, gtk3, libsoup3
- For KDE Plasma users: the bundled KWin focus-bridge script (auto-
  installed by the daemon)

The daemon and CLI work on any Wayland compositor that implements
`wlr-foreign-toplevel-management` (Sway, Hyprland, river, …), plus X11
and KDE Plasma. GNOME isn't supported — Mutter doesn't expose
window-focus events to unprivileged clients.

## Install

### Arch / Manjaro / EndeavourOS

```sh
# From a release artifact:
sudo pacman -U gamerat-*.pkg.tar.zst

# Or build locally from the repo:
cd packaging/arch && makepkg -si
```

### Debian / Ubuntu, Fedora, generic tarball

See [`packaging/README.md`](packaging/README.md). Every tagged release
ships `.deb`, `.rpm`, and a glibc 2.36+ tarball alongside the Arch
package.

### From source

```sh
cargo build --release
cd crates/gamerat-gui && pnpm install && pnpm tauri build
```

The release tarballs are reproducible from `scripts/build-tarball.sh`
if you want to package it yourself.

## Usage

```sh
# Start the user-scoped daemon (also runs as a systemd user service)
systemctl --user start gamerat-daemon

# Launch the GUI
gamerat-gui

# Or skip the GUI and drive everything from the terminal
gameratctl --help
```

Bindings, DPI stages, LED state, and per-game rules live in
`~/.config/appulsauce/gamerat/` (via `directories::ProjectDirs`).

## Status

Pre-1.0. Daemon, CLI, and GUI are all functional and tested against
the maintainer's hardware (Logitech G502 HERO via HID++ 2.0). Other
devices that work in Piper/ratbagd are expected to work but coverage
is community-driven — see `data/mice/` for the rendered button maps
gamerat ships and `crates/gamerat-gui/src/lib/device-defaults.ts` for
the per-device default-binding tables.

## Workspace layout

| Crate              | Purpose                                                  |
| ------------------ | -------------------------------------------------------- |
| `gamerat-proto`    | D-Bus interface definitions and serde wire types         |
| `gamerat-ratbag`   | Async zbus client for `org.freedesktop.ratbag1`          |
| `gamerat-focus`    | Focus backends: X11, wlr-foreign-toplevel, KWin          |
| `gamerat-gamedb`   | Library scanners for Steam, Lutris, Heroic               |
| `gamerat-input`    | evdev reader + uinput emitter for the soft-macro pipeline |
| `gamerat-daemon`   | Long-running service — the brains of the system          |
| `gamerat-cli`      | `gameratctl` — scriptable client                         |
| `gamerat-gui`      | Tauri 2 + Svelte 5 desktop frontend                      |

## Development

Common tasks are wrapped in a `Justfile`. After a clone:

```sh
git config core.hooksPath .githooks
just check          # cargo fmt + clippy + test + drift + GUI checks
```

Without `just` installed, the pre-push hook still runs the Rust gates
on every push. Bypass with `GAMERAT_SKIP_HOOKS=1 git push` for
in-progress branches; never push to `main` with it set.

### Sync scripts

The `scripts/` directory has companion tools for keeping the project
aligned with upstream:

```sh
just drift                                  # ratbagd XML drift check
just refresh-ratbagd                        # re-pull data/ratbagd/*.xml from a live daemon
just sync-piper /path/to/piper              # diff data/mice/* against upstream
just sync-libratbag /path/to/libratbag      # report unsupported-device gaps
```

### Releases

```sh
just release patch    # 0.0.1 → 0.0.2, syncs every versioned file
                      # commits, tags, pushes — release.yml takes over
just wait-release     # block on the resulting workflow run
```

## License

GPL-2.0-or-later. See [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE) for
third-party attribution — notably the mouse SVG diagrams sourced from
[libratbag/piper](https://github.com/libratbag/piper) and the ratbagd
D-Bus interface definitions from libratbag itself.
