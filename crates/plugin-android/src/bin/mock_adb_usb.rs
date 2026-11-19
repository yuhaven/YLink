//! Fake `adb` for the USB-first wireless flow.
//!
//! Simulates a Huawei/HarmonyOS phone that only offers USB debugging:
//! `adb tcpip 5555` plus `adb connect <phone-ip>:5555` switches it to Wi-Fi.
//!
//! Environment:
//! - `MOCK_ADB_STATE_FILE` (required): persists `tcpip` / `connected`.
//! - `MOCK_USB_PHONE_IP`: phone Wi-Fi IPv4, default `192.168.1.50`.
//! - `MOCK_ADB_MODE`: `usb` (default), `none` (no device).

use std::io::Write;

fn state_path() -> std::path::PathBuf {
    let path = std::env::var("MOCK_ADB_STATE_FILE")
        .unwrap_or_else(|_| std::env::temp_dir().join("castpane-mock-adb-state.txt").display().to_string());
    std::path::PathBuf::from(path)
}

fn read_state() -> (bool, bool) {
    let text = std::fs::read_to_string(state_path()).unwrap_or_default();
    let tcpip = text.lines().any(|line| line.trim() == "tcpip=1");
    let connected = text.lines().any(|line| line.trim() == "connected=1");
    (tcpip, connected)
}

fn write_state(tcpip: bool, connected: bool) {
    let path = state_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = format!(
        "tcpip={}\nconnected={}\n",
        if tcpip { 1 } else { 0 },
        if connected { 1 } else { 0 }
    );
    let _ = std::fs::write(path, text);
}

fn phone_ip() -> String {
    std::env::var("MOCK_USB_PHONE_IP").unwrap_or_else(|_| "192.168.1.50".into())
}

fn usb_serial() -> String {
    std::env::var("MOCK_USB_SERIAL").unwrap_or_else(|_| "MOCKUSB123".into())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = std::io::stdout();
    let (mut tcpip, mut connected) = read_state();
    let mode = std::env::var("MOCK_ADB_MODE").unwrap_or_else(|_| "usb".into());
    let ip = phone_ip();
    let endpoint = format!("{ip}:5555");

    if args.first().map(String::as_str) == Some("devices") {
        writeln!(out, "List of devices attached").unwrap();
        if mode != "none" {
            writeln!(
                out,
                "{}   device product:mock model:Mock_Phone device:mock transport_id:1",
                usb_serial()
            )
            .unwrap();
            if tcpip && connected {
                writeln!(
                    out,
                    "{endpoint}   device product:mock model:Mock_Phone device:mock transport_id:2"
                )
                .unwrap();
            }
        }
        return;
    }

    if args.first().map(String::as_str) == Some("connect") {
        let Some(target) = args.get(1) else { return };
        if target.ends_with(":5555") {
            connected = true;
            write_state(tcpip, connected);
            writeln!(out, "connected to {target}").unwrap();
        } else {
            writeln!(out, "failed to connect to {target}: Connection refused").unwrap();
            std::process::exit(1);
        }
        return;
    }

    if args.first().map(String::as_str) == Some("disconnect") {
        connected = false;
        write_state(tcpip, connected);
        writeln!(out, "disconnected").unwrap();
        return;
    }

    if args.first().map(String::as_str) == Some("mdns") {
        writeln!(out, "List of discovered mdns services").unwrap();
        return;
    }

    if args.first().map(String::as_str) == Some("kill-server")
        || args.first().map(String::as_str) == Some("start-server")
    {
        return;
    }

    // `-s <serial> <subcommand> ...`
    if args.first().map(String::as_str) == Some("-s") {
        let Some(serial) = args.get(1) else { return };
        let rest: Vec<&str> = args.iter().skip(2).map(String::as_str).collect();
        match rest.as_slice() {
            ["tcpip", "5555"] => {
                tcpip = true;
                connected = false;
                write_state(tcpip, connected);
                writeln!(out, "restarting in TCP mode port: 5555").unwrap();
            }
            ["shell", "ip", "route"] => {
                writeln!(
                    out,
                    "192.168.1.0/24 dev wlan0 proto kernel scope link src {ip}"
                )
                .unwrap();
            }
            ["shell", "getprop", "ro.product.model"] => {
                writeln!(out, "Mock_Phone").unwrap();
            }
            ["shell", "getprop", "ro.build.version.sdk"] => {
                writeln!(out, "33").unwrap();
            }
            ["shell", "settings", "get", "global", "adb_wifi_enabled"] => {
                writeln!(out, "0").unwrap();
            }
            ["shell", "settings", "put", ..] => {}
            ["shell", "input", ..] => {}
            _ if serial == &endpoint => {
                if rest.first() == Some(&"shell") {
                    match rest.get(1).copied() {
                        Some("getprop") => writeln!(out, "Mock_Phone").unwrap(),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}
