use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use tauri::Emitter;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

fn home() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())
        .map(PathBuf::from)
}

fn sanitize(key: &str) -> String {
    key.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn workspaces_root() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("panorama").join("workspaces"))
}

fn note_path(ws_id: &str, note_id: &str) -> Option<PathBuf> {
    let dir = workspaces_root()?.join(sanitize(ws_id)).join("notes");
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{}.md", sanitize(note_id))))
}

fn binding_path(tile_id: &str) -> Option<PathBuf> {
    let dir = home()?.join(".panorama").join("agent-bindings");
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{}.json", sanitize(tile_id))))
}

fn notes_watcher() -> &'static Mutex<Option<RecommendedWatcher>> {
    static W: OnceLock<Mutex<Option<RecommendedWatcher>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(None))
}

pub fn start_notes_watch(app: tauri::AppHandle) {
    let Some(root) = workspaces_root() else {
        return;
    };
    let _ = fs::create_dir_all(&root);
    let mut watcher = match notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        let Ok(event) = res else { return };
        if event.kind.is_access() || event.kind.is_remove() {
            return;
        }
        for p in event.paths {
            if p.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if !p.components().any(|c| c.as_os_str() == "notes") {
                continue;
            }
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(content) = fs::read_to_string(&p) else {
                continue;
            };
            let _ = app.emit("note:changed", serde_json::json!({ "noteId": stem, "content": content }));
        }
    }) {
        Ok(w) => w,
        Err(_) => return,
    };
    if watcher.watch(&root, RecursiveMode::Recursive).is_ok() {
        *notes_watcher().lock().unwrap() = Some(watcher);
    }
}

#[tauri::command]
pub fn read_note(ws_id: String, note_id: String) -> Option<String> {
    note_path(&ws_id, &note_id).and_then(|p| fs::read_to_string(p).ok())
}

#[tauri::command]
pub fn write_note(ws_id: String, note_id: String, content: String) -> Result<(), String> {
    let p = note_path(&ws_id, &note_id).ok_or("no note path")?;
    fs::write(p, content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_note(ws_id: String, note_id: String) -> Result<(), String> {
    if let Some(p) = note_path(&ws_id, &note_id) {
        let _ = fs::remove_file(p);
    }
    Ok(())
}

#[tauri::command]
pub fn link_note(
    ws_id: String,
    note_id: String,
    term_tile_id: String,
    title: String,
) -> Result<String, String> {
    let p = note_path(&ws_id, &note_id).ok_or("no note path")?;
    if !p.exists() {
        let _ = fs::write(&p, "");
    }
    let md_str = p.to_string_lossy().into_owned();

    let bp = binding_path(&term_tile_id).ok_or("no home dir")?;
    let mut rec: serde_json::Value = fs::read_to_string(&bp)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(|v: &serde_json::Value| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    let obj = rec.as_object_mut().unwrap();
    obj.entry("tileId")
        .or_insert_with(|| serde_json::Value::String(term_tile_id.clone()));
    let notes = obj.entry("notes").or_insert_with(|| serde_json::json!([]));
    if !notes.is_array() {
        *notes = serde_json::json!([]);
    }
    let arr = notes.as_array_mut().unwrap();
    arr.retain(|n| n.get("noteId").and_then(|v| v.as_str()) != Some(note_id.as_str()));
    arr.push(serde_json::json!({ "noteId": note_id, "title": title, "path": md_str }));
    fs::write(&bp, rec.to_string()).map_err(|e| e.to_string())?;

    Ok(md_str)
}

fn peers_upsert(tile_id: &str, peer_id: &str, peer_name: &str) -> Result<(), String> {
    let bp = binding_path(tile_id).ok_or("no home dir")?;
    let mut rec: serde_json::Value = fs::read_to_string(&bp)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(|v: &serde_json::Value| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    let obj = rec.as_object_mut().unwrap();
    obj.entry("tileId")
        .or_insert_with(|| serde_json::Value::String(tile_id.to_string()));
    let peers = obj.entry("peers").or_insert_with(|| serde_json::json!([]));
    if !peers.is_array() {
        *peers = serde_json::json!([]);
    }
    let arr = peers.as_array_mut().unwrap();
    arr.retain(|p| p.get("tileId").and_then(|v| v.as_str()) != Some(peer_id));
    arr.push(serde_json::json!({ "tileId": peer_id, "name": peer_name }));
    fs::write(&bp, rec.to_string()).map_err(|e| e.to_string())
}

fn peers_remove(tile_id: &str, peer_id: &str) {
    let Some(bp) = binding_path(tile_id) else { return };
    let Ok(raw) = fs::read_to_string(&bp) else { return };
    let Ok(mut rec) = serde_json::from_str::<serde_json::Value>(&raw) else { return };
    if let Some(arr) = rec.get_mut("peers").and_then(|p| p.as_array_mut()) {
        arr.retain(|p| p.get("tileId").and_then(|v| v.as_str()) != Some(peer_id));
    }
    let _ = fs::write(&bp, rec.to_string());
}

#[tauri::command]
pub fn link_term(a_id: String, a_name: String, b_id: String, b_name: String) -> Result<(), String> {
    peers_upsert(&a_id, &b_id, &b_name)?;
    peers_upsert(&b_id, &a_id, &a_name)
}

#[tauri::command]
pub fn unlink_term(a_id: String, b_id: String) -> Result<(), String> {
    peers_remove(&a_id, &b_id);
    peers_remove(&b_id, &a_id);
    Ok(())
}

#[tauri::command]
pub fn unlink_note(note_id: String, term_tile_id: String) -> Result<(), String> {
    if let Some(bp) = binding_path(&term_tile_id) {
        if let Ok(raw) = fs::read_to_string(&bp) {
            if let Ok(mut rec) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(arr) = rec.get_mut("notes").and_then(|n| n.as_array_mut()) {
                    arr.retain(|n| n.get("noteId").and_then(|v| v.as_str()) != Some(note_id.as_str()));
                }
                let _ = fs::write(&bp, rec.to_string());
            }
        }
    }
    Ok(())
}
