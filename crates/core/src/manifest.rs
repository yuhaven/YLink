//! Plugin manifest (`plugin.json`) and its declarative UI description.
//!
//! A plugin is a directory:
//!
//! ```text
//! plugins/airplay/
//!   plugin.json
//!   bin/airplay-plugin.exe
//! ```
//!
//! The manifest is the whole contract. Adding a protocol must never require
//! touching the core or the UI.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Root of `plugin.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// Contract version this plugin was written against.
    #[serde(default)]
    pub api: u32,
    /// Host platforms this plugin supports: "windows", "macos", "linux".
    #[serde(default)]
    pub platforms: Vec<String>,
    /// Executable, relative to the plugin directory.
    pub entry: String,
    /// Extra arguments passed to `entry`, e.g. `["plugin.py"]` when the entry
    /// is an interpreter. Empty for a plain executable.
    #[serde(default)]
    pub args: Vec<String>,
    /// What the plugin can do: "discover", "session", "window", "diagnose".
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    /// JSON Schema for the settings object passed back to the plugin.
    #[serde(default)]
    pub settings: serde_json::Value,
    /// Declarative UI. Rendered generically by the shell.
    #[serde(default)]
    pub panel: Vec<PanelItem>,
    /// Where this plugin's directory lives. Filled in by the loader.
    #[serde(skip)]
    pub root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Discover,
    Session,
    Window,
    Diagnose,
}

/// One control in the plugin's settings panel.
///
/// The shell renders these; a plugin never ships UI code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PanelItem {
    Switch {
        key: String,
        label: String,
    },
    Select {
        key: String,
        label: String,
    },
    Slider {
        key: String,
        label: String,
        min: f64,
        max: f64,
        #[serde(default = "default_step")]
        step: f64,
    },
    Button {
        action: String,
        label: String,
    },
    Text {
        text: String,
    },
    Divider,
}

fn default_step() -> f64 {
    1.0
}

impl PluginManifest {
    /// Read `<dir>/plugin.json`, filling in `root`.
    pub fn load(dir: impl Into<PathBuf>) -> Result<Self, ManifestError> {
        let dir = dir.into();
        let path = dir.join("plugin.json");
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| ManifestError::Io(path.clone(), e.to_string()))?;
        let mut manifest: PluginManifest = serde_json::from_str(&raw)
            .map_err(|e| ManifestError::Parse(path.clone(), e.to_string()))?;
        if manifest.api > crate::PLUGIN_API_VERSION {
            return Err(ManifestError::ApiTooNew {
                id: manifest.id,
                found: manifest.api,
                supported: crate::PLUGIN_API_VERSION,
            });
        }
        manifest.root = dir;
        Ok(manifest)
    }

    /// Absolute path of the plugin executable.
    pub fn entry_path(&self) -> PathBuf {
        self.root.join(&self.entry)
    }

    pub fn supports_current_platform(&self) -> bool {
        if self.platforms.is_empty() {
            return true;
        }
        let here = if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        self.platforms.iter().any(|p| p == here)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("cannot read {0}: {1}")]
    Io(PathBuf, String),
    #[error("cannot parse {0}: {1}")]
    Parse(PathBuf, String),
    #[error("plugin `{id}` needs api {found}, core supports {supported}")]
    ApiTooNew {
        id: String,
        found: u32,
        supported: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_manifest() {
        let raw = r#"{
            "id": "airplay",
            "name": "AirPlay",
            "version": "0.1.0",
            "entry": "bin/airplay-plugin.exe",
            "panel": [
                {"type": "switch", "key": "fullscreen", "label": "全屏"},
                {"type": "slider", "key": "bitrate", "label": "码率", "min": 1, "max": 20},
                {"type": "divider"}
            ]
        }"#;
        let m: PluginManifest = serde_json::from_str(raw).unwrap();
        assert_eq!(m.id, "airplay");
        assert_eq!(m.api, 0);
        assert_eq!(m.panel.len(), 3);
        assert!(matches!(m.panel[2], PanelItem::Divider));
    }

    #[test]
    fn empty_platforms_means_all() {
        let raw = r#"{"id":"x","name":"X","version":"0","entry":"x"}"#;
        let m: PluginManifest = serde_json::from_str(raw).unwrap();
        assert!(m.supports_current_platform());
    }
}
