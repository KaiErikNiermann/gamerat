//! Active-window / focus tracking across Linux desktop stacks.
//!
//! The daemon needs to know "what is the user looking at right now?" so
//! it can match against per-application profile rules. This crate hides
//! the absolute zoo of focus APIs behind one trait.
//!
//! Planned backends:
//!
//! | Backend           | Mechanism                                      |
//! | ----------------- | ---------------------------------------------- |
//! | `x11`             | `_NET_ACTIVE_WINDOW` via xcb                   |
//! | `wayland_wlroots` | `wlr-foreign-toplevel-management-unstable-v1`  |
//! | `kwin`            | `KWin` script + D-Bus pipe (Plasma has no ext) |
//! | `gnome_shell`     | Shell extension shim, best-effort              |
//!
//! Scaffolding only.
