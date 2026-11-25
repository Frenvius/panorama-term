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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterEntry {
    pub name: String,
    pub title: String,
}

pub fn roster_section(entries: &[RosterEntry], cli: &str) -> String {
    let mut sorted: Vec<&RosterEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let footer = format!(
        "Send: {cli} agent send <name> \"text\" [--expects-reply]. Reply: {cli} agent reply <msgId> \"text\".\nAfter --expects-reply, end your turn; the reply arrives as a new message. Never reply to a reply."
    );
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
    format!("{head}\n{}\n{footer}", lines.join("\n"))
}

pub fn with_roster(block: &str, entries: &[RosterEntry], cli: &str) -> String {
    format!("{block}\n{}", roster_section(entries, cli))
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

pub fn prompt_block(block: String, tile_id: &str, session_id: &str) -> Option<String> {
    let path = crate::binding_path(tile_id)?;
    let _guard = crate::binding_lock();
    let mut rec = crate::read_binding_rec(tile_id).filter(Value::is_object).unwrap_or_else(|| json!({ "tileId": tile_id }));
    let session = Some(session_id).filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| rec["agentSessionId"].as_str().map(str::to_string))
        .unwrap_or_default();
    let hash = block_hash(&block);
    if !needs_delivery(&rec[DELIVERED_KEY], &session, &hash) {
        return None;
    }
    rec[DELIVERED_KEY] = json!({ "sessionId": session, "hash": hash });
    if let Err(e) = crate::write_binding_rec(&path, &rec) {
        eprintln!("[agents] context marker not saved for {tile_id}: {e}");
    }
    Some(block)
}

const PASTED_KEY: &str = "agentContextPasted";

pub fn pasted_hash(tile_id: &str) -> Option<String> {
    crate::read_binding_rec(tile_id)?[PASTED_KEY].as_str().map(str::to_string)
}

pub fn set_pasted_hash(tile_id: &str, hash: &str) {
    let Some(path) = crate::binding_path(tile_id) else { return };
    let _guard = crate::binding_lock();
    let mut rec = crate::read_binding_rec(tile_id).filter(Value::is_object).unwrap_or_else(|| json!({ "tileId": tile_id }));
    rec[PASTED_KEY] = hash.into();
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
    let block = crate::agents::full_block(agent);
    match crate::send_to_tile(&agent.tile_id(), &bootstrap_message(&block)) {
        Ok(()) => set_pasted_hash(&agent.tile_id(), &block_hash(&block)),
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
        assert!(lines[4].starts_with("Send: /cli agent send <name> \"text\" [--expects-reply]. Reply: /cli agent reply <msgId>"));
        assert!(lines[5].contains("end your turn") && lines[5].contains("Never reply to a reply"));
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
    fn bootstrap_wraps_the_block() {
        let a = agent("a1", "rev");
        let msg = bootstrap_message(&agent_block(&a));
        assert!(msg.starts_with(&agent_block(&a)));
        assert!(msg.ends_with(BOOTSTRAP_TAIL));
        assert!(!has_context_hook("kimi") && !has_context_hook("opencode") && !has_context_hook("agy"));
        assert!(has_context_hook("claude") && has_context_hook("codex") && has_context_hook("pi"));
    }
}
