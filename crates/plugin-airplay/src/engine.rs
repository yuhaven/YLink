//! Locating and launching the AirPlay receiver engine.
//!
//! The engine is `uxplay-windows`, a Qt GUI application. It renders its own
//! window, so this module only starts and stops it.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Executable names we accept, in preference order.
const CANDIDATES: [&str; 3] = ["uxplay-windows.exe", "uxplay.exe", "UxPlay.exe"];

/// Test hook: point the plugin at any executable.
pub const ENV_OVERRIDE: &str = "CASTPANE_AIRPLAY_ENGINE";

/// `<plugin dir>/bin/uxplay/`
pub fn engine_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("uxplay")))
        .unwrap_or_else(|| PathBuf::from("uxplay"))
}

pub fn find_engine() -> Option<PathBuf> {
    if let Ok(override_path) = std::env::var(ENV_OVERRIDE) {
        let path = PathBuf::from(override_path);
        if path.exists() {
            return Some(path);
        }
    }
    let dir = engine_dir();
    CANDIDATES
        .iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.exists())
}

/// Start the receiver. stderr is inherited so its logs reach the host.
pub fn spawn(engine: &Path) -> std::io::Result<Child> {
    let mut command = Command::new(engine);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kept as one test: `set_var` is process-wide and unit tests run in
    /// parallel threads.
    #[test]
    fn override_rules() {
        let sys = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        if sys.exists() {
            std::env::set_var(ENV_OVERRIDE, &sys);
            assert_eq!(find_engine().as_deref(), Some(sys.as_path()));
        }

        std::env::set_var(ENV_OVERRIDE, r"C:\does\not\exist.exe");
        assert!(find_engine().is_none(), "missing override must fall through");

        std::env::remove_var(ENV_OVERRIDE);
    }
}
