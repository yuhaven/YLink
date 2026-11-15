//! `adb devices -l` parsing and execution.

use std::path::Path;
use std::process::Command;

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
pub fn list_devices(adb: &Path) -> Result<Vec<Device>, String> {
    let output = Command::new(adb)
        .arg("devices")
        .arg("-l")
        .output()
        .map_err(|e| format!("cannot run {}: {e}", adb.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("adb exited with {}: {}", output.status, stderr.trim()));
    }

    Ok(parse_devices(&String::from_utf8_lossy(&output.stdout)))
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
