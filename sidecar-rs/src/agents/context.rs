use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agents::model::Agent;
use crate::orch::profiles::Profile;

const DELIVERED_KEY: &str = "agentContext";
const BOOTSTRAP_TAIL: &str = "Reply with one short line to confirm, then wait for the next message.";

pub fn has_context_hook(exe: &str) -> bool {
    matches!(exe, "claude" | "codex" | "pi")
}

pub fn agent_block(agent: &Agent) -> String {
    let mut head = format!("[panorama agent] You are \"{}\"", agent.name);
    if !agent.title.is_empty() {
        head.push_str(&format!(" - {}", agent.title));
    }
    if !agent.description.is_empty() {
        head.push_str(&format!(": {}", agent.description));
    }
    let mut block = format!(
        "{head}\nYou are a persistent Panorama agent: this terminal stays open between tasks, and your session is resumed after restarts. Work in the current directory unless told otherwise."
    );
    if !agent.instructions.trim().is_empty() {
        block.push_str("\nInstructions from the user:\n");
        block.push_str(agent.instructions.trim_end());
    }
    block
}

pub const ROSTER_MAX_ENTRIES: usize = 12;
pub const ROSTER_MAX_BYTES: usize = 900;
const ROSTER_TITLE_CHARS: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterEntry {
    pub name: String,
    pub title: String,
}

pub fn footer(cli: &str) -> String {
    format!(
        "Send: {cli} agent send <name> \"text\" [--expects-reply]. Reply: {cli} agent reply <msgId> \"text\".\nAfter --expects-reply, end your turn; the reply arrives as a new message. Never reply to a reply.\nWait without polling: {cli} agent wait --on run:<id>|task:<run>/<task>|session:build@<cwd>|in:<dur> [--note ...], then end your turn."
    )
}

pub fn stable_block(agent: &Agent, cli: &str, orch: Option<&str>) -> String {
    let mut block = format!("{}\n{}", agent_block(agent), footer(cli));
    if let Some(orch) = orch.filter(|o| !o.is_empty()) {
        block.push_str("\n\n");
        block.push_str(orch);
    }
    block
}

fn sorted(entries: &[RosterEntry]) -> Vec<&RosterEntry> {
    let mut sorted: Vec<&RosterEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    sorted
}

pub fn roster_section(entries: &[RosterEntry], cli: &str) -> String {
    let sorted = sorted(entries);
    let head = "Panorama agents you can message:";
    let more = |n: usize| format!("... and {n} more: {cli} agent list");
    let mut lines: Vec<String> = Vec::new();
    for (i, entry) in sorted.iter().enumerate() {
        let title: String = entry.title.chars().take(ROSTER_TITLE_CHARS).collect();
        let line = if title.is_empty() { format!("- {}", entry.name) } else { format!("- {}: {title}", entry.name) };
        let rest = sorted.len() - i;
        let used: usize = lines.iter().map(|l| l.len() + 1).sum();
        let reserve = if rest > 1 { more(rest).len() + 1 } else { 0 };
        if i >= ROSTER_MAX_ENTRIES || head.len() + used + line.len() + 1 + reserve > ROSTER_MAX_BYTES {
            lines.push(more(rest));
            break;
        }
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push("(no other agents yet)".into());
    }
    format!("{head}\n{}", lines.join("\n"))
}

pub fn with_roster(block: &str, entries: &[RosterEntry], cli: &str) -> String {
    format!("{block}\n{}", roster_section(entries, cli))
}

pub fn roster_hash(entries: &[RosterEntry]) -> String {
    block_hash(&sorted(entries).iter().map(|e| format!("{}\t{}\n", e.name, e.title)).collect::<String>())
}

pub const DELTA_MAX_BYTES: usize = 300;

