use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde_json::{json, Value};

use super::{host_handle, send_to_tile, sessions};

const MAX_IMAGE_BYTES: usize = 700 * 1024;
const IMAGE_WAIT: Duration = Duration::from_millis(2500);
const IMAGE_POLL: Duration = Duration::from_millis(100);
const IMAGE_TAG: &str = "[Image #";

// Same folder the desktop prompt box pastes from, so both sides clean up the same way.
fn paste_dir() -> PathBuf {
    std::env::temp_dir().join("panorama-paste")
}

fn extension(mime: &str) -> Option<&'static str> {
    match mime {
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

pub fn save(data: &str, mime: &str) -> Result<Value, String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let ext = extension(mime).ok_or("unsupported image type")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| "image is not base64")?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("image is too large".into());
    }
    let dir = paste_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let name = format!("phone_{stamp}_{}.{ext}", SEQ.fetch_add(1, Ordering::Relaxed));
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(json!({ "path": path.to_string_lossy() }))
}

fn is_attachment(path: &str) -> bool {
    let path = Path::new(path);
    path.parent() == Some(paste_dir().as_path()) && path.is_file()
}

fn image_count(tile_id: &str) -> usize {
    let Some(session) = sessions().lock().unwrap().get(tile_id).cloned() else {
        return 0;
    };
    let parser = session.parser.lock().unwrap_or_else(|e| e.into_inner());
    parser.screen().contents().matches(IMAGE_TAG).count()
}

/// Pastes each image path into the agent's prompt, waits for it to turn into an image tag,
/// then submits `text`.
pub async fn send(tile_id: &str, text: &str, images: &[String]) -> Result<(), String> {
    if images.iter().any(|p| !is_attachment(p)) {
        return Err("unknown attachment".into());
    }
    let mut expected = image_count(tile_id);
    for path in images {
        host_handle().write(tile_id, format!("\x1b[200~{path}\x1b[201~").as_bytes());
        expected += 1;
        let deadline = Instant::now() + IMAGE_WAIT;
        while image_count(tile_id) < expected && Instant::now() < deadline {
            tokio::time::sleep(IMAGE_POLL).await;
        }
    }
    send_to_tile(tile_id, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_known_types_inside_the_paste_folder() {
        let data = base64::engine::general_purpose::STANDARD.encode([0xff, 0xd8, 0xff]);
        let saved = save(&data, "image/jpeg").unwrap();
        let path = saved["path"].as_str().unwrap().to_string();
        assert!(path.ends_with(".jpg"));
        assert!(is_attachment(&path));
        std::fs::remove_file(&path).unwrap();
        assert!(save(&data, "text/html").is_err());
        assert!(save("not base64!", "image/png").is_err());
        assert!(!is_attachment(&paste_dir().join("..").join("x.jpg").to_string_lossy()));
    }
}
