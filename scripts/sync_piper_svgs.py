#!/usr/bin/env python3
"""Sync mouse SVGs and svg-lookup.ini from an upstream piper checkout.

The 67 SVGs in `data/mice/` plus `svg-lookup.ini` are vendored verbatim
from libratbag/piper's `data/svgs/`. There is no auto-pull — the
authoritative pin lives in `data/mice/ATTRIBUTION.md`. This script is
the maintained refresh path:

  * `--mode check`  (default) — diff our tree against an upstream
    checkout, list adds / removes / content changes, exit non-zero on
    any drift. Suitable for CI to fail loudly when the pin is stale.
  * `--mode apply`            — copy upstream over our tree, rewrite
    the `ATTRIBUTION.md` pin (commit hash, commit date, SVG count) and
    print the rsync-style summary.

The script never modifies anything when run in `check` mode, so it is
safe to wire into CI alongside `check_ratbagd_drift.py`.

Exit codes:

  0  no drift in `check` mode, or apply succeeded.
  1  drift detected in `check` mode.
  2  unrunnable — piper checkout missing, ATTRIBUTION.md unparseable,
     or the upstream `data/svgs/` directory is empty.
"""

from __future__ import annotations

import argparse
import filecmp
import logging
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Final

LOG: Final = logging.getLogger("sync_piper_svgs")

REPO_ROOT: Final = Path(__file__).resolve().parent.parent
MICE_DIR: Final = REPO_ROOT / "data" / "mice"
ATTRIBUTION: Final = MICE_DIR / "ATTRIBUTION.md"

# Files that live alongside the SVGs but are not themselves SVGs.
NON_SVG_TRACKED: Final = frozenset({"svg-lookup.ini"})


@dataclass(frozen=True, slots=True)
class Drift:
    """Categorised difference between our tree and upstream."""

    added: tuple[str, ...]      # in upstream, not in ours
    removed: tuple[str, ...]    # in ours, not in upstream
    modified: tuple[str, ...]   # present in both but content differs

    @property
    def is_empty(self) -> bool:
        return not (self.added or self.removed or self.modified)


def _list_tracked(directory: Path) -> dict[str, Path]:
    """Return {filename: path} for every SVG + tracked non-SVG file."""
    entries: dict[str, Path] = {}
    for child in directory.iterdir():
        if not child.is_file():
            continue
        if child.suffix == ".svg" or child.name in NON_SVG_TRACKED:
            entries[child.name] = child
    return entries


def _diff(ours: Path, theirs: Path) -> Drift:
    ours_map = _list_tracked(ours)
    theirs_map = _list_tracked(theirs)
    ours_names = set(ours_map)
    theirs_names = set(theirs_map)
    added = sorted(theirs_names - ours_names)
    removed = sorted(ours_names - theirs_names)
    common = sorted(ours_names & theirs_names)
    modified = [
        name
        for name in common
        if not filecmp.cmp(ours_map[name], theirs_map[name], shallow=False)
    ]
    return Drift(tuple(added), tuple(removed), tuple(modified))


def _git_head(repo: Path) -> tuple[str, str]:
    """Return (full commit hash, YYYY-MM-DD commit date) for HEAD of `repo`."""
    sha = subprocess.run(
        ["git", "-C", str(repo), "rev-parse", "HEAD"],
        check=True, capture_output=True, text=True,
    ).stdout.strip()
    iso = subprocess.run(
        ["git", "-C", str(repo), "log", "-1", "--format=%cI", "HEAD"],
        check=True, capture_output=True, text=True,
    ).stdout.strip()
    # `%cI` is full ISO-8601 (`2026-04-25T18:55:59+00:00`). Trim to date.
    return sha, iso[:10]


def _format_drift(drift: Drift) -> str:
    if drift.is_empty:
        return "no drift — vendored tree matches upstream byte-for-byte"
    lines: list[str] = []
    if drift.added:
        lines.append(f"  added upstream    ({len(drift.added)}):")
        lines.extend(f"    + {name}" for name in drift.added)
    if drift.removed:
        lines.append(f"  removed upstream  ({len(drift.removed)}):")
        lines.extend(f"    - {name}" for name in drift.removed)
    if drift.modified:
        lines.append(f"  content changed   ({len(drift.modified)}):")
        lines.extend(f"    ~ {name}" for name in drift.modified)
    return "\n".join(lines)


