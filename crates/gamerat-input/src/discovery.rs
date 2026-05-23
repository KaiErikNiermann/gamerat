//! Map a ratbagd device to its `/dev/input/event*` nodes via udev.
//!
//! ratbagd identifies devices by their HID `bustype:vid:pid:version`
//! string (exposed as `gamerat_proto::DeviceInfo::model`). The kernel
//! exposes the same HID device under several evdev nodes — typically
//! one for the mouse interface (`BTN_LEFT` / `BTN_RIGHT` / etc.) and
//! one or more for the keyboard interface (where remapped keys like
//! `KEY_MACRO*` show up). We need to listen on every node belonging
//! to the mouse so the trampoline keycodes reach the dispatcher
//! regardless of which interface the firmware routes them through.

use std::path::PathBuf;

use thiserror::Error;
use tracing::{debug, warn};

/// Parsed device identifier from a ratbagd `Model` string.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceMatch {
    /// USB vendor id, e.g. `0x046d` for Logitech.
    pub vendor_id: u16,
    /// USB product id.
    pub product_id: u16,
}

/// Failure modes for [`find_evdev_nodes`].
#[derive(Debug, Error)]
pub enum DiscoveryError {
    /// The ratbagd `Model` string didn't parse as
    /// `bustype:vid:pid:version`.
    #[error("malformed ratbagd model string `{0}` (expected bustype:vid:pid:version)")]
    BadModel(String),
    /// udev enumeration failed at the syscall level.
    #[error("udev enumeration failed: {0}")]
    Udev(#[from] std::io::Error),
}

/// Parse a ratbagd `Device.Model` string into a [`DeviceMatch`].
///
/// The expected shape is `bustype:vid:pid:version`. Tolerant of casing
/// on the hex digits and of a missing trailing `:version` (some
/// ratbagd builds drop it).
pub fn parse_model(model: &str) -> Result<DeviceMatch, DiscoveryError> {
    let mut parts = model.split(':');
    // Skip bustype — we only key on vid + pid.
    let _bustype = parts.next();
    let vid = parts.next().and_then(|s| u16::from_str_radix(s, 16).ok());
    let pid = parts.next().and_then(|s| u16::from_str_radix(s, 16).ok());
    match (vid, pid) {
        (Some(vendor_id), Some(product_id)) => Ok(DeviceMatch {
            vendor_id,
            product_id,
        }),
        _ => Err(DiscoveryError::BadModel(model.to_owned())),
    }
}

/// Return every `/dev/input/event*` node that belongs to the HID
/// device matching `target`. Empty Vec is a legitimate "not plugged
/// in" result (vs. an `Err` which means udev itself failed).
pub fn find_evdev_nodes(target: DeviceMatch) -> Result<Vec<PathBuf>, DiscoveryError> {
    let mut enumerator = udev::Enumerator::new()?;
    enumerator.match_subsystem("input")?;

    let mut matches = Vec::new();
    for device in enumerator.scan_devices()? {
        // Only event nodes — udev also reports the synthetic
        // `input*` parent devices which have no devnode.
        let Some(devnode) = device.devnode() else {
            continue;
        };
        if !devnode
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("event"))
        {
            continue;
        }

        let Some((vid, pid)) = read_vid_pid(&device) else {
            continue;
        };
        if vid == target.vendor_id && pid == target.product_id {
            debug!(
                ?devnode,
                vid, pid, "matched evdev node for soft-input target"
            );
            matches.push(devnode.to_path_buf());
        }
    }

    if matches.is_empty() {
        warn!(
            vendor_id = format_args!("{:04x}", target.vendor_id),
            product_id = format_args!("{:04x}", target.product_id),
            "no evdev nodes found for soft-input target — device unplugged?"
        );
    }
    Ok(matches)
}

/// Walk up the udev parent chain looking for `idVendor` / `idProduct`
/// attributes. The event node itself doesn't carry them — they live
/// on the USB or HID ancestor.
fn read_vid_pid(device: &udev::Device) -> Option<(u16, u16)> {
    let mut current = Some(device.clone());
    while let Some(dev) = current {
        let vid_attr = dev.attribute_value("idVendor");
        let pid_attr = dev.attribute_value("idProduct");
        if let (Some(vid), Some(pid)) = (vid_attr, pid_attr) {
            let vid = u16::from_str_radix(&vid.to_string_lossy(), 16).ok()?;
            let pid = u16::from_str_radix(&pid.to_string_lossy(), 16).ok()?;
            return Some((vid, pid));
        }
        current = dev.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_ratbagd_model() {
        let m = parse_model("usb:046d:c08b:0").expect("parse");
        assert_eq!(m.vendor_id, 0x046d);
        assert_eq!(m.product_id, 0xc08b);
    }

    #[test]
    fn parses_uppercase_hex() {
        let m = parse_model("usb:046D:C08B:0").expect("parse");
        assert_eq!(m.vendor_id, 0x046d);
        assert_eq!(m.product_id, 0xc08b);
    }

    #[test]
    fn parses_without_version_suffix() {
        let m = parse_model("usb:046d:c08b").expect("parse");
        assert_eq!(m.vendor_id, 0x046d);
        assert_eq!(m.product_id, 0xc08b);
    }

    #[test]
    fn rejects_malformed_model() {
        assert!(parse_model("not a model string").is_err());
        assert!(parse_model("usb:xx:yy:0").is_err());
        assert!(parse_model("usb:").is_err());
    }
}
