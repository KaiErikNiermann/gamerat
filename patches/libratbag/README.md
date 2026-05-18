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

## Build / install

```bash
cd ~/path/to/libratbag/
git apply ~/path/to/gamerat/patches/libratbag/0001-refresh-active-resolution.patch
meson setup builddir
meson compile -C builddir
sudo meson install -C builddir
```

ratbagd is D-Bus activated, so the next client call will spawn the
patched binary. No service restart needed.

## Verify

```
dbus-send --system --print-reply --dest=org.freedesktop.ratbag1 \
  /org/freedesktop/ratbag1/device/hidrawN \
  org.freedesktop.ratbag1.Device.RefreshActive
```

returns `uint32 0` on success, an `InvalidArgs` error if the driver
doesn't support live tracking, or `Failed` on a hardware error.

## Upstream

These changes are independent of any other gamerat code and are good
candidates for an upstream PR — see
`https://github.com/libratbag/libratbag`. They don't introduce a
runtime dependency between libratbag and gamerat; any consumer that
wants live DPI tracking can use the new API.
