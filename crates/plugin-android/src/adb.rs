//! `adb` command execution and output parsing.
//!
//! All adb invocations redirect stdout/stderr to temp files instead of pipes.
//! adb forks a long-lived server that inherits stdio handles; a pipe would stay
//! open after the child exits and hang the reader. Files also let us return
//! partial output when an invocation times out.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How long adb may take before we give up. A hung adb must never block the
/// plugin's single-threaded RPC loop.
pub const ADB_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    /// Human name: `model:` when present, otherwise the serial.
    pub name: String,
    /// `device`, `unauthorized`, `offline`, ...
    pub state: String,
}

#[derive(Debug, Clone, Default)]
pub struct AdbOutput {
    pub success: bool,
    pub status: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
}

impl AdbOutput {
    pub fn combined(&self) -> String {
        let mut text = String::new();
        let out = self.stdout.trim();
        let err = self.stderr.trim();
        if !out.is_empty() {
            text.push_str(out);
        }
        if !err.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(err);
        }
        text
    }

    pub fn contains(&self, needle: &str) -> bool {
        self.stdout.contains(needle) || self.stderr.contains(needle)
    }
}

fn temp_path(tag: &str) -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "castpane-adb-{}-{stamp}-{id}-{tag}.txt",
        std::process::id()
    ))
}

/// Run adb with a hard timeout. Only failure to spawn is an `Err`; a non-zero
/// exit code or timeout is returned in `AdbOutput` so callers can classify it.
pub fn run(adb: &Path, args: &[&str], timeout: Duration) -> Result<AdbOutput, String> {
    let out_path = temp_path("out");
    let err_path = temp_path("err");
    let out_file = std::fs::File::create(&out_path)
        .map_err(|e| format!("cannot create {}: {e}", out_path.display()))?;
    let err_file = std::fs::File::create(&err_path)
        .map_err(|e| format!("cannot create {}: {e}", err_path.display()))?;

    let mut command = Command::new(adb);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = std::fs::remove_file(&out_path);
            let _ = std::fs::remove_file(&err_path);
            return Err(format!("cannot run {}: {e}", adb.display()));
        }
    };

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(40));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break None;
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&out_path);
                let _ = std::fs::remove_file(&err_path);
                return Err(format!("waiting for adb failed: {e}"));
            }
        }
    };

    let stdout = std::fs::read_to_string(&out_path).unwrap_or_default();
    let stderr = std::fs::read_to_string(&err_path).unwrap_or_default();
    let _ = std::fs::remove_file(&out_path);
    let _ = std::fs::remove_file(&err_path);

    Ok(AdbOutput {
        success: status.map(|s| s.success()).unwrap_or(false) && !timed_out,
        status: status.and_then(|s| s.code()),
        timed_out,
        stdout,
        stderr,
    })
}

/// Parse the output of `adb devices -l`.
///
/// ```text
/// List of devices attached
/// R58M123ABC            device product:beyond1 model:SM_G973F device:beyond1 transport_id:1
/// emulator-5554         offline
/// ```
pub fn parse_devices(output: &str) -> Vec<Device> {
    let mut devices = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("List of devices")
            || line.starts_with('*')
            || line.starts_with("adb server")
        {
            continue;
        }

        let mut parts = line.split_whitespace();
        let (Some(serial), Some(state)) = (parts.next(), parts.next()) else {
            continue;
        };

        let model = line
            .split_whitespace()
            .find_map(|token| token.strip_prefix("model:"));

        devices.push(Device {
            serial: serial.to_string(),
            name: model
                .map(|m| m.replace('_', " "))
                .unwrap_or_else(|| serial.to_string()),
            state: state.to_string(),
        });
    }

    devices
}

pub fn list_devices(adb: &Path) -> Result<Vec<Device>, String> {
    let output = run(adb, &["devices", "-l"], ADB_TIMEOUT)?;
    if output.timed_out {
        return Err(format!("adb 在 {} 秒内没有响应", ADB_TIMEOUT.as_secs()));
    }
    if !output.success && output.combined().trim().is_empty() {
        return Err(format!("adb exited with {:?}", output.status));
    }
    Ok(parse_devices(&output.stdout))
}

/// Validate `host:port` (IPv4 or bracketed IPv6). Returns the host and port.
pub fn parse_endpoint(value: &str) -> Option<(String, u16)> {
    let value = value.trim().trim_end_matches('.');
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix('[') {
        let (host, port) = rest.split_once("]:")?;
        if host.is_empty() {
            return None;
        }
        let port: u16 = port.parse().ok()?;
        return (port > 0).then(|| (host.to_string(), port));
    }
    let (host, port) = value.rsplit_once(':')?;
    if host.is_empty() || host.contains(':') {
        return None;
    }
    let port: u16 = port.parse().ok()?;
    (port > 0).then(|| (host.to_string(), port))
}

