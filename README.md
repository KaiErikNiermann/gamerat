# gamerat

A gaming-oriented configuration daemon and frontends layered on top of
[`ratbagd`](https://github.com/libratbag/libratbag), with per-application
hardware profile switching and a software-abstracted profile model.

Think *Piper* for gamers: rather than treating mouse profiles as opaque
device-local slots, `gamerat` stores rich profiles in user space, watches
focus / running games, and pushes the right hardware state to the device
on the fly.

## Status

Early scaffolding. Nothing works yet.

## Workspace layout

| Crate              | Purpose                                          |
| ------------------ | ------------------------------------------------ |
| `gamerat-proto`    | D-Bus interface definitions, serde types         |
| `gamerat-ratbag`   | Ergonomic async client wrapper around `ratbagd`  |
| `gamerat-focus`    | Focus backends: X11, wayland-ext, KWin           |
| `gamerat-gamedb`   | Steam / Lutris / Heroic library scanners         |
| `gamerat-daemon`   | Long-running service (the brains)                |
| `gamerat-cli`      | `gameratctl` — scriptable client                 |
| `gamerat-gui`      | Slint frontend                                   |

## Building

```sh
cargo check --workspace
```

## License

GPL-2.0-or-later. See [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE) for
third-party attribution (notably mouse SVG diagrams sourced from
[libratbag/piper](https://github.com/libratbag/piper)).
