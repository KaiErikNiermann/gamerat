#!/usr/bin/env bash
# Run a single release.yml job under `act` with disciplined cleanup.
#
# Why this exists: iterating on release.yml means running act
# repeatedly across multiple distro container images (archlinux,
# debian, fedora, …). Each run leaves containers and layered images
# behind; over a session those add up to several GB. The user is
# explicit about preferring re-pull time over disk balloon, so we
# tear down everything net-new the run produced — except the
# catthehacker base image, which act needs every run and is ~2 GB.
#
# Usage:
#   scripts/act-smoke.sh build-deb
#   scripts/act-smoke.sh smoke-deb
#   scripts/act-smoke.sh build-arch
#
# Skip gh-release and aur-publish locally — those need real GitHub
# Releases / aur.archlinux.org and either fail or no-op under act.
set -euo pipefail

JOB="${1:?usage: $0 <release.yml job name>}"

if ! command -v act >/dev/null 2>&1; then
    echo "FAIL: act not installed. Install with:" >&2
    echo "  yay -S act     # Arch" >&2
    echo "  or download from https://github.com/nektos/act" >&2
    exit 1
fi

# Capture pre-run state so we can compute the exact set of NEW
# containers/images at cleanup time. Bulletproof and session-local —
# doesn't depend on labels or naming heuristics.
BEFORE_CTRS="$(docker ps -aq | sort -u)"
BEFORE_IMGS="$(docker images -q | sort -u)"

ARTIFACT_DIR="/tmp/act-artifacts"

cleanup() {
    local rc=$?
    AFTER_CTRS="$(docker ps -aq | sort -u)"
    AFTER_IMGS="$(docker images -q | sort -u)"

    # Anything that wasn't there before the run is fair game.
    local new_ctrs new_imgs
    new_ctrs="$(comm -13 <(echo "$BEFORE_CTRS") <(echo "$AFTER_CTRS"))"
    new_imgs="$(comm -13 <(echo "$BEFORE_IMGS") <(echo "$AFTER_IMGS"))"

    if [ -n "$new_ctrs" ]; then
        # shellcheck disable=SC2086
        docker rm -f $new_ctrs >/dev/null 2>&1 || true
    fi

    if [ -n "$new_imgs" ]; then
        for img in $new_imgs; do
            # Preserve the catthehacker base — act will re-pull every
            # run otherwise, which dominates the wall-clock for any
            # iterative work. Tear down everything else, including
            # the distro images (archlinux, debian, fedora) that act
            # pulls for `container:` jobs.
            tag="$(docker inspect --format='{{index .RepoTags 0}}' "$img" 2>/dev/null || true)"
            case "$tag" in
                catthehacker/*) : ;;
                *) docker rmi -f "$img" >/dev/null 2>&1 || true ;;
            esac
        done
    fi

    rm -rf "$ARTIFACT_DIR"

    exit "$rc"
}
trap cleanup EXIT

mkdir -p "$ARTIFACT_DIR"

# `--rm` removes successful containers as act exits each step; the
# cleanup hook above catches anything left from failed runs.
# `--artifact-server-path` lets jobs use actions/upload-artifact +
# download-artifact without GitHub's real artifact backend.
act \
    -W .github/workflows/release.yml \
    -j "$JOB" \
    --artifact-server-path "$ARTIFACT_DIR" \
    --rm \
    "$@"
