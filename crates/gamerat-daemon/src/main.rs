//! Entry point for the gamerat daemon.
//!
//! Eventually: connect to the session bus, expose the
//! `org.appulsauce.GameRat1` service, sit between ratbagd, the focus
//! backends, and the game library scanners. Today: a banner.

#[allow(clippy::print_stdout)]
fn main() {
    println!(
        "gamerat-daemon v{} — scaffolding, nothing to do yet.",
        env!("CARGO_PKG_VERSION"),
    );
}
