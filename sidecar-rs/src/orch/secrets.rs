use std::path::Path;

fn service_name() -> &'static str {
    if cfg!(debug_assertions) { "panorama-orch-dev" } else { "panorama-orch" }
}

pub fn get(id: &str, base: Option<&Path>) -> Option<String> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let _ = base;
        let entry = keyring::Entry::new(service_name(), id).ok()?;
        entry.get_password().ok()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let base = base?;
        let map = read_secrets_file(base)?;
        map.get(id).cloned()
    }
}

pub fn set(id: &str, value: &str, base: Option<&Path>) -> Result<(), String> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let _ = base;
        let entry = keyring::Entry::new(service_name(), id)
            .map_err(|e| format!("keyring entry: {e}"))?;
        entry.set_password(value).map_err(|e| format!("keyring set: {e}"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let base = base.ok_or_else(|| "no base dir for secrets".to_string())?;
        let mut map = read_secrets_file(base).unwrap_or_default();
        map.insert(id.to_string(), value.to_string());
        write_secrets_file(base, &map)
    }
}

pub fn delete(id: &str, base: Option<&Path>) {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let _ = base;
        if let Ok(entry) = keyring::Entry::new(service_name(), id) {
            let _ = entry.delete_credential();
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let Some(base) = base else { return };
        let mut map = read_secrets_file(base).unwrap_or_default();
        if map.remove(id).is_some() {
            let _ = write_secrets_file(base, &map);
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn secrets_path(base: &Path) -> std::path::PathBuf {
    base.join("orch").join("secrets.json")
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn read_secrets_file(base: &Path) -> Option<std::collections::HashMap<String, String>> {
    let text = std::fs::read_to_string(secrets_path(base)).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn write_secrets_file(base: &Path, map: &std::collections::HashMap<String, String>) -> Result<(), String> {
    use std::io::Write;
    let path = secrets_path(base);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(map).map_err(|e| format!("serialize: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&tmp).map_err(|e| format!("create tmp: {e}"))?;
    file.write_all(json.as_bytes()).map_err(|e| format!("write tmp: {e}"))?;
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("set permissions: {e}"))?;
    }
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_dir(suffix: &str) -> PathBuf {
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("orch-secrets-{suffix}-{ns}"))
    }

    #[test]
    fn missing_secret_returns_none() {
        let dir = tmp_dir("miss");
        std::fs::create_dir_all(&dir).unwrap();
        let result = get("nonexistent-id-xyz", Some(&dir));
        assert!(result.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
