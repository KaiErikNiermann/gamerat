//! Hand-written zbus proxies for ratbagd's three core interfaces.
//!
//! Derived from the `sd_bus_add_object_vtable` registrations in
//! `libratbag/ratbagd/ratbagd*.c` — keep these in lockstep if ratbagd
//! grows or renames anything. Methods follow Rust `snake_case`; zbus
//! automatically `PascalCase`s them on the wire, with explicit
//! `#[zbus(name = "...")]` overrides where ratbagd uses non-standard
//! casing (e.g. `APIVersion`).

use zbus::proxy;
use zbus::zvariant::OwnedObjectPath;

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
