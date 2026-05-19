#!/usr/bin/env bash
# Stamp a release version across every file that hard-codes it.
#
# Usage:
#   scripts/stamp-version.sh 0.1.0
#
# Designed for the release.yml workflow: the `stamp-version` job
# runs this once after checkout, then every build job inherits the
# stamped Cargo.toml + PKGBUILD via the same checkout. The cargo
# lockfile is refreshed in-place so `cargo build --locked` in the
# downstream build jobs doesn't fail on the workspace version
# change. Idempotent — re-running with the same version is a no-op.
set -euo pipefail

VERSION="${1:?usage: $0 <version> (e.g. 0.1.0)}"

# Strip any leading `v` so callers can pass either `0.1.0` or `v0.1.0`.
VERSION="${VERSION#v}"

# Workspace version lives in the root Cargo.toml's [workspace.package].
# Every crate inherits via `version.workspace = true`.
sed -i -E "s/^(version = )\"[^\"]*\"/\1\"${VERSION}\"/" Cargo.toml

# PKGBUILD pkgver is independent of the cargo workspace because makepkg
# parses its own metadata file.
sed -i -E "s/^pkgver=.*/pkgver=${VERSION}/" packaging/arch/PKGBUILD

# Refresh the lockfile so workspace crates re-resolve to the new
# version. Offline mode is safe here — we're only touching path
# dependencies, which don't require a network round-trip. Failures
# (e.g. lockfile already perfectly in sync) are non-fatal.
cargo update --workspace --offline 2>/dev/null || true

echo "stamped version ${VERSION} across Cargo.toml + packaging/arch/PKGBUILD"
