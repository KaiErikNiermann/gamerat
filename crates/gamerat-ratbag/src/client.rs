//! Ergonomic async wrapper around the ratbagd D-Bus surface.
//!
//! The proxy traits in [`crate::proxy`] are a faithful mirror of
//! ratbagd's wire shape; this module hides the dance of "find the
//! right profile object → call `SetActive` → call `Commit` on the
//! device" behind one method on [`Device`].

use gamerat_proto::{ButtonAction, ProfileButton, RatbagButton};
use tracing::{debug, instrument, warn};
use zbus::Connection;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, Value};

use crate::button;
use crate::error::{Error, Result};
use crate::proxy::{ButtonProxy, DeviceProxy, ManagerProxy, ProfileProxy, ResolutionProxy};

/// Which ratbagd variant to connect to. Production ratbagd claims
/// `org.freedesktop.ratbag1`; the test/dev build claims
/// `org.freedesktop.ratbag_devel1`. Both run on the system bus.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Service {
    /// `org.freedesktop.ratbag1` — the system-installed ratbagd.
    #[default]
    Production,
    /// `org.freedesktop.ratbag_devel1` — `ratbagd.devel`, the variant
    /// built from a local libratbag tree with `LoadTestDevice` enabled.
    Devel,
    /// Arbitrary service name — for test rigs or alternative builds.
    Custom(String),
}

impl Service {
    /// Resolves the variant to its well-known D-Bus name.
    #[must_use]
    pub fn bus_name(&self) -> &str {
        match self {
            Self::Production => "org.freedesktop.ratbag1",
            Self::Devel => "org.freedesktop.ratbag_devel1",
            Self::Custom(s) => s,
        }
    }
}

/// Top-level handle to a running ratbagd. Cheap to clone — the
/// underlying [`zbus::Connection`] is internally `Arc`-shared.
#[derive(Clone, Debug)]
pub struct Client {
    conn: Connection,
    service: Service,
}

impl Client {
    /// Connect to production ratbagd on the system bus.
    pub async fn connect() -> Result<Self> {
        Self::connect_to(Service::Production).await
    }

    /// Connect to a specific ratbagd variant on the system bus.
    #[instrument(skip_all, fields(service = %service.bus_name()))]
    pub async fn connect_to(service: Service) -> Result<Self> {
        let conn = Connection::system().await?;
        let client = Self { conn, service };
        // Smoke-check: probe the manager's APIVersion. If the service
        // isn't on the bus we get a clearer error here than later when
        // a caller tries to enumerate devices.
        match client.manager().await?.api_version().await {
            Ok(version) => {
                debug!(api_version = version, "connected to ratbagd");
                Ok(client)
            }
            Err(zbus::Error::MethodError(_, _, _)) => {
                Err(Error::NotConnected(client.service.bus_name().to_owned()))
            }
            Err(other) => Err(Error::from(other)),
        }
    }

    /// Enumerate every device ratbagd currently sees.
    pub async fn devices(&self) -> Result<Vec<Device>> {
        let paths = self.manager().await?.devices().await?;
        let mut out = Vec::with_capacity(paths.len());
        for path in paths {
            out.push(Device::new(self.clone(), path).await?);
        }
        Ok(out)
    }

    /// Probe ratbagd's `Manager.APIVersion`. Cheap (one property read)
    /// — callers use this for startup compatibility banners.
    pub async fn api_version(&self) -> Result<i32> {
        Ok(self.manager().await?.api_version().await?)
    }

    /// Inject a virtual device into a `ratbagd.devel` instance. Returns
    /// the device index ratbagd assigned. Fails on production ratbagd.
    pub async fn load_test_device(&self, profile_name: &str) -> Result<i32> {
        let result = self.manager().await?.load_test_device(profile_name).await?;
        if result < 0 {
            warn!(
                returned = result,
                profile = profile_name,
                "LoadTestDevice failed"
            );
        }
        Ok(result)
    }

    async fn manager(&self) -> Result<ManagerProxy<'_>> {
        Ok(ManagerProxy::builder(&self.conn)
            .destination(self.service.bus_name().to_owned())?
            .build()
            .await?)
    }

    pub(crate) const fn conn(&self) -> &Connection {
        &self.conn
    }

    pub(crate) const fn service(&self) -> &Service {
        &self.service
    }
}

/// A connected mouse, addressable via its ratbagd object path.
///
/// Const properties (name, model, firmware) are read once at
/// construction and cached; dynamic state (profile list, active index)
/// is queried on demand.
#[derive(Clone, Debug)]
pub struct Device {
    client: Client,
    path: OwnedObjectPath,
    name: String,
    model: String,
    firmware: String,
}

