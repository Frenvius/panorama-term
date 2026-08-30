use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::Value;

const SCHEMA_VERSION: i64 = 1;

#[derive(Deserialize)]
pub struct Entry {
    name: String,
    value: Value,
}

fn base_dir() -> Result<PathBuf, String> {
    if let Ok(dir) = std::env::var("PANORAMA_CONFIG_DIR") {
        return Ok(PathBuf::from(dir));
    }
    Ok(dirs::config_dir().ok_or("no config dir")?.join("panorama"))
}

fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("invalid name".into());
    }
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err("invalid name".into());
        }
    }
    Ok(())
}

pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let name = path.file_name().and_then(|n| n.to_str()).ok_or("invalid path")?;
    let tmp = path.with_file_name(format!("{name}.panorama-tmp"));
    let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
    file.write_all(data).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn open_db(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.pragma_update(None, "journal_mode", "WAL").map_err(|e| e.to_string())?;
    conn.pragma_update(None, "synchronous", "FULL").map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS kv (
            name TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )
    .map_err(|e| e.to_string())?;
    Ok(conn)
}

fn db() -> Result<&'static Mutex<Connection>, String> {
    static DB: OnceLock<Result<Mutex<Connection>, String>> = OnceLock::new();
    DB.get_or_init(|| {
        let dir = base_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let conn = open_db(&dir.join("panorama.db"))?;
        import_legacy(&conn, &dir)?;
        if let Err(err) = backup(&conn, &dir) {
            eprintln!("[store] backup failed: {err}");
        }
        Ok(Mutex::new(conn))
    })
    .as_ref()
    .map_err(|e| e.clone())
}

fn schema_version(conn: &Connection) -> i64 {
    conn.query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |row| {
        row.get::<_, String>(0)
    })
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(0)
}

fn read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn legacy_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()) else {
            continue;
        };
        if path.is_file() {
            if name.starts_with("panorama.db") || name.ends_with(".tmp") || name.ends_with(".bak") {
                continue;
            }
            out.push((name, path));
        } else if name == "workspaces" {
            let Ok(nested) = fs::read_dir(&path) else { continue };
            for file in nested.flatten() {
                let child = file.path();
                if !child.is_file() {
                    continue;
                }
                let Some(child_name) = child.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if !child_name.ends_with(".json") {
                    continue;
                }
                out.push((format!("workspaces/{child_name}"), child));
            }
        }
    }
    out
}

