//! `wlr-foreign-toplevel-management-unstable-v1` backend.
//!
//! Most Wayland compositors (Sway, Hyprland, river, KWin/Plasma 6, …)
//! expose every toplevel window through this protocol along with state
//! flags including `Activated`. We bind the manager global, watch the
//! per-handle events, and collapse each `Done`-terminated batch into
//! one [`FocusEvent`] when a toplevel transitions to focused.
//!
//! ## Threading model
//!
//! `wayland-client` is dispatch-driven and blocking — it doesn't play
//! cleanly with tokio's async fns. We sidestep the impedance mismatch
//! by running the wayland event loop on its own OS thread:
//!
//! ```text
//!   wayland thread                  daemon (tokio) thread
//!   ──────────────                  ─────────────────────
//!   EventQueue.blocking_dispatch
//!     │
//!     ▼
//!   WlrState.event(...)
//!     │  on focus-gain
//!     ▼
//!   mpsc::Sender<FocusEvent>  ──►  mpsc::Receiver<FocusEvent>
//!                                          │
//!                                          ▼
//!                                  ReceiverStream
//! ```
//!
//! The thread terminates if the Wayland connection drops or the
//! receiver is dropped (channel send fails).

use std::collections::HashMap;
use std::thread;

use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info, trace, warn};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self as toplevel_handle, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self as toplevel_manager, ZwlrForeignToplevelManagerV1},
};

use crate::{BackendKind, FocusBackend, FocusEvent, FocusStream};

const PROTOCOL_NAME: &str = "zwlr_foreign_toplevel_manager_v1";
const ACTIVATED_FLAG: u32 = toplevel_handle::State::Activated as u32;

#[derive(Debug, Error)]
pub enum WlrError {
    #[error("could not connect to a Wayland compositor (WAYLAND_DISPLAY): {0}")]
    Connect(#[from] wayland_client::ConnectError),

    #[error("Wayland I/O error during initialization: {0}")]
    Io(#[from] wayland_client::DispatchError),

    #[error(
        "this compositor does not advertise `zwlr_foreign_toplevel_manager_v1`; \
         the wlr backend is unavailable. Use --backend synthetic instead."
    )]
    ProtocolUnavailable,

    #[error("could not spawn the wlr backend's dispatch thread: {0}")]
    Spawn(#[from] std::io::Error),
}

/// Focus backend built on `wlr-foreign-toplevel-management-unstable-v1`.
///
/// Spawns a dedicated thread on construction; that thread owns the
/// wayland event queue and translates protocol traffic into
/// [`FocusEvent`]s on this side.
#[derive(Debug)]
pub struct WlrForeignToplevelBackend {
    rx: mpsc::Receiver<FocusEvent>,
}

impl WlrForeignToplevelBackend {
    /// Try to bind the manager global. Returns
    /// [`WlrError::ProtocolUnavailable`] if the compositor doesn't
    /// advertise it (e.g. older GNOME without the wlr protocols).
    pub fn try_connect() -> Result<Self, WlrError> {
        let conn = Connection::connect_to_env()?;
        let display = conn.display();
        let mut queue = conn.new_event_queue::<RegistryProbe>();
        let qh = queue.handle();
        let _registry = display.get_registry(&qh, ());

        // First roundtrip: registry fires `Global` events for every
        // available interface. We grab the manager name if announced.
        let mut probe = RegistryProbe::default();
        queue.roundtrip(&mut probe)?;
        let Some((name, version)) = probe.manager else {
            return Err(WlrError::ProtocolUnavailable);
        };
        info!(
            interface = PROTOCOL_NAME,
            version, "compositor advertises wlr-foreign-toplevel-management"
        );

        // Re-key the queue around our real state and bind the manager.
        let queue = conn.new_event_queue::<WlrState>();
        let qh = queue.handle();
        let registry = display.get_registry(&qh, ());
        let manager: ZwlrForeignToplevelManagerV1 = registry.bind(name, version, &qh, ());

        let (tx, rx) = mpsc::channel(64);
        let state = WlrState::new(tx);

        thread::Builder::new()
            .name("gamerat-wlr-focus".to_owned())
            .spawn(move || run_event_loop(queue, state, manager))?;

        Ok(Self { rx })
    }
}

impl FocusBackend for WlrForeignToplevelBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::WlrForeignToplevel
    }

    fn into_stream(self) -> FocusStream {
        Box::pin(ReceiverStream::new(self.rx))
    }
}

// ─── Registry probe (first roundtrip only) ──────────────────────────────────

#[derive(Default)]
struct RegistryProbe {
    /// `(global_name, version)` of the manager, if advertised.
    manager: Option<(u32, u32)>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for RegistryProbe {
    fn event(
        state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
            && interface == PROTOCOL_NAME
        {
            state.manager = Some((name, version));
        }
    }
}

// ─── Real state (after probe) ───────────────────────────────────────────────

/// Per-toplevel buffer. `current` is the last committed (post-Done)
/// state; `pending` collects deltas during the current batch and is
/// merged into `current` on Done.
#[derive(Debug, Default)]
struct ToplevelState {
    current_app_id: String,
    current_title: String,
    currently_activated: bool,