impl Device {
    async fn new(client: Client, path: OwnedObjectPath) -> Result<Self> {
        let proxy = DeviceProxy::builder(client.conn())
            .destination(client.service().bus_name().to_owned())?
            .path(path.as_ref())?
            .build()
            .await?;
        let name = proxy.name().await?;
        let model = proxy.model().await?;
        let firmware = proxy.firmware_version().await?;
        Ok(Self {
            client,
            path,
            name,
            model,
            firmware,
        })
    }

    #[must_use]
    pub fn object_path(&self) -> ObjectPath<'_> {
        self.path.as_ref()
    }

    #[must_use]
    pub fn owned_object_path(&self) -> OwnedObjectPath {
        self.path.clone()
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    #[must_use]
    pub fn firmware(&self) -> &str {
        &self.firmware
    }

    /// Number of profile slots on the device.
    pub async fn profile_count(&self) -> Result<u32> {
        let count = self.proxy().await?.profiles().await?.len();
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    /// Number of DPI/resolution slots the device exposes per profile.
    /// libratbag enforces a consistent count across profiles on the
    /// same device, so a single query is enough. Used by the GUI to
    /// cap the DPI editor's "+ add stage" affordance at the hardware
    /// max — adding more wouldn't store anywhere.
    pub async fn max_dpi_stages(&self) -> Result<u32> {
        let active = self.active_profile_index().await?;
        let profile_path = self.find_profile_path(active).await?;
        let profile = self.profile_proxy(profile_path).await?;
        let count = profile.resolutions().await?.len();
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    /// Index of the currently active profile.
    pub async fn active_profile_index(&self) -> Result<u32> {
        for path in self.proxy().await?.profiles().await? {
            // Move `path` into the proxy — no outer borrow to worry about.
            let proxy = self.profile_proxy(path).await?;
            if proxy.is_active().await? {
                return Ok(proxy.index().await?);
            }
        }
        // ratbagd guarantees one active profile per device, but be
        // defensive — surface a clear error if reality disagrees.
        Err(Error::Ratbagd {
            op: "active_profile_index (no profile reported IsActive)",
            status: 0,
        })
    }

    /// Index of the active DPI stage on the currently-active profile.
    ///
    /// Walks the active profile's `Resolutions` list and returns the
    /// one whose `IsActive` flag is set. Used by `GetActiveDpiStage`
    /// to surface hardware-level stage changes (DPI-up / DPI-down
    /// presses on the device) back to the GUI without requiring the
    /// user to re-select the profile.
    pub async fn active_dpi_stage_index(&self) -> Result<u32> {
        let active_idx = self.active_profile_index().await?;
        let profile_path = self.find_profile_path(active_idx).await?;
        let profile = self.profile_proxy(profile_path).await?;
        for path in profile.resolutions().await? {
            let res = self.resolution_proxy(path).await?;
            if res.is_active().await? {
                return Ok(res.index().await?);
            }
        }
        Err(Error::Ratbagd {
            op: "active_dpi_stage_index (no resolution reported IsActive)",
            status: 0,
        })
    }

    /// Set the profile at `index` active and persist via `Commit`.
    /// Use this when the slot already contains the desired content —
    /// no resolution writes happen.
    #[instrument(skip(self), fields(device = %self.path.as_str()))]
    pub async fn set_active_profile(&self, index: u32) -> Result<()> {
        let target_path = self.find_profile_path(index).await?;
        let target = self.profile_proxy(target_path).await?;
        let rc = target.set_active().await?;
        if rc != 0 {
            return Err(Error::Ratbagd {
                op: "Profile.SetActive",
                status: rc,
            });
        }
        debug!(index, "profile.SetActive returned 0, committing");
        self.commit().await
    }

    /// Write a gamerat-style DPI profile into hardware slot `index`
    /// and activate it.
    ///
    /// The semantics:
    ///
    ///   1. For each stage in `dpi_stages` (up to the device's
    ///      resolution count), write the value to the matching
    ///      `Resolution.Resolution` variant. The variant shape is
    ///      detected from the current value — `u` for single-axis
    ///      mice or `(uu)` (with `value, value`) for separate-XY
    ///      capable ones.
    ///   2. Mark `Resolution[active_stage]` as the active stage via
    ///      `Resolution.SetActive` (clamped if out of range).
    ///   3. Mark `Profile[index]` active via `Profile.SetActive`.
    ///   4. Call `Device.Commit` to flush everything.
    ///
    /// Excess `dpi_stages` beyond the device's resolution count are
    /// silently dropped; missing stages leave the existing values
    /// alone.
    #[instrument(skip(self, dpi_stages), fields(device = %self.path.as_str(), slot = index, stages = dpi_stages.len()))]
    pub async fn apply_profile_dpi(
        &self,
        index: u32,
        dpi_stages: &[u32],
        active_stage: u32,
    ) -> Result<()> {
        let target_path = self.find_profile_path(index).await?;
        let profile = self.profile_proxy(target_path).await?;
        let resolution_paths = profile.resolutions().await?;
        let resolution_count = resolution_paths.len();

        debug!(resolution_count, "writing DPI stages");

        let stages_to_write = dpi_stages.len().min(resolution_count);
        for (stage_idx, target_dpi) in dpi_stages.iter().take(stages_to_write).enumerate() {
            let Some(res_path) = resolution_paths.get(stage_idx) else {
                continue;
            };
            let res = self.resolution_proxy(res_path.clone()).await?;
            write_resolution_dpi(&res, *target_dpi).await?;
        }

        // Activate the requested stage if it's in range. Clamp
        // silently rather than erroring — the profile validator in
        // gamerat-daemon already prevents out-of-range stages, but be
        // defensive at the ratbagd boundary.
        let clamped_stage = (active_stage as usize).min(resolution_count.saturating_sub(1));
        if let Some(active_path) = resolution_paths.get(clamped_stage) {
            let res = self.resolution_proxy(active_path.clone()).await?;
            let rc = res.set_active().await?;
            if rc != 0 {
                return Err(Error::Ratbagd {
                    op: "Resolution.SetActive",
                    status: rc,
                });
            }
        }

        // Activate the profile + commit.
        let rc = profile.set_active().await?;
        if rc != 0 {
            return Err(Error::Ratbagd {
                op: "Profile.SetActive",
                status: rc,
            });
        }
        debug!("profile + resolution writes ok, committing");
        self.commit().await
    }

    /// Snapshot the active profile's DPI stages plus the index of
    /// the `IsActive` resolution. Used by the GUI's Base-mode DPI
    /// editor to render the live hardware state (since there's no
    /// gamerat profile record to read from in that mode).
    pub async fn active_profile_dpi(&self) -> Result<(Vec<u32>, u32)> {
        let active_idx = self.active_profile_index().await?;
        let profile_path = self.find_profile_path(active_idx).await?;
        let profile = self.profile_proxy(profile_path).await?;
        let resolution_paths = profile.resolutions().await?;
        let mut dpi_stages = Vec::with_capacity(resolution_paths.len());
        let mut active_stage = 0u32;
        for (i, path) in resolution_paths.iter().enumerate() {
            let res = self.resolution_proxy(path.clone()).await?;
            // Read DPI: the resolution() property is a variant
            // around either `u` (single-axis) or `(uu)` (separate XY);
            // we collapse to a scalar by taking the X component.
            let current = res.resolution().await?;
            let dpi = if let Ok((x, _y)) = current.downcast_ref::<(u32, u32)>() {
                x
            } else if let Ok(v) = current.downcast_ref::<u32>() {
                v
            } else {
                return Err(Error::ratbagd_op(
                    "Resolution.Resolution: unexpected variant shape",
                ));
            };
            dpi_stages.push(dpi);
            if res.is_active().await? {
                active_stage = u32::try_from(i).unwrap_or(0);
            }
        }
        Ok((dpi_stages, active_stage))
    }

    async fn find_profile_path(&self, index: u32) -> Result<OwnedObjectPath> {
        for path in self.proxy().await?.profiles().await? {
            // Clone so we can return `path` even after building the
            // proxy from it (the proxy moves a copy in).
            let proxy = self.profile_proxy(path.clone()).await?;
            if proxy.index().await? == index {
                return Ok(path);
            }
        }
        Err(Error::NoSuchProfile {
            device: self.path.clone(),
            index,
        })
    }

    async fn profile_proxy(&self, path: OwnedObjectPath) -> Result<ProfileProxy<'static>> {
        Ok(ProfileProxy::builder(self.client.conn())
            .destination(self.client.service().bus_name().to_owned())?
            .path(path)?
            .build()
            .await?)
    }

    async fn button_proxy(&self, path: OwnedObjectPath) -> Result<ButtonProxy<'static>> {
        Ok(ButtonProxy::builder(self.client.conn())
            .destination(self.client.service().bus_name().to_owned())?
            .path(path)?
            .build()
            .await?)
    }

    /// Snapshot every button on the active profile, paired with its
    /// current binding and the set of action kinds the firmware
    /// accepts. Callers use this to populate per-button editor UI.
    #[instrument(skip(self), fields(device = %self.path.as_str()))]
    pub async fn buttons(&self) -> Result<Vec<RatbagButton>> {
        let active_idx = self.active_profile_index().await?;
        self.buttons_on_profile(active_idx).await
    }

    /// Snapshot the buttons of the profile at `profile_index`. Useful
    /// for showing the binding the user has set on profile 2 even
    /// when profile 0 is currently active.
    pub async fn buttons_on_profile(&self, profile_index: u32) -> Result<Vec<RatbagButton>> {
        let profile_path = self.find_profile_path(profile_index).await?;
        let profile = self.profile_proxy(profile_path).await?;
        let button_paths = profile.buttons().await?;

        let mut out = Vec::with_capacity(button_paths.len());
        for path in button_paths {
            let proxy = self.button_proxy(path).await?;
            let index = proxy.index().await?;
            let mapping = proxy.mapping().await?;
            let action = button::decode_mapping(&mapping)?;
            let supported = proxy.action_types().await?;
            out.push(RatbagButton {
                index,
                action,
                supported_action_types: supported,
            });
        }
        Ok(out)
    }

    /// Write a new binding into the active profile's button at
    /// `button_index`, then commit. Use [`Self::set_button_on_profile`]
    /// if you need to edit a non-active profile.
    pub async fn set_button(&self, button_index: u32, action: &ButtonAction) -> Result<()> {
        let active_idx = self.active_profile_index().await?;
        self.set_button_on_profile(active_idx, button_index, action)
            .await
    }

    #[instrument(skip(self, action), fields(device = %self.path.as_str(), profile_index, button_index))]
    pub async fn set_button_on_profile(
        &self,
        profile_index: u32,
        button_index: u32,
        action: &ButtonAction,
    ) -> Result<()> {
        let profile_path = self.find_profile_path(profile_index).await?;
        let profile = self.profile_proxy(profile_path).await?;
        let button_paths = profile.buttons().await?;

        // Find the button object whose Index property matches.
        let mut target_path: Option<OwnedObjectPath> = None;
        for path in &button_paths {
            let proxy = self.button_proxy(path.clone()).await?;
            if proxy.index().await? == button_index {
                target_path = Some(path.clone());
                break;
            }
        }
        let target_path = target_path.ok_or(Error::Ratbagd {
            op: "set_button (no matching index)",
            status: 0,
        })?;

        let proxy = self.button_proxy(target_path).await?;
        let encoded = button::encode_mapping(action);
        proxy.set_mapping(encoded).await?;
        debug!("button mapping written, committing");
        self.commit().await
    }

    /// Materialise a complete profile into a hardware slot: write
    /// all DPI stages, the active stage, every button binding, mark
    /// the profile active, then a single Commit. This is what the
    /// dispatch loop uses when the allocator says a profile needs to
    /// be written to a slot.
    ///
    /// Unlike `set_button_on_profile` (which Commits per binding to
    /// stay safe for individual edits), this method batches every
    /// write inside one `Device.Commit` — important because each
    /// Commit takes ~50ms of round-trip latency on most mice and
    /// firing a profile-switch with N buttons would otherwise be
    /// noticeably laggy.
    ///
    /// Buttons not listed in `buttons` are left at whatever the
    /// slot already holds. For self-contained profiles (the gamerat
    /// convention) the GUI populates every button so this is a non-issue.
    #[instrument(
        skip(self, dpi_stages, buttons),
        fields(device = %self.path.as_str(), slot = profile_index, stages = dpi_stages.len(), bindings = buttons.len())
    )]
    pub async fn apply_profile_complete(
        &self,
        profile_index: u32,
        dpi_stages: &[u32],
        active_stage: u32,
        buttons: &[ProfileButton],
    ) -> Result<()> {
        let profile_path = self.find_profile_path(profile_index).await?;
        let profile = self.profile_proxy(profile_path).await?;

        // ---- DPI stages ------------------------------------------
        let resolution_paths = profile.resolutions().await?;
        let resolution_count = resolution_paths.len();
        debug!(resolution_count, "writing DPI stages");
        let stages_to_write = dpi_stages.len().min(resolution_count);
        for (stage_idx, target_dpi) in dpi_stages.iter().take(stages_to_write).enumerate() {
            let Some(res_path) = resolution_paths.get(stage_idx) else {
                continue;
            };
            let res = self.resolution_proxy(res_path.clone()).await?;
            write_resolution_dpi(&res, *target_dpi).await?;
        }
        let clamped_stage = (active_stage as usize).min(resolution_count.saturating_sub(1));
        if let Some(active_path) = resolution_paths.get(clamped_stage) {
            let res = self.resolution_proxy(active_path.clone()).await?;
            let rc = res.set_active().await?;
            if rc != 0 {
                return Err(Error::Ratbagd {
                    op: "Resolution.SetActive",
                    status: rc,
                });
            }
        }

        // ---- Button bindings -------------------------------------
        // Resolve the per-button proxy paths once so we can write
        // each binding without re-iterating every time.
        if !buttons.is_empty() {
            let button_paths = profile.buttons().await?;
            debug!(button_count = button_paths.len(), "writing button bindings");
            let mut path_by_index: std::collections::BTreeMap<u32, OwnedObjectPath> =
                std::collections::BTreeMap::new();
            for path in &button_paths {
                let proxy = self.button_proxy(path.clone()).await?;
                let idx = proxy.index().await?;
                path_by_index.insert(idx, path.clone());
            }
            for binding in buttons {
                let Some(path) = path_by_index.get(&binding.index) else {
                    warn!(
                        index = binding.index,
                        "profile binds button index not present on hardware; skipping"
                    );
                    continue;
                };
                let proxy = self.button_proxy(path.clone()).await?;
                let encoded = button::encode_mapping(&binding.action);
                proxy.set_mapping(encoded).await?;
            }
        }

        // ---- Mark profile active + commit ------------------------
        let rc = profile.set_active().await?;
        if rc != 0 {
            return Err(Error::Ratbagd {
                op: "Profile.SetActive",
                status: rc,
            });
        }
        debug!("dpi + buttons + active set; committing once");
        self.commit().await
    }

    async fn resolution_proxy(&self, path: OwnedObjectPath) -> Result<ResolutionProxy<'static>> {
        Ok(ResolutionProxy::builder(self.client.conn())
            .destination(self.client.service().bus_name().to_owned())?
            .path(path)?
            .build()
            .await?)
    }

    /// Persist pending writes to the device.
    pub async fn commit(&self) -> Result<()> {
        let rc = self.proxy().await?.commit().await?;
        if rc == 0 {
            Ok(())
        } else {
            Err(Error::Ratbagd {
                op: "Device.Commit",
                status: rc,
            })
        }
    }

    async fn proxy(&self) -> Result<DeviceProxy<'_>> {
        Ok(DeviceProxy::builder(self.client.conn())
            .destination(self.client.service().bus_name().to_owned())?
            .path(self.path.as_ref())?
            .build()
            .await?)
    }
}

