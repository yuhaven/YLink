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

#[derive(Debug, Clone, Deserialize)]
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max_size: default_max_size(),
            max_fps: default_max_fps(),
            audio: true,
            stay_awake: true,
            lock_orientation: true,
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
    if settings.lock_orientation {
        // `@0` locks the capture to the device's natural (portrait)
        // orientation. A bare `@` would lock to whatever orientation the
        // device happened to be in when the session started.
        args.push("--capture-orientation=@0".to_string());
    }

    if let Some(w) = window {
        args.push(format!("--window-x={}", w.x));
        args.push(format!("--window-y={}", w.y));
        args.push(format!("--window-width={}", w.width));
        args.push(format!("--window-height={}", w.height));
        if w.borderless {
            args.push("--window-borderless".to_string());
        }
        if w.always_on_top {
            args.push("--always-on-top".to_string());
        }
    }

    args
}

pub fn spawn(scrcpy: &Path, args: &[String]) -> std::io::Result<Child> {
    Command::new(scrcpy)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // scrcpy logs go to the plugin's stderr, which the host turns into
        // `log` notifications.
        .stderr(Stdio::inherit())
        .spawn()
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
        let args = build_args("R58M", "Pixel 5", &Settings::default(), Some(&portrait()));
        let joined = args.join(" ");

        assert!(joined.contains("-s R58M"));
        assert!(joined.contains("--window-x=700"));
        assert!(joined.contains("--window-width=527"));
        assert!(joined.contains("--window-height=936"));
        assert!(joined.contains("--window-borderless"));
        assert!(joined.contains("--always-on-top"));
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
        });
        let settings: Settings = serde_json::from_value(from_manifest).expect("settings");

        let defaults = Settings::default();
        assert_eq!(settings.max_size, defaults.max_size);
        assert_eq!(settings.max_fps, defaults.max_fps);
        assert_eq!(settings.audio, defaults.audio);
        assert_eq!(settings.stay_awake, defaults.stay_awake);
        assert_eq!(settings.lock_orientation, defaults.lock_orientation);
    }
}