    pending_app_id: Option<String>,
    pending_title: Option<String>,
    pending_activated: Option<bool>,
}

impl ToplevelState {
    fn apply_pending(&mut self) {
        if let Some(s) = self.pending_app_id.take() {
            self.current_app_id = s;
        }
        if let Some(s) = self.pending_title.take() {
            self.current_title = s;
        }
        if let Some(b) = self.pending_activated.take() {
            self.currently_activated = b;
        }
    }
}

#[derive(Debug)]
struct WlrState {
    tx: mpsc::Sender<FocusEvent>,
    /// Keyed by the handle proxy's object id — uniquely identifies a
    /// toplevel for the lifetime of its handle.
    toplevels: HashMap<u32, ToplevelState>,
    /// Object id of the currently focused toplevel, if any. Used to
    /// suppress emitting a focus-gain when the same toplevel re-emits
    /// `Activated` (e.g., on title change while still focused).
    focused: Option<u32>,
}

impl WlrState {
    fn new(tx: mpsc::Sender<FocusEvent>) -> Self {
        Self {
            tx,
            toplevels: HashMap::new(),
            focused: None,
        }
    }

    /// Send a focus-gain event. If the receiver is gone the channel
    /// send fails; we propagate that up the dispatch loop so the
    /// thread can exit cleanly.
    fn emit(&self, app_id: &str, title: &str) -> Result<(), mpsc::error::TrySendError<FocusEvent>> {
        let event = FocusEvent {
            app_id: app_id.to_owned(),
            title: title.to_owned(),
            source: BackendKind::WlrForeignToplevel,
        };
        trace!(?event, "wlr emitting focus event");
        self.tx.try_send(event)
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for WlrState {
    fn event(
        _state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        (): &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // After the probe roundtrip we don't care about further
        // registry events — the manager is already bound. Future
        // hot-plug of additional globals (outputs etc.) doesn't affect
        // focus tracking.
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for WlrState {
    fn event(
        _state: &mut Self,
        _manager: &ZwlrForeignToplevelManagerV1,
        event: toplevel_manager::Event,
        (): &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // `Toplevel { toplevel }` is handled by `event_created_child!`
        // below — the new proxy is automatically routed to our
        // per-handle Dispatch impl.
        if matches!(event, toplevel_manager::Event::Finished) {
            debug!("wlr manager Finished");
        }
    }

    event_created_child!(WlrState, ZwlrForeignToplevelManagerV1, [
        toplevel_manager::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for WlrState {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: toplevel_handle::Event,
        (): &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let id = handle.id().protocol_id();
        let entry = state.toplevels.entry(id).or_default();

        match event {
            toplevel_handle::Event::AppId { app_id } => {
                entry.pending_app_id = Some(app_id);
            }
            toplevel_handle::Event::Title { title } => {
                entry.pending_title = Some(title);
            }
            toplevel_handle::Event::State { state: flags } => {
                // `state` is a flat byte buffer of u32 LE; decode and
                // check for the Activated discriminant.
                let activated = flags
                    .chunks_exact(4)
                    .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
                    .any(|f| f == ACTIVATED_FLAG);
                entry.pending_activated = Some(activated);
            }
            toplevel_handle::Event::Done => {
                // Pull what we need out of the entry, then drop the
                // borrow so we can touch state.focused / state.emit.
                let (was_activated, is_activated, app_id, title) = {
                    let was = entry.currently_activated;
                    entry.apply_pending();
                    let is = entry.currently_activated;
                    (
                        was,
                        is,
                        entry.current_app_id.clone(),
                        entry.current_title.clone(),
                    )
                };

                let just_focused = is_activated && !was_activated;
                if just_focused {
                    let prior = state.focused.replace(id);
                    if prior != Some(id) {
                        if let Err(e) = state.emit(&app_id, &title) {
                            match e {
                                mpsc::error::TrySendError::Full(_) => {
                                    warn!("wlr focus channel full; dropping event");
                                }
                                mpsc::error::TrySendError::Closed(_) => {
                                    info!("wlr focus receiver dropped; thread will exit");
                                }
                            }
                        }
                    }
                } else if !is_activated && state.focused == Some(id) {
                    state.focused = None;
                }
            }
            toplevel_handle::Event::Closed => {
                state.toplevels.remove(&id);
                if state.focused == Some(id) {
                    state.focused = None;
                }
                handle.destroy();
            }
            // OutputEnter / OutputLeave / Parent are irrelevant to
            // app_id-based rule matching.
            _ => {}
        }
    }
}

fn run_event_loop(
    mut queue: wayland_client::EventQueue<WlrState>,
    mut state: WlrState,
    _manager: ZwlrForeignToplevelManagerV1,
) {
    info!("wlr backend dispatch loop started");
    loop {
        match queue.blocking_dispatch(&mut state) {
            Ok(_) => {
                // If the receiver dropped, the next try_send will fail
                // closed; we don't need to also check it here. But if
                // a no-handles dispatch returns with no events ever,
                // the Tx-closed signal won't arrive — handled via the
                // Closed branch in emit().
                if state.tx.is_closed() {
                    info!("wlr backend channel closed; exiting");
                    return;
                }
            }
            Err(e) => {
                error!(error = ?e, "wlr dispatch error; thread exiting");
                return;
            }
        }
    }
}