/// Write `dpi` to the resolution proxy, matching the variant shape
/// the property currently reports. Mice with single-axis DPI expose
/// a `u`-variant; mice with `RATBAG_RESOLUTION_CAP_SEPARATE_XY_RESOLUTION`
/// expose `(uu)` with separate X and Y. We mirror what the device
/// shows us — for separate-XY we write `(dpi, dpi)` (equal X/Y).
///
/// The property's D-Bus type is `v` (variant); zbus marshals a bare
/// `Value::U32(n)` as `u<n>` (the contained type) rather than wrapping
/// it in a variant. Wrap manually in `Value::Value(Box::new(inner))`
/// to force the wire shape `v<u<n>>` that ratbagd expects.
async fn write_resolution_dpi(res: &ResolutionProxy<'_>, dpi: u32) -> Result<()> {
    let current = res.resolution().await?;
    let inner: Value<'_> = if current.downcast_ref::<(u32, u32)>().is_ok() {
        Value::from((dpi, dpi))
    } else {
        Value::from(dpi)
    };
    let variant = Value::Value(Box::new(inner));
    res.set_resolution(variant).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_names_are_well_known() {
        assert_eq!(Service::Production.bus_name(), "org.freedesktop.ratbag1");
        assert_eq!(Service::Devel.bus_name(), "org.freedesktop.ratbag_devel1");
        assert_eq!(
            Service::Custom("org.test.ratbagd".to_owned()).bus_name(),
            "org.test.ratbagd"
        );
    }

    #[test]
    fn service_default_is_production() {
        assert_eq!(Service::default(), Service::Production);
    }
}
