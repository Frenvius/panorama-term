use std::path::{Path, PathBuf};

pub fn settings_path() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE").ok().or_else(|| std::env::var("HOME").ok())?;
    Some(Path::new(&home).join(".gemini").join("antigravity-cli").join("settings.json"))
}

pub fn write_settings(settings_path: &Path, v: &serde_json::Value) -> Result<(), String> {
    let json = serde_json::to_string_pretty(v).map_err(|e| format!("serialize: {e}"))?;
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let tmp = settings_path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, settings_path).map_err(|e| format!("rename: {e}"))
}

pub fn strip_model_provider(settings_path: &Path) -> Result<bool, String> {
    let Ok(raw) = std::fs::read_to_string(settings_path) else {
        return Ok(false);
    };
    let mut v = serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| format!("parse settings: {e}"))?;
    let removed = v.as_object_mut().is_some_and(|o| o.remove("modelProvider").is_some());
    if removed {
        write_settings(settings_path, &v)?;
    }
    Ok(removed)
}

pub fn ensure_trusted_workspace(cwd: &str) {
    let Some(settings_path) = settings_path() else {
        return;
    };
    let raw = std::fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".into());
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let normalized = normalize_cwd(cwd);
    let trusted = v
        .get("trustedWorkspaces")
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().any(|e| e.as_str().map(normalize_cwd).as_deref() == Some(normalized.as_str())))
        .unwrap_or(false);
    if trusted {
        return;
    }
    let arr = v
        .get_mut("trustedWorkspaces")
        .and_then(|t| t.as_array_mut());
    match arr {
        Some(arr) => {
            arr.push(serde_json::Value::String(normalized));
        }
        None => {
            v["trustedWorkspaces"] = serde_json::json!([normalized]);
        }
    }
    let _ = write_settings(&settings_path, &v);
}

#[cfg(windows)]
fn normalize_cwd(cwd: &str) -> String {
    cwd.replace('/', "\\").trim_end_matches('\\').to_string()
}

#[cfg(not(windows))]
fn normalize_cwd(cwd: &str) -> String {
    cwd.trim_end_matches('/').to_string()
}
