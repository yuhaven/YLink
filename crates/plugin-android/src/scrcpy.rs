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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max_size: default_max_size(),
            max_fps: default_max_fps(),
            audio: true,
            stay_awake: true,
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
}
