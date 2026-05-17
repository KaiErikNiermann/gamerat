// Suppress the extra console window the OS would spawn alongside a
// release build on Windows. Harmless on Linux/macOS.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKit's DMA-BUF renderer trips up KWin's Wayland surface
    // handling on Plasma 6 with the GDK error "Error 71 (Protocol
    // error) dispatching to Wayland display". Disabling it forces
    // WebKit onto the older renderer, which works reliably. Only
    // touch the var if the user hasn't already set it (so explicit
    // overrides win), and only on Linux — no other platform reads
    // it.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            // SAFETY: main() runs before any threads spawn or any GTK
            // / WebKit code reads env vars, so mutating the process
            // environment here is race-free.
            unsafe {
                std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            }
        }
    }

    gamerat_gui_lib::run();
}
