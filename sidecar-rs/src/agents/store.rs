use std::path::{Path, PathBuf};

use crate::agents::reducer::State;
pub use crate::orch::store::{append_event, EventRecord};

pub fn agents_dir(base: &Path) -> PathBuf {
    base.join("agents")
}

pub fn agent_dir(base: &Path, agent_id: &str) -> PathBuf {
    agents_dir(base).join(agent_id)
}

pub fn events_path(base: &Path, agent_id: &str) -> PathBuf {
    agent_dir(base, agent_id).join("events.jsonl")
}

pub fn ensure_agent_dir(base: &Path, agent_id: &str) -> Result<(), String> {
    std::fs::create_dir_all(agent_dir(base, agent_id)).map_err(|e| format!("create agent dir: {e}"))
}

pub fn remove_agent_dir(base: &Path, agent_id: &str) -> Result<(), String> {
    match std::fs::remove_dir_all(agent_dir(base, agent_id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove agent dir: {e}")),
    }
}

pub fn remove_binding(base: &Path, tile_id: &str) -> Result<(), String> {
    let path = base.join("agent-bindings").join(format!("{tile_id}.json"));
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove binding: {e}")),
    }
}

pub fn load_into(state: &mut State, path: &Path, agent_id: &str) {
    let Ok(bytes) = std::fs::read(path) else { return };
    let mut last_seq = 0u32;
    for (i, chunk) in bytes.split(|&b| b == b'\n').enumerate() {
        let Ok(line) = std::str::from_utf8(chunk) else {
            eprintln!("[agents] skip invalid UTF-8 on line {} in {}", i + 1, path.display());
            continue;
        };
        let line = line.trim();
        if line.is_empty() { continue; }
        match serde_json::from_str::<EventRecord>(line) {
            Ok(record) => {
                last_seq = last_seq.max(record.seq);
                if let Err(e) = state.replay(&record.kind, &record.data) {
                    eprintln!("[agents] skip bad event line {} in {}: {e}", i + 1, path.display());
                }
            }
            Err(e) => eprintln!("[agents] skip corrupt event line {} in {}: {e}", i + 1, path.display()),
        }
    }
    if state.agents.contains_key(agent_id) {
        state.seqs.insert(agent_id.to_string(), last_seq);
    }
}

pub fn load_all(state: &mut State, base: &Path) {
    let Ok(entries) = std::fs::read_dir(agents_dir(base)) else { return };
    for entry in entries.flatten() {
        let Some(id) = entry.file_name().to_str().map(str::to_string) else { continue };
        load_into(state, &entry.path().join("events.jsonl"), &id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use crate::agents::reducer::tests::agent;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agents-{name}-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn create_record(seq: u32) -> EventRecord {
        EventRecord { seq, at: 1, kind: "agent-create".into(), data: serde_json::to_value(agent("a1", "rev")).unwrap() }
    }

    #[test]
    fn append_load_round_trip_skips_corrupt_lines() {
        let dir = tmp_dir("roundtrip");
        ensure_agent_dir(&dir, "a1").unwrap();
        let path = events_path(&dir, "a1");
        append_event(&path, &create_record(1)).unwrap();
        writeln!(std::fs::OpenOptions::new().append(true).open(&path).unwrap(), "{{broken").unwrap();
        append_event(&path, &EventRecord { seq: 3, at: 9, kind: "agent-start".into(), data: serde_json::json!({"id":"a1","at":9}) }).unwrap();
        let mut state = State::default();
        load_all(&mut state, &dir);
        assert_eq!(state.agents["a1"].last_started_at, Some(9));
        assert_eq!(state.seqs["a1"], 3);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn delete_removes_dir_and_binding() {
        let dir = tmp_dir("delete");
        ensure_agent_dir(&dir, "a1").unwrap();
        append_event(&events_path(&dir, "a1"), &create_record(1)).unwrap();
        let binding = dir.join("agent-bindings").join("agent-a1.json");
        std::fs::create_dir_all(binding.parent().unwrap()).unwrap();
        std::fs::write(&binding, "{}").unwrap();
        remove_agent_dir(&dir, "a1").unwrap();
        remove_binding(&dir, "agent-a1").unwrap();
        remove_agent_dir(&dir, "a1").unwrap();
        assert!(!agent_dir(&dir, "a1").exists());
        assert!(!binding.exists());
        let mut state = State::default();
        load_all(&mut state, &dir);
        assert!(state.agents.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
