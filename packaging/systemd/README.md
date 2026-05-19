# systemd unit

`gamerat-daemon.service` is the canonical user-scoped systemd unit. All
distro packages (`.deb`, `.rpm`, `.pkg.tar.zst`) install this exact file at
`/usr/lib/systemd/user/gamerat-daemon.service`, and the binary tarball drops
the same file under its install prefix.

## Manual install (outside any package)

```sh
mkdir -p ~/.config/systemd/user
cp gamerat-daemon.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now gamerat-daemon.service
```

## Hardening notes

The unit runs with a tightened sandbox by default:

- `PrivateNetwork=true` — daemon never reaches the network.
- `ProtectSystem=strict` — `/usr` and `/etc` are read-only.
- `ReadWritePaths=%h/.config/gamerat %h/.local/state/gamerat` — the daemon's
  own state directories are the only writable paths under `$HOME`.
  (`ProtectHome=read-only` is intentionally NOT set: the daemon writes its
  own `rules.toml` / `profiles.toml` / `settings.toml` here.)
- `PrivateTmp=true`, `NoNewPrivileges=true` — standard hardening.

If you're adding features that need other system paths (e.g., reading from
`/sys/class/hidraw` directly, writing somewhere outside `$HOME`), extend
`ReadWritePaths` rather than loosening the broader directives.
