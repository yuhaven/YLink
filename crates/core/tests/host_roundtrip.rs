//! End-to-end check of the plugin host against the bundled mock plugin:
//! spawn -> initialize -> request/response -> notification -> shutdown.

use castpane_core::{PluginEvent, PluginHost};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn write_manifest(dir: &std::path::Path, entry: &str) -> PathBuf {
    let manifest = serde_json::json!({
        "id": "mock",
        "name": "Mock",
        "version": "0.0.1",
        "api": 0,
        "entry": entry,
        "capabilities": ["discover", "diagnose"],
        "panel": [{"type": "text", "text": "只用于测试"}]
    });
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
    dir.to_path_buf()
}

fn mock_plugin_path() -> PathBuf {
    // `cargo test` exports this; fall back to the target dir for manual runs.
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_mock-plugin") {
        return PathBuf::from(path);
    }
    let exe = std::env::current_exe().expect("test exe path");
    let dir = exe.parent().expect("target dir");
    for candidate in [
        dir.join("mock-plugin.exe"),
        dir.join("mock-plugin"),
        dir.parent()
            .map(|p| p.join("mock-plugin.exe"))
            .unwrap_or_default(),
    ] {
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("mock-plugin binary not found next to {}", dir.display());
}

#[test]
fn host_roundtrip_with_mock_plugin() {
    let entry = mock_plugin_path();
    let dir = std::env::temp_dir().join(format!("castpane-mock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_manifest(&dir, entry.to_str().expect("utf-8 path"));

    let mut host = PluginHost::load(&dir).expect("load manifest");
    assert_eq!(host.manifest().id, "mock");
    assert!(host.manifest().panel.len() == 1);

    host.start().expect("handshake");
    assert!(host.is_running());

    let result = host
        .call("discover.start", serde_json::json!({}))
        .expect("discover.start");
    assert_eq!(result["ok"], serde_json::json!(true));

    // The device.found notification races the response; wait for it briefly.
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut found = false;
    while Instant::now() < deadline {
        match host.try_next_event() {
            Some(PluginEvent::Notification { method, params }) if method == "device.found" => {
                assert_eq!(params["id"], "mock-1");
                found = true;
                break;
            }
            Some(_) => continue,
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    assert!(found, "device.found was not delivered");

    let checks = host
        .call("diagnose", serde_json::json!({}))
        .expect("diagnose");
    assert_eq!(checks["checks"][0]["ok"], serde_json::json!(true));

    // Unknown methods surface as an Rpc error carrying a hint.
    let err = host.call("nope", serde_json::json!({})).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("E_UNSUPPORTED"), "unexpected error: {text}");

    host.stop();
    assert!(!host.is_running());

    let _ = std::fs::remove_dir_all(&dir);
}
