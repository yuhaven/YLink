//! scrcpy command line construction and launch.
//!
//! The window is positioned by scrcpy itself (`--window-x/y/width/height`),
//! so this plugin never needs a native window handle.

use serde::Deserialize;
use std::path::Path;
use std::process::{Child, Command, Stdio};

fn default_max_size() -> u32 {
    1080
}
fn default_max_fps() -> u32 {
    60
}
fn default_true() -> bool {
    true
}
fn default_orientation() -> String {
    "0".to_string()
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "default_max_size")]
    pub max_size: u32,
    #[serde(default = "default_max_fps")]
    pub max_fps: u32,
    #[serde(default = "default_true")]
    pub audio: bool,
    #[serde(default = "default_true")]
    pub stay_awake: bool,
    /// Keep the capture orientation fixed so the picture cannot flip to
    /// landscape when the phone rotates.
    #[serde(default = "default_true")]
    pub lock_orientation: bool,
    /// `portrait`, `landscape` or `auto`.
    #[serde(default = "default_orientation")]
    pub orientation: String,
    /// Pin the window position (dragging snaps back). Off by default.
    #[serde(default)]
    pub lock_position: bool,
    /// 0..100 screen brightness; 0 leaves the phone untouched.
    #[serde(default)]
    pub brightness: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max_size: default_max_size(),
            max_fps: default_max_fps(),
            audio: true,
            stay_awake: true,
            lock_orientation: true,
            orientation: default_orientation(),
            lock_position: false,
            brightness: 0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSpec {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub borderless: bool,
    #[serde(default)]
    pub always_on_top: bool,
}

/// Translate a session request into scrcpy arguments.
pub fn build_args(
    serial: &str,
    title: &str,
    settings: &Settings,
    window: Option<&WindowSpec>,
) -> Vec<String> {
    let mut args = vec![
        "-s".to_string(),
        serial.to_string(),
        format!("--window-title={title}"),
        format!("--max-size={}", settings.max_size.clamp(480, 2160)),
        format!("--max-fps={}", settings.max_fps.clamp(15, 120)),
    ];

    if settings.audio {
        args.push("--audio-source=playback".to_string());
    } else {
        args.push("--no-audio".to_string());
    }
    if settings.stay_awake {
        args.push("--stay-awake".to_string());
    }
    // Rotate the captured video only. Rotating the device display would turn
    // the phone's own screen too, and a portrait-locked app cannot relayout
    // anyway. 0 means "as the phone shows it", so it is never upside down.
    match settings.orientation.as_str() {
        "auto" => {
            if settings.lock_orientation {
                args.push("--capture-orientation=@".to_string());
            }
        }
        angle => {
            let angle = angle.parse::<u32>().unwrap_or(0) % 360;
            args.push(format!("--capture-orientation=@{angle}"));
        }
    }

    if let Some(w) = window {
        // Size follows the video aspect (the phone decides it); only the
        // position is ours, so a rotated picture is never squashed.
        args.push(format!("--window-x={}", w.x));
        args.push(format!("--window-y={}", w.y));
        // Native frame on purpose: title bar, minimise, maximise and close.
        // Never topmost: it would cover the console.
        let _ = w;
    }

    args
}

