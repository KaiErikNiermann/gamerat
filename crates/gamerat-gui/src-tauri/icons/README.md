# Icons

Placeholder PNGs (dark crosshair on a 32–512px grey field). Replace with
real artwork once we have an `org.appulsauce.GameRat.svg` source —
regenerate the full set with:

```sh
cargo tauri icon path/to/gamerat.svg
```

Tauri's `generate_context!()` macro reads these files at compile time,
so the four sizes listed in `../tauri.conf.json` under `bundle.icon`
must exist or the Rust build fails.
