use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

struct UpdateStatus {
    status: String,
    version: Option<String>,
    current_version: Option<String>,
    error: Option<String>,
    downloaded: u64,
    content_length: u64,
    seq: u64,
    reported_at: u64,
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self {
            status: String::new(),
            version: None,
            current_version: None,
            error: None,
            downloaded: 0,
            content_length: 0,
            seq: 0,
            reported_at: 0,
        }
    }
}

fn store() -> &'static Mutex<UpdateStatus> {
    static STORE: OnceLock<Mutex<UpdateStatus>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(UpdateStatus::default()))
}

pub(crate) fn record(body: &[u8]) {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else { return };
    let mut s = store().lock().unwrap_or_else(|e| e.into_inner());
    s.status = v["status"].as_str().unwrap_or("idle").to_string();
    s.version = v["version"].as_str().map(str::to_string);
    s.current_version = v["currentVersion"].as_str().map(str::to_string);
    s.error = v["error"].as_str().map(str::to_string);
    s.downloaded = v["downloaded"].as_u64().unwrap_or(0);
    s.content_length = v["contentLength"].as_u64().unwrap_or(0);
    s.seq += 1;
    s.reported_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
}

pub(crate) fn get_json() -> String {
    let s = store().lock().unwrap_or_else(|e| e.into_inner());
    let mut obj = serde_json::json!({
        "status": s.status,
        "seq": s.seq,
        "reportedAt": s.reported_at,
        "downloaded": s.downloaded,
        "contentLength": s.content_length,
    });
    if let Some(v) = &s.version { obj["version"] = v.clone().into(); }
    if let Some(v) = &s.current_version { obj["currentVersion"] = v.clone().into(); }
    if let Some(v) = &s.error { obj["error"] = v.clone().into(); }
    obj.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_increments_on_each_record() {
        let before = store().lock().unwrap().seq;
        record(br#"{"status":"checking"}"#);
        let after = store().lock().unwrap().seq;
        assert_eq!(after, before + 1);
    }
}
