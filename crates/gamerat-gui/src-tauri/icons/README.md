# Icons

Lucide `rat` glyph (ISC-licensed, see lucide's own LICENSE under
`node_modules/@lucide/svelte/`) over a rounded amber square — matches
the app's accent-color token. Source: `icon.svg`. Rasterised at
32px, 128px, 256px (`128x128@2x.png`), and 512px (`icon.png`).

## Regenerate

```sh
# rsvg-convert ships with `librsvg`
rsvg-convert -w 512 -h 512 icon.svg -o icon.png
rsvg-convert -w 256 -h 256 icon.svg -o 128x128@2x.png
rsvg-convert -w 128 -h 128 icon.svg -o 128x128.png
rsvg-convert -w  32 -h  32 icon.svg -o 32x32.png
```

Tauri's `generate_context!()` reads these PNGs at compile time, so the
four sizes listed in `../tauri.conf.json` under `bundle.icon` must
exist or the Rust build fails. The Arch PKGBUILD installs `icon.png`
to `/usr/share/icons/hicolor/512x512/apps/gamerat.png`.
