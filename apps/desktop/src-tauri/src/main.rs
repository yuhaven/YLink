// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;

fn main() {
    if std::env::args().any(|arg| arg == "--diagnose") {
        // Headless: never creates a window.
        let code = castpane_lib::headless_diagnose();
        // stdout is block-buffered when redirected; process::exit skips the
        // flush, which would leave an empty report file.
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        std::process::exit(code);
    }
    castpane_lib::run()
}
