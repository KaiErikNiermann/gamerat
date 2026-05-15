// Suppress the extra console window the OS would spawn alongside a
// release build on Windows. Harmless on Linux/macOS.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    gamerat_gui_lib::run();
}
