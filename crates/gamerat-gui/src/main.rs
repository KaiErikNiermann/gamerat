//! Entry point for the Slint-based gamerat GUI.
//!
//! Real implementation will load a `.slint` UI from the sibling `ui/`
//! directory and bind it to a `gamerat-proto` client. For now: a banner.

#[allow(clippy::print_stdout)]
fn main() {
    println!(
        "gamerat-gui v{} — Slint frontend not yet wired.",
        env!("CARGO_PKG_VERSION"),
    );
}