pub fn is_wireless_serial(serial: &str) -> bool {
    serial.contains(':') && parse_endpoint(serial).is_some()
}

pub fn connect(adb: &Path, endpoint: &str) -> Result<AdbOutput, String> {
    run(adb, &["connect", endpoint], CONNECT_TIMEOUT)
}

pub fn disconnect(adb: &Path, endpoint: &str) -> Result<AdbOutput, String> {
    run(adb, &["disconnect", endpoint], CONNECT_TIMEOUT)
}

pub fn tcpip(adb: &Path, serial: &str, port: u16) -> Result<AdbOutput, String> {
    run(
        adb,
        &["-s", serial, "tcpip", &port.to_string()],
        ADB_TIMEOUT,
    )
}

pub fn getprop(adb: &Path, serial: &str, prop: &str) -> Result<String, String> {
    let output = run(
        adb,
        &["-s", serial, "shell", "getprop", prop],
        ADB_TIMEOUT,
    )?;
    Ok(output.stdout.trim().to_string())
}

/// Android API level of a device, used to explain audio-forwarding support.
pub fn sdk_level(adb: &Path, serial: &str) -> Option<u32> {
    getprop(adb, serial, "ro.build.version.sdk")
        .ok()
        .and_then(|s| s.parse().ok())
}

/// The phone's Wi-Fi IPv4, read from `ip route` (`... src 192.168.1.5`).
pub fn wifi_ipv4(adb: &Path, serial: &str) -> Option<String> {
    let output = run(adb, &["-s", serial, "shell", "ip", "route"], ADB_TIMEOUT).ok()?;
    if output.timed_out {
        return None;
    }
    for line in output.stdout.lines() {
        if !line.contains("wlan") {
            continue;
        }
        let mut tokens = line.split_whitespace();
        while let Some(token) = tokens.next() {
            if token == "src" {
                if let Some(ip) = tokens.next() {
                    if ip.parse::<std::net::Ipv4Addr>().is_ok() {
                        return Some(ip.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Current display rotation in quarter turns (0..3), read from the input
/// service. `settings get system user_rotation` stays 0 while accelerometer
/// rotation is on, so it cannot be used.
pub fn surface_rotation(adb: &Path, serial: &str) -> Option<u32> {
    let output = run(adb, &["-s", serial, "shell", "dumpsys", "input"], ADB_TIMEOUT).ok()?;
    if output.timed_out {
        return None;
    }
    for line in output.stdout.lines() {
        if let Some(rest) = line.trim().strip_prefix("SurfaceOrientation:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
* daemon not running; starting now at tcp:5037
* daemon started successfully
List of devices attached
R58M123ABC            device product:beyond1 model:SM_G973F device:beyond1 transport_id:1
emulator-5554         offline
9A221FFAZ         unauthorized usb:1-2 transport_id:2
";

    #[test]
    fn parses_states_and_model_names() {
        let devices = parse_devices(SAMPLE);
        assert_eq!(devices.len(), 3);

        assert_eq!(devices[0].serial, "R58M123ABC");
        assert_eq!(devices[0].name, "SM G973F");
        assert_eq!(devices[0].state, "device");

        assert_eq!(devices[1].serial, "emulator-5554");
        assert_eq!(devices[1].name, "emulator-5554");
        assert_eq!(devices[1].state, "offline");

        assert_eq!(devices[2].state, "unauthorized");
    }

    #[test]
    fn empty_list_is_not_an_error() {
        assert!(parse_devices("List of devices attached\n\n").is_empty());
    }

    #[test]
    fn parses_wireless_serial_as_a_device() {
        let devices = parse_devices(
            "List of devices attached\n192.168.1.5:41234 device product:mock model:Mock_Phone device:mock\n",
        );
        assert_eq!(devices[0].serial, "192.168.1.5:41234");
        assert!(is_wireless_serial(&devices[0].serial));
        assert!(!is_wireless_serial("R58M123ABC"));
    }

    #[test]
    fn validates_endpoints() {
        assert_eq!(
            parse_endpoint("192.168.1.5:41234"),
            Some(("192.168.1.5".into(), 41234))
        );
        assert_eq!(
            parse_endpoint("[fe80::1]:5555"),
            Some(("fe80::1".into(), 5555))
        );
        assert_eq!(parse_endpoint("192.168.1.5"), None);
        assert_eq!(parse_endpoint("192.168.1.5:0"), None);
        assert_eq!(parse_endpoint(":5555"), None);
        assert_eq!(parse_endpoint("host:notaport"), None);
    }
}
