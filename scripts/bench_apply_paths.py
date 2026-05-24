"""Benchmark the three "make this profile active" code paths on the G502 HERO.

Goal: empirically confirm (or refute) that the slot-allocator's `Cached`
decision actually saves latency vs the full-rewrite `ContentChanged`
path, and quantify how much of the felt stutter is "the daemon" vs "the
hardware needing time to commit flash".

Three paths timed:

1. **Cached**: `gameratctl profile apply noita` with a warm cache. After
   the first apply primes the slot, subsequent calls hit
   `AllocationReason::Cached` → just `Profile.SetActive` + `Device.Commit`
   on ratbagd. No DPI / button / LED writes.

2. **ContentChanged**: invalidate the slot cache (via `profile led set`,
   which writes the on-disk profile and calls `invalidate_content`), then
   `gameratctl profile apply noita`. Hits `apply_profile_complete` —
   writes every DPI stage, every button, every LED, then commits.

3. **Raw `SetActive`**: bypass gamerat entirely. Direct `gdbus` call to
   `org.freedesktop.ratbag1.Profile.SetActive` + `Device.Commit`. Floor
   on the cached path — measures pure HID++ + flash latency without
   gamerat's IPC overhead.

Run N iterations per path. Report min/p50/p95/max. Designed to be
re-runnable — the only persistent side-effect is a single LED-color
edit per ContentChanged sample (we toggle between two colors so the
profile doesn't end up with a meaningless final state).
"""

from __future__ import annotations

import argparse
import dataclasses
import signal
import statistics
import subprocess
import sys
import time
from pathlib import Path
from typing import Sequence

import dbus

# Convert SIGTERM into a normal SystemExit so the `bench-other` cleanup
# in `bench_cached_slot_switch`'s `finally` block still runs when the
# script is killed via `systemctl stop` or plain `kill <pid>`. Without
# this, a SIGTERM mid-bench leaves the throwaway profile in the user's
# profiles.toml — it then shows up in the GUI's Profiles panel as if it
# were a real entry. (KeyboardInterrupt from Ctrl+C already propagates
# through `finally`; SIGKILL is unreachable and handled below via the
# best-effort pre-cleanup at bench start.)
def _on_sigterm(*_: object) -> None:  # noqa: ARG001  # signal handler ABI
    sys.exit(143)


signal.signal(signal.SIGTERM, _on_sigterm)

GAMERATCTL = Path(__file__).resolve().parent.parent / "target" / "release" / "gameratctl"
RATBAGD_BUS = "org.freedesktop.ratbag1"
DEVICE_PATH = "/org/freedesktop/ratbag1/device/hidraw1"
PROFILE_INTERFACE = "org.freedesktop.ratbag1.Profile"
DEVICE_INTERFACE = "org.freedesktop.ratbag1.Device"
NOITA_SLOT = 1  # observed from `gameratctl device slots`
BASE_SLOT = 0


@dataclasses.dataclass
class Sample:
    name: str
    durations_ms: list[float]

    def summary(self) -> str:
        if not self.durations_ms:
            return f"{self.name}: (no samples)"
        sorted_durs = sorted(self.durations_ms)
        n = len(sorted_durs)
        p50 = statistics.median(sorted_durs)
        p95 = sorted_durs[min(n - 1, int(n * 0.95))]
        return (
            f"{self.name:<22} n={n:3d}  "
            f"min={min(sorted_durs):6.1f}ms  "
            f"p50={p50:6.1f}ms  "
            f"p95={p95:6.1f}ms  "
            f"max={max(sorted_durs):6.1f}ms  "
            f"mean={statistics.mean(sorted_durs):6.1f}ms"
        )


def time_call(cmd: list[str]) -> float:
    """Run cmd, return wall-clock ms. Raises on non-zero exit."""
    start = time.perf_counter()
    result = subprocess.run(cmd, capture_output=True, text=True, check=False)
    end = time.perf_counter()
    if result.returncode != 0:
        raise RuntimeError(
            f"{' '.join(cmd)} failed (rc={result.returncode}):\n"
            f"stdout: {result.stdout}\nstderr: {result.stderr}"
        )
    return (end - start) * 1000.0