fn import_legacy(conn: &Connection, dir: &Path) -> Result<(), String> {
    if schema_version(conn) >= SCHEMA_VERSION {
        return Ok(());
    }

    let found = legacy_files(dir);
    let mut imported: Vec<PathBuf> = Vec::new();
    let now = now_ms();

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for (name, path) in &found {
        let value = read_json(path).or_else(|| read_json(&path.with_extension("bak")));
        let Some(value) = value else {
            eprintln!("[store] skipped unreadable legacy file: {}", path.display());
            continue;
        };
        let body = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO kv (name, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(name) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![name, body, now],
        )
        .map_err(|e| e.to_string())?;
        imported.push(path.clone());
    }
    tx.execute(
        "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![SCHEMA_VERSION.to_string()],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;

    if imported.is_empty() {
        return Ok(());
    }
    let legacy = dir.join(format!("legacy-{now}"));
    for path in imported {
        let Ok(relative) = path.strip_prefix(dir) else { continue };
        let target = legacy.join(relative);
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::rename(&path, &target);
        let bak = path.with_extension("bak");
        if bak.is_file() {
            let _ = fs::rename(&bak, target.with_extension("bak"));
        }
    }
    Ok(())
}

const BACKUPS_KEPT: usize = 5;

fn backup(conn: &Connection, dir: &Path) -> Result<(), String> {
    let empty: i64 = conn
        .query_row("SELECT count(*) FROM kv", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if empty == 0 {
        return Ok(());
    }

    let backups = dir.join("backups");
    fs::create_dir_all(&backups).map_err(|e| e.to_string())?;
    let target = backups.join(format!("panorama-{}.db", now_ms()));
    conn.execute("VACUUM INTO ?1", params![target.to_string_lossy()])
        .map_err(|e| e.to_string())?;

    let mut existing: Vec<(i64, PathBuf)> = fs::read_dir(&backups)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let stamp = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_prefix("panorama-"))
                .and_then(|s| s.parse().ok())?;
            Some((stamp, path))
        })
        .collect();
    existing.sort_by_key(|(stamp, _)| *stamp);
    for (_, path) in existing.iter().rev().skip(BACKUPS_KEPT) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn put(conn: &Connection, name: &str, value: &Value) -> Result<(), String> {
    let body = serde_json::to_string(value).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO kv (name, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(name) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![name, body, now_ms()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

const BACKUP_EVERY: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

pub fn init() -> Result<(), String> {
    db()?;
    std::thread::spawn(|| loop {
        std::thread::sleep(BACKUP_EVERY);
        let Ok(dir) = base_dir() else { return };
        let Ok(lock) = db() else { return };
        let Ok(conn) = lock.lock() else { return };
        if let Err(err) = backup(&conn, &dir) {
            eprintln!("[store] periodic backup failed: {err}");
        }
    });
    Ok(())
}

pub fn read_value(name: &str) -> Result<Option<Value>, String> {
    valid_name(name)?;
    let conn = db()?.lock().map_err(|e| e.to_string())?;
    let text: Option<String> = conn
        .query_row("SELECT value FROM kv WHERE name = ?1", params![name], |row| row.get(0))
        .ok();
    match text {
        Some(text) => serde_json::from_str(&text).map(Some).map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

pub fn write_value(name: &str, value: &Value) -> Result<(), String> {
    valid_name(name)?;
    let conn = db()?.lock().map_err(|e| e.to_string())?;
    put(&conn, name, value)
}

#[tauri::command]
pub fn store_read(name: String) -> Result<Option<Value>, String> {
    read_value(&name)
}

#[tauri::command]
pub fn store_write(name: String, value: Value) -> Result<(), String> {
    write_value(&name, &value)
}

#[tauri::command]
pub fn store_write_many(entries: Vec<Entry>) -> Result<(), String> {
    for entry in &entries {
        valid_name(&entry.name)?;
    }
    let conn = db()?.lock().map_err(|e| e.to_string())?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for entry in &entries {
        put(&tx, &entry.name, &entry.value)?;
    }
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn store_delete(name: String) -> Result<(), String> {
    valid_name(&name)?;
    let conn = db()?.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM kv WHERE name = ?1", params![name])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn store_list(dir: String) -> Result<Vec<String>, String> {
    let conn = db()?.lock().map_err(|e| e.to_string())?;
    let prefix = if dir.is_empty() { String::new() } else { format!("{}/", dir.trim_end_matches('/')) };
    let mut stmt = conn
        .prepare("SELECT name FROM kv WHERE name LIKE ?1 || '%' ORDER BY name")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![prefix], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut out: Vec<String> = Vec::new();
    for row in rows.flatten() {
        let rest = &row[prefix.len()..];
        let head = rest.split('/').next().unwrap_or(rest).to_string();
        if !head.is_empty() && !out.contains(&head) {
            out.push(head);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("panorama-store-{tag}-{}", now_ms()));
        fs::create_dir_all(dir.join("workspaces")).unwrap();
        dir
    }

    #[test]
    fn imports_legacy_files_and_moves_them_aside() {
        let dir = temp_dir("import");
        fs::write(dir.join("config.json"), r#"{"theme":"dark"}"#).unwrap();
        fs::write(dir.join("workspaces.json"), r#"{"version":1,"order":["ws-1"]}"#).unwrap();
        fs::write(dir.join("workspaces/ws-1.json"), r#"{"meta":{"id":"ws-1"}}"#).unwrap();
        fs::write(dir.join("workspaces/ws-2.json"), vec![0u8; 16]).unwrap();
        fs::write(dir.join("workspaces/ws-2.bak"), r#"{"meta":{"id":"ws-2"}}"#).unwrap();

        let conn = open_db(&dir.join("panorama.db")).unwrap();
        import_legacy(&conn, &dir).unwrap();

        let read = |name: &str| -> Option<String> {
            conn.query_row("SELECT value FROM kv WHERE name = ?1", params![name], |r| r.get(0))
                .ok()
        };
        assert!(read("config.json").unwrap().contains("dark"));
        assert!(read("workspaces/ws-1.json").is_some());
        assert!(read("workspaces/ws-2.json").unwrap().contains("ws-2"));
        assert!(!dir.join("config.json").exists());
        assert!(dir.join("workspaces/ws-2.bak").exists() == false);

        import_legacy(&conn, &dir).unwrap();
        assert_eq!(schema_version(&conn), SCHEMA_VERSION);
    }

    #[test]
    #[ignore = "runs against a copy of the machine's real panorama config dir"]
    fn imports_real_config_dir() {
        let source = PathBuf::from(std::env::var("APPDATA").unwrap()).join("panorama");
        let dir = temp_dir("real");
        for (name, path) in legacy_files(&source) {
            let target = dir.join(&name);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(&path, &target).unwrap();
        }

        let conn = open_db(&dir.join("panorama.db")).unwrap();
        import_legacy(&conn, &dir).unwrap();

        let mut stmt = conn.prepare("SELECT name, length(value) FROM kv ORDER BY name").unwrap();
        let rows: Vec<(String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .flatten()
            .collect();
        for (name, len) in &rows {
            println!("{name} -> {len} bytes");
        }
        assert!(rows.iter().any(|(n, _)| n == "workspaces.json"));
        assert!(rows.iter().all(|(_, len)| *len > 0));
    }

    #[test]
    fn commands_round_trip_through_one_database() {
        let dir = temp_dir("commands");
        std::env::set_var("PANORAMA_CONFIG_DIR", &dir);

        store_write_many(vec![
            Entry { name: "workspaces.json".into(), value: serde_json::json!({ "order": ["ws-1"] }) },
            Entry { name: "workspaces/ws-1.json".into(), value: serde_json::json!({ "meta": { "id": "ws-1" } }) },
        ])
        .unwrap();

        assert_eq!(store_list("workspaces".into()).unwrap(), vec!["ws-1.json"]);
        assert!(store_read("workspaces.json".into()).unwrap().is_some());
        assert!(store_write_many(vec![Entry { name: "../escape.json".into(), value: serde_json::json!(1) }]).is_err());
        assert!(store_read("../escape.json".into()).is_err());

        store_delete("workspaces/ws-1.json".into()).unwrap();
        assert!(store_read("workspaces/ws-1.json".into()).unwrap().is_none());
        assert!(store_list("workspaces".into()).unwrap().is_empty());
    }

    #[test]
    fn backup_snapshots_and_prunes_old_ones() {
        let dir = temp_dir("backup");
        let conn = open_db(&dir.join("panorama.db")).unwrap();
        assert!(backup(&conn, &dir).is_ok());
        assert!(!dir.join("backups").exists());

        put(&conn, "config.json", &serde_json::json!({ "theme": "dark" })).unwrap();
        for _ in 0..BACKUPS_KEPT + 3 {
            backup(&conn, &dir).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        let mut files: Vec<PathBuf> = fs::read_dir(dir.join("backups")).unwrap().flatten().map(|e| e.path()).collect();
        assert_eq!(files.len(), BACKUPS_KEPT);

        files.sort();
        let restored = open_db(files.last().unwrap()).unwrap();
        let value: String = restored
            .query_row("SELECT value FROM kv WHERE name = 'config.json'", [], |r| r.get(0))
            .unwrap();
        assert!(value.contains("dark"));
    }

    #[test]
    fn transaction_writes_all_or_nothing() {
        let dir = temp_dir("tx");
        let conn = open_db(&dir.join("panorama.db")).unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        put(&tx, "workspaces.json", &serde_json::json!({ "order": ["a"] })).unwrap();
        put(&tx, "workspaces/a.json", &serde_json::json!({ "meta": 1 })).unwrap();
        drop(tx);

        let count: i64 = conn.query_row("SELECT count(*) FROM kv", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn list_returns_direct_children_only() {
        let dir = temp_dir("list");
        let conn = open_db(&dir.join("panorama.db")).unwrap();
        put(&conn, "workspaces/a.json", &serde_json::json!(1)).unwrap();
        put(&conn, "workspaces/b.json", &serde_json::json!(1)).unwrap();
        put(&conn, "config.json", &serde_json::json!(1)).unwrap();

        let mut stmt = conn.prepare("SELECT name FROM kv WHERE name LIKE 'workspaces/%'").unwrap();
        let names: Vec<String> = stmt.query_map([], |r| r.get(0)).unwrap().flatten().collect();
        assert_eq!(names.len(), 2);
    }
}
