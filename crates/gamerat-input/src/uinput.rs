//! Virtual keyboard fronted by `/dev/uinput`.
//!
//! Built once at daemon startup with the full keyboard key range
//! declared, so any keycode the user binds to a soft-toggle can be
//! emitted without re-creating the device. The single shared device
//! is fine: there's no semantic difference between "one soft-toggle
//! emits `KEY_A`, another emits `KEY_B`" and "a normal keyboard
//! emits both" — apps don't distinguish event sources.

use std::path::PathBuf;

use evdev::{
    AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode, KeyEvent, uinput::VirtualDevice,
};
use gamerat_proto::trampoline_keycode;
use thiserror::Error;
use tracing::{info, warn};

/// Failure modes when working with `/dev/uinput`.
#[derive(Debug, Error)]
pub enum UinputError {
    /// `/dev/uinput` couldn't be opened (kernel module missing, or
    /// permissions are wrong — `input` group + udev rule fixes the
    /// latter).
    #[error("creating /dev/uinput virtual device: {0}")]
    Create(#[source] std::io::Error),
    /// Writing an event batch to the virtual device failed.
    #[error("emitting events: {0}")]
    Emit(#[source] std::io::Error),
    /// `evdev::uinput::VirtualDevice::get_syspath` failed — only used
    /// for the introspection helper.
    #[error("reading syspath: {0}")]
    Syspath(#[source] std::io::Error),
}

/// Wraps the virtual keyboard.
///
/// Methods are `&mut self` because uinput writes go through the
/// underlying file descriptor; the daemon keeps the emitter behind a
/// `tokio::sync::Mutex` so concurrent toggles serialize cleanly.
#[derive(Debug)]
pub struct UinputEmitter {
    device: VirtualDevice,
}

impl UinputEmitter {
    /// Build the virtual keyboard. Declares the full evdev key range
    /// up-front so any keycode the user binds is already supported —
    /// the kernel cost is a couple of bitmask bytes.
    pub fn new() -> Result<Self, UinputError> {
        let mut keys = AttributeSet::<KeyCode>::new();
        // `KEY_RESERVED` (0) up to the kernel's max key. Inserting in
        // bulk avoids a per-keycode allocation pass.
        for code in 1..=0x2ff {
            keys.insert(KeyCode::new(code));
        }
        // Sanity-check: every trampoline candidate keycode is in the
        // inserted set, so the daemon can always emit/relay one. (Cheap,
        // doc-as-test-style.) Candidates come from the proto crate and
        // are well below the 0x2ff ceiling; the truncation is fine here.
        debug_assert!(
            trampoline_keycode::candidates()
                .all(|k| u16::try_from(k).is_ok_and(|c| keys.contains(KeyCode::new(c))))
        );

        let device = VirtualDevice::builder()
            .map_err(UinputError::Create)?
            .name("gamerat soft-input")
            .input_id(InputId::new(BusType::BUS_VIRTUAL, 0x1209, 0x1, 1))
            .with_keys(&keys)
            .map_err(UinputError::Create)?
            .build()
            .map_err(UinputError::Create)?;

        info!("uinput emitter online (gamerat soft-input)");
        Ok(Self { device })
    }

    /// Press every keycode in `keys` and then release every keycode
    /// in `keys` would be a momentary tap. Toggling is the daemon's
    /// concern; this method just emits the supplied direction.
    ///
    /// `pressed = true` emits `KEY_PRESS`; `false` emits `KEY_RELEASE`.
    /// All events go in one batch so the kernel sees them atomically
    /// w.r.t. focus changes.
    pub fn emit_keys(&mut self, keys: &[u32], pressed: bool) -> Result<(), UinputError> {
        let value = i32::from(pressed);
        let events: Vec<InputEvent> = keys
            .iter()
            .filter_map(|k| {
                let code = u16::try_from(*k).ok()?;
                Some(*KeyEvent::new(KeyCode::new(code), value))
            })
            .collect();
        self.device.emit(&events).map_err(UinputError::Emit)?;
        Ok(())
    }

    /// Release every keycode in `keys`. Convenience for the "panic"
    /// path that releases everything we know to be held.
    pub fn release_all(&mut self, keys: &[u32]) {
        if let Err(e) = self.emit_keys(keys, false) {
            warn!(?e, "best-effort uinput release failed");
        }
    }

    /// Path under `/sys` for the virtual device, for diagnostics
    /// (`udevadm info -p <syspath>`).
    pub fn syspath(&mut self) -> Result<PathBuf, UinputError> {
        self.device.get_syspath().map_err(UinputError::Syspath)
    }
}

#[allow(dead_code)]
const fn _silence_dead_code_for_event_type() {
    // `EventType` is re-exported above for downstream users that want
    // to extend us with non-key events later (mouse buttons, wheels).
    let _: EventType = EventType::KEY;
}