pub fn roster_delta(old: &[RosterEntry], new: &[RosterEntry], cli: &str) -> String {
    let label = |e: &RosterEntry| if e.title.is_empty() { e.name.clone() } else { format!("{} ({})", e.name, e.title.chars().take(ROSTER_TITLE_CHARS).collect::<String>()) };
    let find = |list: &[RosterEntry], name: &str| list.iter().find(|e| e.name == name).cloned();
    let added: Vec<String> = sorted(new).into_iter().filter(|e| find(old, &e.name).is_none()).map(label).collect();
    let changed: Vec<String> = sorted(new).into_iter().filter(|e| find(old, &e.name).is_some_and(|o| o.title != e.title)).map(label).collect();
    let removed: Vec<String> = sorted(old).into_iter().filter(|e| find(new, &e.name).is_none()).map(|e| e.name.clone()).collect();
    let mut parts = Vec::new();
    for (word, list) in [("added", added), ("changed", changed), ("removed", removed)] {
        if !list.is_empty() {
            parts.push(format!("{word}: {}", list.join(", ")));
        }
    }
    let pointer = format!("Full list: {cli} agent list");
    let line = format!("[panorama agents] {}. {pointer}", parts.join("; "));
    if parts.is_empty() || line.len() > DELTA_MAX_BYTES {
        return format!("[panorama agents] The agent list changed. {pointer}");
    }
    line
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterMark {
    pub session_id: String,
    pub hash: String,
    pub names: Vec<RosterEntry>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Gate {
    Full,
    Delta(String),
    Nothing,
}

pub fn gate(stable_mark: &Value, roster_mark: Option<&RosterMark>, session: &str, stable_hash: &str, roster: &[RosterEntry], cli: &str) -> Gate {
    if needs_delivery(stable_mark, session, stable_hash) {
        return Gate::Full;
    }
    match roster_mark {
        Some(mark) if mark.session_id == session && mark.hash == roster_hash(roster) => Gate::Nothing,
        Some(mark) if mark.session_id == session => Gate::Delta(roster_delta(&mark.names, roster, cli)),
        _ => Gate::Full,
    }
}

const ROSTER_KEY: &str = "agentRoster";
const ROSTER_PASTED_KEY: &str = "agentRosterPasted";

fn roster_mark(rec: &Value, key: &str) -> Option<RosterMark> {
    serde_json::from_value(rec[key].clone()).ok()
}

fn mark_for(session: &str, roster: &[RosterEntry]) -> Value {
    serde_json::to_value(RosterMark { session_id: session.into(), hash: roster_hash(roster), names: roster.to_vec() }).unwrap_or_default()
}

pub fn block_hash(block: &str) -> String {
    let hash = block.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    format!("{hash:016x}")
}

pub fn needs_delivery(delivered: &Value, session_id: &str, hash: &str) -> bool {
    delivered["sessionId"].as_str() != Some(session_id) || delivered["hash"].as_str() != Some(hash)
}

pub fn bootstrap_message(block: &str) -> String {
    format!("{block}\n{BOOTSTRAP_TAIL}")
}

pub fn prompt_block(stable: String, roster: &[RosterEntry], cli: &str, tile_id: &str, session_id: &str) -> Option<String> {
    let path = crate::binding_path(tile_id)?;
    let _guard = crate::binding_lock();
    let mut rec = crate::read_binding_rec(tile_id).filter(Value::is_object).unwrap_or_else(|| json!({ "tileId": tile_id }));
    let session = Some(session_id).filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| rec["agentSessionId"].as_str().map(str::to_string))
        .unwrap_or_default();
    let hash = block_hash(&stable);
    let out = match gate(&rec[DELIVERED_KEY], roster_mark(&rec, ROSTER_KEY).as_ref(), &session, &hash, roster, cli) {
        Gate::Nothing => return None,
        Gate::Full => with_roster(&stable, roster, cli),
        Gate::Delta(line) => line,
    };
    rec[DELIVERED_KEY] = json!({ "sessionId": session, "hash": hash });
    rec[ROSTER_KEY] = mark_for(&session, roster);
    if let Err(e) = crate::write_binding_rec(&path, &rec) {
        eprintln!("[agents] context marker not saved for {tile_id}: {e}");
    }
    Some(out)
}

pub fn frozen_roster(tile_id: &str, session: &str, roster: &[RosterEntry]) -> Vec<RosterEntry> {
    let Some(path) = crate::binding_path(tile_id) else { return roster.to_vec() };
    let _guard = crate::binding_lock();
    let mut rec = crate::read_binding_rec(tile_id).filter(Value::is_object).unwrap_or_else(|| json!({ "tileId": tile_id }));
    if let Some(mark) = roster_mark(&rec, ROSTER_KEY).filter(|m| m.session_id == session) {
        return mark.names;
    }
    rec[ROSTER_KEY] = mark_for(session, roster);
    if let Err(e) = crate::write_binding_rec(&path, &rec) {
        eprintln!("[agents] roster marker not saved for {tile_id}: {e}");
    }
    roster.to_vec()
}

pub fn paste_prefix(stable: &str, roster: &[RosterEntry], cli: &str, tile_id: &str) -> Option<String> {
    let rec = crate::read_binding_rec(tile_id).unwrap_or(Value::Null);
    let stable_mark = json!({ "sessionId": "paste", "hash": rec[PASTED_KEY].clone() });
    match gate(&stable_mark, roster_mark(&rec, ROSTER_PASTED_KEY).as_ref(), "paste", &block_hash(stable), roster, cli) {
        Gate::Nothing => None,
        Gate::Full => Some(with_roster(stable, roster, cli)),
        Gate::Delta(line) => Some(line),
    }
}

const PASTED_KEY: &str = "agentContextPasted";

pub fn set_pasted(tile_id: &str, stable: &str, roster: &[RosterEntry]) {
    let Some(path) = crate::binding_path(tile_id) else { return };
    let _guard = crate::binding_lock();
    let mut rec = crate::read_binding_rec(tile_id).filter(Value::is_object).unwrap_or_else(|| json!({ "tileId": tile_id }));
    rec[PASTED_KEY] = block_hash(stable).into();
    rec[ROSTER_PASTED_KEY] = mark_for("paste", roster);
    if let Err(e) = crate::write_binding_rec(&path, &rec) {
        eprintln!("[agents] pasted marker not saved for {tile_id}: {e}");
    }
}

pub fn forget_delivery(tile_id: &str) {
    let Some(path) = crate::binding_path(tile_id) else { return };
    let _guard = crate::binding_lock();
    let Some(mut rec) = crate::read_binding_rec(tile_id).filter(Value::is_object) else { return };
    if rec.as_object_mut().and_then(|o| o.remove(DELIVERED_KEY)).is_some() {
        let _ = crate::write_binding_rec(&path, &rec);
    }
}

pub async fn after_fresh_start(agent: &Agent, profile: &Profile) {
    if has_context_hook(&profile.exe) {
        return;
    }
    let (stable, roster) = crate::agents::block_parts(agent);
    let block = with_roster(&stable, &roster, &crate::panorama_cli_path());
    match crate::send_to_tile(&agent.tile_id(), &bootstrap_message(&block)) {
        Ok(()) => set_pasted(&agent.tile_id(), &stable, &roster),
        Err(e) => eprintln!("[agents] {}: bootstrap not sent: {e}", agent.id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::model::*;
    use crate::agents::reducer::tests::agent;

    const FIXED_MAX: usize = 600;
    const BLOCK_MAX: usize = 4800;

    #[test]
    fn block_is_byte_stable_and_ignores_runtime_fields() {
        let a = agent("a1", "rev");
        let mut b = a.clone();
        b.desired = Desired::Running;
        b.model = Some("sonnet".into());
        b.restarts = vec![1, 2];
        b.updated_at = 99;
        assert_eq!(agent_block(&a), agent_block(&a));
        assert_eq!(agent_block(&a), agent_block(&b));
        assert!(agent_block(&a).starts_with("[panorama agent] You are \"rev\" - Reviewer\n"));
        assert!(agent_block(&a).ends_with("Instructions from the user:\nAnswer in one line."));
    }

    #[test]
    fn gate_delivers_once_per_session_and_on_change() {
        let a = agent("a1", "rev");
        let hash = block_hash(&agent_block(&a));
        let mut delivered = Value::Null;
        assert!(needs_delivery(&delivered, "s1", &hash));
        delivered = json!({ "sessionId": "s1", "hash": hash });
        assert!(!needs_delivery(&delivered, "s1", &hash));
        assert!(needs_delivery(&Value::Null, "s1", &hash), "after compact the marker is gone");
        assert!(needs_delivery(&delivered, "s2", &hash), "new session id");
        let edited = Agent { instructions: "Answer in two lines.".into(), ..a };
        assert!(needs_delivery(&delivered, "s1", &block_hash(&agent_block(&edited))), "instructions edited");
    }

    #[test]
    fn budget_fixed_part_and_whole_block() {
        let empty = Agent { title: String::new(), instructions: String::new(), ..agent("a1", "r") };
        let fixed = agent_block(&empty).len() + "\nInstructions from the user:\n".len();
        assert!(fixed <= FIXED_MAX, "fixed part is {fixed} bytes");
        let full = Agent {
            name: "x".repeat(NAME_MAX),
            title: "t".repeat(TITLE_MAX_CHARS),
            description: "d".repeat(DESCRIPTION_MAX_CHARS),
            instructions: "i".repeat(INSTRUCTIONS_MAX_BYTES),
            ..agent("a1", "r")
        };
        let size = agent_block(&full).len();
        assert!(size <= BLOCK_MAX, "block is {size} bytes");
    }

    fn entry(name: &str, title: &str) -> RosterEntry {
        RosterEntry { name: name.into(), title: title.into() }
    }

    #[test]
    fn roster_is_sorted_cut_and_carries_the_commands() {
        let section = roster_section(&[entry("rev", "Reviewer"), entry("planner", &"P".repeat(50)), entry("z", "")], "/cli");
        let lines: Vec<&str> = section.lines().collect();
        assert_eq!(lines[0], "Panorama agents you can message:");
        assert_eq!(lines[1], format!("- planner: {}", "P".repeat(40)));
        assert_eq!(lines[2], "- rev: Reviewer");
        assert_eq!(lines[3], "- z");
        assert_eq!(lines.len(), 4, "the commands live in the stable footer");
        let foot: Vec<String> = footer("/cli").lines().map(str::to_string).collect();
        assert!(foot[0].starts_with("Send: /cli agent send <name> \"text\" [--expects-reply]. Reply: /cli agent reply <msgId>"));
        assert!(foot[1].contains("end your turn") && foot[1].contains("Never reply to a reply"));
        assert!(foot[2].starts_with("Wait without polling: /cli agent wait --on run:<id>|task:<run>/<task>|session:build@<cwd>|in:<dur>"));
        assert!(roster_section(&[], "/cli").contains("(no other agents yet)"));
    }

    #[test]
    fn roster_caps_entries_and_bytes() {
        let many: Vec<RosterEntry> = (0..30).map(|i| entry(&format!("agent-{i:02}"), "Short")).collect();
        let section = roster_section(&many, "/cli");
        let listed = section.lines().filter(|l| l.starts_with("- ")).count();
        assert_eq!(listed, ROSTER_MAX_ENTRIES);
        assert!(section.contains("... and 18 more: /cli agent list"), "{section}");
        let long: Vec<RosterEntry> = (0..12).map(|i| entry(&format!("{}{i:02}", "n".repeat(30)), &"t".repeat(60))).collect();
        let section = roster_section(&long, "/cli");
        let roster_part: usize = section.lines().take_while(|l| !l.starts_with("Send:")).map(|l| l.len() + 1).sum();
        assert!(roster_part <= ROSTER_MAX_BYTES, "{roster_part}");
        assert!(section.contains(" more: /cli agent list"));
    }

    #[test]
    fn block_hash_follows_the_roster_but_not_runtime_state() {
        let me = agent("a1", "me");
        let roster = vec![entry("rev", "Reviewer"), entry("planner", "Planner")];
        let base = block_hash(&with_roster(&agent_block(&me), &roster, "/cli"));
        let mut running = me.clone();
        running.desired = Desired::Running;
        running.restarts = vec![1];
        assert_eq!(base, block_hash(&with_roster(&agent_block(&running), &roster, "/cli")), "status changes keep the hash");
        let reordered = vec![entry("planner", "Planner"), entry("rev", "Reviewer")];
        assert_eq!(base, block_hash(&with_roster(&agent_block(&me), &reordered, "/cli")));
        for changed in [
            vec![entry("reviewer", "Reviewer"), entry("planner", "Planner")],
            vec![entry("rev", "Senior reviewer"), entry("planner", "Planner")],
            vec![entry("rev", "Reviewer"), entry("planner", "Planner"), entry("z", "")],
            vec![entry("rev", "Reviewer")],
        ] {
            assert_ne!(base, block_hash(&with_roster(&agent_block(&me), &changed, "/cli")), "{changed:?}");
        }
    }

    #[test]
    fn full_block_with_a_maximal_roster_stays_in_budget() {
        let full = Agent {
            name: "x".repeat(NAME_MAX), title: "t".repeat(TITLE_MAX_CHARS), description: "d".repeat(DESCRIPTION_MAX_CHARS),
            instructions: "i".repeat(INSTRUCTIONS_MAX_BYTES), ..agent("a1", "r")
        };
        let roster: Vec<RosterEntry> = (0..20).map(|i| entry(&format!("{}{i:02}", "n".repeat(30)), &"t".repeat(60))).collect();
        let cli = "C:/Users/someone/AppData/Local/Panorama/panorama-cli.exe";
        let size = with_roster(&agent_block(&full), &roster, cli).len();
        assert!(size <= BLOCK_MAX + ROSTER_MAX_BYTES + 2 * cli.len() + 260, "{size}");
    }
    #[test]
    fn the_gate_sends_the_block_once_then_only_roster_deltas() {
        let me = agent("a1", "me");
        let stable = stable_block(&me, "/cli", None);
        let hash = block_hash(&stable);
        let roster = vec![entry("rev", "Reviewer"), entry("k", "")];
        let mark = |s: &str, r: &[RosterEntry]| RosterMark { session_id: s.into(), hash: roster_hash(r), names: r.to_vec() };
        let delivered = json!({ "sessionId": "s1", "hash": hash });
        assert_eq!(gate(&Value::Null, None, "s1", &hash, &roster, "/cli"), Gate::Full);
        assert_eq!(gate(&delivered, Some(&mark("s1", &roster)), "s1", &hash, &roster, "/cli"), Gate::Nothing);
        let edited = vec![entry("rev", "Senior reviewer"), entry("planner", "Planner")];
        assert_eq!(gate(&delivered, Some(&mark("s1", &roster)), "s1", &hash, &edited, "/cli"),
            Gate::Delta("[panorama agents] added: planner (Planner); changed: rev (Senior reviewer); removed: k. Full list: /cli agent list".into()));
        assert_eq!(gate(&delivered, Some(&mark("s1", &edited)), "s1", &hash, &edited, "/cli"), Gate::Nothing, "the second turn gets nothing");
        assert_eq!(gate(&delivered, Some(&mark("s1", &roster)), "s2", &hash, &roster, "/cli"), Gate::Full, "new session");
        let other = block_hash(&stable_block(&me, "/cli", Some("orch rules")));
        assert_eq!(gate(&delivered, Some(&mark("s1", &roster)), "s1", &other, &roster, "/cli"), Gate::Full, "the stable part changed");
        assert_eq!(roster_hash(&roster), roster_hash(&[entry("k", ""), entry("rev", "Reviewer")]), "order does not matter");
    }

    #[test]
    fn a_long_roster_delta_falls_back_to_the_pointer() {
        let many: Vec<RosterEntry> = (0..20).map(|i| entry(&format!("agent-{i:02}"), &"t".repeat(40))).collect();
        let line = roster_delta(&[], &many, "/cli");
        assert_eq!(line, "[panorama agents] The agent list changed. Full list: /cli agent list");
        assert!(roster_delta(&[], &[entry("a", "")], "/cli").len() <= DELTA_MAX_BYTES);
    }

    #[test]
    fn bootstrap_wraps_the_block() {
        let a = agent("a1", "rev");
        let msg = bootstrap_message(&agent_block(&a));
        assert!(msg.starts_with(&agent_block(&a)));
        assert!(msg.ends_with(BOOTSTRAP_TAIL));
        assert!(!has_context_hook("kimi") && !has_context_hook("opencode") && !has_context_hook("agy"));
        assert!(has_context_hook("claude") && has_context_hook("codex") && has_context_hook("pi"));
    }
}
