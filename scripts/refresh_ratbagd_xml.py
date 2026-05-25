#!/usr/bin/env python3
"""Refresh `data/ratbagd/*.xml` from a live ratbagd over the system bus.

The five interface snapshots in `data/ratbagd/` (manager / device /
profile / resolution / button) are the static input for
`check_ratbagd_drift.py`. When ratbagd ships a new interface revision,
those files need to be re-pulled — this script is the one-shot way to
do that without hand-editing the XML.

It walks the manager → first device → first profile chain and runs
`gdbus introspect --xml` against each object kind, writing the result
to the matching `data/ratbagd/{name}.xml`. By default it stages the
update through `*.xml.new` and refuses to overwrite if there is no
drift, so a no-op run leaves the tree pristine.

Modes:

  * `--mode check` (default)  — write to `*.xml.new`, diff against the
    committed file, report drift, exit non-zero if any. CI-friendly:
    run it on a host that has the right mouse plugged in and you'll
    catch upstream changes without the maintainer having to remember
    to re-introspect.
  * `--mode apply`            — overwrite the committed `*.xml` files
    in place when drift exists. Leaves a clean tree on success.

Requires:
  * `gdbus` on PATH (ships with glib2's `gio` utilities).
  * A reachable `org.freedesktop.ratbag1` on the system bus.
  * **At least one mouse plugged in** — without a device, the
    Device / Profile / Resolution / Button introspections have nothing
    to anchor to. The script prints the offending path and exits 2.

Exit codes:

  0  no drift (`check`) or refresh completed (`apply`).
  1  drift detected in `check` mode.
  2  unrunnable — gdbus missing, ratbagd unreachable, no mouse, or a
     subprocess failed.
"""

from __future__ import annotations

import argparse
import logging
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Final

LOG: Final = logging.getLogger("refresh_ratbagd_xml")

REPO_ROOT: Final = Path(__file__).resolve().parent.parent
RATBAGD_DIR: Final = REPO_ROOT / "data" / "ratbagd"
BUS_NAME: Final = "org.freedesktop.ratbag1"
MANAGER_PATH: Final = "/org/freedesktop/ratbag1"


@dataclass(frozen=True, slots=True)
class Target:
    """One XML file we maintain and the object path that produces it."""

    filename: str
    path: str    # D-Bus object path to introspect


# ---- subprocess helpers --------------------------------------------------


class GdbusError(RuntimeError):
    """Raised when a `gdbus` invocation fails or returns unparseable output."""


def _run_gdbus(*args: str) -> str:
    """Invoke `gdbus` with the given args; return stdout, raise on failure."""
    cmd = ["gdbus", *args]
    LOG.debug("running: %s", " ".join(cmd))
    try:
        result = subprocess.run(cmd, check=True, capture_output=True, text=True)
    except FileNotFoundError as exc:
        msg = "gdbus not found on PATH (install glib2 / gio utilities)"
        raise GdbusError(msg) from exc
    except subprocess.CalledProcessError as exc:
        stderr = (exc.stderr or "").strip()
        msg = f"gdbus failed (exit {exc.returncode}): {stderr}"
        raise GdbusError(msg) from exc
    return result.stdout


def _introspect_xml(object_path: str) -> str:
    """Return the raw introspection XML for `object_path` on the system bus."""
    return _run_gdbus(
        "introspect", "--xml",
        "--system", "--dest", BUS_NAME,
        "--object-path", object_path,
    )


def _call_property_get(object_path: str, interface: str, prop: str) -> str:
    """Call `org.freedesktop.DBus.Properties.Get` and return raw stdout."""
    return _run_gdbus(
        "call",
        "--system", "--dest", BUS_NAME,
        "--object-path", object_path,
        "--method", "org.freedesktop.DBus.Properties.Get",
        interface, prop,
    )


# ---- discovery -----------------------------------------------------------


_FIRST_OBJECT_PATH_RE: Final = re.compile(r"objectpath '([^']+)'")


def _first_device_path() -> str:
    """Return the object path of the first device the manager exposes."""
    out = _call_property_get(MANAGER_PATH, f"{BUS_NAME}.Manager", "Devices")
    if match := _FIRST_OBJECT_PATH_RE.search(out):
        return match.group(1)
    msg = (
        "ratbagd Manager.Devices is empty — plug in a supported mouse "
        "before running this script. (libratbag only exposes objects "
        "for hardware that's actually present.)"
    )
    raise GdbusError(msg)


_NODE_NAME_RE: Final = re.compile(r'<node name="([^"]+)"')


def _first_child(object_path: str) -> str:
    """Return the first child node name listed at `object_path`."""
    xml = _introspect_xml(object_path)
    if match := _NODE_NAME_RE.search(xml):
        return match.group(1)
    msg = f"{object_path} has no child nodes — cannot probe deeper"
    raise GdbusError(msg)


