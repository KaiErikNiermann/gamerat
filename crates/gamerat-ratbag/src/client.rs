//! Ergonomic async wrapper around the ratbagd D-Bus surface.
//!
//! The proxy traits in [`crate::proxy`] are a faithful mirror of
//! ratbagd's wire shape; this module hides the dance of "find the
//! right profile object → call `SetActive` → call `Commit` on the
//! device" behind one method on [`Device`].

use tracing::{debug, instrument, warn};
use zbus::Connection;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

use crate::error::{Error, Result};
use crate::proxy::{DeviceProxy, ManagerProxy, ProfileProxy};

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

    /// Set the profile at `index` active and persist via `Commit`. This
    /// is the only mutation gamerat-ratbag currently exposes.
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
