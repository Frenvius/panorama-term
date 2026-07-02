use std::fs;
use std::path::PathBuf;

use serde_json::Value;

fn base_dir() -> Result<PathBuf, String> {
    Ok(dirs::config_dir().ok_or("no config dir")?.join("panorama"))
}

fn resolve(name: &str) -> Result<PathBuf, String> {
    let mut path = base_dir()?;
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err("invalid path".into());
        }
        path.push(part);
    }
    Ok(path)
}

#[tauri::command]
pub fn store_read(name: String) -> Result<Option<Value>, String> {
    let path = resolve(&name)?;
    match fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).map(Some).map_err(|e| e.to_string()),
        Err(_) => Ok(None),
    }
}

#[tauri::command]
pub fn store_write(name: String, value: Value) -> Result<(), String> {
    let path = resolve(&name)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, body).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn store_delete(name: String) -> Result<(), String> {
    let path = resolve(&name)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn store_list(dir: String) -> Result<Vec<String>, String> {
    let path = if dir.is_empty() { base_dir()? } else { resolve(&dir)? };
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&path) {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                out.push(name);
            }
        }
    }
    Ok(out)
}
