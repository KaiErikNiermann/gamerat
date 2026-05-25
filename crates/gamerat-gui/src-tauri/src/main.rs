// Suppress the extra console window the OS would spawn alongside a
// release build on Windows. Harmless on Linux/macOS.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Honor `--version` / `--help` before doing anything that
    // touches GTK or the WebKit env. Tauri's runtime tries to init
    // GTK during `run()` and panics in headless environments like
    // CI smoke containers — short-circuiting here keeps
    // `gamerat-gui --version` working everywhere with zero extra
    // deps.
    //
    // Printing version/help text to stdout *is* the intended output
    // here, so the workspace-wide print_stdout lint doesn't apply.
    #[allow(clippy::print_stdout)]
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("gamerat-gui {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                println!(
                    "gamerat-gui — Tauri desktop frontend for gamerat\n\n\
                     Usage: gamerat-gui [--version|--help]\n\n\
                     Run with no args to launch the GUI."
                );
                return;
            }
            _ => {}
        }
    }

    // Two Linux-specific env vars need to be in place before GTK /
    // WebKit init. We only set each if the user hasn't already set
    // it, so explicit overrides win.
    //
    // SAFETY: main() runs before any threads spawn or any GTK /
    // WebKit code reads env vars, so mutating the process
    // environment here is race-free.
    #[cfg(target_os = "linux")]
    unsafe {
        // WebKit's DMA-BUF renderer trips up KWin's Wayland surface
        // handling on Plasma 6 with the GDK error "Error 71
        // (Protocol error) dispatching to Wayland display".
        // Disabling it forces WebKit onto the older renderer, which
        // works reliably.
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }

        // Force the GTK X11 backend (i.e. route via XWayland on
        // Wayland sessions). On native Wayland with
        // `decorations: false`, tao's implicit 5px edge-resize
        // handler takes a strict compositor pointer grab via
        // `xdg_toplevel.resize`, which swallows the matching
        // mouseup. Page scrollbars whose interactive zone overlaps
        // those 5px get pinned in their "thumb pressed" state until
        // another click jolts the lock loose. Under XWayland the
        // pointer grab is looser, the mouseup leaks through to the
        // webview, and the scrollbar resets cleanly. See the
        // tao-implicit-resize-saga memory for the full archaeology.
        if std::env::var_os("GDK_BACKEND").is_none() {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }

    gamerat_gui_lib::run();
}
