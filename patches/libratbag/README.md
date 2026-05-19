# libratbag / ratbagd patches

Patches gamerat carries against upstream libratbag. Upstream as of `805e7fb`
(`feature: Add support for ASUS TUF GAMING M4 AIR`).

## `0001-refresh-active-resolution.patch`

Adds a `RefreshActive` D-Bus method on ratbagd's `Device` interface
that re-queries the device for its currently-active resolution and
updates the cached `IsActive` flag on each `Resolution` object. Without
this, on-mouse DPI cycling (DPI-up / DPI-down / DPI-cycle button
presses) is invisible to userspace — libratbag stores the value
written by the last `SetActive` call and never re-reads.

The new pieces:

- **`ratbag_device_refresh_active_resolution()`** — new public
  libratbag API, dispatches through a driver callback.
- **`refresh_active_resolution` driver vtable slot** — drivers that
  don't track live state leave it NULL; callers get `-ENOTSUP`.
- **HID++ 2.0 driver impl** uses `hidpp20_onboard_profiles_get_current_dpi_index`
  (HID++ feature 0x8100, command 0xb0) — the same call libratbag
  already makes during profile load. Adding other vendors (Razer,
  Roccat, etc.) is a matter of filling in the slot.
- **`Device.RefreshActive`** D-Bus method — calls the libratbag
  refresh, then emits `PropertiesChanged` on every `Resolution`
  belonging to the active profile so clients with cached state
  pick up the new value.
- **`ratbagd_profile_get_is_active`** helper — exposes the active
  flag from `ratbagd-profile.c`'s private struct so the device-level
  refresh handler can find the active profile.

## `0002-cheap-set-active-profile.patch`

Eliminates the ~1.8 s flash-write stutter that fires every time the
active hardware profile changes on a HID++ 2.0 device (e.g. switching
between two already-materialised profiles on a focus event).

**Problem.** `ratbag_profile_set_active()` marks both the
previously-active and the newly-active profiles `dirty = true`. That
flag is what gates the per-property write loop in `hidpp20drv_commit()`
and the subsequent `hidpp20_onboard_profiles_commit()` call — the
latter is the slow part: a ~1 KB sector erase + page-by-page write
for every "dirty" profile, taking ~1.8 s of HID++ traffic on the
device's interrupt endpoint. Result: the input-report pipe is
saturated for that entire duration and the mouse stops tracking +
buttons stop responding.

Nothing in the profile *data* actually needs to change for a
"switch which profile is active" operation — feature 0x8100 has a
dedicated `SetCurrentProfile` command (sub-command 0x30) that flips
the firmware's active-profile pointer in a single ~3-5 ms HID++ short
message, no flash involved. libratbag's post-commit pass already
calls this via the driver's `set_active_profile` hook, gated on
`is_active_dirty`. The expensive write was redundant.

**Fix.** Two minimal changes:

- **`libratbag.c`** — decouple `is_active_dirty` from `dirty`.
  `ratbag_profile_set_active()` no longer flips `dirty` on either
  profile. The post-commit `set_active_profile` callback still fires
  because that gate is `is_active_dirty`, not `dirty`.
- **`driver-hidpp20.c`** — track whether the per-property loop in
  `hidpp20drv_commit()` actually touched any data. If every profile
  was skipped (loop body never entered), gate the expensive
  `hidpp20_onboard_profiles_commit()` + `set_current_dpi_index`
  follow-up out. Profile-data edits (DPI / button / LED writes) still
  hit the flash exactly as before — they have to.

**Empirical impact** (G502 HERO, two profiles already materialised on
slots `p0` and `p1`):

| path                                       | before  | after  |
|--------------------------------------------|---------|--------|
| cached slot switch (`SetActive + Commit`)  | 1875 ms | 205 ms |
| `Profile.SetActive (no-op)` + `Commit`     | 1662 ms |   4 ms |
| in-place content rewrite (LED edit)        |    5 ms |   5 ms |
| direct `SetLed + Commit`                   |    7 ms |   7 ms |

The residual ~200 ms on slot switches is sd-event / dbus-broker
scheduling overhead — `--verbose=raw` confirms **zero HID++ traffic**
during the call, so the device's input-report endpoint stays
uncongested and the mouse no longer stutters. Functionally instant
from the user's perspective.

## Build / install

```bash
cd ~/path/to/libratbag/
git apply ~/path/to/gamerat/patches/libratbag/0001-refresh-active-resolution.patch
git apply ~/path/to/gamerat/patches/libratbag/0002-cheap-set-active-profile.patch
meson setup builddir
meson compile -C builddir
sudo meson install -C builddir
sudo systemctl restart ratbagd   # picks up the new binary
```

ratbagd is D-Bus activated, so a fresh client call would also spawn
the patched binary on demand — but a long-running daemon (e.g.
the one started by an earlier connection) keeps the old binary in
memory until restarted explicitly.

## Verify

```bash
dbus-send --system --print-reply --dest=org.freedesktop.ratbag1 \
  /org/freedesktop/ratbag1/device/hidrawN \
  org.freedesktop.ratbag1.Device.RefreshActive
```

returns `uint32 0` on success — confirms `0001` is in.

```bash
strings /usr/bin/ratbagd | grep any_profile_data_dirty
```

prints the variable name iff `0002` is in.

```bash
python3 scripts/bench_apply_paths.py -n 10
```

`cached slot switch` p50 should be ~200 ms (was ~1.8 s pre-patch).

## Upstream

Both patches are independent of any other gamerat code and are good
candidates for an upstream PR — see
<https://github.com/libratbag/libratbag>. They don't introduce a
runtime dependency between libratbag and gamerat; any consumer
benefits from the same improvements.
