// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|arg| arg == "--diagnose") {
        // Headless: never creates a window.
        std::process::exit(castpane_lib::headless_diagnose());
    }
    castpane_lib::run()
}
