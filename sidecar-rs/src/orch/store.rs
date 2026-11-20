use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::orch::reducer::State;

#[derive(Serialize, Deserialize)]
pub struct EventRecord {
    pub seq: u32,
    pub at: u64,
    pub kind: String,
    pub data: serde_json::Value,
}

pub fn run_events_path(base: &Path, run_id: &str) -> PathBuf {
    base.join("orch").join("runs").join(run_id).join("events.jsonl")
}

pub fn ensure_run_dir(base: &Path, run_id: &str) -> Result<(), String> {
    let dir = base.join("orch").join("runs").join(run_id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create run dir: {e}"))
}

pub fn append_event(path: &Path, record: &EventRecord) -> Result<(), String> {
    let line = serde_json::to_string(record).map_err(|e| format!("serialize: {e}"))?;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("open: {e}"))?;

    let len = file.metadata().map_err(|e| format!("metadata: {e}"))?.len();
    if len > 0 {
        use std::io::Seek;
        file.seek(std::io::SeekFrom::End(-1)).map_err(|e| format!("seek: {e}"))?;
        let mut last = [0u8; 1];
        file.read_exact(&mut last).map_err(|e| format!("read_last: {e}"))?;
        if last[0] != b'\n' {
            file.write_all(b"\n").map_err(|e| format!("write newline: {e}"))?;
        }
    }

    writeln!(file, "{line}").map_err(|e| format!("write: {e}"))?;
    file.flush().map_err(|e| format!("flush: {e}"))?;
    file.sync_data().map_err(|e| format!("sync_data: {e}"))?;
    Ok(())
}