def _apply(
    upstream_svgs_dir: Path,
    upstream_repo: Path,
    drift: Drift,
) -> None:
    """Mirror upstream into MICE_DIR and rewrite ATTRIBUTION.md."""
    if drift.is_empty:
        LOG.info("nothing to apply; already in sync")
        return

    # Remove files we have that upstream dropped.
    for name in drift.removed:
        target = MICE_DIR / name
        LOG.info("rm  %s", target.relative_to(REPO_ROOT))
        target.unlink()

    # Copy adds + modifications.
    for name in (*drift.added, *drift.modified):
        src = upstream_svgs_dir / name
        dst = MICE_DIR / name
        LOG.info("cp  %s -> %s", src, dst.relative_to(REPO_ROOT))
        shutil.copy2(src, dst)

    # Refresh the pin.
    svg_count = sum(1 for p in MICE_DIR.iterdir() if p.suffix == ".svg")
    sha, date = _git_head(upstream_repo)
    _rewrite_attribution(sha=sha, date=date, svg_count=svg_count)


_ATTR_HASH_RE: Final = re.compile(
    r"(\|\s*Imported at commit\s*\|\s*`)[^`]+(`\s*\|)"
)
_ATTR_DATE_RE: Final = re.compile(
    r"(\|\s*Commit date\s*\|\s*)\d{4}-\d{2}-\d{2}(\s*\|)"
)
_ATTR_COUNT_RE: Final = re.compile(
    r"(\|\s*Number of SVGs\s*\|\s*)\d+(\s*\|)"
)


def _rewrite_attribution(*, sha: str, date: str, svg_count: int) -> None:
    """In-place rewrite of the commit pin block in ATTRIBUTION.md."""
    text = ATTRIBUTION.read_text(encoding="utf-8")

    def must(regex: re.Pattern[str], repl: str, field: str) -> str:
        new_text, n = regex.subn(repl, text, count=1)
        if n != 1:
            msg = (
                f"ATTRIBUTION.md is missing the '{field}' row in the "
                f"pin table — refusing to silently fall behind."
            )
            raise RuntimeError(msg)
        return new_text

    text = must(_ATTR_HASH_RE, rf"\g<1>{sha}\g<2>", "Imported at commit")
    text = must(_ATTR_DATE_RE, rf"\g<1>{date}\g<2>", "Commit date")
    text = must(_ATTR_COUNT_RE, rf"\g<1>{svg_count}\g<2>", "Number of SVGs")
    ATTRIBUTION.write_text(text, encoding="utf-8")
    LOG.info(
        "ATTRIBUTION.md pin updated: commit=%s date=%s svg_count=%d",
        sha[:12], date, svg_count,
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Sync vendored piper SVGs against an upstream checkout."
    )
    parser.add_argument(
        "--piper", type=Path, required=True,
        help="Path to a local piper git checkout.",
    )
    parser.add_argument(
        "--mode", choices=("check", "apply"), default="check",
        help="`check` reports drift (read-only); `apply` rewrites our tree.",
    )
    parser.add_argument(
        "-v", "--verbose", action="store_true",
        help="Enable DEBUG logging.",
    )
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.INFO,
        format="%(message)s",
    )

    upstream_repo: Path = args.piper.expanduser().resolve()
    upstream_svgs = upstream_repo / "data" / "svgs"

    if not upstream_svgs.is_dir():
        LOG.error("upstream svgs dir not found: %s", upstream_svgs)
        return 2
    if not any(p.suffix == ".svg" for p in upstream_svgs.iterdir()):
        LOG.error("upstream svgs dir contains no .svg files: %s", upstream_svgs)
        return 2
    if not ATTRIBUTION.is_file():
        LOG.error("ATTRIBUTION.md not found: %s", ATTRIBUTION)
        return 2

    drift = _diff(MICE_DIR, upstream_svgs)
    LOG.info("%s", _format_drift(drift))

    if args.mode == "check":
        return 0 if drift.is_empty else 1

    _apply(upstream_svgs, upstream_repo, drift)
    LOG.info("done. re-run with --mode check to confirm a clean tree.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
