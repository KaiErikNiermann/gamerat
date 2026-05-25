#!/usr/bin/env python3
"""Compare libratbag's device catalogue against gamerat's coverage.

libratbag's `data/devices/*.device` files are the upstream source of
truth for which mice are supported, how many buttons / LEDs / DPI
stages each has, and which USB / Bluetooth IDs they match. gamerat
relies on the same hardware but maintains two independent mappings:

  * `data/mice/svg-lookup.ini`                              — which
    SVG to render for a given `usb:vid:pid`.
  * `crates/gamerat-gui/src/lib/device-defaults.ts`         — per-
    device button-binding defaults keyed by `bus:vid:pid:version`.

Whenever libratbag adds support for a new mouse upstream, gamerat
quietly falls back to its generic 8-button SVG and 5-button default
template — usable, but suboptimal. This script surfaces that gap so
the maintainer can backfill SVGs and device-defaults.ts entries on a
schedule rather than reacting to user reports.

The script is *report-only* — it does not modify any file. Wire it
into CI alongside `sync_piper_svgs.py` and `check_ratbagd_drift.py` to
get a periodic "you are N devices behind upstream" heartbeat.

Exit codes:

  0  no gap — every upstream device has both an SVG mapping AND a
     button-defaults entry in gamerat.
  1  gap detected — at least one upstream device is missing one or
     both. Use `--strict` to also fail when gamerat has entries that
     upstream no longer recognises (orphans).
  2  unrunnable — libratbag checkout missing or no `.device` files.
"""

from __future__ import annotations

import argparse
import configparser
import logging
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Final

LOG: Final = logging.getLogger("sync_libratbag_devices")

REPO_ROOT: Final = Path(__file__).resolve().parent.parent
SVG_LOOKUP: Final = REPO_ROOT / "data" / "mice" / "svg-lookup.ini"
DEVICE_DEFAULTS_TS: Final = (
    REPO_ROOT / "crates" / "gamerat-gui" / "src" / "lib" / "device-defaults.ts"
)


@dataclass(frozen=True, slots=True)
class UpstreamDevice:
    """One [Device] section from libratbag/data/devices/*.device."""

    name: str
    matches: tuple[str, ...]   # e.g. ('usb:046d:c08b', 'bluetooth:046d:c08b')
    source_file: str           # basename, for error reporting
    buttons: int | None = None
    leds: int | None = None
    dpis: int | None = None


@dataclass(slots=True)
class Coverage:
    upstream: tuple[UpstreamDevice, ...]
    gamerat_svg_matches: frozenset[str]       # `bus:vid:pid` keys
    gamerat_default_matches: frozenset[str]   # `bus:vid:pid` keys (stripped of :version)

    missing_svg: list[UpstreamDevice] = field(default_factory=list)
    missing_defaults: list[UpstreamDevice] = field(default_factory=list)
    orphan_svgs: list[str] = field(default_factory=list)
    orphan_defaults: list[str] = field(default_factory=list)


# ---- parsing helpers -----------------------------------------------------


def _parse_libratbag_device(path: Path) -> UpstreamDevice | None:
    """Parse a single `.device` file, return None if it isn't a mouse."""
    cp = configparser.ConfigParser(strict=False, interpolation=None)
    # `.device` files sometimes contain `=` in values; configparser handles that.
    try:
        cp.read(path, encoding="utf-8")
    except configparser.Error as exc:
        LOG.warning("skipping unparseable %s: %s", path.name, exc)
        return None

    if not cp.has_section("Device"):
        return None
    dev = cp["Device"]
    if dev.get("DeviceType", "").strip().lower() != "mouse":
        return None

    name = dev.get("Name", "").strip()
    raw_match = dev.get("DeviceMatch", "").strip()
    if not (name and raw_match):
        return None
    matches = tuple(m.strip() for m in raw_match.split(";") if m.strip())

    # Driver-specific block lives in `[Driver/<name>]`. Capture the
    # most-quoted counts when present so the report can flag big
    # mice we have no defaults for.
    driver = dev.get("Driver", "").strip()
    drv_section = f"Driver/{driver}" if driver else ""
    buttons = leds = dpis = None
    if drv_section and cp.has_section(drv_section):
        drv = cp[drv_section]
        buttons = _maybe_int(drv.get("Buttons"))
        leds = _maybe_int(drv.get("Leds"))
        dpis = _maybe_int(drv.get("Dpis"))

    return UpstreamDevice(
        name=name,
        matches=matches,
        source_file=path.name,
        buttons=buttons,
        leds=leds,
        dpis=dpis,
    )


def _maybe_int(raw: str | None) -> int | None:
    if not raw:
        return None
    try:
        return int(raw.strip())
    except ValueError:
        return None


def _load_upstream(devices_dir: Path) -> tuple[UpstreamDevice, ...]:
    out: list[UpstreamDevice] = []
    for path in sorted(devices_dir.glob("*.device")):
        if parsed := _parse_libratbag_device(path):
            out.append(parsed)
    return tuple(out)


def _load_gamerat_svg_matches() -> frozenset[str]:
    cp = configparser.ConfigParser(strict=False, interpolation=None)
    cp.read(SVG_LOOKUP, encoding="utf-8")
    out: set[str] = set()
    for section in cp.sections():
        raw = cp[section].get("DeviceMatch", "")
        for token in raw.split(";"):
            token = token.strip()
            if token:
                out.add(token)
    return frozenset(out)


