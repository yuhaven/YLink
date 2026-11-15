//! `adb devices -l` parsing and execution.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long adb may take before we give up. A hung adb must never block the
/// plugin's single-threaded RPC loop.
const ADB_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    /// Human name: `model:` when present, otherwise the serial.
    pub name: String,
    /// `device`, `unauthorized`, `offline`, ...
    pub state: String,
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

/// Run `adb devices -l` and parse it.
///
/// Output goes to a temp file rather than a pipe on purpose: adb forks a
/// long-lived server that inherits stdio handles, and a pipe would stay open
/// after we exit, hanging whoever is reading our output.
pub fn list_devices(adb: &Path) -> Result<Vec<Device>, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let out_path = std::env::temp_dir().join(format!(
        "castpane-adb-{}-{stamp}.txt",
        std::process::id()
    ));

    let file = std::fs::File::create(&out_path)
        .map_err(|e| format!("cannot create {}: {e}", out_path.display()))?;

    let mut child = Command::new(adb)
        .args(["devices", "-l"])
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot run {}: {e}", adb.display()))?;

    let deadline = Instant::now() + ADB_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&out_path);
                return Err(format!("adb 在 {} 秒内没有响应", ADB_TIMEOUT.as_secs()));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&out_path);
                return Err(format!("waiting for adb failed: {e}"));
            }
        }
    };

    let text = std::fs::read_to_string(&out_path).unwrap_or_default();
    let _ = std::fs::remove_file(&out_path);

    if !status.success() {
        return Err(format!("adb exited with {status}"));
    }

    Ok(parse_devices(&text))
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
}