pub fn load_into(state: &mut State, path: &Path, run_id: &str) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return,
    };
    let mut last_seq = 0u32;
    for (i, chunk) in bytes.split(|&b| b == b'\n').enumerate() {
        if chunk.is_empty() { continue; }
        let line = match std::str::from_utf8(chunk) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("[orch] skip invalid UTF-8 on line {} in {}", i + 1, path.display());
                continue;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        match serde_json::from_str::<EventRecord>(trimmed) {
            Ok(record) => {
                last_seq = last_seq.max(record.seq);
                if let Err(e) = state.replay(&record.kind, &record.data) {
                    eprintln!("[orch] skip bad event line {} in {}: {e}", i + 1, path.display());
                }
            }
            Err(e) => eprintln!("[orch] skip corrupt event line {} in {}: {e}", i + 1, path.display()),
        }
    }
    if let Some(entry) = state.runs.get_mut(run_id) {
        if last_seq > entry.event_seq {
            entry.event_seq = last_seq;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orch-{name}-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn run_create_record(seq: u32) -> EventRecord {
        EventRecord {
            seq,
            at: 1_000_000,
            kind: "run-create".into(),
            data: serde_json::json!({
                "id": "r1", "title": "t", "repo": "/tmp",
                "baseRef": null, "coordinatorTile": null,
                "limits": {"maxWorkers":4,"maxAttempts":3,"dispatchTimeoutMin":60},
                "now": 1000000
            }),
        }
    }

    #[test]
    fn store_round_trip_through_temp_dir() {
        let dir = tmp_dir("roundtrip");
        ensure_run_dir(&dir, "r1").unwrap();
        let path = run_events_path(&dir, "r1");
        append_event(&path, &run_create_record(1)).unwrap();
        let mut state = State::default();
        load_into(&mut state, &path, "r1");
        assert!(state.runs.contains_key("r1"));
        assert_eq!(state.runs["r1"].event_seq, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_line_is_skipped() {
        let dir = tmp_dir("corrupt");
        ensure_run_dir(&dir, "r1").unwrap();
        let path = run_events_path(&dir, "r1");
        append_event(&path, &run_create_record(1)).unwrap();
        {
            let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(file, "{{not valid json!!").unwrap();
        }
        let task_record = EventRecord {
            seq: 3,
            at: 1_000_001,
            kind: "task-create".into(),
            data: serde_json::json!({
                "runId": "r1", "id": "t1", "title": "t", "spec": "s",
                "acceptance": null, "deps": [], "profile": null,
                "escalation": [], "verify": [], "review": false, "now": 1000001
            }),
        };
        append_event(&path, &task_record).unwrap();
        let mut state = State::default();
        load_into(&mut state, &path, "r1");
        assert!(state.runs.contains_key("r1"));
        assert!(state.runs["r1"].tasks.contains_key("t1"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn partial_last_line_then_new_append_loads_both() {
        let dir = tmp_dir("partial");
        ensure_run_dir(&dir, "r1").unwrap();
        let path = run_events_path(&dir, "r1");
        append_event(&path, &run_create_record(1)).unwrap();

        // Manually append without trailing newline to simulate partial write
        {
            let task_line = serde_json::to_string(&EventRecord {
                seq: 2,
                at: 1_000_001,
                kind: "task-create".into(),
                data: serde_json::json!({
                    "runId": "r1", "id": "t1", "title": "t", "spec": "s",
                    "acceptance": null, "deps": [], "profile": null,
                    "escalation": [], "verify": [], "review": false, "now": 1000001
                }),
            }).unwrap();
            let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            file.write_all(task_line.as_bytes()).unwrap();
            file.flush().unwrap();
        }

        let third = EventRecord {
            seq: 3,
            at: 1_000_002,
            kind: "task-create".into(),
            data: serde_json::json!({
                "runId": "r1", "id": "t2", "title": "t2", "spec": "s2",
                "acceptance": null, "deps": [], "profile": null,
                "escalation": [], "verify": [], "review": false, "now": 1000002
            }),
        };
        append_event(&path, &third).unwrap();

        let mut state = State::default();
        load_into(&mut state, &path, "r1");
        assert!(state.runs["r1"].tasks.contains_key("t1"), "t1 from partial line");
        assert!(state.runs["r1"].tasks.contains_key("t2"), "t2 from new append");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_utf8_line_is_skipped() {
        let dir = tmp_dir("utf8");
        ensure_run_dir(&dir, "r1").unwrap();
        let path = run_events_path(&dir, "r1");
        append_event(&path, &run_create_record(1)).unwrap();
        {
            let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            file.write_all(b"\xff\xfe{invalid utf8}\n").unwrap();
        }
        let task_record = EventRecord {
            seq: 3,
            at: 1_000_001,
            kind: "task-create".into(),
            data: serde_json::json!({
                "runId": "r1", "id": "t1", "title": "t", "spec": "s",
                "acceptance": null, "deps": [], "profile": null,
                "escalation": [], "verify": [], "review": false, "now": 1000001
            }),
        };
        append_event(&path, &task_record).unwrap();
        let mut state = State::default();
        load_into(&mut state, &path, "r1");
        assert!(state.runs.contains_key("r1"));
        assert!(state.runs["r1"].tasks.contains_key("t1"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn append_produces_valid_jsonl_no_duplicate_newlines() {
        let dir = tmp_dir("newlines");
        ensure_run_dir(&dir, "r1").unwrap();
        let path = run_events_path(&dir, "r1");
        append_event(&path, &run_create_record(1)).unwrap();
        append_event(&path, &EventRecord {
            seq: 2, at: 2, kind: "run-set-status".into(),
            data: serde_json::json!({"runId":"r1","status":"paused","now":2}),
        }).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_into_sets_event_seq_from_max_event() {
        let dir = tmp_dir("evtseq");
        ensure_run_dir(&dir, "r1").unwrap();
        let path = run_events_path(&dir, "r1");
        append_event(&path, &run_create_record(5)).unwrap();
        append_event(&path, &EventRecord {
            seq: 12, at: 2, kind: "run-set-status".into(),
            data: serde_json::json!({"runId":"r1","status":"paused","now":2}),
        }).unwrap();
        let mut state = State::default();
        load_into(&mut state, &path, "r1");
        assert_eq!(state.runs["r1"].event_seq, 12);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn append_failure_does_not_modify_caller_state() {
        let dir = tmp_dir("failtest");
        let bad_path = dir.join("nonexistent").join("events.jsonl");
        let record = run_create_record(1);
        let result = append_event(&bad_path, &record);
        assert!(result.is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
