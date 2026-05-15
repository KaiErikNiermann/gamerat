# Attribution — mouse SVG diagrams

All `.svg` files in this directory, together with `svg-lookup.ini`, are
sourced **unmodified** from the [libratbag/piper] project and remain the
copyright of their original authors.

## Source

| Field                | Value                                                    |
| -------------------- | -------------------------------------------------------- |
| Upstream project     | [`libratbag/piper`](https://github.com/libratbag/piper)  |
| Upstream path        | `data/svgs/`                                             |
| Imported at commit   | `ff75616c5c4fa6173692040b2246bcfee55bd1c3`               |
| Commit date          | 2026-04-25                                               |
| Number of SVGs       | 67                                                       |

## License

The Piper project is licensed under **GPL-2.0-only**. These assets are
redistributed here under the same terms. gamerat as a whole is licensed
under GPL-2.0-or-later, which is compatible with consuming GPL-2.0-only
inputs.

A verbatim copy of GPL-2.0 is in the repository root: [`LICENSE`](../../LICENSE).

## Why these files are here

`svg-lookup.ini` maps USB vendor/product IDs to the corresponding
device-illustration SVG filename. Both the lookup table and the
illustrations are needed for the GUI to render labeled overlays of the
buttons on a connected mouse.

## Modifications

None. If a file in this directory is ever modified relative to the
upstream commit pinned above, this section must be updated to enumerate
the change, in keeping with GPL §2(a).

## Reporting upstream issues

Bugs in an illustration (mislabeled buttons, missing models, etc.) are
best reported and fixed in [`libratbag/piper`] upstream so the entire
ecosystem benefits, and the fix can be pulled back into gamerat.

[libratbag/piper]: https://github.com/libratbag/piper
