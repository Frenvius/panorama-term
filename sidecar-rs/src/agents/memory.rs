use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agents::{compact, err_resp, lock, ok_resp, store, text_field, unix_ms};

pub const MAX_MEMORIES: usize = 64;
pub const TEXT_MAX_CHARS: usize = 500;
pub const ESSENTIAL_MAX_BYTES: usize = 2_048;
pub const ESSENTIAL_MAX_ENTRIES: usize = 12;
pub const WRITES_PER_HOUR: usize = 20;
pub const SEARCH_MAX_RESULTS: usize = 10;
pub const SEARCH_MAX_BYTES: usize = 2_000;
const HOUR_MS: u64 = 3_600_000;
const ID_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const SECRET_PREFIXES: &[&str] = &["sk-", "ghp_", "gho_", "github_pat_", "xoxb-", "xoxp-", "AKIA"];
const SECRET_RUN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum By {
    User,
    Agent,
}

impl By {
    pub fn label(self) -> &'static str {
        match self {
            By::User => "user",
            By::Agent => "agent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: String,
    pub text: String,
    pub essential: bool,
    pub by: By,
    pub created_at: u64,
    pub updated_at: u64,
}

pub enum MemCmd {
    Remember { id: String, text: String, essential: bool, by: By, at: u64 },
    Edit { id: String, text: String, at: u64 },
    Set { id: String, essential: bool, at: u64 },
    Forget { id: String, at: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemError {
    pub code: &'static str,
    pub message: String,
}

fn merr(code: &'static str, message: impl Into<String>) -> MemError {
    MemError { code, message: message.into() }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Memories {
    pub list: Vec<Memory>,
    pub writes: VecDeque<u64>,
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn looks_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let assigned = |key: &str| lower.match_indices(key).any(|(at, _)| lower[at + key.len()..].chars().next().is_some_and(|c| !c.is_whitespace()));
    if text.contains("-----BEGIN") || assigned("password=") || assigned("token=") {
        return true;
    }
    let mut tokens = text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    if tokens.any(|t| SECRET_PREFIXES.iter().any(|p| t.starts_with(p) && t.len() > p.len() + 3)) {
        return true;
    }
    let is_b64 = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=');
    text.split(|c: char| !is_b64(c)).any(|run| {
        let hex = run.chars().all(|c| c.is_ascii_hexdigit());
        let mixed = run.chars().any(|c| c.is_ascii_digit()) && run.chars().any(|c| c.is_ascii_alphabetic());
        run.len() >= SECRET_RUN && (hex || mixed)
    })
}

pub fn check_text(raw: &str) -> Result<String, MemError> {
    let text = raw.trim();
    if text.is_empty() {
        return Err(merr("bad_request", "the memory text is empty"));
    }
    if text.chars().any(char::is_control) || crate::sanitize_paste(text) != text {
        return Err(merr("bad_request", "one line of plain text: no control, format or hidden characters"));
    }
    if text.to_ascii_lowercase().contains("[panorama") {
        return Err(merr("bad_request", "the text may not contain [panorama"));
    }
    if text.chars().count() > TEXT_MAX_CHARS {
        return Err(merr("bad_request", format!("at most {TEXT_MAX_CHARS} characters")));
    }
    if looks_secret(text) {
        return Err(merr("bad_request", "this looks like a secret; do not store secrets in memories"));
    }
    Ok(text.to_string())
}

impl Memories {
    pub fn get(&self, id: &str) -> Option<&Memory> {
        self.list.iter().find(|m| m.id == id)
    }

    pub fn essential(&self) -> impl Iterator<Item = &Memory> {
        self.list.iter().filter(|m| m.essential)
    }

    pub fn essential_bytes(&self) -> usize {
        self.essential().map(|m| m.text.len()).sum()
    }

    pub fn usage(&self) -> String {
        format!(
            "essential {}/{} bytes, {}/{}",
            thousands(self.essential_bytes()), thousands(ESSENTIAL_MAX_BYTES), self.essential().count(), ESSENTIAL_MAX_ENTRIES
        )
    }

    fn check_essential(&self, without: Option<&str>, text: &str) -> Result<(), MemError> {
        let others = self.essential().filter(|m| Some(m.id.as_str()) != without);
        let (bytes, entries) = others.fold((text.len(), 1), |(b, n), m| (b + m.text.len(), n + 1));
        if bytes > ESSENTIAL_MAX_BYTES || entries > ESSENTIAL_MAX_ENTRIES {
            return Err(merr("limit", format!("essential notes are full ({}); forget one or make it searchable first", self.usage())));
        }
        Ok(())
    }

    fn writes_in_hour(&self, now: u64) -> usize {
        self.writes.iter().filter(|at| now.saturating_sub(**at) < HOUR_MS).count()
    }

    fn existing(&self, id: &str) -> Result<&Memory, MemError> {
        self.get(id).ok_or_else(|| merr("not_found", format!("memory {id} not found")))
    }

    pub fn apply(&self, cmd: &MemCmd) -> Result<(&'static str, Value), MemError> {
        let at = match cmd {
            MemCmd::Remember { at, .. } | MemCmd::Edit { at, .. } | MemCmd::Set { at, .. } | MemCmd::Forget { at, .. } => *at,
        };
        if self.writes_in_hour(at) + 1 > WRITES_PER_HOUR {
            return Err(merr("limit", format!("at most {WRITES_PER_HOUR} memory changes per hour; try again later")));
        }
        match cmd {
            MemCmd::Remember { id, text, essential, by, at } => {
                let text = check_text(text)?;
                if self.list.len() + 1 > MAX_MEMORIES {
                    return Err(merr("limit", format!("{MAX_MEMORIES} memories already; forget one first")));
                }
                if *essential {
                    self.check_essential(None, &text)?;
                }
                Ok(("memory-add", json!({ "id": id, "text": text, "essential": essential, "by": by, "at": at })))
            }
            MemCmd::Edit { id, text, at } => {
                let current = self.existing(id)?;
                let text = check_text(text)?;
                if current.essential {
                    self.check_essential(Some(id), &text)?;
                }
                Ok(("memory-edit", json!({ "id": id, "text": text, "at": at })))
            }
            MemCmd::Set { id, essential, at } => {
                let current = self.existing(id)?;
                if *essential && !current.essential {
                    self.check_essential(Some(id), &current.text)?;
                }
                Ok(("memory-set", json!({ "id": id, "essential": essential, "at": at })))
            }
            MemCmd::Forget { id, at } => {
                self.existing(id)?;
                Ok(("memory-forget", json!({ "id": id, "at": at })))
            }
        }
    }

    fn wrote(&mut self, at: u64) {
        self.writes.push_back(at);
        while self.writes.front().is_some_and(|t| at.saturating_sub(*t) >= HOUR_MS) {
            self.writes.pop_front();
        }
    }

    pub fn replay(&mut self, kind: &str, data: &Value) -> Result<(), String> {
        let id = data["id"].as_str().unwrap_or("").to_string();
        let at = data["at"].as_u64().unwrap_or(0);
        match kind {
            "memory-add" => {
                let by: By = serde_json::from_value(data["by"].clone()).map_err(|e| e.to_string())?;
                let text = data["text"].as_str().ok_or("missing text")?.to_string();
                self.list.retain(|m| m.id != id);
                self.list.push(Memory { id, text, essential: data["essential"].as_bool().unwrap_or(false), by, created_at: at, updated_at: at });
                self.wrote(at);
            }
            "memory-edit" => {
                if let (Some(m), Some(text)) = (self.list.iter_mut().find(|m| m.id == id), data["text"].as_str()) {
                    m.text = text.to_string();
                    m.updated_at = at;
                }
                self.wrote(at);
            }
            "memory-set" => {
                if let Some(m) = self.list.iter_mut().find(|m| m.id == id) {
                    m.essential = data["essential"].as_bool().unwrap_or(false);
                    m.updated_at = at;
                }
                self.wrote(at);
            }
            "memory-forget" => {
                self.list.retain(|m| m.id != id);
                self.wrote(at);
            }
            "memories-snapshot" => {
                self.list = serde_json::from_value(data["memories"].clone()).map_err(|e| e.to_string())?;
                self.writes = serde_json::from_value(data["writes"].clone()).unwrap_or_default();
            }
            _ => {}
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Value {
        json!({ "memories": self.list, "writes": self.writes })
    }

    pub fn search(&self, query: &str) -> Vec<&Memory> {
        let words: HashSet<String> = query.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() >= 2)
            .map(str::to_string)
            .collect();
        let mut scored: Vec<(usize, usize, &Memory)> = self.list.iter().enumerate()
            .filter_map(|(i, m)| {
                let text = m.text.to_lowercase();
                let score = words.iter().filter(|w| text.contains(w.as_str())).count();
                (score > 0).then_some((score, i, m))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.2.updated_at.cmp(&a.2.updated_at)).then(b.1.cmp(&a.1)));
        let mut out = Vec::new();
        let mut bytes = 0;
        for (_, _, m) in scored.into_iter().take(SEARCH_MAX_RESULTS) {
            let size = line(m).len() + 1;
            if bytes + size > SEARCH_MAX_BYTES {
                break;
            }
            bytes += size;
            out.push(m);
        }
        out
    }
}

pub fn line(m: &Memory) -> String {
    let kind = if m.essential { "essential" } else { "searchable" };
    format!("{} [{kind}, by {}] {}", m.id, m.by.label(), m.text)
}

pub fn memory_hash(memories: &[Memory]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for m in memories {
        for b in format!("{}\t{}\t{}\n", m.id, m.by.label(), m.text).bytes() {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

#[derive(Default)]
struct AgentMem {
    mem: Memories,
    log_seq: u32,
    snap_seq: u32,
}

fn store_map() -> &'static Mutex<HashMap<String, AgentMem>> {
    static MAP: OnceLock<Mutex<HashMap<String, AgentMem>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

fn mem_lock() -> std::sync::MutexGuard<'static, HashMap<String, AgentMem>> {
    store_map().lock().unwrap_or_else(|e| e.into_inner())
}

pub fn memories_path(base: &std::path::Path, agent_id: &str) -> PathBuf {
    store::agent_dir(base, agent_id).join("memories.jsonl")
}

fn load(path: &std::path::Path) -> AgentMem {
    compact::restore_backup(path);
    let mut entry = AgentMem::default();
    let Ok(bytes) = std::fs::read(path) else { return entry };
    for line in bytes.split(|b| *b == b'\n') {
        let Ok(text) = std::str::from_utf8(line) else { continue };
        let Ok(record) = serde_json::from_str::<store::EventRecord>(text.trim()) else { continue };
        entry.log_seq = entry.log_seq.max(record.seq);
        if record.kind == "memories-snapshot" {
            entry.snap_seq = record.seq;
        }
        if let Err(e) = entry.mem.replay(&record.kind, &record.data) {
            eprintln!("[memory] skip event {}: {e}", record.seq);
        }
    }
    entry
}

pub fn init() {
    let Some(base) = crate::panorama_dir() else { return };
    let ids: Vec<String> = lock().agents.keys().cloned().collect();
    let mut map = mem_lock();
    for id in ids {
        let entry = load(&memories_path(&base, &id));
        if entry.log_seq > 0 {
            map.insert(id, entry);
        }
    }
}

pub(crate) fn on_agent_deleted(agent_id: &str) {
    mem_lock().remove(agent_id);
}

pub(crate) fn with_memories<T>(agent_id: &str, f: impl FnOnce(&Memories) -> T) -> T {
    let map = mem_lock();
    match map.get(agent_id) {
        Some(entry) => f(&entry.mem),
        None => f(&Memories::default()),
    }
}

pub(crate) fn essential_of(agent_id: &str) -> Vec<Memory> {
    with_memories(agent_id, |m| m.essential().cloned().collect())
}

fn commit(agent_id: &str, cmd: MemCmd) -> Result<(String, Memories), MemError> {
    let io = |m: String| merr("io", m);
    let result = {
        let mut map = mem_lock();
        let entry = map.entry(agent_id.to_string()).or_default();
        let applied = entry.mem.apply(&cmd).and_then(|(kind, data)| {
            let base = crate::panorama_dir().ok_or_else(|| io("no panorama dir".into()))?;
            let seq = entry.log_seq + 1;
            let record = store::EventRecord { seq, at: unix_ms(), kind: kind.into(), data: data.clone() };
            store::append_event(&memories_path(&base, agent_id), &record).map_err(io)?;
            entry.log_seq = seq;
            if let Err(e) = entry.mem.replay(kind, &data) {
                eprintln!("[memory] replay after commit {kind}: {e}");
            }
            Ok((data["id"].as_str().unwrap_or("").to_string(), entry.mem.clone()))
        });
        if entry.log_seq == 0 {
            map.remove(agent_id);
        }
        applied
    };
    if result.is_ok() {
        crate::ui::broadcast_agents_memory(agent_id);
    }
    result
}

pub(crate) fn compact_all(force: bool) -> compact::Sizes {
    let mut total = compact::Sizes::default();
    let Some(base) = crate::panorama_dir() else { return total };
    let mut map = mem_lock();
    for (id, entry) in map.iter_mut() {
        let path = memories_path(&base, id);
        if entry.log_seq == 0 || (!force && !compact::needs_compaction(&path, entry.log_seq - entry.snap_seq)) {
            total.add(compact::unchanged(&path));
            continue;
        }
        match compact::compact_log(&path, entry.log_seq, "memories-snapshot", entry.mem.snapshot()) {
            Ok(sizes) => {
                total.add(sizes);
                entry.snap_seq = entry.log_seq;
            }
            Err(e) => eprintln!("[compact] memories {id}: {e}"),
        }
    }
    total
}

fn gen_id() -> String {
    let mut bytes = [0u8; 6];
    getrandom::getrandom(&mut bytes).expect("os rng");
    let mut id = String::from("m");
    id.extend(bytes.iter().map(|b| ID_ALPHABET[*b as usize % ID_ALPHABET.len()] as char));
    id
}

enum Caller {
    User,
    Agent(String),
}

impl Caller {
    fn by(&self) -> By {
        match self {
            Caller::User => By::User,
            Caller::Agent(_) => By::Agent,
        }
    }
}

fn caller(v: &Value) -> Result<Caller, String> {
    if v.get("worker").and_then(Value::as_bool) == Some(true) {
        return Err(err_resp("forbidden", "workers do not use memories"));
    }
    if text_field(v, "fromAgent").is_some() {
        return crate::agents::identity::live_agent(v, "memories are managed from the agent's own live tile").map(Caller::Agent);
    }
    let from_tile = text_field(v, "fromTile").is_some();
    let cap = v.get("userCap").and_then(Value::as_str).is_some_and(crate::agents::routine_hook::user_cap_matches);
    if from_tile || !cap {
        return Err(err_resp("forbidden", "the user manages memories in the Panorama panel; agents manage their own"));
    }
    Ok(Caller::User)
}

fn target(v: &Value, who: &Caller) -> Result<String, String> {
    match who {
        Caller::Agent(id) => {
            if let Some(other) = text_field(v, "agent") {
                let resolved = lock().resolve(&other).map(|a| a.id.clone());
                if resolved.as_deref() != Some(id.as_str()) {
                    return Err(err_resp("forbidden", "an agent manages only its own memories"));
                }
            }
            Ok(id.clone())
        }
        Caller::User => {
            let name = text_field(v, "agent").ok_or_else(|| err_resp("bad_request", "agent is required"))?;
            lock().resolve(&name).map(|a| a.id.clone()).ok_or_else(|| err_resp("not_found", &format!("agent {name} not found")))
        }
    }
}

fn who_and_target(v: &Value) -> Result<(Caller, String), String> {
    let who = caller(v)?;
    let id = target(v, &who)?;
    Ok((who, id))
}

fn mem_err(e: MemError) -> String {
    err_resp(e.code, &e.message)
}

fn usage_json(m: &Memories) -> Value {
    json!({
        "count": m.list.len(), "essentialBytes": m.essential_bytes(), "essentialCount": m.essential().count(),
        "maxBytes": ESSENTIAL_MAX_BYTES, "maxEntries": ESSENTIAL_MAX_ENTRIES, "max": MAX_MEMORIES,
    })
}

fn changed(agent: &str, cmd: MemCmd, verb: &str) -> String {
    match commit(agent, cmd) {
        Ok((id, mem)) => ok_resp(json!({ "id": id, "text": format!("{verb} {id} ({})", mem.usage()), "usage": usage_json(&mem) })),
        Err(e) => mem_err(e),
    }
}

pub fn handle_remember(v: &Value) -> String {
    let (who, agent) = match who_and_target(v) { Ok(x) => x, Err(e) => return e };
    let text = text_field(v, "text").unwrap_or_default();
    let essential = v.get("essential").and_then(Value::as_bool).unwrap_or(false);
    changed(&agent, MemCmd::Remember { id: gen_id(), text, essential, by: who.by(), at: unix_ms() }, "saved")
}

fn owned_id(v: &Value) -> Result<(String, String), String> {
    let (_, agent) = who_and_target(v)?;
    let id = text_field(v, "id").ok_or_else(|| err_resp("bad_request", "memory id is required"))?;
    if !with_memories(&agent, |m| m.get(&id).is_some()) {
        return Err(err_resp("not_found", &format!("memory {id} not found")));
    }
    Ok((agent, id))
}

pub fn handle_edit(v: &Value) -> String {
    let (agent, id) = match owned_id(v) { Ok(x) => x, Err(e) => return e };
    changed(&agent, MemCmd::Edit { id, text: text_field(v, "text").unwrap_or_default(), at: unix_ms() }, "edited")
}

pub fn handle_set(v: &Value) -> String {
    let (agent, id) = match owned_id(v) { Ok(x) => x, Err(e) => return e };
    let Some(essential) = v.get("essential").and_then(Value::as_bool) else {
        return err_resp("bad_request", "use --essential or --searchable");
    };
    changed(&agent, MemCmd::Set { id, essential, at: unix_ms() }, "set")
}

pub fn handle_forget(v: &Value) -> String {
    let (agent, id) = match owned_id(v) { Ok(x) => x, Err(e) => return e };
    changed(&agent, MemCmd::Forget { id, at: unix_ms() }, "forgot")
}

pub fn handle_search(v: &Value) -> String {
    let (_, agent) = match who_and_target(v) { Ok(x) => x, Err(e) => return e };
    let query = text_field(v, "query").unwrap_or_default();
    with_memories(&agent, |m| {
        let found = m.search(&query);
        let lines: Vec<String> = found.iter().map(|x| line(x)).collect();
        ok_resp(json!({ "memories": found, "lines": lines }))
    })
}

pub fn handle_list(v: &Value) -> String {
    let (_, agent) = match who_and_target(v) { Ok(x) => x, Err(e) => return e };
    let only_essential = v.get("essential").and_then(Value::as_bool).unwrap_or(false);
    with_memories(&agent, |m| {
        let mut list: Vec<&Memory> = m.list.iter().filter(|x| !only_essential || x.essential).collect();
        list.sort_by_key(|x| !x.essential);
        let lines: Vec<String> = list.iter().map(|x| line(x)).collect();
        ok_resp(json!({ "memories": list, "lines": lines, "usage": usage_json(m) }))
    })
}

pub(crate) fn summary_for(agent_id: &str) -> Value {
    with_memories(agent_id, |m| json!({ "count": m.list.len(), "essentialBytes": m.essential_bytes() }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const T0: u64 = 1_800_000_000_000;

    fn step(m: &mut Memories, cmd: MemCmd) -> Result<(), MemError> {
        let (k, d) = m.apply(&cmd)?;
        m.replay(k, &d).unwrap();
        Ok(())
    }

    fn remember(id: &str, text: &str, essential: bool, at: u64) -> MemCmd {
        MemCmd::Remember { id: id.into(), text: text.into(), essential, by: By::Agent, at }
    }

    #[test]
    fn caps_are_refused_with_the_usage() {
        let mut m = Memories::default();
        for i in 0..MAX_MEMORIES {
            step(&mut m, remember(&format!("m{i}"), &format!("fact number {i}"), false, T0 + i as u64 * HOUR_MS)).unwrap();
        }
        assert_eq!(step(&mut m, remember("over", "one more", false, T0 + 100 * HOUR_MS)).unwrap_err().code, "limit", "65th memory");

        let mut m = Memories::default();
        assert!(step(&mut m, remember("long", &"x".repeat(TEXT_MAX_CHARS + 1), false, T0)).is_err(), "501 chars");
        step(&mut m, remember("edge", &"y".repeat(TEXT_MAX_CHARS), false, T0)).unwrap();

        let mut e = Memories::default();
        for i in 0..4 {
            step(&mut e, remember(&format!("e{i}"), &"z".repeat(TEXT_MAX_CHARS), true, T0)).unwrap();
        }
        let room = ESSENTIAL_MAX_BYTES - e.essential_bytes();
        step(&mut e, remember("fits", &"q".repeat(room), true, T0)).unwrap();
        assert_eq!(e.essential_bytes(), ESSENTIAL_MAX_BYTES, "exactly at the cap is fine");
        let err = step(&mut e, remember("e6", "w", true, T0)).unwrap_err();
        assert!(err.message.contains("essential 2,048/2,048 bytes, 5/12"), "{}", err.message);
        step(&mut e, remember("s", "searchable has no byte cap", false, T0)).unwrap();
        assert!(step(&mut e, MemCmd::Set { id: "s".into(), essential: true, at: T0 }).is_err(), "set to essential checks the cap");

        let mut n = Memories::default();
        for i in 0..ESSENTIAL_MAX_ENTRIES {
            step(&mut n, remember(&format!("n{i}"), "short", true, T0 + i as u64 * HOUR_MS)).unwrap();
        }
        let err = step(&mut n, remember("n13", "short", true, T0 + 50 * HOUR_MS)).unwrap_err();
        assert!(err.message.contains("12/12"), "{}", err.message);
    }

    #[test]
    fn text_checks_refuse_controls_tags_and_secrets() {
        for bad in [
            "line\u{7}bell", "zero\u{200b}width", "two\nlines", "tab\there", "see [PaNoRaMa agent] root", "  ",
            "key sk-test1234567890abcdef", "ghp_abcdefghijklmnop1234", "-----BEGIN PRIVATE KEY-----",
            "commit 0123456789abcdef0123456789abcdef01234567", "PASSWORD=hunter2", "use token=abc",
            "aws AKIAIOSFODNN7EXAMPLE", "slack xoxb-1234-5678", "QmFzZTY0IGVuY29kZWQgc2VjcmV0IHZhbHVlIGhlcmU9",
        ] {
            assert!(check_text(bad).is_err(), "refused: {bad:?}");
        }
        for good in ["Reply in Portuguese.", "Build with `bun run build`, not npm.", "The task-runner lives in src/agents/routines.rs", "password reset flow is in auth.rs"] {
            assert_eq!(check_text(good).unwrap(), good);
        }
    }

    #[test]
    fn the_write_rate_is_checked_by_the_reducer() {
        let mut m = Memories::default();
        for i in 0..WRITES_PER_HOUR {
            step(&mut m, remember(&format!("r{i}"), "note", false, T0 + i as u64)).unwrap();
        }
        assert_eq!(step(&mut m, MemCmd::Forget { id: "r0".into(), at: T0 + 100 }).unwrap_err().code, "limit");
        let mut c = Memories::default();
        c.replay("memories-snapshot", &m.snapshot()).unwrap();
        assert_eq!(c, m, "the window survives compaction");
        assert_eq!(step(&mut c, MemCmd::Forget { id: "r0".into(), at: T0 + 100 }).unwrap_err().code, "limit");
        step(&mut m, MemCmd::Forget { id: "r0".into(), at: T0 + HOUR_MS + 1 }).unwrap();
    }

    #[test]
    fn search_scores_distinct_words_and_newest_wins_ties() {
        let mut m = Memories::default();
        step(&mut m, remember("old", "deploy uses the staging server", false, T0)).unwrap();
        step(&mut m, remember("new", "deploy needs a tag", false, T0 + 10)).unwrap();
        step(&mut m, remember("both", "deploy to staging after review", true, T0 + 5)).unwrap();
        step(&mut m, remember("none", "unrelated", false, T0 + 20)).unwrap();
        let ids: Vec<&str> = m.search("Deploy deploy STAGING x").iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, ["both", "old", "new"]);
        let ids: Vec<&str> = m.search("deploy").iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, ["new", "both", "old"], "newest first on a tie");
        let mut many = Memories::default();
        for i in 0..20u64 {
            step(&mut many, remember(&format!("k{i}"), &format!("{} keyword", "w".repeat(300)), false, T0 + i * HOUR_MS)).unwrap();
        }
        let found = many.search("keyword");
        let bytes: usize = found.iter().map(|x| line(x).len() + 1).sum();
        assert!(found.len() <= SEARCH_MAX_RESULTS && bytes <= SEARCH_MAX_BYTES, "{} {bytes}", found.len());
        assert_eq!(found[0].id, "k19");
    }

    #[test]
    fn a_memories_snapshot_drops_forgotten_ones_and_replays_equal() {
        let mut m = Memories::default();
        step(&mut m, remember("a", "keep me", true, T0)).unwrap();
        step(&mut m, remember("b", "forget me", false, T0 + 1)).unwrap();
        step(&mut m, MemCmd::Edit { id: "a".into(), text: "keep me, edited".into(), at: T0 + 2 }).unwrap();
        step(&mut m, MemCmd::Forget { id: "b".into(), at: T0 + 3 }).unwrap();
        step(&mut m, MemCmd::Set { id: "a".into(), essential: false, at: T0 + 4 }).unwrap();
        let snap = m.snapshot();
        assert!(!snap.to_string().contains("forget me"));
        let mut c = Memories::default();
        c.replay("memories-snapshot", &snap).unwrap();
        assert_eq!(c, m);
    }

    #[test]
    fn memory_hash_follows_id_by_and_text() {
        let a = Memory { id: "m1".into(), text: "x".into(), essential: true, by: By::User, created_at: 1, updated_at: 1 };
        let b = Memory { by: By::Agent, ..a.clone() };
        assert_ne!(memory_hash(std::slice::from_ref(&a)), memory_hash(&[b]));
        assert_eq!(memory_hash(std::slice::from_ref(&a)), memory_hash(&[Memory { updated_at: 9, ..a }]));
    }

    fn reply(text: String) -> Value {
        serde_json::from_str(&text).unwrap()
    }

    fn test_agent(tag: &str, live: bool) -> (String, String) {
        crate::orch::tests::f12_test_home();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let id = format!("am{tag}{:08}", nanos % 100_000_000);
        let name = format!("m{tag}-{}", &id[3..]);
        store::ensure_agent_dir(&crate::panorama_dir().unwrap(), &id).unwrap();
        lock().agents.insert(id.clone(), crate::agents::reducer::tests::agent(&id, &name));
        if live {
            crate::insert_test_session(&crate::agents::model::tile_id(&id));
        }
        (id, name)
    }

    fn from(id: &str, extra: Value) -> Value {
        let mut v = json!({ "fromAgent": id, "fromTile": crate::agents::model::tile_id(id), "agentToken": crate::agents::identity::issue(id) });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        v
    }

    #[test]
    fn by_comes_from_the_caller_and_each_agent_owns_its_memories() {
        let (a, a_name) = test_agent("a", true);
        let (b, _) = test_agent("b", true);
        let (dead, _) = test_agent("d", false);
        let saved = reply(handle_remember(&from(&a, json!({ "text": "Reply in Portuguese.", "essential": true, "by": "user" }))));
        assert_eq!(saved["ok"], true, "{saved}");
        let id = saved["id"].as_str().unwrap().to_string();
        assert!(saved["text"].as_str().unwrap().starts_with(&format!("saved {id} (essential 20/2,048 bytes, 1/12)")), "{saved}");
        assert_eq!(with_memories(&a, |m| m.get(&id).unwrap().by), By::Agent, "a body cannot claim to be the user");

        for route in [handle_list as fn(&Value) -> String, handle_search] {
            assert_eq!(reply(route(&from(&b, json!({ "agent": a_name, "query": "portuguese" }))))["error"]["code"], "forbidden");
        }
        for route in [handle_edit as fn(&Value) -> String, handle_forget, handle_set] {
            let r = reply(route(&from(&b, json!({ "id": id, "text": "x", "essential": false }))));
            assert_eq!(r["ok"], false, "{r}");
        }
        assert_eq!(with_memories(&a, |m| m.list.len()), 1);
        let b_token = crate::agents::identity::issue(&b);
        let forged = json!({ "fromAgent": a, "fromTile": crate::agents::model::tile_id(&a), "agentToken": b_token, "id": id });
        assert_eq!(reply(handle_forget(&forged))["error"]["code"], "forbidden", "B cannot pass as A without A's token");
        let no_token = json!({ "fromAgent": a, "fromTile": crate::agents::model::tile_id(&a), "id": id });
        assert_eq!(reply(handle_list(&no_token))["error"]["code"], "forbidden");
        assert_eq!(reply(handle_remember(&from(&dead, json!({ "text": "x" }))))["error"]["code"], "forbidden", "dead tile");
        let other_tile = json!({ "fromAgent": a, "fromTile": crate::agents::model::tile_id(&b), "text": "x" });
        assert_eq!(reply(handle_remember(&other_tile))["error"]["code"], "forbidden", "another agent's tile");
        assert_eq!(reply(handle_remember(&from(&a, json!({ "text": "x", "worker": true }))))["error"]["code"], "forbidden");
        assert_eq!(reply(handle_remember(&json!({ "agent": a_name, "text": "x" })))["error"]["code"], "forbidden", "no user capability");

        let user = reply(handle_remember(&json!({ "agent": a_name, "text": "Prefers short answers.", "userCap": crate::agents::routines::tests::cap() })));
        let uid = user["id"].as_str().unwrap().to_string();
        assert_eq!(with_memories(&a, |m| m.get(&uid).unwrap().by), By::User);
        let found = reply(handle_search(&from(&a, json!({ "query": "short answers" }))));
        assert_eq!(found["lines"][0], format!("{uid} [searchable, by user] Prefers short answers."));
    }

    #[test]
    fn two_racing_writes_at_the_rate_edge_save_one() {
        for round in 0..10 {
            let (a, _) = test_agent(&format!("r{round}"), true);
            for i in 0..WRITES_PER_HOUR - 1 {
                commit(&a, remember(&format!("x{i}"), "note", false, unix_ms())).unwrap();
            }
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let racers: Vec<_> = (0..2).map(|n| {
                let (a, barrier) = (a.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    commit(&a, remember(&format!("race{n}"), "note", false, unix_ms())).is_ok()
                })
            }).collect();
            let saved = racers.into_iter().map(|r| r.join().unwrap_or(false)).filter(|ok| *ok).count();
            assert_eq!(saved, 1, "round {round}");
            assert_eq!(with_memories(&a, |m| m.writes.len()), WRITES_PER_HOUR);
        }
    }

    #[tokio::test]
    async fn deleting_the_agent_removes_its_memories_for_good() {
        let (a, name) = test_agent("x", false);
        commit(&a, remember("m1", "remember this", true, unix_ms())).unwrap();
        let base = crate::panorama_dir().unwrap();
        let path = memories_path(&base, &a);
        assert!(path.exists());
        let deleted = reply(crate::agents::handle("delete", json!({ "name": name }).to_string().as_bytes()).await);
        assert_eq!(deleted["ok"], true, "{deleted}");
        assert!(!path.exists() && !store::agent_dir(&base, &a).exists());
        assert!(with_memories(&a, |m| m.list.is_empty()));
        lock().agents.insert(a.clone(), crate::agents::reducer::tests::agent(&a, &name));
        init();
        lock().agents.remove(&a);
        assert!(with_memories(&a, |m| m.list.is_empty()), "boot does not bring them back");
        assert!(!path.exists());
    }
}