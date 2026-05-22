#!/usr/bin/env bash
# Run Miri (https://github.com/rust-lang/miri) over the parts of the
# workspace it can interpret, to catch undefined behaviour — out-of-bounds
# access, use-after-free, invalid pointer/enum/bool values, data races,
# and misuse of unsafe — including UB reached transitively through
# dependencies' unsafe code (serde, zvariant, tokio sync primitives).
#
# Usage:
#     scripts/miri.sh                 # run the Miri-compatible test set
#     scripts/miri.sh -p gamerat-proto -- some::test   # forward args
#
# Requires the nightly toolchain + the miri component:
#     rustup toolchain install nightly --component miri
#
# ── What runs, and what can't ──────────────────────────────────────────
# Miri is a pure-Rust interpreter: it cannot make real syscalls, call into
# C libraries, or drive an OS event loop. So it covers the pure-logic
# crates and excludes the ones whose tests need the outside world:
#
#   gamerat-proto    ✓  wire types, serde / zvariant round-trips
#   gamerat-focus    ✓  backend channels + stream plumbing (tokio sync)
#   gamerat-daemon   ✓  allocator, rule/profile/settings stores, dispatch
#   gamerat-gamedb   ✗  Lutris scanner calls SQLite via rusqlite (C FFI)
#   gamerat-ratbag   ✗  every path talks to a live ratbagd over D-Bus
#   gamerat-gui      ✗  Tauri/WebKit binary; no interpretable unit tests
#
# `-Zmiri-disable-isolation` is set because the daemon's store tests touch
# real temp files and read the clock; isolation otherwise blocks both.

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

if ! cargo +nightly miri --version >/dev/null 2>&1; then
    cat >&2 <<'EOF'
error: `cargo +nightly miri` is not available.

Install it with:
    rustup toolchain install nightly --component miri

Then re-run this script.
EOF
    exit 1
fi

# Crates Miri can interpret end-to-end. Keep this list in sync with the
# table above when adding a crate or moving FFI/IO behind a feature flag.
miri_crates=(
    -p gamerat-proto
    -p gamerat-focus
    -p gamerat-daemon
)

export MIRIFLAGS="${MIRIFLAGS:-} -Zmiri-disable-isolation"

echo "▸ MIRIFLAGS=${MIRIFLAGS}" >&2
echo "▸ cargo +nightly miri test ${miri_crates[*]} $*" >&2
exec cargo +nightly miri test "${miri_crates[@]}" "$@"
