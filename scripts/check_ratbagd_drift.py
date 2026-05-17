#!/usr/bin/env python3
"""Detect drift between gamerat's expected ratbagd interfaces and reality.

Compares the wire-shape we hand-rolled in
`crates/gamerat-ratbag/src/proxy.rs` against:

  * `--mode snapshot` — the captured XML in `data/ratbagd/*.xml`
    (default; works without ratbagd running, suitable for CI).
  * `--mode live`     — a live `gdbus introspect` of system-bus
    ratbagd (verifies the snapshot still matches reality).

Exit codes:

  0  no drift — every member we use is present with matching signatures.
  1  drift detected — at least one expected member is missing or
     signature-changed. Output names every offender.
  2  unrunnable — ratbagd not reachable in `--mode live`, or XML
     missing in `--mode snapshot`.

The script has no third-party dependencies — `xml.etree` from the
stdlib does all the parsing. Run it from any directory; paths are
resolved relative to the repo root via the script's own location.
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET
from collections.abc import Iterable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Final

# ─────────────────────────────────────────────────────────────────────
# Expected interfaces. Mirror gamerat-ratbag/src/proxy.rs — every
# member we call must be listed here with the exact wire signature.
# Adding a new member to the Rust proxy is a two-step change: bump
# this map first, then the proxy.
# ─────────────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class Method:
    """A method we depend on. `input`/`output` are D-Bus type strings."""

    input_sig: str = ""
    output_sig: str = ""


@dataclass(frozen=True)
class Property:
    """A property we depend on. `sig` is the D-Bus value type."""

    sig: str
    access: str = "read"


@dataclass(frozen=True)
class Signal:
    """A signal we subscribe to. `sig` is the arg signature."""

    sig: str = ""


@dataclass(frozen=True)
class Interface:
    name: str
    methods: dict[str, Method] = field(default_factory=dict)
    properties: dict[str, Property] = field(default_factory=dict)
    signals: dict[str, Signal] = field(default_factory=dict)


EXPECTED: Final[tuple[Interface, ...]] = (
    Interface(
        name="org.freedesktop.ratbag1.Manager",
        properties={
            "APIVersion": Property(sig="i"),
            "Devices": Property(sig="ao"),
        },
    ),
    Interface(
        name="org.freedesktop.ratbag1.Device",
        properties={
            "Model": Property(sig="s"),
            "Name": Property(sig="s"),
            "FirmwareVersion": Property(sig="s"),
            "Profiles": Property(sig="ao"),
            # DeviceType is exposed by ratbagd but not currently used
            # by gamerat — keep it tracked so we notice if it goes away.
            "DeviceType": Property(sig="u"),
        },
        methods={
            "Commit": Method(input_sig="", output_sig="u"),
        },
        signals={
            "Resync": Signal(sig=""),
        },
    ),
    Interface(
        name="org.freedesktop.ratbag1.Profile",
        properties={
            "Index": Property(sig="u"),
            "IsActive": Property(sig="b"),
            "Resolutions": Property(sig="ao"),
        },
        methods={
            "SetActive": Method(input_sig="", output_sig="u"),
        },
    ),
    Interface(
        name="org.freedesktop.ratbag1.Resolution",
        properties={
            # Variant — `u` on most mice, `(uu)` on separate-XY hardware.
            "Resolution": Property(sig="v", access="readwrite"),
            "IsActive": Property(sig="b"),
        },
        methods={
            "SetActive": Method(input_sig="", output_sig="u"),
        },
    ),
    Interface(
        name="org.freedesktop.ratbag1.Button",
        properties={
            "Index": Property(sig="u"),
            # Tagged variant — see crates/gamerat-ratbag/src/button.rs.
            "Mapping": Property(sig="(uv)", access="readwrite"),
            # Subset of RATBAG_BUTTON_ACTION_TYPE_* the hardware accepts.
            "ActionTypes": Property(sig="au"),
        },
    ),
)


# ─────────────────────────────────────────────────────────────────────
# Snapshot paths, paired with the interface they describe.
# ─────────────────────────────────────────────────────────────────────

REPO_ROOT: Final[Path] = Path(__file__).resolve().parent.parent
SNAPSHOT_DIR: Final[Path] = REPO_ROOT / "data" / "ratbagd"

SNAPSHOT_FILES: Final[dict[str, Path]] = {
    "org.freedesktop.ratbag1.Manager": SNAPSHOT_DIR / "manager.xml",
    "org.freedesktop.ratbag1.Device": SNAPSHOT_DIR / "device.xml",
    "org.freedesktop.ratbag1.Profile": SNAPSHOT_DIR / "profile.xml",
    "org.freedesktop.ratbag1.Resolution": SNAPSHOT_DIR / "resolution.xml",
    "org.freedesktop.ratbag1.Button": SNAPSHOT_DIR / "button.xml",
}

# When in `--mode live`, these are the object paths to introspect. The
# device, profile, and resolution paths depend on which mouse is
# attached — discover them dynamically from the Manager.
LIVE_MANAGER_PATH: Final[str] = "/org/freedesktop/ratbag1"


# ─────────────────────────────────────────────────────────────────────
# XML parsing
# ─────────────────────────────────────────────────────────────────────


def parse_interface(xml_text: str, name: str) -> Interface | None:
    """Pull a single interface out of a D-Bus introspection XML."""
    root = ET.fromstring(xml_text)
    for ifn in root.findall("interface"):
        if ifn.get("name") != name:
            continue
        methods: dict[str, Method] = {}
        for m in ifn.findall("method"):
            mname = m.get("name") or ""
            in_sig = "".join(
                a.get("type") or "" for a in m.findall("arg") if a.get("direction") != "out"
            )
            out_sig = "".join(
                a.get("type") or "" for a in m.findall("arg") if a.get("direction") == "out"
            )
            methods[mname] = Method(input_sig=in_sig, output_sig=out_sig)
        properties: dict[str, Property] = {}
        for p in ifn.findall("property"):
            properties[p.get("name") or ""] = Property(
                sig=p.get("type") or "",
                access=p.get("access") or "read",
            )
        signals: dict[str, Signal] = {}
        for s in ifn.findall("signal"):
            sig = "".join(a.get("type") or "" for a in s.findall("arg"))
            signals[s.get("name") or ""] = Signal(sig=sig)
        return Interface(name=name, methods=methods, properties=properties, signals=signals)
    return None


# ─────────────────────────────────────────────────────────────────────
# Live-probe helpers
# ─────────────────────────────────────────────────────────────────────


def gdbus_introspect(path: str) -> str:
    """Run `gdbus introspect --system --dest=… --object-path=… --xml`."""
    if shutil.which("gdbus") is None:
        raise FileNotFoundError("gdbus not on PATH — install glib2 / glib-2.0-bin")
    result = subprocess.run(
        [
            "gdbus",
            "introspect",
            "--system",
            "--dest",
            "org.freedesktop.ratbag1",
            "--object-path",
            path,
            "--xml",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"gdbus introspect {path} failed: {result.stderr.strip() or result.stdout.strip()}"
        )
    return result.stdout


def discover_live_paths() -> dict[str, str]:
    """Pick one Device / Profile / Resolution path to introspect.

    ratbagd's object tree is deep — for each tier we just need *some*
    live object to grab the interface shape from."""
    # Probe the manager root once to fail fast if ratbagd isn't there;
    # the returned XML isn't reused here — we re-introspect inside the
    # main loop with the discovered paths.
    gdbus_introspect(LIVE_MANAGER_PATH)
    # `Devices` is a property — easier to get via busctl.
    busctl = shutil.which("busctl")
    if busctl is None:
        raise FileNotFoundError("busctl not on PATH")
    devs_out = subprocess.run(
        [
            busctl,
            "--system",
            "get-property",
            "org.freedesktop.ratbag1",
            LIVE_MANAGER_PATH,
            "org.freedesktop.ratbag1.Manager",
            "Devices",
        ],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    # Output looks like: ao 1 "/org/freedesktop/ratbag1/device/hidraw1"
    device_paths = [tok.strip('"') for tok in devs_out.split() if tok.startswith('"/')]
    if not device_paths:
        raise RuntimeError(
            "ratbagd has no devices attached — connect a supported mouse "
            "or run the script in `--mode snapshot`."
        )
    device_path = device_paths[0]
    # Profile path off the device.
    prof_out = subprocess.run(
        [
            busctl,
            "--system",
            "get-property",
            "org.freedesktop.ratbag1",
            device_path,
            "org.freedesktop.ratbag1.Device",
            "Profiles",
        ],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    profile_paths = [tok.strip('"') for tok in prof_out.split() if tok.startswith('"/')]
    if not profile_paths:
        raise RuntimeError(f"device {device_path} reports zero profiles")
    profile_path = profile_paths[0]
    # Resolution path off the profile.
    res_out = subprocess.run(
        [
            busctl,
            "--system",
            "get-property",
            "org.freedesktop.ratbag1",
            profile_path,
            "org.freedesktop.ratbag1.Profile",
            "Resolutions",
        ],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    res_paths = [tok.strip('"') for tok in res_out.split() if tok.startswith('"/')]
    if not res_paths:
        raise RuntimeError(f"profile {profile_path} reports zero resolutions")
    # Pick any button on the profile for the Button interface probe.
    btn_out = subprocess.run(
        [
            busctl,
            "--system",
            "get-property",
            "org.freedesktop.ratbag1",
            profile_path,
            "org.freedesktop.ratbag1.Profile",
            "Buttons",
        ],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    btn_paths = [tok.strip('"') for tok in btn_out.split() if tok.startswith('"/')]
    if not btn_paths:
        raise RuntimeError(f"profile {profile_path} reports zero buttons")
    return {
        "org.freedesktop.ratbag1.Manager": LIVE_MANAGER_PATH,
        "org.freedesktop.ratbag1.Device": device_path,
        "org.freedesktop.ratbag1.Profile": profile_path,
        "org.freedesktop.ratbag1.Resolution": res_paths[0],
        "org.freedesktop.ratbag1.Button": btn_paths[0],
    }


# ─────────────────────────────────────────────────────────────────────
# Comparison
# ─────────────────────────────────────────────────────────────────────


@dataclass
class Drift:
    missing_methods: list[str] = field(default_factory=list)
    sig_changed_methods: list[str] = field(default_factory=list)
    missing_props: list[str] = field(default_factory=list)
    sig_changed_props: list[str] = field(default_factory=list)
    missing_signals: list[str] = field(default_factory=list)
    sig_changed_signals: list[str] = field(default_factory=list)
    extras: list[str] = field(default_factory=list)

    def has_breaking(self) -> bool:
        return bool(
            self.missing_methods
            or self.sig_changed_methods
            or self.missing_props
            or self.sig_changed_props
            or self.missing_signals
            or self.sig_changed_signals
        )


def compare(expected: Interface, actual: Interface) -> Drift:
    drift = Drift()
    for mname, m in expected.methods.items():
        if mname not in actual.methods:
            drift.missing_methods.append(f"{expected.name}.{mname}")
            continue
        live = actual.methods[mname]
        if live.input_sig != m.input_sig or live.output_sig != m.output_sig:
            drift.sig_changed_methods.append(
                f"{expected.name}.{mname}: expected ({m.input_sig})->({m.output_sig}), "
                f"got ({live.input_sig})->({live.output_sig})"
            )
    for pname, p in expected.properties.items():
        if pname not in actual.properties:
            drift.missing_props.append(f"{expected.name}.{pname}")
            continue
        live_prop = actual.properties[pname]
        if live_prop.sig != p.sig:
            drift.sig_changed_props.append(
                f"{expected.name}.{pname}: expected {p.sig}, got {live_prop.sig}"
            )
    for sname, s in expected.signals.items():
        if sname not in actual.signals:
            drift.missing_signals.append(f"{expected.name}.{sname}")
            continue
        live_sig = actual.signals[sname]
        if live_sig.sig != s.sig:
            drift.sig_changed_signals.append(
                f"{expected.name}.{sname}: expected ({s.sig}), got ({live_sig.sig})"
            )
    # Informational: new members ratbagd has that gamerat doesn't track.
    for name in set(actual.methods) - set(expected.methods):
        drift.extras.append(f"new method {expected.name}.{name}")
    for name in set(actual.properties) - set(expected.properties):
        drift.extras.append(f"new property {expected.name}.{name}")
    for name in set(actual.signals) - set(expected.signals):
        drift.extras.append(f"new signal {expected.name}.{name}")
    return drift


# ─────────────────────────────────────────────────────────────────────
# Reporting
# ─────────────────────────────────────────────────────────────────────


def fmt_drift(d: Drift) -> Iterable[str]:
    if d.missing_methods:
        yield f"  [missing methods]    {', '.join(d.missing_methods)}"
    if d.sig_changed_methods:
        for entry in d.sig_changed_methods:
            yield f"  [method signature]   {entry}"
    if d.missing_props:
        yield f"  [missing properties] {', '.join(d.missing_props)}"
    if d.sig_changed_props:
        for entry in d.sig_changed_props:
            yield f"  [property signature] {entry}"
    if d.missing_signals:
        yield f"  [missing signals]    {', '.join(d.missing_signals)}"
    if d.sig_changed_signals:
        for entry in d.sig_changed_signals:
            yield f"  [signal signature]   {entry}"
    if d.extras:
        for entry in d.extras:
            yield f"  [new upstream]       {entry}"


# ─────────────────────────────────────────────────────────────────────
# main
# ─────────────────────────────────────────────────────────────────────


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--mode",
        choices=("snapshot", "live"),
        default="snapshot",
        help="snapshot: diff against data/ratbagd/*.xml (default, CI-friendly). "
        "live: probe ratbagd directly via gdbus + busctl.",
    )
    parser.add_argument(
        "--allow-extras",
        action="store_true",
        help="Don't print informational notes about new upstream members.",
    )
    args = parser.parse_args()

    if args.mode == "live":
        try:
            live_paths = discover_live_paths()
        except (RuntimeError, FileNotFoundError) as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 2
        xmls = {name: gdbus_introspect(path) for name, path in live_paths.items()}
    else:
        xmls = {}
        for name, path in SNAPSHOT_FILES.items():
            if not path.exists():
                print(f"error: snapshot missing: {path}", file=sys.stderr)
                return 2
            xmls[name] = path.read_text()

    overall_breaking = False
    overall_extras = False

    print(f"== ratbagd drift check ({args.mode}) ==")
    for expected in EXPECTED:
        actual = parse_interface(xmls[expected.name], expected.name)
        if actual is None:
            print(f"\n{expected.name}")
            print("  [interface missing] not advertised at the probed path")
            overall_breaking = True
            continue
        drift = compare(expected, actual)
        if not drift.has_breaking() and not drift.extras:
            print(f"\n{expected.name}  ok")
            continue
        print(f"\n{expected.name}")
        for line in fmt_drift(drift):
            print(line)
        if drift.has_breaking():
            overall_breaking = True
        if drift.extras and not args.allow_extras:
            overall_extras = True

    print()
    if overall_breaking:
        print("FAIL — breaking drift detected; gamerat-ratbag/src/proxy.rs must be updated.")
        return 1
    if overall_extras:
        print("ok — no breaking drift. Upstream has additions you may want to support.")
    else:
        print("ok — interfaces match.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
