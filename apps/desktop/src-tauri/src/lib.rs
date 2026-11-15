use castpane_core::{Capability, PanelItem, PluginHost};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;

/// What the UI needs to render one plugin card. Everything here comes from
/// `plugin.json`; the shell has no per-protocol knowledge.
#[derive(Debug, Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Vec<Capability>,
    pub settings: serde_json::Value,
    pub panel: Vec<PanelItem>,
    pub running: bool,
}

struct AppState {
    plugins: Mutex<Vec<PluginHost>>,
}

/// In development the plugins live in the repo; in a release build they ship
/// next to the executable.
fn plugins_dir() -> PathBuf {
    if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("plugins")))
            .unwrap_or_else(|| PathBuf::from("plugins"))
    }
}

fn load_plugins() -> Vec<PluginHost> {
    let dir = plugins_dir();
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("plugins directory not found: {}", dir.display());
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        match PluginHost::load(&path) {
            Ok(host) => {
                if host.manifest().supports_current_platform() {
                    out.push(host);
                }
            }
            Err(err) => eprintln!("skip plugin {}: {err}", path.display()),
        }
    }
    out.sort_by(|a, b| a.manifest().id.cmp(&b.manifest().id));
    out
}

#[tauri::command]
fn list_plugins(state: tauri::State<'_, AppState>) -> Vec<PluginInfo> {
    let plugins = state.plugins.lock().unwrap();
    plugins
        .iter()
        .map(|host| {
            let m = host.manifest();
            PluginInfo {
                id: m.id.clone(),
                name: m.name.clone(),
                version: m.version.clone(),
                description: m.description.clone(),
                capabilities: m.capabilities.clone(),
                settings: m.settings.clone(),
                panel: m.panel.clone(),
                running: host.is_running(),
            }
        })
        .collect()
}

#[tauri::command]
fn start_plugin(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut plugins = state.plugins.lock().unwrap();
    let host = plugins
        .iter_mut()
        .find(|p| p.manifest().id == id)
        .ok_or_else(|| format!("plugin `{id}` not found"))?;
    host.start().map_err(|e| e.to_string())
}

pub fn run() {
    let plugins = load_plugins();
    eprintln!(
        "loaded {} plugin(s) from {}",
        plugins.len(),
        plugins_dir().display()
    );

    tauri::Builder::default()
        .manage(AppState {
            plugins: Mutex::new(plugins),
        })
        .invoke_handler(tauri::generate_handler![list_plugins, start_plugin])
        .run(tauri::generate_context!())
        .expect("error while running CastPane");
}
