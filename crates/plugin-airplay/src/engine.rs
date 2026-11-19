//! Locating, configuring and controlling the AirPlay receiver engine.
//!
//! The engine is `uxplay-windows`, a Qt GUI application. It renders its own
//! video window and keeps a tray icon. Its command line is *not* the effective
//! configuration: uxplay-windows reads its arguments from
//! `%APPDATA%\leapbtw\uxplay-windows\arguments.txt`, so the plugin writes that
//! file before starting it.
//!
//! The published 2.0.0.1736 zip is missing the GStreamer plugin scanner that
//! the bundled plugins need. `scripts/fetch-engines.ps1` installs it separately
//! and the plugin refuses to start without it, so a receiver is never launched
//! that can advertise but cannot decode video.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Executable names we accept, in preference order.
const CANDIDATES: [&str; 3] = ["uxplay-windows.exe", "uxplay.exe", "UxPlay.exe"];

/// Test hook: point the plugin at any executable.
pub const ENV_OVERRIDE: &str = "CASTPANE_AIRPLAY_ENGINE";
/// Test hook: point the plugin at another engine directory (scanner included).
pub const ENV_DIR_OVERRIDE: &str = "CASTPANE_AIRPLAY_ENGINE_DIR";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bonjour {
    Running,
    Stopped,
    Missing,
}

#[derive(Debug, Clone)]
pub struct EngineError {
    pub code: &'static str,
    pub message: String,
    pub hint: Option<String>,
}

impl EngineError {
    fn new(code: &'static str, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: Some(hint.into()),
        }
    }
}

/// `<plugin dir>/bin/uxplay/`
pub fn engine_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(ENV_DIR_OVERRIDE) {
        return PathBuf::from(dir);
    }
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

pub fn scanner_path() -> PathBuf {
    engine_dir().join("libexec/gstreamer-1.0/gst-plugin-scanner.exe")
}

/// The GStreamer plugin files that UxPlay needs for H.264 video.
pub fn video_plugins() -> [(PathBuf, &'static str); 4] {
    let lib = engine_dir().join("lib/gstreamer-1.0");
    [
        (lib.join("libgstlibav.dll"), "H.264 解码"),
        (lib.join("libgstd3d11.dll"), "D3D11 渲染"),
        (lib.join("libgstplayback.dll"), "播放管线"),
        (lib.join("libgstcoreelements.dll"), "GStreamer 核心"),
    ]
}

/// Check every file the receiver needs before spawning it.
pub fn check_ready() -> Result<PathBuf, EngineError> {
    // Test hook: an explicit engine path is assumed complete. Production uses
    // the bundled engine and gets the full component/Bonjour checks below.
    if let Ok(override_path) = std::env::var(ENV_OVERRIDE) {
        let path = PathBuf::from(override_path);
        if path.exists() {
            return Ok(path);
        }
    }

    let Some(engine) = find_engine() else {
        return Err(EngineError::new(
            "E_ENGINE_MISSING",
            format!("AirPlay 接收引擎没有安装（{}）", engine_dir().display()),
            "运行 scripts/fetch-engines.ps1 -Only airplay，或重新安装 CastPane",
        ));
    };

    let scanner = scanner_path();
    if !scanner.exists() {
        return Err(EngineError::new(
            "E_SCANNER_MISSING",
            "接收引擎缺少视频组件（GStreamer scanner）",
            "运行 scripts/fetch-engines.ps1 -Only airplay 补齐；补好后重试",
        ));
    }

    for (path, label) in video_plugins() {
        if !path.exists() {
            return Err(EngineError::new(
                "E_PLUGIN_MISSING",
                format!("接收引擎缺少{label}组件（{}）", path.display()),
                "运行 scripts/fetch-engines.ps1 -Only airplay 重新安装引擎",
            ));
        }
    }

    match bonjour_status() {
        Bonjour::Running => {}
        Bonjour::Stopped => {
            return Err(EngineError::new(
                "E_BONJOUR_STOPPED",
                "Bonjour 发现服务已安装但没有运行，iPad 会看不到本机",
                "在「服务」里启动 Bonjour Service，或点下面的「安装接收组件」",
            ));
        }
        Bonjour::Missing => {
            return Err(EngineError::new(
                "E_BONJOUR_MISSING",
                "首次使用 AirPlay 需要 Bonjour 发现服务（mDNS）",
                "点下面的「安装接收组件」；Windows 会弹出一次管理员确认",
            ));
        }
    }

    Ok(engine)
}

/// `%APPDATA%\leapbtw\uxplay-windows\arguments.txt` (the path the GUI reads).
pub fn arguments_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    base.join("leapbtw").join("uxplay-windows").join("arguments.txt")
}

