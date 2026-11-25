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

pub fn block_hash(block: &str) -> String {
    let hash = block.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    format!("{hash:016x}")
}

pub fn needs_delivery(delivered: &Value, session_id: &str, hash: &str) -> bool {
    delivered["sessionId"].as_str() != Some(session_id) || delivered["hash"].as_str() != Some(hash)
}

pub fn bootstrap_message(agent: &Agent) -> String {
    format!("{}\n{BOOTSTRAP_TAIL}", agent_block(agent))
}

pub fn prompt_block(agent: &Agent, tile_id: &str, session_id: &str, extra: Option<&str>) -> Option<String> {
    let path = crate::binding_path(tile_id)?;
    let _guard = crate::binding_lock();
    let mut rec = crate::read_binding_rec(tile_id).filter(Value::is_object).unwrap_or_else(|| json!({ "tileId": tile_id }));
    let session = Some(session_id).filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| rec["agentSessionId"].as_str().map(str::to_string))
        .unwrap_or_default();
    let mut block = agent_block(agent);
    if let Some(extra) = extra.filter(|e| !e.is_empty()) {
        block.push_str("\n\n");
        block.push_str(extra);
    }
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
    if let Err(e) = crate::send_to_tile(&agent.tile_id(), &bootstrap_message(agent)) {
        eprintln!("[agents] {}: bootstrap not sent: {e}", agent.id);
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

    #[test]
    fn bootstrap_wraps_the_block() {
        let a = agent("a1", "rev");
        let msg = bootstrap_message(&a);
        assert!(msg.starts_with(&agent_block(&a)));
        assert!(msg.ends_with(BOOTSTRAP_TAIL));
        assert!(!has_context_hook("kimi") && !has_context_hook("opencode") && !has_context_hook("agy"));
        assert!(has_context_hook("claude") && has_context_hook("codex") && has_context_hook("pi"));
    }
}