def bench_led_set_direct(n: int) -> Sample:
    """`gameratctl led set` writes one LED + Commit on ratbagd. This is
    what the GUI's LedColorEditor Apply does in Base mode (no profile
    record, write straight through). Each iteration uses a fresh color
    to force a real dirty bit, then verifies the hardware updated."""
    bus = dbus.SystemBus()
    led_obj = bus.get_object(
        RATBAGD_BUS, "/org/freedesktop/ratbag1/led/hidraw1/p1/l0"
    )
    led_props = dbus.Interface(led_obj, "org.freedesktop.DBus.Properties")

    durs: list[float] = []
    for i in range(n):
        # Pick a unique color per iteration distinct from the
        # content-changed bench's space so the two benchmarks don't
        # accidentally collide on the same value.
        r = (i * 41 + 100) % 256
        g = (i * 67 + 60) % 256
        b = (i * 89 + 30) % 256
        color = f"#{r:02x}{g:02x}{b:02x}"
        durs.append(time_call([
            str(GAMERATCTL), "led", "set",
            "--device", "0", "--led", "0",
            "--mode", "solid", "--color", color, "--brightness", "200",
        ]))
        hw = led_props.Get(
            "org.freedesktop.ratbag1.Led", "Color",
            dbus_interface="org.freedesktop.DBus.Properties",
        )
        if (int(hw[0]), int(hw[1]), int(hw[2])) != (r, g, b):
            raise RuntimeError(
                f"iter {i}: wrote {color} but hardware reports "
                f"#{int(hw[0]):02x}{int(hw[1]):02x}{int(hw[2]):02x}"
            )
    return Sample("direct SetLed+Commit", durs)


def bench_cached(n: int) -> Sample:
    """Prime the cache once, then time N consecutive applies. Each should
    hit `AllocationReason::Cached` AND `from == decision.slot`, which
    means apply_rule does literally nothing (debug-logs and returns).
    Lower bound for "gamerat dispatch IPC overhead", not hardware."""
    subprocess.run([str(GAMERATCTL), "profile", "apply", "noita"], check=True, capture_output=True)
    subprocess.run([str(GAMERATCTL), "profile", "apply", "noita"], check=True, capture_output=True)
    durs: list[float] = []
    for _ in range(n):
        durs.append(time_call([str(GAMERATCTL), "profile", "apply", "noita"]))
    return Sample("cached apply (no-op)", durs)


def bench_cached_slot_switch(n: int) -> Sample:
    """The path the user actually triggers on focus changes: profile A
    and B both already materialised on their slots, focus event flips
    which one is active. Allocator returns `Cached, needs_write=false`
    but `from != decision.slot`, so dispatch calls `set_active_profile`
    → Profile.SetActive + Device.Commit on ratbagd. This is the
    "happy-path switch" — if THIS is slow, the cache isn't really
    saving the user anything.

    To exercise it without two real profiles, we toggle between noita
    (a real profile) and a temporary 'bench-other' profile that we
    create + warm + delete around the bench."""
    # Best-effort pre-clean: if a prior run was SIGKILL'd or otherwise
    # hard-died before reaching the `finally` cleanup below, `bench-other`
    # may still be in profiles.toml. `delete` is idempotent (returns ok
    # whether or not the profile exists), so this is safe regardless.
    subprocess.run(
        [str(GAMERATCTL), "profile", "delete", "bench-other"],
        check=False, capture_output=True,
    )
    # Set up a second profile so we have two slots in play.
    subprocess.run(
        [str(GAMERATCTL), "profile", "add",
         "--id", "bench-other", "--name", "bench-other",
         "--category", "agnostic", "--dpi", "800"],
        check=True, capture_output=True,
    )
    try:
        # Prime both slots.
        subprocess.run([str(GAMERATCTL), "profile", "apply", "noita"], check=True, capture_output=True)
        subprocess.run([str(GAMERATCTL), "profile", "apply", "bench-other"], check=True, capture_output=True)
        # Now alternate. Each apply is a cache hit + slot change.
        durs: list[float] = []
        targets = ["noita", "bench-other"]
        for i in range(n):
            durs.append(time_call([str(GAMERATCTL), "profile", "apply", targets[i % 2]]))
        return Sample("cached slot switch", durs)
    finally:
        subprocess.run(
            [str(GAMERATCTL), "profile", "delete", "bench-other"],
            check=False, capture_output=True,
        )


def bench_content_changed(n: int) -> Sample:
    """Each iteration: edit the profile with a UNIQUE color (forces a
    real dirty bit, no libratbag short-circuit), apply, verify the
    hardware Color property actually matches what we wrote. Timer
    covers just the apply call."""
    bus = dbus.SystemBus()
    led_obj = bus.get_object(
        RATBAGD_BUS, "/org/freedesktop/ratbag1/led/hidraw1/p1/l0"
    )
    led_props = dbus.Interface(led_obj, "org.freedesktop.DBus.Properties")

    durs: list[float] = []
    for i in range(n):
        # Use the iteration index to seed the color so no two writes
        # repeat. Stays inside 0..255 per channel.
        r = (i * 37 + 23) % 256
        g = (i * 53 + 47) % 256
        b = (i * 71 + 13) % 256
        color = f"#{r:02x}{g:02x}{b:02x}"
        subprocess.run(
            [str(GAMERATCTL), "profile", "led", "set", "noita",
             "--led", "0", "--mode", "solid", "--color", color, "--brightness", "200"],
            check=True, capture_output=True,
        )
        durs.append(time_call([str(GAMERATCTL), "profile", "apply", "noita"]))
        # Sanity: confirm ratbagd reports the new color (mismatches
        # would mean apply silently no-opped or wrote the wrong value).
        hw = led_props.Get(
            "org.freedesktop.ratbag1.Led", "Color",
            dbus_interface="org.freedesktop.DBus.Properties",
        )
        if (int(hw[0]), int(hw[1]), int(hw[2])) != (r, g, b):
            raise RuntimeError(
                f"iter {i}: wrote {color} but hardware reports "
                f"#{int(hw[0]):02x}{int(hw[1]):02x}{int(hw[2]):02x}"
            )
    return Sample("content-changed apply", durs)


