use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::Emitter;

const MANIFEST: &str = "plugin.json";

#[derive(Serialize, Debug, PartialEq)]
pub struct PluginSource {
    dir: String,
    manifest: Option<String>,
}

#[derive(Serialize)]
pub struct PluginScan {
    root: String,
    plugins: Vec<PluginSource>,
}

fn plugins_dir() -> Result<PathBuf, String> {
    Ok(crate::store::base_dir()?.join("plugins"))
}

fn scan(root: &Path) -> Vec<PluginSource> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut plugins: Vec<PluginSource> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let dir = e
                .file_name()
                .into_string()
                .ok()
                .filter(|d| !d.starts_with('.'))?;
            let manifest = fs::read_to_string(e.path().join(MANIFEST)).ok();
            Some(PluginSource { dir, manifest })
        })
        .collect();
    plugins.sort_by(|a, b| a.dir.cmp(&b.dir));
    plugins
}

#[tauri::command]
pub fn plugins_scan() -> Result<PluginScan, String> {
    let root = plugins_dir()?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(PluginScan {
        plugins: scan(&root),
        root: root.to_string_lossy().into_owned(),
    })
}

fn plugins_watcher() -> &'static Mutex<Option<RecommendedWatcher>> {
    static W: OnceLock<Mutex<Option<RecommendedWatcher>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(None))
}

pub fn start_plugins_watch(app: tauri::AppHandle) {
    let Ok(root) = plugins_dir() else {
        return;
    };
    if fs::create_dir_all(&root).is_err() {
        return;
    }
    let watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        if res.is_ok_and(|e| !e.kind.is_access()) {
            let _ = app.emit("plugins:changed", ());
        }
    });
    if let Ok(mut watcher) = watcher {
        if watcher.watch(&root, RecursiveMode::Recursive).is_ok() {
            *plugins_watcher().lock().unwrap() = Some(watcher);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_lists_plugin_folders_with_their_manifest() {
        let root = std::env::temp_dir().join(format!("panorama-plugins-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("b-theme")).unwrap();
        fs::write(root.join("b-theme").join(MANIFEST), "{}").unwrap();
        fs::create_dir_all(root.join("a-empty")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join("stray.json"), "{}").unwrap();

        let found = scan(&root);
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(
            found,
            vec![
                PluginSource {
                    dir: "a-empty".into(),
                    manifest: None
                },
                PluginSource {
                    dir: "b-theme".into(),
                    manifest: Some("{}".into())
                },
            ]
        );
    }

    #[test]
    fn scan_of_a_missing_folder_is_empty() {
        assert!(scan(Path::new("/nonexistent/panorama/plugins")).is_empty());
    }
}