pub fn spawn(scrcpy: &Path, args: &[String]) -> std::io::Result<Child> {
    let mut command = Command::new(scrcpy);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // scrcpy logs go to the plugin's stderr, which the host turns into
        // `log` notifications.
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window for the engine either.
        command.creation_flags(0x0800_0000);
    }
    command.spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn portrait() -> WindowSpec {
        WindowSpec {
            x: 700,
            y: 52,
            width: 527,
            height: 936,
            borderless: true,
            always_on_top: true,
        }
    }

    #[test]
    fn builds_portrait_window_arguments() {
        let settings = Settings::default();
        let args = build_args("R58M", "Pixel 5", &settings, Some(&portrait()));
        let joined = args.join(" ");

        assert!(joined.contains("-s R58M"));
        assert!(joined.contains("--window-x=700"));
        // Native frame on purpose: title bar, minimise, maximise and close.
        assert!(
            !joined.contains("--window-borderless"),
            "the cast window must keep its Windows title bar: {joined}"
        );
        assert!(!joined.contains("--always-on-top"));
        assert!(joined.contains("--max-size=1080"));
        assert!(joined.contains("--audio-source=playback"));
    }


    #[test]
    fn clamps_insane_settings() {
        let settings = Settings {
            max_size: 9000,
            max_fps: 1000,
            ..Settings::default()
        };
        let args = build_args("dev", "t", &settings, None);
        let joined = args.join(" ");
        assert!(joined.contains("--max-size=2160"));
        assert!(joined.contains("--max-fps=120"));
    }

    #[test]
    fn no_audio_flag_when_disabled() {
        let settings = Settings {
            audio: false,
            ..Settings::default()
        };
        let joined = build_args("dev", "t", &settings, None).join(" ");
        assert!(joined.contains("--no-audio"));
        assert!(!joined.contains("--audio-source"));
    }

    #[test]
    fn omits_window_flags_without_spec() {
        let joined = build_args("dev", "t", &Settings::default(), None).join(" ");
        assert!(!joined.contains("--window-x"));
    }

    #[test]
    fn deserializes_camel_case_settings_from_the_host() {
        let raw = serde_json::json!({
            "maxSize": 720,
            "maxFps": 30,
            "audio": false,
            "stayAwake": false
        });
        let settings: Settings = serde_json::from_value(raw).expect("settings");
        assert_eq!(settings.max_size, 720);
        assert_eq!(settings.max_fps, 30);
        assert!(!settings.audio);
        assert!(!settings.stay_awake);
    }

    #[test]
    fn empty_settings_fall_back_to_defaults() {
        let settings: Settings = serde_json::from_value(serde_json::json!({})).expect("settings");
        assert_eq!(settings.max_size, Settings::default().max_size);
        assert!(settings.audio);
    }

    #[test]
    fn landscape_rotates_the_capture() {
        let settings = Settings {
            orientation: "90".to_string(),
            ..Settings::default()
        };
        let joined = build_args("dev", "t", &settings, None).join(" ");
        assert!(joined.contains("--capture-orientation=@90"), "{joined}");
    }

    #[test]
    fn portrait_locks_to_natural_orientation() {
        let joined = build_args("dev", "t", &Settings::default(), None).join(" ");
        assert!(joined.contains("--capture-orientation=@0"), "{joined}");
    }

    /// The manifest is the contract the UI renders; the code must agree with
    /// its defaults, otherwise the panel lies about what will happen.
    #[test]
    fn manifest_defaults_agree_with_code() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/android/plugin.json");
        let raw = std::fs::read_to_string(&path).expect("read plugin.json");
        let manifest: serde_json::Value = serde_json::from_str(&raw).expect("parse plugin.json");
        let props = &manifest["settings"]["properties"];

        let from_manifest = serde_json::json!({
            "maxSize": props["maxSize"]["default"],
            "maxFps": props["maxFps"]["default"],
            "audio": props["audio"]["default"],
            "stayAwake": props["stayAwake"]["default"],
            "lockOrientation": props["lockOrientation"]["default"],
            "orientation": props["orientation"]["default"],
        });
        let settings: Settings = serde_json::from_value(from_manifest).expect("settings");

        let defaults = Settings::default();
        assert_eq!(settings.max_size, defaults.max_size);
        assert_eq!(settings.max_fps, defaults.max_fps);
        assert_eq!(settings.audio, defaults.audio);
        assert_eq!(settings.stay_awake, defaults.stay_awake);
        assert_eq!(settings.lock_orientation, defaults.lock_orientation);
        assert_eq!(settings.orientation, defaults.orientation);
    }
}