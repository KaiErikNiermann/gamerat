//! Hand-written zbus proxies for ratbagd's three core interfaces.
//!
//! Derived from the `sd_bus_add_object_vtable` registrations in
//! `libratbag/ratbagd/ratbagd*.c` — keep these in lockstep if ratbagd
//! grows or renames anything. Methods follow Rust `snake_case`; zbus
//! automatically `PascalCase`s them on the wire, with explicit
//! `#[zbus(name = "...")]` overrides where ratbagd uses non-standard
//! casing (e.g. `APIVersion`).

use zbus::proxy;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

/// The Manager interface — one instance per ratbagd, at
/// `/org/freedesktop/ratbag1`. Owns the device list and the dev-only
/// test-injection method.
#[proxy(
    interface = "org.freedesktop.ratbag1.Manager",
    default_service = "org.freedesktop.ratbag1",
    default_path = "/org/freedesktop/ratbag1",
    gen_blocking = false
)]
pub trait Manager {
    #[zbus(property, name = "APIVersion")]
    fn api_version(&self) -> zbus::Result<i32>;

    #[zbus(property)]
    fn devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    /// Inject a virtual device by profile name. **Only present when
    /// ratbagd is built with the `_devel1` (test) variant** — calls on
    /// production ratbagd will return a D-Bus method-not-found error.
    /// Returns ratbagd's internal device index, or a negative value on
    /// failure.
    fn load_test_device(&self, profile_name: &str) -> zbus::Result<i32>;
}

