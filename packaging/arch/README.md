# Arch Linux packaging

`PKGBUILD` is a skeleton — no `source=()` array, no real `build()` /
`package()` bodies. It will become functional once gamerat has a tagged
release. For local development, just use `cargo build --release` and run
the binaries out of `target/release/`.