def _profile_proxy(bus: dbus.Bus, slot: int) -> dbus.Interface:
    """Build a libratbag Profile proxy with a persistent connection so
    bench loops aren't measuring subprocess startup / connection-setup
    cost. `gdbus call` per-iteration adds ~800ms of fork-exec overhead
    on this machine, dominating the actual hardware latency."""
    obj = bus.get_object(RATBAGD_BUS, f"/org/freedesktop/ratbag1/profile/hidraw1/p{slot}")
    return dbus.Interface(obj, PROFILE_INTERFACE)


def _device_proxy(bus: dbus.Bus) -> dbus.Interface:
    obj = bus.get_object(RATBAGD_BUS, DEVICE_PATH)
    return dbus.Interface(obj, DEVICE_INTERFACE)


def bench_raw_setactive(n: int) -> Sample:
    """Bypass gamerat. Call ratbagd's Profile.SetActive + Device.Commit
    directly with a persistent system-bus connection (avoids gdbus
    subprocess overhead). Alternates between p0 (base) and p1 (noita)
    each iteration so every call is a real slot change."""
    bus = dbus.SystemBus()
    profiles = [_profile_proxy(bus, 0), _profile_proxy(bus, 1)]
    device = _device_proxy(bus)
    durs: list[float] = []
    for i in range(n):
        target = profiles[i % 2]
        start = time.perf_counter()
        target.SetActive()
        device.Commit()
        end = time.perf_counter()
        durs.append((end - start) * 1000.0)
    return Sample("raw SetActive+Commit", durs)


def bench_raw_setactive_same_slot(n: int) -> Sample:
    """SetActive on the already-active slot + Commit. ratbagd should
    treat this as a no-op (no dirty bits to flush), so the time is the
    HID++/D-Bus method-call floor."""
    bus = dbus.SystemBus()
    noita = _profile_proxy(bus, 1)
    device = _device_proxy(bus)
    # Prime: make sure we're on noita.
    noita.SetActive()
    device.Commit()
    durs: list[float] = []
    for _ in range(n):
        start = time.perf_counter()
        noita.SetActive()
        device.Commit()
        end = time.perf_counter()
        durs.append((end - start) * 1000.0)
    return Sample("raw SetActive (no-op)", durs)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("-n", "--iterations", type=int, default=12,
                        help="samples per path (default 12)")
    args = parser.parse_args(argv)

    if not GAMERATCTL.exists():
        print(f"ERROR: {GAMERATCTL} not built. Run `cargo build -p gamerat-cli --release`.")
        return 1

    print(f"Benchmark: {args.iterations} iterations per path on G502 HERO.\n")

    results: list[Sample] = [
        bench_raw_setactive_same_slot(args.iterations),
        bench_raw_setactive(args.iterations),
        bench_cached(args.iterations),
        bench_cached_slot_switch(args.iterations),
        bench_content_changed(args.iterations),
        bench_led_set_direct(args.iterations),
    ]

    print("\nResults (lower is better):\n")
    for r in results:
        print(r.summary())

    # Quick sanity ratio.
    by_name = {r.name: r for r in results}

    def med(name: str) -> float | None:
        r = by_name.get(name)
        return statistics.median(r.durations_ms) if r and r.durations_ms else None

    cached_noop = med("cached apply (no-op)")
    cached_switch = med("cached slot switch")
    content = med("content-changed apply")
    raw_switch = med("raw SetActive+Commit")

    print("\nInterpretation:")
    if cached_noop is not None and cached_switch is not None:
        print(
            f"  Cache-hit on the same slot: ~{cached_noop:.0f}ms (no hardware work — gamerat returns early)."
        )
        print(
            f"  Cache-hit but slot changed: ~{cached_switch:.0f}ms (gamerat calls "
            f"Profile.SetActive + Device.Commit on ratbagd — ratbagd flashes a "
            f"profile blob)."
        )
    if raw_switch is not None and cached_switch is not None:
        diff = abs(raw_switch - cached_switch)
        verdict = (
            "essentially the same"
            if diff < 200
            else f"{cached_switch / raw_switch:.2f}× the raw cost"
        )
        print(
            f"  Cached slot switch vs raw SetActive+Commit: {verdict}."
        )
    if content is not None:
        print(
            f"  In-place content rewrite (LED only): ~{content:.0f}ms — LEDs go "
            f"through HID++ SW-control, bypassing flash."
        )

    return 0


if __name__ == "__main__":
    sys.exit(main())