/// The Device interface — one instance per connected mouse, at
/// `/org/freedesktop/ratbag1/device/<encoded-name>`.
#[proxy(
    interface = "org.freedesktop.ratbag1.Device",
    default_service = "org.freedesktop.ratbag1",
    gen_blocking = false
)]
pub trait Device {
    #[zbus(property)]
    fn model(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn device_type(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn name(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn firmware_version(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn profiles(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    /// Flush pending writes (everything set since the last `Commit`) to
    /// the hardware. Returns 0 on success, otherwise a ratbagd errno.
    fn commit(&self) -> zbus::Result<u32>;

    /// Re-query the device for the currently-active resolution and
    /// refresh the cached `IsActive` flag on each `Resolution`
    /// belonging to the active profile. Returns 0 on success,
    /// `NotSupported` if the driver doesn't track live resolution
    /// (anything other than HID++ 2.0 today), `Failed` on hardware
    /// error.
    ///
    /// Requires the libratbag `0001-refresh-active-resolution.patch`
    /// in our `patches/libratbag/` to be applied to the installed
    /// ratbagd. Without it ratbagd returns an `UnknownMethod` error
    /// that the gamerat-daemon DPI tracker treats as "live tracking
    /// unavailable" and logs once.
    fn refresh_active(&self) -> zbus::Result<u32>;

    /// Emitted by ratbagd when the device state was changed externally
    /// (e.g. another client wrote a profile). The daemon should
    /// re-read its cached snapshot when this fires.
    #[zbus(signal)]
    fn resync(&self) -> zbus::Result<()>;
}

/// The Profile interface — one instance per profile slot on a device,
/// at `/org/freedesktop/ratbag1/profile/<dev>/p<idx>`.
#[proxy(
    interface = "org.freedesktop.ratbag1.Profile",
    default_service = "org.freedesktop.ratbag1",
    gen_blocking = false
)]
pub trait Profile {
    #[zbus(property)]
    fn index(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn capabilities(&self) -> zbus::Result<Vec<u32>>;

    #[zbus(property)]
    fn resolutions(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    #[zbus(property)]
    fn buttons(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    #[zbus(property)]
    fn leds(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    #[zbus(property)]
    fn is_active(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn is_dirty(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn report_rates(&self) -> zbus::Result<Vec<u32>>;

    #[zbus(property)]
    fn debounces(&self) -> zbus::Result<Vec<u32>>;

    /// Mark this profile as the active one. Does **not** persist —
    /// caller must invoke `Device::commit` afterwards.
    fn set_active(&self) -> zbus::Result<u32>;
}

/// The Resolution interface — one per DPI stage on a device profile,
/// at `/org/freedesktop/ratbag1/resolution/<dev>/p<pidx>/r<ridx>`.
///
/// The `Resolution` property is a D-Bus variant carrying either `u`
/// (single DPI) or `(uu)` (separate X/Y DPI) depending on whether the
/// device exposes the `SEPARATE_XY_RESOLUTION` capability. Callers
/// must read the current value first to learn its shape and then
/// write back the same shape.
#[proxy(
    interface = "org.freedesktop.ratbag1.Resolution",
    default_service = "org.freedesktop.ratbag1",
    gen_blocking = false
)]
pub trait Resolution {
    #[zbus(property)]
    fn index(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn is_active(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn is_default(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn is_disabled(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn set_is_disabled(&self, value: bool) -> zbus::Result<()>;

    /// Current DPI value, wrapped in a variant of `u` or `(uu)`.
    #[zbus(property)]
    fn resolution(&self) -> zbus::Result<OwnedValue>;

    /// Set the DPI. The value's variant signature must match what the
    /// current `resolution()` returns — `u` for single-axis DPI mice,
    /// `(uu)` for separate-X/Y. Mismatch produces a ratbagd error.
    #[zbus(property)]
    fn set_resolution(&self, value: Value<'_>) -> zbus::Result<()>;

    /// Supported DPI values on this stage (a fixed list of valid CPI
    /// settings, typically 50-step increments).
    #[zbus(property)]
    fn resolutions(&self) -> zbus::Result<Vec<u32>>;

    #[zbus(property)]
    fn capabilities(&self) -> zbus::Result<Vec<u32>>;

    /// Promote this resolution stage to the active one on its profile.
    fn set_active(&self) -> zbus::Result<u32>;

    fn set_default(&self) -> zbus::Result<u32>;
}

/// The Button interface — one instance per button on a profile, at
/// `/org/freedesktop/ratbag1/button/<dev>/p<pidx>/b<bidx>`.
///
/// `Mapping` is the only mutable property and it carries a tagged
/// variant: `(uv)` where the leading `u` is the action kind
/// (`RATBAG_BUTTON_ACTION_TYPE_*`) and the variant's inner type
/// depends on that kind:
///
/// | kind         | variant signature   |
/// |--------------|---------------------|
/// | `NONE`(0)    | typically `u(0)`    |
/// | `MOUSE`(1)   | `u` (target button) |
/// | `SPECIAL`(2) | `u` (special enum)  |
/// | `KEY`(3)     | `u` (keycode)       |
/// | `MACRO`(4)   | `a(uu)` (events)    |
///
/// The variant-of-variant shape means we can't reuse zbus's automatic
/// `Type` derive — see [`crate::button`] for the conversion helpers.
#[proxy(
    interface = "org.freedesktop.ratbag1.Button",
    default_service = "org.freedesktop.ratbag1",
    gen_blocking = false
)]
pub trait Button {
    #[zbus(property)]
    fn index(&self) -> zbus::Result<u32>;

    /// Tuple of `(action_type, value_variant)` — read-side only. Use
    /// [`crate::button::decode_mapping`] to flatten into a
    /// `gamerat_proto::ButtonAction`.
    #[zbus(property)]
    fn mapping(&self) -> zbus::Result<OwnedValue>;

    /// Write a new mapping. The `Value` MUST be a `(uv)` tuple whose
    /// variant payload matches the action kind. See
    /// [`crate::button::encode_mapping`].
    #[zbus(property)]
    fn set_mapping(&self, value: Value<'_>) -> zbus::Result<()>;

    /// Action kinds the firmware accepts on this button. Subset of
    /// `RATBAG_BUTTON_ACTION_TYPE_*`. Editors gate UI on this list so
    /// macros aren't offered for buttons that don't support them.
    #[zbus(property)]
    fn action_types(&self) -> zbus::Result<Vec<u32>>;
}
