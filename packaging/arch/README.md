# Arch Linux packaging

`PKGBUILD` is a working makepkg recipe. The package is named `gamerat`,
installs the daemon (`gamerat-daemon`), CLI (`gameratctl`), and Tauri-based
GUI (`gamerat-gui`) under `/usr/bin/`, plus a `.desktop` entry, a 512×512
icon under `hicolor`, the canonical systemd user unit from
`packaging/systemd/gamerat-daemon.service`, the D-Bus interface descriptor
under `/usr/share/dbus-1/interfaces/`, the mouse SVG bundle, and the KWin
focus script.

## Local build

```sh
cd packaging/arch
makepkg -si
```

The `prepare()` step rsyncs the workspace root into `src/`, the `build()`
step does `cargo build --release --workspace --exclude gamerat-gui`
followed by `pnpm tauri build --no-bundle` for the GUI, and `package()`
assembles the install layout. `options=(!lto)` is set because `rusqlite`'s
bundled feature compiles `sqlite3.c` via `cc-rs`, and makepkg's default
`LTOFLAGS=-flto=auto` makes the resulting `.a` incompatible with
`rust-lld`'s mixed-bitcode dance.

## CI

Tag-driven builds run through the `build-arch` job in
`.github/workflows/release.yml` inside an `archlinux:latest` container.
That job is the source of truth for the GitHub Releases upload; the AUR
push (when wired up — gated on `secrets.AUR_SSH_PRIVATE_KEY`) consumes the
same artifact.

## Post-install

`gamerat.install` refreshes the hicolor icon cache and the desktop entry
database on every install/upgrade/remove so the launcher tile picks up the
icon immediately. Identical refreshes are wired into the `.deb` postinst
and `.rpm` `%post` scriptlets so the install experience is consistent
across all three native formats.
