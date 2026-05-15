//! Entry point for `gameratctl`, the scriptable gamerat client.
//!
//! Will speak the daemon's D-Bus interface; today, a banner.

#[allow(clippy::print_stdout)]
fn main() {
    println!(
        "gameratctl v{} — scaffolding, no subcommands wired yet.",
        env!("CARGO_PKG_VERSION"),
    );
}