_TS_ENTRY_RE: Final = re.compile(
    r"^\s*'((?:usb|bluetooth):[0-9a-fA-F]{4}:[0-9a-fA-F]{4})(?::\d+)?'\s*:",
    re.MULTILINE,
)


def _load_gamerat_default_matches() -> frozenset[str]:
    """Pull the `bus:vid:pid` keys out of DEVICE_TABLE in device-defaults.ts.

    We strip the trailing `:version` segment because libratbag's
    `DeviceMatch` is version-agnostic. If we ever start branching
    defaults on firmware version, this stripping should become
    explicit-version-aware instead.
    """
    text = DEVICE_DEFAULTS_TS.read_text(encoding="utf-8")
    return frozenset(match.group(1) for match in _TS_ENTRY_RE.finditer(text))


# ---- diff ---------------------------------------------------------------


def _compute_coverage(devices_dir: Path) -> Coverage:
    upstream = _load_upstream(devices_dir)
    svg_matches = _load_gamerat_svg_matches()
    default_matches = _load_gamerat_default_matches()

    coverage = Coverage(
        upstream=upstream,
        gamerat_svg_matches=svg_matches,
        gamerat_default_matches=default_matches,
    )

    upstream_match_set: set[str] = set()
    for dev in upstream:
        upstream_match_set.update(dev.matches)
        if not any(m in svg_matches for m in dev.matches):
            coverage.missing_svg.append(dev)
        if not any(m in default_matches for m in dev.matches):
            coverage.missing_defaults.append(dev)

    coverage.orphan_svgs = sorted(svg_matches - upstream_match_set)
    coverage.orphan_defaults = sorted(default_matches - upstream_match_set)
    return coverage


# ---- rendering ----------------------------------------------------------


def _fmt_dev(dev: UpstreamDevice) -> str:
    counts: list[str] = []
    if dev.buttons is not None:
        counts.append(f"{dev.buttons}b")
    if dev.leds is not None:
        counts.append(f"{dev.leds}l")
    if dev.dpis is not None:
        counts.append(f"{dev.dpis}d")
    counts_str = f" [{'/'.join(counts)}]" if counts else ""
    match_str = ", ".join(dev.matches)
    return f"{dev.name:<48}{counts_str}  ({match_str})  — {dev.source_file}"


def _render(coverage: Coverage, *, show_orphans: bool) -> str:
    lines: list[str] = [
        f"upstream devices:        {len(coverage.upstream):>4}",
        f"gamerat svg matches:     {len(coverage.gamerat_svg_matches):>4}",
        f"gamerat default matches: {len(coverage.gamerat_default_matches):>4}",
        "",
    ]
    if coverage.missing_svg:
        lines.append(f"missing svg-lookup entries ({len(coverage.missing_svg)}):")
        lines.extend(f"  ~ {_fmt_dev(d)}" for d in coverage.missing_svg)
        lines.append("")
    if coverage.missing_defaults:
        lines.append(
            f"missing device-defaults.ts entries ({len(coverage.missing_defaults)}):"
        )
        lines.extend(f"  ~ {_fmt_dev(d)}" for d in coverage.missing_defaults)
        lines.append("")

    if show_orphans:
        if coverage.orphan_svgs:
            lines.append(
                f"svg-lookup entries with no upstream device ({len(coverage.orphan_svgs)}):"
            )
            lines.extend(f"  ! {m}" for m in coverage.orphan_svgs)
            lines.append("")
        if coverage.orphan_defaults:
            lines.append(
                f"device-defaults.ts entries with no upstream device "
                f"({len(coverage.orphan_defaults)}):"
            )
            lines.extend(f"  ! {m}" for m in coverage.orphan_defaults)
            lines.append("")

    if (
        not coverage.missing_svg
        and not coverage.missing_defaults
        and (not show_orphans or not (coverage.orphan_svgs or coverage.orphan_defaults))
    ):
        lines.append("no gap — gamerat covers every upstream device libratbag supports.")
    return "\n".join(lines)


# ---- entry point --------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Report gamerat's coverage gap vs libratbag's device catalogue.",
    )
    parser.add_argument(
        "--libratbag", type=Path, required=True,
        help="Path to a local libratbag git checkout.",
    )
    parser.add_argument(
        "--strict", action="store_true",
        help="Also exit non-zero when gamerat has entries upstream doesn't recognise.",
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

    devices_dir: Path = (args.libratbag.expanduser().resolve() / "data" / "devices")
    if not devices_dir.is_dir():
        LOG.error("libratbag devices dir not found: %s", devices_dir)
        return 2
    if not any(devices_dir.glob("*.device")):
        LOG.error("no .device files in: %s", devices_dir)
        return 2
    if not SVG_LOOKUP.is_file():
        LOG.error("svg-lookup.ini not found: %s", SVG_LOOKUP)
        return 2
    if not DEVICE_DEFAULTS_TS.is_file():
        LOG.error("device-defaults.ts not found: %s", DEVICE_DEFAULTS_TS)
        return 2

    coverage = _compute_coverage(devices_dir)
    LOG.info("%s", _render(coverage, show_orphans=args.strict))

    has_gap = bool(coverage.missing_svg or coverage.missing_defaults)
    if args.strict:
        has_gap = has_gap or bool(coverage.orphan_svgs or coverage.orphan_defaults)
    return 1 if has_gap else 0


if __name__ == "__main__":
    sys.exit(main())
