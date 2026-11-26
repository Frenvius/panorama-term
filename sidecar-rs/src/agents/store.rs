use std::path::{Path, PathBuf};

use crate::agents::reducer::State;
pub use crate::orch::store::{append_event, EventRecord};

pub fn agents_dir(base: &Path) -> PathBuf {
    base.join("agents")
}

pub fn messages_path(base: &Path) -> PathBuf {
    agents_dir(base).join("messages").join("events.jsonl")
}

pub fn routines_path(base: &Path) -> PathBuf {
    agents_dir(base).join("routines").join("events.jsonl")
}

pub fn waits_path(base: &Path) -> PathBuf {
    agents_dir(base).join("waits").join("events.jsonl")
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

pub fn valid_agent_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 8 && bytes[0] == b'a' && bytes[1..].iter().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
}

pub fn load_all(state: &mut State, base: &Path) {
    let Ok(entries) = std::fs::read_dir(agents_dir(base)) else { return };
    for entry in entries.flatten() {
        let Some(id) = entry.file_name().to_str().map(str::to_string) else { continue };
        if !valid_agent_id(&id) {
            eprintln!("[agents] skip directory {id}: not an agent id");
            continue;
        }
        let mut loaded = State::default();
        load_into(&mut loaded, &entry.path().join("events.jsonl"), &id);
        if loaded.agents.len() != 1 || !loaded.agents.contains_key(&id) {
            eprintln!("[agents] skip {id}: log does not describe exactly this agent");
            continue;
        }
        state.agents.extend(loaded.agents);
        state.seqs.extend(loaded.seqs);
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
        EventRecord { seq, at: 1, kind: "agent-create".into(), data: serde_json::to_value(agent("a1234567", "rev")).unwrap() }
    }

    #[test]
    fn append_load_round_trip_skips_corrupt_lines() {
        let dir = tmp_dir("roundtrip");
        ensure_agent_dir(&dir, "a1234567").unwrap();
        let path = events_path(&dir, "a1234567");
        append_event(&path, &create_record(1)).unwrap();
        writeln!(std::fs::OpenOptions::new().append(true).open(&path).unwrap(), "{{broken").unwrap();
        append_event(&path, &EventRecord { seq: 3, at: 9, kind: "agent-start".into(), data: serde_json::json!({"id":"a1234567","at":9}) }).unwrap();
        let mut state = State::default();
        load_all(&mut state, &dir);
        assert_eq!(state.agents["a1234567"].last_started_at, Some(9));
        assert_eq!(state.seqs["a1234567"], 3);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_log_whose_agent_id_does_not_match_its_directory_is_skipped() {
        let dir = tmp_dir("mismatch");
        for (folder, id) in [("a1234567", "a7654321"), ("..", "a1234567"), ("abc", "abc")] {
            let path = agents_dir(&dir).join(folder);
            std::fs::create_dir_all(&path).unwrap();
            let record = EventRecord { seq: 1, at: 1, kind: "agent-create".into(), data: serde_json::to_value(agent(id, "x")).unwrap() };
            append_event(&path.join("events.jsonl"), &record).unwrap();
        }
        let mut state = State::default();
        load_all(&mut state, &dir);
        assert!(state.agents.is_empty(), "{:?}", state.agents.keys().collect::<Vec<_>>());
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
