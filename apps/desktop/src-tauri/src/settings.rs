//! Settings persisted next to the executable.
//!
//! The portable build must keep its settings when the folder is moved, so the
//! file lives beside `CastPane.exe` rather than in a per-user app-data path.
//! The file is a flat map of `plugin id -> values`:
//!
//! ```json
//! { "android": { "maxSize": 1080 }, "airplay": { "fullscreen": false } }
//! ```

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Serialises read-modify-write cycles across command invocations.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

pub fn settings_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("settings.json")))
        .unwrap_or_else(|| PathBuf::from("settings.json"))
}

pub fn read_from(path: &Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

pub fn write_to(path: &Path, map: &Map<String, Value>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(dir);
        }
    }
    let text = serde_json::to_string_pretty(&Value::Object(map.clone()))
        .map_err(|e| format!("serialize settings: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

pub fn get(id: &str) -> Value {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    read_from(&settings_path())
        .get(id)
        .cloned()
        .unwrap_or(Value::Null)
}

pub fn set(id: &str, values: Value) -> Result<(), String> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = settings_path();
    let mut all = read_from(&path);
    all.insert(id.to_string(), values);
    write_to(&path, &all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "castpane-settings-{}-{name}.json",
            std::process::id()
        ))
    }

    #[test]
    fn missing_file_reads_as_empty() {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        assert!(read_from(&path).is_empty());
    }

    #[test]
    fn round_trips_per_plugin_values() {
        let path = temp_path("roundtrip");
        let _ = std::fs::remove_file(&path);

        let mut map = Map::new();
        map.insert("android".into(), json!({"maxSize": 720, "audio": false}));
        write_to(&path, &map).expect("write");

        let read = read_from(&path);
        assert_eq!(read["android"]["maxSize"], json!(720));
        assert_eq!(read["android"]["audio"], json!(false));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_file_is_ignored_not_fatal() {
        let path = temp_path("corrupt");
        std::fs::write(&path, "{not json").unwrap();
        assert!(read_from(&path).is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
