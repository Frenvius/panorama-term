use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const TEXT_MAX_BYTES: usize = 16 * 1024;
pub const MAX_HOPS: u8 = 4;
pub const THREAD_MAX: usize = 24;
pub const PAIR_PENDING_MAX: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Party {
    Agent { id: String },
    Tile { id: String },
    User,
}

impl Party {
    pub fn key(&self) -> String {
        match self {
            Party::Agent { id } => format!("agent:{id}"),
            Party::Tile { id } => format!("tile:{id}"),
            Party::User => "user".into(),
        }
    }

    pub fn agent_id(&self) -> Option<&str> {
        match self {
            Party::Agent { id } => Some(id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MsgStatus {
    Queued,
    Delivered,
    Dropped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub from: Party,
    pub to: Party,
    #[serde(default)]
    pub from_name: String,
    pub text: String,
    #[serde(default)]
    pub expects_reply: bool,
    #[serde(default)]
    pub reply_to: Option<String>,
    pub thread: String,
    #[serde(default)]
    pub hops: u8,
    #[serde(default)]
    pub at: u64,
    #[serde(default)]
    pub seq: u32,
    pub status: MsgStatus,
    #[serde(default)]
    pub delivered_at: Option<u64>,
    #[serde(default)]
    pub replied_by: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub notice: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailError {
    pub code: &'static str,
    pub message: String,
}

impl MailError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

pub enum MailCmd {
    Send { message: Message, recipient_exists: bool },
    Deliver { ids: Vec<String>, at: u64 },
    Fail { id: String, at: u64, reason: String },
    Drop { id: String, at: u64, reason: String },
    Read { ids: Vec<String>, at: u64 },
}

#[derive(Debug, Default)]
pub struct Mailbox {
    pub messages: BTreeMap<String, Message>,
    pub seq: u32,
}

fn pending_reply(m: &Message) -> bool {
    m.expects_reply && m.replied_by.is_none() && !matches!(m.status, MsgStatus::Dropped | MsgStatus::Failed)
}

impl Mailbox {
    pub fn queue_for(&self, party: &Party) -> Vec<&Message> {
        let mut queued: Vec<&Message> = self.messages.values()
            .filter(|m| m.status == MsgStatus::Queued && m.to == *party)
            .collect();
        queued.sort_by_key(|m| m.seq);
        queued
    }

    pub fn awaiting_replies(&self, party: &Party) -> usize {
        self.messages.values().filter(|m| m.from == *party && pending_reply(m)).count()
    }

    pub fn unread_for(&self, party: &Party) -> usize {
        self.messages.values().filter(|m| m.to == *party && !m.read && m.status != MsgStatus::Dropped).count()
    }

    pub fn thread_len(&self, thread: &str) -> usize {
        self.messages.values().filter(|m| m.thread == thread).count()
    }

    pub fn apply(&self, cmd: &MailCmd) -> Result<(&'static str, Value), MailError> {
        match cmd {
            MailCmd::Send { message, recipient_exists } => {
                self.check_send(message, *recipient_exists)?;
                let mut stored = message.clone();
                stored.seq = self.seq + 1;
                stored.status = MsgStatus::Queued;
                let data = serde_json::to_value(stored).map_err(|e| MailError::new("io", e.to_string()))?;
                Ok(("msg-send", data))
            }
            MailCmd::Deliver { ids, at } => {
                let mut recipient = None;
                for id in ids {
                    let m = self.queued(id)?;
                    if recipient.get_or_insert(&m.to) != &&m.to {
                        return Err(MailError::new("bad_request", "a delivery batch has one recipient"));
                    }
                }
                if ids.is_empty() {
                    return Err(MailError::new("bad_request", "empty delivery"));
                }
                Ok(("msg-deliver", json!({ "ids": ids, "at": at })))
            }
            MailCmd::Fail { id, at, reason } => {
                self.queued(id)?;
                Ok(("msg-fail", json!({ "id": id, "at": at, "reason": reason })))
            }
            MailCmd::Drop { id, at, reason } => {
                self.queued(id)?;
                Ok(("msg-drop", json!({ "id": id, "at": at, "reason": reason })))
            }
            MailCmd::Read { ids, at } => {
                for id in ids {
                    let m = self.messages.get(id).ok_or_else(|| MailError::new("not_found", format!("message {id} not found")))?;
                    if m.to != Party::User {
                        return Err(MailError::new("bad_request", format!("{id} is not addressed to the user")));
                    }
                }
                Ok(("msg-read", json!({ "ids": ids, "at": at })))
            }
        }
    }

    fn queued(&self, id: &str) -> Result<&Message, MailError> {
        let m = self.messages.get(id).ok_or_else(|| MailError::new("not_found", format!("message {id} not found")))?;
        if m.status != MsgStatus::Queued {
            return Err(MailError::new("conflict", format!("message {id} is not queued")));
        }
        Ok(m)
    }

    fn check_send(&self, m: &Message, recipient_exists: bool) -> Result<(), MailError> {
        if self.messages.contains_key(&m.id) {
            return Err(MailError::new("conflict", "message id exists"));
        }
        if m.text.trim().is_empty() {
            return Err(MailError::new("bad_request", "text required"));
        }
        if m.text.len() > TEXT_MAX_BYTES {
            return Err(MailError::new("bad_request", format!("text is longer than {TEXT_MAX_BYTES} bytes")));
        }
        if !recipient_exists {
            return Err(MailError::new("not_found", "recipient not found"));
        }
        if m.from == m.to {
            return Err(MailError::new("bad_request", "cannot send to yourself"));
        }
        if m.notice {
            return Ok(());
        }
        if m.hops > MAX_HOPS {
            return Err(MailError::new("rejected", format!("hop limit {MAX_HOPS} (thread {})", m.thread)));
        }
        if self.thread_len(&m.thread) >= THREAD_MAX {
            return Err(MailError::new("rejected", format!("thread limit {THREAD_MAX} (thread {})", m.thread)));
        }
        if let Some(parent_id) = &m.reply_to {
            let parent = self.messages.get(parent_id)
                .ok_or_else(|| MailError::new("not_found", format!("message {parent_id} not found")))?;
            if !parent.expects_reply {
                return Err(MailError::new("rejected", format!("{parent_id} does not expect a reply")));
            }
            if parent.replied_by.is_some() {
                return Err(MailError::new("conflict", format!("{parent_id} already answered")));
            }
            if parent.to != m.from || parent.from != m.to {
                return Err(MailError::new("rejected", format!("{parent_id} was not sent to you")));
            }
            if m.expects_reply {
                return Err(MailError::new("rejected", "a reply cannot expect a reply"));
            }
        }
        if m.expects_reply {
            let open = self.messages.values().filter(|o| o.from == m.from && o.to == m.to && pending_reply(o)).count();
            if open >= PAIR_PENDING_MAX {
                return Err(MailError::new("conflict", format!("{PAIR_PENDING_MAX} replies already pending from this recipient")));
            }
        }
        Ok(())
    }

    pub fn replay(&mut self, kind: &str, data: &Value) -> Result<(), String> {
        let ids = |v: &Value| -> Vec<String> {
            v["ids"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
        };
        let at = data["at"].as_u64();
        match kind {
            "msg-send" => {
                let m: Message = serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
                if let Some(parent) = m.reply_to.as_ref().and_then(|p| self.messages.get_mut(p)) {
                    parent.replied_by = Some(m.id.clone());
                }
                self.seq = self.seq.max(m.seq);
                self.messages.insert(m.id.clone(), m);
            }
            "msg-deliver" => {
                for id in ids(data) {
                    if let Some(m) = self.messages.get_mut(&id) {
                        m.status = MsgStatus::Delivered;
                        m.delivered_at = at;
                    }
                }
            }
            "msg-fail" | "msg-drop" => {
                let id = data["id"].as_str().ok_or("missing id")?;
                if let Some(m) = self.messages.get_mut(id) {
                    m.status = if kind == "msg-fail" { MsgStatus::Failed } else { MsgStatus::Dropped };
                    m.reason = data["reason"].as_str().map(str::to_string);
                }
            }
            "msg-read" => {
                for id in ids(data) {
                    if let Some(m) = self.messages.get_mut(&id) {
                        m.read = true;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn agent(id: &str) -> Party {
        Party::Agent { id: id.into() }
    }

    pub(crate) fn msg(id: &str, from: Party, to: Party) -> Message {
        Message {
            id: id.into(), from, to, from_name: "x".into(), text: "hi".into(), expects_reply: false,
            reply_to: None, thread: id.into(), hops: 0, at: 1, seq: 0, status: MsgStatus::Queued,
            delivered_at: None, replied_by: None, reason: None, read: false, notice: false,
        }
    }

    fn send(mb: &mut Mailbox, m: Message) -> Result<(), MailError> {
        let (kind, data) = mb.apply(&MailCmd::Send { message: m, recipient_exists: true })?;
        mb.replay(kind, &data).unwrap();
        Ok(())
    }

    #[test]
    fn send_validates_text_recipient_and_self() {
        let mut mb = Mailbox::default();
        let empty = Message { text: "  ".into(), ..msg("m1", agent("a"), agent("b")) };
        assert_eq!(send(&mut mb, empty).unwrap_err().code, "bad_request");
        let big = Message { text: "x".repeat(TEXT_MAX_BYTES + 1), ..msg("m1", agent("a"), agent("b")) };
        assert_eq!(send(&mut mb, big).unwrap_err().code, "bad_request");
        assert_eq!(send(&mut mb, msg("m1", agent("a"), agent("a"))).unwrap_err().code, "bad_request");
        let missing = mb.apply(&MailCmd::Send { message: msg("m1", agent("a"), agent("z")), recipient_exists: false });
        assert_eq!(missing.unwrap_err().code, "not_found");
        send(&mut mb, msg("m1", agent("a"), agent("b"))).unwrap();
        assert_eq!(mb.messages["m1"].seq, 1);
        assert_eq!(send(&mut mb, msg("m1", agent("a"), agent("b"))).unwrap_err().code, "conflict");
    }

    #[test]
    fn hop_and_thread_caps_reject() {
        let mut mb = Mailbox::default();
        let deep = Message { hops: MAX_HOPS + 1, ..msg("m1", agent("a"), agent("b")) };
        let err = send(&mut mb, deep).unwrap_err();
        assert_eq!(err.code, "rejected");
        assert!(err.message.contains("hop limit"), "{}", err.message);
        send(&mut mb, Message { hops: MAX_HOPS, ..msg("m1", agent("a"), agent("b")) }).unwrap();
        for i in 2..=THREAD_MAX {
            send(&mut mb, Message { thread: "m1".into(), ..msg(&format!("m{i}"), agent("a"), agent("b")) }).unwrap();
        }
        let err = send(&mut mb, Message { thread: "m1".into(), ..msg("mx", agent("a"), agent("b")) }).unwrap_err();
        assert!(err.message.contains("thread limit"), "{}", err.message);
    }

    #[test]
    fn reply_rules() {
        let mut mb = Mailbox::default();
        send(&mut mb, msg("plain", agent("a"), agent("b"))).unwrap();
        send(&mut mb, Message { expects_reply: true, ..msg("ask", agent("a"), agent("b")) }).unwrap();
        let reply = |id: &str, to: &str, from: Party, target: Party| Message { reply_to: Some(to.into()), thread: to.into(), hops: 1, ..msg(id, from, target) };
        assert_eq!(send(&mut mb, reply("r0", "nope", agent("b"), agent("a"))).unwrap_err().code, "not_found");
        assert_eq!(send(&mut mb, reply("r1", "plain", agent("b"), agent("a"))).unwrap_err().code, "rejected");
        assert_eq!(send(&mut mb, reply("r2", "ask", agent("c"), agent("a"))).unwrap_err().code, "rejected");
        let asks_back = Message { expects_reply: true, ..reply("r3", "ask", agent("b"), agent("a")) };
        assert_eq!(send(&mut mb, asks_back).unwrap_err().code, "rejected");
        send(&mut mb, reply("r4", "ask", agent("b"), agent("a"))).unwrap();
        assert_eq!(mb.messages["ask"].replied_by.as_deref(), Some("r4"));
        assert_eq!(send(&mut mb, reply("r5", "ask", agent("b"), agent("a"))).unwrap_err().code, "conflict");
        let reply_to_reply = Message { reply_to: Some("r4".into()), ..msg("r6", agent("a"), agent("b")) };
        assert_eq!(send(&mut mb, reply_to_reply).unwrap_err().code, "rejected");
    }

    #[test]
    fn pair_cap_counts_open_expects_reply() {
        let mut mb = Mailbox::default();
        for i in 0..PAIR_PENDING_MAX {
            send(&mut mb, Message { expects_reply: true, ..msg(&format!("q{i}"), agent("a"), agent("b")) }).unwrap();
        }
        let err = send(&mut mb, Message { expects_reply: true, ..msg("q9", agent("a"), agent("b")) }).unwrap_err();
        assert_eq!(err.code, "conflict");
        send(&mut mb, Message { expects_reply: true, ..msg("other", agent("a"), agent("c")) }).unwrap();
        send(&mut mb, msg("no-reply", agent("a"), agent("b"))).unwrap();
        assert_eq!(mb.awaiting_replies(&agent("a")), PAIR_PENDING_MAX + 1);
    }

    #[test]
    fn deliver_fail_drop_read_and_replay_round_trip() {
        let mut live = Mailbox::default();
        let mut log = Vec::new();
        let mut run = |mb: &mut Mailbox, cmd: MailCmd| {
            let (kind, data) = mb.apply(&cmd).unwrap();
            mb.replay(kind, &data).unwrap();
            log.push((kind, data));
        };
        run(&mut live, MailCmd::Send { message: msg("m1", agent("a"), agent("b")), recipient_exists: true });
        run(&mut live, MailCmd::Send { message: msg("m2", agent("a"), agent("b")), recipient_exists: true });
        run(&mut live, MailCmd::Send { message: msg("m3", agent("a"), Party::User), recipient_exists: true });
        run(&mut live, MailCmd::Send { message: msg("m4", agent("a"), agent("b")), recipient_exists: true });
        assert_eq!(live.queue_for(&agent("b")).iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["m1", "m2", "m4"]);
        run(&mut live, MailCmd::Deliver { ids: vec!["m1".into(), "m2".into()], at: 5 });
        run(&mut live, MailCmd::Fail { id: "m4".into(), at: 6, reason: "stuck".into() });
        assert_eq!(live.unread_for(&Party::User), 1);
        run(&mut live, MailCmd::Read { ids: vec!["m3".into()], at: 7 });
        assert_eq!(live.unread_for(&Party::User), 0);
        assert!(live.apply(&MailCmd::Deliver { ids: vec!["m1".into()], at: 8 }).is_err(), "already delivered");
        assert!(live.apply(&MailCmd::Read { ids: vec!["m1".into()], at: 8 }).is_err(), "not to user");
        assert!(live.queue_for(&agent("b")).is_empty());

        let mut replayed = Mailbox::default();
        for (kind, data) in &log {
            replayed.replay(kind, data).unwrap();
        }
        replayed.replay("msg-future", &json!({})).unwrap();
        assert_eq!(replayed.messages, live.messages);
        assert_eq!(replayed.seq, 4);
    }

    #[test]
    fn delivery_batch_needs_one_recipient() {
        let mut mb = Mailbox::default();
        send(&mut mb, msg("m1", agent("a"), agent("b"))).unwrap();
        send(&mut mb, msg("m2", agent("a"), agent("c"))).unwrap();
        let err = mb.apply(&MailCmd::Deliver { ids: vec!["m1".into(), "m2".into()], at: 1 }).unwrap_err();
        assert_eq!(err.code, "bad_request");
    }

    #[test]
    fn notices_skip_the_guards() {
        let mut mb = Mailbox::default();
        let notice = Message { notice: true, hops: 99, ..msg("n1", Party::User, agent("a")) };
        send(&mut mb, notice).unwrap();
    }
}
