//! Fake `adb` for tests: prints a fixed device list.
//!
//! `MOCK_ADB_DEVICE` / `MOCK_ADB_STATE` override the defaults.

use std::io::Write;

fn main() {
    let device = std::env::var("MOCK_ADB_DEVICE").unwrap_or_else(|_| "MOCK123456".to_string());
    let state = std::env::var("MOCK_ADB_STATE").unwrap_or_else(|_| "device".to_string());

    let mut out = std::io::stdout();
    writeln!(out, "List of devices attached").unwrap();
    writeln!(
        out,
        "{device}   {state} product:mock model:Mock_Phone device:mock transport_id:1"
    )
    .unwrap();
    out.flush().unwrap();
}
