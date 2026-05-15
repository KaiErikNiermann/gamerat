//! Tauri backend for the gamerat GUI.
//!
//! Exposes a tiny `greet` command as a sanity-check for the Rust ↔
//! Svelte IPC path. Real commands will land here as the daemon's D-Bus
//! surface stabilizes.

#[tauri::command]
fn greet(name: &str) -> String {
    format!("hello {name}, from the gamerat-gui rust backend")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::expect_used)] // Tauri entry-point convention: bail loudly on launch failure.
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running gamerat-gui tauri app");
}