/// Write the receiver name and latency choices where uxplay-windows reads them.
pub fn write_arguments(receiver_name: &str, low_latency: bool) -> std::io::Result<PathBuf> {
    let path = arguments_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let name = sanitize_name(receiver_name);
    let mut args = vec!["-n".to_string(), name, "-nh".to_string()];
    if low_latency {
        // 60 fps cap, and tell the client a short audio delay. 0.1s is well
        // within UxPlay's accepted range and keeps lip-sync acceptable.
        args.extend(["-fps".into(), "60".into(), "-al".into(), "0.1".into()]);
    }

    // Keep one backup of a user-edited file the first time we touch it.
    let backup = path.with_extension("txt.castpane.bak");
    if path.exists() && !backup.exists() {
        let _ = std::fs::copy(&path, &backup);
    }

    std::fs::write(&path, args.join(" "))?;
    Ok(path)
}

fn sanitize_name(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|c| !matches!(c, '\r' | '\n' | '"'))
        .take(40)
        .collect();
    if cleaned.trim().is_empty() {
        "CastPane".to_string()
    } else {
        cleaned.trim().to_string()
    }
}

/// Apply the GUI-level renderer/fullscreen switches through QSettings.
///
/// uxplay-windows overrides its args file from `HKCU\Software\leapbtw\
/// uxplay-windows`; writing the same QSettings keys keeps the panel honest.
pub fn apply_gui_settings(renderer: &str, fullscreen: bool) {
    let renderer = match renderer {
        "d3d11" => "d3d11",
        "d3d12" => "d3d12",
        _ => "auto",
    };
    set_qsettings("renderer_mode", renderer);
    set_qsettings("force_fs_enabled", if fullscreen { "true" } else { "false" });
    // The Bluetooth beacon is not needed when Bonjour works and spawns two
    // extra processes; keep it off.
    set_qsettings("ble_enabled", "false");
}