def _discover_targets() -> tuple[Target, ...]:
    """Walk manager → device → profile chain to pick representative paths.

    Layout in ratbagd:
      /org/freedesktop/ratbag1                                 (manager)
      /org/freedesktop/ratbag1/device/{hidraw}                 (device)
      /org/freedesktop/ratbag1/profile/{hidraw}/p{N}           (profile)
      /org/freedesktop/ratbag1/button/{hidraw}/p{N}/b{N}       (button)
      /org/freedesktop/ratbag1/resolution/{hidraw}/p{N}/r{N}   (resolution)
      /org/freedesktop/ratbag1/led/{hidraw}/p{N}/l{N}          (led)

    Note: not every mouse exposes LEDs, so led discovery is best-effort
    — if no `l*` child exists the led snapshot is skipped (the rest of
    the targets are still refreshed).
    """
    device_path = _first_device_path()
    # `/device/hidrawN` → hidrawN
    hidraw = device_path.rsplit("/", 1)[-1]

    profile_root = f"{MANAGER_PATH}/profile/{hidraw}"
    button_root = f"{MANAGER_PATH}/button/{hidraw}"
    resolution_root = f"{MANAGER_PATH}/resolution/{hidraw}"
    led_root = f"{MANAGER_PATH}/led/{hidraw}"

    profile_id = _first_child(profile_root)             # e.g. "p0"
    profile_path = f"{profile_root}/{profile_id}"

    button_id = _first_child(f"{button_root}/{profile_id}")     # e.g. "b0"
    button_path = f"{button_root}/{profile_id}/{button_id}"

    resolution_id = _first_child(f"{resolution_root}/{profile_id}")  # e.g. "r0"
    resolution_path = f"{resolution_root}/{profile_id}/{resolution_id}"

    targets: list[Target] = [
        Target("manager.xml",    MANAGER_PATH),
        Target("device.xml",     device_path),
        Target("profile.xml",    profile_path),
        Target("button.xml",     button_path),
        Target("resolution.xml", resolution_path),
    ]

    # Best-effort LED discovery — skip the target if the device has no
    # LEDs (some lower-end mice, kernel-input gadgets, etc.).
    try:
        led_id = _first_child(f"{led_root}/{profile_id}")           # e.g. "l0"
        targets.append(Target("led.xml", f"{led_root}/{profile_id}/{led_id}"))
    except GdbusError as exc:
        LOG.info("skipping led snapshot — no LEDs on this device (%s)", exc)

    return tuple(targets)


# ---- diff + write --------------------------------------------------------


@dataclass(frozen=True, slots=True)
class RefreshOutcome:
    target: Target
    drift: bool
    fresh_path: Path


def _refresh_one(target: Target) -> RefreshOutcome:
    """Write fresh XML to `<filename>.new` and compare against committed."""
    fresh = RATBAGD_DIR / f"{target.filename}.new"
    committed = RATBAGD_DIR / target.filename
    xml = _introspect_xml(target.path)
    fresh.write_text(xml, encoding="utf-8")

    if not committed.is_file():
        # No baseline — treat any output as drift.
        return RefreshOutcome(target, drift=True, fresh_path=fresh)

    existing = committed.read_text(encoding="utf-8")
    drift = existing != xml
    return RefreshOutcome(target, drift=drift, fresh_path=fresh)


def _apply(outcomes: list[RefreshOutcome]) -> None:
    """Replace committed XML with fresh XML for every drifting target."""
    for outcome in outcomes:
        committed = RATBAGD_DIR / outcome.target.filename
        if outcome.drift:
            LOG.info(
                "apply: %s ← %s",
                committed.relative_to(REPO_ROOT),
                outcome.target.path,
            )
            shutil.move(outcome.fresh_path, committed)
        else:
            outcome.fresh_path.unlink(missing_ok=True)


def _cleanup_check(outcomes: list[RefreshOutcome]) -> None:
    """Remove staged `*.xml.new` files after a check-mode run."""
    for outcome in outcomes:
        outcome.fresh_path.unlink(missing_ok=True)


# ---- entry point ---------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Refresh data/ratbagd/*.xml from live ratbagd introspection.",
    )
    parser.add_argument(
        "--mode", choices=("check", "apply"), default="check",
        help="`check` reports drift without modifying tracked files; "
             "`apply` overwrites them.",
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

    if not RATBAGD_DIR.is_dir():
        LOG.error("ratbagd snapshot dir not found: %s", RATBAGD_DIR)
        return 2

    try:
        targets = _discover_targets()
    except GdbusError as exc:
        LOG.error("%s", exc)
        return 2

    outcomes: list[RefreshOutcome] = []
    for target in targets:
        try:
            outcomes.append(_refresh_one(target))
        except GdbusError as exc:
            LOG.error("failed to introspect %s: %s", target.path, exc)
            _cleanup_check(outcomes)
            return 2

    drifted = [o for o in outcomes if o.drift]
    if drifted:
        LOG.info("drift detected in %d file(s):", len(drifted))
        for outcome in drifted:
            LOG.info("  ~ %s   (%s)", outcome.target.filename, outcome.target.path)
    else:
        LOG.info("no drift — committed snapshots match live introspection.")

    if args.mode == "check":
        _cleanup_check(outcomes)
        return 1 if drifted else 0

    _apply(outcomes)
    LOG.info(
        "done — re-run `scripts/check_ratbagd_drift.py` to verify the proxy "
        "matches the refreshed snapshot.",
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
