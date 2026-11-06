use std::fs::{self, OpenOptions};
use std::io::Write;

use crate::store::base_dir;

const LOG_NAME: &str = "ui-errors.log";
const MAX_BYTES: u64 = 1024 * 1024;

#[tauri::command]
pub fn log_ui_error(entry: String) -> Result<(), String> {
    let dir = base_dir()?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(LOG_NAME);
    if fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        fs::rename(&path, dir.join(format!("{LOG_NAME}.old"))).map_err(|e| e.to_string())?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())?;
    writeln!(file, "{entry}\n").map_err(|e| e.to_string())
}
