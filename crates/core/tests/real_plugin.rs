//! Exercises the host against the *real* android plugin binary.
//!
//! Skipped when the plugin has not been packaged yet
//! (`scripts/package-plugin.ps1 -Plugin android`).

use castpane_core::{PluginEvent, PluginHost};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn talks_to_the_packaged_android_plugin() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/android");
    if !dir.join("bin/android-plugin.exe").exists() {
        eprintln!("skip: run scripts/package-plugin.ps1 -Plugin android first");
        return;
    }

    let mut host = PluginHost::load(&dir).expect("load android manifest");
    assert_eq!(host.manifest().id, "android");
    host.start().expect("plugin handshake");

    host.call("discover.start", serde_json::json!({}))
        .expect("discover.start");

    // With adb present and no device attached the plugin emits nothing; with
    // adb missing it emits a log line. Both are valid, so just drain and make
    // sure the plugin stayed responsive afterwards.
    std::thread::sleep(Duration::from_millis(300));
    let _ = host.drain_events();

    // Declarative panel buttons call `action.<name>`; diagnose must answer
    // with a checks array the UI can render.
    let diag = host
        .call("action.diagnose", serde_json::json!({}))
        .expect("action.diagnose (stale binary? run scripts/package-plugin.ps1 -Plugin android)");
    let checks = diag["checks"].as_array().expect("checks array");
    assert!(checks.iter().any(|c| c["name"] == "scrcpy"));

    host.stop();
}
