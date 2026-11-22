use std::path::Path;

pub fn ensure_trusted_workspace(cwd: &str) {
    let Some(home) = std::env::var("USERPROFILE").ok().or_else(|| std::env::var("HOME").ok()) else {
        return;
    };
    let settings_path = Path::new(&home)
        .join(".gemini")
        .join("antigravity-cli")
        .join("settings.json");
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
    if let Ok(json) = serde_json::to_string_pretty(&v) {
        if let Some(parent) = settings_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = settings_path.with_extension("json.tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, &settings_path);
        }
    }
}

#[cfg(windows)]
fn normalize_cwd(cwd: &str) -> String {
    cwd.replace('/', "\\").trim_end_matches('\\').to_string()
}

#[cfg(not(windows))]
fn normalize_cwd(cwd: &str) -> String {
    cwd.trim_end_matches('/').to_string()
}
