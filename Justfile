set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# List recipes
default:
    @just --list

# ─── Workspace checks (Rust) ────────────────────────────────────────────

# Run cargo fmt across the workspace
fmt:
    cargo fmt --all

# Check rust formatting (CI gate)
fmt-check:
    cargo fmt --all --check

# Clippy across the workspace (excludes gamerat-gui — Tauri/WebKit deps not in CI)
clippy:
    cargo clippy --workspace --exclude gamerat-gui --all-targets -- -D warnings

# cargo check across the workspace, sans GUI (same reason as clippy)
check-rust:
    cargo check --workspace --exclude gamerat-gui --all-targets

# Run the test suite, sans GUI
test-rust:
    cargo test --workspace --exclude gamerat-gui

# Miri — slow but catches UB in unsafe blocks. Mirrors scripts/miri.sh.
miri:
    bash scripts/miri.sh

# ─── Frontend checks (gamerat-gui) ──────────────────────────────────────

# Install GUI deps (frozen lockfile)
install-gui:
    cd crates/gamerat-gui && pnpm install --frozen-lockfile

# svelte-check
typecheck-gui:
    cd crates/gamerat-gui && pnpm check

# ESLint
lint-gui:
    cd crates/gamerat-gui && pnpm lint

# Fix lint issues
lint-gui-fix:
    cd crates/gamerat-gui && pnpm lint:fix

# vitest
test-gui:
    cd crates/gamerat-gui && pnpm test

# vite build (smoke test that the Svelte side compiles)
build-gui:
    cd crates/gamerat-gui && pnpm build

# One-time: download the headless Chromium the a11y suite drives
a11y-install:
    cd crates/gamerat-gui && pnpm exec playwright install chromium

# Runtime contrast + a11y scan (Playwright + axe; builds + previews internally)
test-a11y-gui:
    cd crates/gamerat-gui && pnpm test:a11y

# All GUI checks
check-gui: install-gui typecheck-gui lint-gui test-gui build-gui test-a11y-gui

# ─── Drift / sync scripts ───────────────────────────────────────────────

# Detect ratbagd interface drift against the pinned snapshots
drift:
    python3 scripts/check_ratbagd_drift.py

# Re-introspect ratbagd into data/ratbagd/*.xml (requires live ratbagd)
refresh-ratbagd:
    python3 scripts/refresh_ratbagd_xml.py --mode apply

# Diff data/mice/*.svg against an upstream piper checkout
sync-piper piper:
    python3 scripts/sync_piper_svgs.py --piper "{{piper}}" --mode check

# Apply upstream piper SVGs over the vendored tree
sync-piper-apply piper:
    python3 scripts/sync_piper_svgs.py --piper "{{piper}}" --mode apply

# Report gamerat's coverage gap vs libratbag's device catalogue
sync-libratbag libratbag:
    python3 scripts/sync_libratbag_devices.py --libratbag "{{libratbag}}"

# ─── Combined ───────────────────────────────────────────────────────────

# All Rust checks (matches what .githooks/pre-push runs)
check-workspace: fmt-check clippy check-rust test-rust drift

# Everything — Rust + frontend + drift. Use before pushing.
check: check-workspace check-gui

# Clean every build artifact
clean:
    cargo clean
    rm -rf crates/gamerat-gui/node_modules crates/gamerat-gui/build crates/gamerat-gui/src-tauri/target

# ─── Versioning & Release ──────────────────────────────────────────────

# Show current workspace version
version:
    @grep '^version' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/'

# Bump version + sync + commit + tag + push (patch | minor | major)
release bump="patch":
    #!/usr/bin/env bash
    set -euo pipefail
    current=$(grep '^version' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
    IFS='.' read -r major minor patch <<< "$current"
    case "{{bump}}" in
        major) major=$((major + 1)); minor=0; patch=0 ;;
        minor) minor=$((minor + 1)); patch=0 ;;
        patch) patch=$((patch + 1)) ;;
        *) echo "invalid bump: {{bump}} (use major | minor | patch)"; exit 1 ;;
    esac
    just _release "$major.$minor.$patch"

# Release with an explicit version (e.g. `just release-version 1.2.3`)
release-version version:
    @just _release "{{version}}"

# Re-tag HEAD and re-trigger the release workflow for an existing version
rerun version:
    #!/usr/bin/env bash
    set -euo pipefail
    version="{{version}}"
    git push
    git tag -d "v$version" 2>/dev/null || true
    git push --delete origin "v$version" 2>/dev/null || true
    git tag "v$version"
    git push origin "v$version"
    echo "re-triggered release workflow for v$version"

# Delete the GitHub release and recreate it (also retags HEAD)
rerelease version:
    #!/usr/bin/env bash
    set -euo pipefail
    version="{{version}}"
    gh release delete "v$version" -y 2>/dev/null || true
    just rerun "$version"
    gh release create "v$version" --title "v$version" --notes ""

# Wait for the most recent release workflow run to finish
wait-release:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "waiting for the latest release workflow run…"
    run_id=$(gh run list --workflow release --limit 1 --json databaseId -q '.[0].databaseId')
    gh run watch "$run_id" --exit-status \
        && echo "release workflow succeeded" \
        || { echo "release workflow failed"; exit 1; }

# ─── Internals ──────────────────────────────────────────────────────────

# Sync versions, commit, tag, push (tag push triggers release.yml).
_release version:
    #!/usr/bin/env bash
    set -euo pipefail
    version="{{version}}"
    just _sync-versions "$version"
    git add \
        Cargo.toml \
        packaging/arch/PKGBUILD \
        crates/gamerat-gui/package.json \
        data/kwin-script/gamerat-focus/metadata.json
    # Cargo.lock is optional — committed if present (binary workspace),
    # silently skipped if untracked or absent (library-style workflow).
    if [[ -f Cargo.lock ]]; then
        git add -f Cargo.lock 2>/dev/null || true
    fi
    git commit -m "chore(release): v$version"
    git push
    git tag "v$version"
    git push origin "v$version"
    echo "release v$version pushed — GitHub Actions will build + publish artifacts"

# Apply a new version to every file that hard-codes it. Wraps the
# stamp-version.sh script that the release.yml workflow also calls,
# so local + CI stamping share one source of truth.
_sync-versions version:
    bash scripts/stamp-version.sh "{{version}}"