fn set_qsettings(name: &str, value: &str) {
    let mut command = Command::new("reg.exe");
    command.args([
        "add",
        r"HKCU\Software\leapbtw\uxplay-windows",
        "/v",
        name,
        "/t",
        "REG_SZ",
        "/d",
        value,
        "/f",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let _ = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Start the receiver. The engine writes its own session log; stdout/stderr
/// are not used because the GUI redirects them to that log.
pub fn spawn(engine: &Path) -> std::io::Result<Child> {
    let mut command = Command::new(engine);
    if let Some(dir) = engine.parent() {
        command.current_dir(dir);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.spawn()
}

/// Kill the receiver and its Bluetooth beacon child. `taskkill /T` is the only
/// reliable way to take the whole tree down on Windows.
pub fn kill_tree(pid: u32) -> bool {
    let mut command = Command::new("taskkill.exe");
    command.args(["/PID", &pid.to_string(), "/T", "/F"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// How many `uxplay-windows.exe` processes are running (single-instance makes
/// a second spawn exit immediately, so we check first to give a human error).
pub fn running_receivers() -> usize {
    let mut command = Command::new("tasklist.exe");
    command.args([
        "/FI",
        "IMAGENAME eq uxplay-windows.exe",
        "/FO",
        "CSV",
        "/NH",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let Ok(output) = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return 0;
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.trim_start().starts_with('"'))
        .count()
}

pub fn bonjour_status() -> Bonjour {
    let mut command = Command::new("sc.exe");
    command.args(["query", "Bonjour Service"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let Ok(output) = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Bonjour::Missing;
    };
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if text.contains("SERVICE_NAME") || text.contains("SERVICE_NAME:") {
        if text.contains("RUNNING") {
            Bonjour::Running
        } else {
            Bonjour::Stopped
        }
    } else {
        Bonjour::Missing
    }
}

/// Install Bonjour by running the bundled mDNSResponder installer. This is
/// user-initiated (the UI has a button) because Windows shows a UAC prompt.
pub fn install_bonjour(engine: &Path) -> Result<String, String> {
    let dir = engine.parent().unwrap_or_else(|| Path::new("."));
    let responder = dir.join("mDNSResponder.exe");
    if !responder.exists() {
        return Err(format!("找不到 {}", responder.display()));
    }

    let mut install = Command::new(&responder);
    install.arg("-install").current_dir(dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        install.creation_flags(0x0800_0000);
    }
    let output = install
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("启动 Bonjour 安装失败：{e}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            format!("Bonjour 安装器返回 {}", output.status)
        } else {
            detail
        });
    }

    let mut start = Command::new("net.exe");
    start.args(["start", "Bonjour Service"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        start.creation_flags(0x0800_0000);
    }
    let start_output = start
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("启动 Bonjour 服务失败：{e}"))?;

    match bonjour_status() {
        Bonjour::Running => Ok("Bonjour 服务已安装并运行".to_string()),
        _ => {
            let detail = format!(
                "{}\n{}",
                String::from_utf8_lossy(&start_output.stdout),
                String::from_utf8_lossy(&start_output.stderr)
            );
            Err(format!("安装后服务仍未运行：{}", detail.trim()))
        }
    }
}

/// Best-effort local IPv4 (the interface used to reach the internet).
pub fn local_ipv4() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|addr| addr.ip().to_string())
}

/// Wi-Fi SSID, when the active adapter is wireless.
pub fn wifi_ssid() -> Option<String> {
    let mut command = Command::new("netsh.exe");
    command.args(["wlan", "show", "interfaces"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("SSID") && !line.starts_with("BSSID") {
            if let Some((_, value)) = line.split_once(':') {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// TCP ports the receiver process is listening on (diagnostics only).
pub fn listening_ports(pid: u32) -> Vec<u16> {
    let mut command = Command::new("netstat.exe");
    command.args(["-ano", "-p", "tcp"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let Ok(output) = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let needle = pid.to_string();
    let mut ports = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 || !fields[3].eq_ignore_ascii_case("LISTENING") || fields[4] != needle {
            continue;
        }
        if let Some(port) = fields[1].rsplit(':').next().and_then(|p| p.parse().ok()) {
            ports.push(port);
        }
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}

pub fn wait_for_listener(pid: u32, timeout: Duration) -> Vec<u16> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let ports = listening_ports(pid);
        if !ports.is_empty() || std::time::Instant::now() >= deadline {
            return ports;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
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

    #[test]
    fn receiver_name_is_sanitized() {
        assert_eq!(sanitize_name("  CastPane  "), "CastPane");
        assert_eq!(sanitize_name(""), "CastPane");
        assert_eq!(sanitize_name("客厅\n\"iPad\""), "客厅iPad");
        assert_eq!(sanitize_name(&"x".repeat(80)).len(), 40);
    }

    #[test]
    fn arguments_path_is_under_appdata() {
        let path = arguments_path();
        assert!(path.ends_with(r"leapbtw\uxplay-windows\arguments.txt"));
    }
}
