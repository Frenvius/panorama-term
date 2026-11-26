use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::agents::store::EventRecord;
use crate::agents::{err_resp, ok_resp};

pub const MAX_BYTES: u64 = 1024 * 1024;
pub const MAX_RECORDS: u32 = 5_000;
pub const SIZE_MIN_RECORDS: u32 = 500;
pub const CHECK_EVERY: Duration = Duration::from_secs(600);
const RENAME_TRIES: u32 = 3;
const RENAME_GAP: Duration = Duration::from_millis(50);

pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".1");
    path.with_file_name(name)
}

pub fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

pub fn restore_backup(path: &Path) {
    let backup = backup_path(path);
    if !path.exists() && backup.exists() {
        if let Err(e) = std::fs::rename(&backup, path) {
            eprintln!("[compact] restore {}: {e}", path.display());
        }
    }
    let _ = std::fs::remove_file(tmp_path(path));
}

fn retry_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut last = None;
    for attempt in 0..RENAME_TRIES {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
        if attempt + 1 < RENAME_TRIES {
            std::thread::sleep(RENAME_GAP);
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("rename failed")))
}

pub fn swap_log(path: &Path, records: &[EventRecord]) -> Result<(), String> {
    swap_log_with(path, records, &retry_rename)
}

pub fn swap_log_with(path: &Path, records: &[EventRecord], rename: &dyn Fn(&Path, &Path) -> std::io::Result<()>) -> Result<(), String> {
    let tmp = tmp_path(path);
    let backup = backup_path(path);
    let write = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        for r in records {
            let line = serde_json::to_string(r).map_err(std::io::Error::other)?;
            file.write_all(line.as_bytes())?;
            file.write_all(b"\n")?;
        }
        file.sync_all()
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("write {}: {e}", tmp.display()));
    }
    if path.exists() {
        if let Err(e) = rename(path, &backup) {
            let _ = std::fs::remove_file(&tmp);
            eprintln!("[compact] keep {} as it was: {e}", path.display());
            return Err(format!("backup {}: {e}", path.display()));
        }
    }
    if let Err(e) = rename(&tmp, path) {
        let _ = rename(&backup, path);
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("swap {}: {e}", path.display()));
    }
    Ok(())
}

pub fn snapshot_record(seq: u32, kind: &str, data: Value) -> EventRecord {
    EventRecord { seq, at: crate::agents::unix_ms(), kind: kind.into(), data }
}

pub fn compact_log(path: &Path, seq: u32, kind: &str, data: Value) -> Result<Sizes, String> {
    let before = file_size(path);
    swap_log(path, &[snapshot_record(seq, kind, data)])?;
    Ok(Sizes { before, after: file_size(path) })
}

pub fn unchanged(path: &Path) -> Sizes {
    let size = file_size(path);
    Sizes { before: size, after: size }
}

pub fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

pub fn needs_compaction(path: &Path, records_since_snapshot: u32) -> bool {
    records_since_snapshot > MAX_RECORDS || (records_since_snapshot >= SIZE_MIN_RECORDS && file_size(path) > MAX_BYTES)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Sizes {
    pub before: u64,
    pub after: u64,
}

impl Sizes {
    pub fn add(&mut self, other: Sizes) {
        self.before += other.before;
        self.after += other.after;
    }
}

pub fn compact_all(force: bool) -> Sizes {
    let mut total = Sizes::default();
    total.add(crate::agents::waits::compact(force));
    total.add(crate::agents::routines::compact(force));
    total.add(crate::agents::memory::compact_all(force));
    total.add(crate::agents::compact_agent_logs(force));
    total.add(crate::agents::messages::compact_mail(force));
    total
}

pub fn start() {
    crate::agents::runtime::spawn_background(async {
        loop {
            let _ = tokio::task::spawn_blocking(|| compact_all(false)).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

pub fn handle_compact(v: &Value) -> String {
    let from_env = ["fromAgent", "fromTile"].iter().any(|k| v.get(*k).and_then(Value::as_str).is_some_and(|s| !s.is_empty()));
    if from_env || v.get("worker").and_then(Value::as_bool) == Some(true) {
        return err_resp("forbidden", "compaction is run by the user");
    }
    let sizes = compact_all(true);
    ok_resp(json!({ "before": sizes.before, "after": sizes.after }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("compact-{tag}-{}", crate::agents::unix_ms()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn rec(seq: u32) -> EventRecord {
        EventRecord { seq, at: 1, kind: "x".into(), data: json!({ "n": seq }) }
    }

    #[test]
    fn a_swap_replaces_the_log_and_keeps_one_backup() {
        let d = dir("swap");
        let path = d.join("events.jsonl");
        std::fs::write(&path, "old\n").unwrap();
        swap_log(&path, &[rec(7)]).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"seq\":7"));
        assert_eq!(std::fs::read_to_string(backup_path(&path)).unwrap(), "old\n");
        swap_log(&path, &[rec(8)]).unwrap();
        assert!(std::fs::read_to_string(backup_path(&path)).unwrap().contains("\"seq\":7"), "one backup, the previous log");
        assert!(!tmp_path(&path).exists());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn a_crash_between_the_renames_is_restored_at_boot() {
        let d = dir("crash");
        let path = d.join("events.jsonl");
        std::fs::write(backup_path(&path), "the old log\n").unwrap();
        std::fs::write(tmp_path(&path), "half").unwrap();
        restore_backup(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "the old log\n");
        assert!(!tmp_path(&path).exists());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn a_failed_rename_keeps_the_old_log_byte_identical() {
        let d = dir("fail");
        let path = d.join("events.jsonl");
        std::fs::write(&path, "keep me exactly\n").unwrap();
        let backup_fails = |from: &Path, to: &Path| -> std::io::Result<()> {
            if to.to_string_lossy().ends_with(".1") { Err(std::io::Error::other("sharing violation")) } else { std::fs::rename(from, to) }
        };
        assert!(swap_log_with(&path, &[rec(1)], &backup_fails).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"keep me exactly\n");
        assert!(!tmp_path(&path).exists());
        let swap_fails = |from: &Path, to: &Path| -> std::io::Result<()> {
            if from.to_string_lossy().ends_with(".tmp") { Err(std::io::Error::other("busy")) } else { std::fs::rename(from, to) }
        };
        assert!(swap_log_with(&path, &[rec(1)], &swap_fails).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"keep me exactly\n", "the backup is moved back");
        assert!(!tmp_path(&path).exists());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn a_double_rename_failure_never_lets_an_append_shadow_the_backup() {
        let d = dir("double");
        let path = d.join("events.jsonl");
        std::fs::write(&path, "{\"seq\":1,\"at\":1,\"kind\":\"x\",\"data\":{}}\n").unwrap();
        let both_fail = |from: &Path, to: &Path| -> std::io::Result<()> {
            if from.to_string_lossy().ends_with(".tmp") || from.to_string_lossy().ends_with(".1") { Err(std::io::Error::other("locked")) } else { std::fs::rename(from, to) }
        };
        assert!(swap_log_with(&path, &[rec(9)], &both_fail).is_err());
        assert!(!path.exists() && backup_path(&path).exists(), "the live log is gone, only the backup is left");
        crate::agents::store::append_event(&path, &rec(2)).unwrap();
        restore_backup(&path);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"seq\":1") && text.contains("\"seq\":2"), "the history is kept and the new event follows it: {text}");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn thresholds() {        let d = dir("size");
        let path = d.join("events.jsonl");
        std::fs::write(&path, vec![b'x'; 10]).unwrap();
        assert!(!needs_compaction(&path, MAX_RECORDS));
        assert!(needs_compaction(&path, MAX_RECORDS + 1));
        std::fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        assert!(needs_compaction(&path, SIZE_MIN_RECORDS));
        assert!(!needs_compaction(&path, SIZE_MIN_RECORDS - 1), "a log that stays big after a compaction is not rewritten again at once");
        std::fs::remove_dir_all(d).ok();
    }

    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self, n: u64) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 33) % n.max(1)
        }
    }

    use crate::agents::mail::tests::{agent, msg};
    use crate::agents::mail::{MailCmd, Mailbox, MsgStatus, THREAD_MAX};

    fn mail_step(mb: &mut Mailbox, cmd: MailCmd) {
        if let Ok((kind, data)) = mb.apply(&cmd) {
            mb.replay(kind, &data).unwrap();
        }
    }

    fn mail_events(seed: u64, now: u64, mb: &mut Mailbox, ids: &mut Vec<String>) {
        let mut rng = Lcg(seed);
        let parties = ["a", "b", "c"];
        for i in 0..120 {
            let id = format!("m{seed}x{i}");
            let ops = rng.next(6);
            match ops {
                0 | 1 => {
                    let from = parties[rng.next(3) as usize];
                    let to = parties[(rng.next(2) as usize + 1 + parties.iter().position(|p| *p == from).unwrap()) % 3];
                    let mut m = msg(&id, agent(from), agent(to));
                    m.at = now;
                    m.expects_reply = rng.next(2) == 0;
                    if let Some(parent) = ids.get(rng.next(ids.len() as u64 + 1) as usize).filter(|_| rng.next(2) == 0) {
                        if let Some(p) = mb.messages.get(parent).filter(|p| p.to == agent(from) || p.from == agent(from)) {
                            m.reply_to = Some(p.id.clone());
                            m.thread = p.thread.clone();
                            m.to = if p.to == agent(from) { p.from.clone() } else { p.to.clone() };
                        }
                    }
                    mail_step(mb, MailCmd::Send { message: m, recipient_exists: true });
                    ids.push(id);
                }
                2 if !ids.is_empty() => {
                    let pick = ids[rng.next(ids.len() as u64) as usize].clone();
                    mail_step(mb, MailCmd::Deliver { ids: vec![pick], at: now });
                }
                3 if !ids.is_empty() => {
                    let pick = ids[rng.next(ids.len() as u64) as usize].clone();
                    mail_step(mb, MailCmd::Fail { id: pick, at: now, reason: "x".into() });
                }
                4 if !ids.is_empty() => {
                    let pick = ids[rng.next(ids.len() as u64) as usize].clone();
                    mail_step(mb, MailCmd::Read { ids: vec![pick], at: now });
                }
                _ if !ids.is_empty() => {
                    let pick = ids[rng.next(ids.len() as u64) as usize].clone();
                    mail_step(mb, MailCmd::Update { id: pick, text: format!("edit {i}"), key: Some(format!("k{i}")), at: now });
                }
                _ => {}
            }
        }
    }

    fn assert_same_mail(a: &Mailbox, b: &Mailbox) {
        assert_eq!(a.messages, b.messages);
        assert_eq!(a.seq, b.seq);
        let threads: std::collections::BTreeSet<&String> = a.messages.values().map(|m| &m.thread).chain(a.thread_base.keys()).collect();
        for t in threads {
            assert_eq!(a.thread_len(t), b.thread_len(t), "thread {t}");
        }
    }

    #[test]
    fn a_mail_snapshot_plus_later_events_replays_like_the_full_log() {
        let now = 1_800_000_000_000;
        for seed in 1..20 {
            let (mut full, mut ids) = (Mailbox::default(), vec![]);
            mail_events(seed, now, &mut full, &mut ids);
            let mut compacted = Mailbox::default();
            compacted.replay("mail-snapshot", &full.snapshot(now, &Default::default())).unwrap();
            assert_same_mail(&full, &compacted);
            let mut more_ids = ids.clone();
            mail_events(seed + 100, now, &mut full, &mut ids);
            mail_events(seed + 100, now, &mut compacted, &mut more_ids);
            assert_same_mail(&full, &compacted);
        }
    }

    #[test]
    fn a_full_thread_stays_refused_after_its_old_messages_are_pruned() {
        let mut mb = Mailbox::default();
        for i in 0..THREAD_MAX {
            let mut m = msg(&format!("t{i}"), agent("a"), agent("b"));
            m.thread = "t0".into();
            mail_step(&mut mb, MailCmd::Send { message: m, recipient_exists: true });
            mail_step(&mut mb, MailCmd::Deliver { ids: vec![format!("t{i}")], at: 1 });
        }
        let now = 1 + 40 * 24 * 3_600_000;
        let mut pruned = Mailbox::default();
        pruned.replay("mail-snapshot", &mb.snapshot(now, &Default::default())).unwrap();
        assert!(pruned.messages.is_empty(), "old delivered mail is pruned");
        let mut m = msg("t-next", agent("a"), agent("b"));
        m.thread = "t0".into();
        let err = pruned.apply(&MailCmd::Send { message: m, recipient_exists: true }).unwrap_err();
        assert_eq!(err.code, "rejected");
        let mut twice = Mailbox::default();
        twice.replay("mail-snapshot", &pruned.snapshot(now, &Default::default())).unwrap();
        assert_eq!(twice.thread_len("t0"), THREAD_MAX, "a second compaction keeps the count");
    }

    #[test]
    fn mail_retention_keeps_what_is_open_or_pointed_at() {
        let day = 24 * 3_600_000;
        let now = 100 * day;
        let mut mb = Mailbox::default();
        let mut send = |id: &str, at: u64, expects: bool, deliver: bool| {
            let mut m = msg(id, agent("a"), agent("b"));
            m.at = at;
            m.expects_reply = expects;
            mail_step(&mut mb, MailCmd::Send { message: m, recipient_exists: true });
            if deliver {
                mail_step(&mut mb, MailCmd::Deliver { ids: vec![id.into()], at });
            }
        };
        send("old-queued", day, false, false);
        send("old-asked", day, true, true);
        send("old-done", day, false, true);
        send("ewold", day, false, true);
        send("uold-1", day, false, true);
        for i in 0..2_100 {
            send(&format!("new{i:04}"), now - 1, false, true);
        }
        let referenced: HashSet<String> = ["ewold".to_string(), "uold-1".to_string()].into();
        let mut kept = Mailbox::default();
        kept.replay("mail-snapshot", &mb.snapshot(now, &referenced)).unwrap();
        for id in ["old-queued", "old-asked", "ewold", "uold-1"] {
            assert!(kept.messages.contains_key(id), "{id} is kept");
        }
        assert!(!kept.messages.contains_key("old-done"));
        assert_eq!(kept.messages.len(), crate::agents::mail::MAIL_KEEP_MAX, "at most 2,000, the ones that must stay included");
        assert!(kept.messages.contains_key("new0104") && !kept.messages.contains_key("new0103"), "newest first");
        assert_eq!(kept.messages["old-queued"].status, MsgStatus::Queued);
    }

    use crate::agents::waits::{unsent_wakes, wake_id, Fired, Target, Wait, WaitStatus, Waits};

    fn wait(id: &str, created: u64, status: WaitStatus, fired_at: Option<u64>) -> Wait {
        Wait {
            id: id.into(), waiter: agent("a"), waiter_name: "a".into(), targets: vec![Target::Time { at: created + 1 }], note: String::new(),
            created_at: created, timeout_at: None, status,
            fired: fired_at.map(|at| Fired { target: Some(0), at, summary: "time".into(), wake: true, started: false }), cancelled_by: None,
        }
    }

    #[test]
    fn after_compacting_every_log_no_boot_repair_sends_anything() {
        let day = 24 * 3_600_000;
        let now = 100 * day;
        let mut waits = Waits::default();
        let mut mb = Mailbox::default();
        for (id, at) in [("w1", now - 6 * day), ("w2", now - 1000), ("w3", now - 9 * day)] {
            waits.waits.insert(id.into(), wait(id, at, WaitStatus::Fired, Some(at)));
            let mut m = msg(&wake_id(id), crate::agents::mail::Party::Brain, agent("a"));
            m.at = at;
            mail_step(&mut mb, MailCmd::Send { message: m, recipient_exists: true });
            mail_step(&mut mb, MailCmd::Deliver { ids: vec![wake_id(id)], at });
        }
        for i in 0..2_100 {
            let mut m = msg(&format!("n{i}"), agent("b"), agent("a"));
            m.at = now - 10;
            mail_step(&mut mb, MailCmd::Send { message: m, recipient_exists: true });
            mail_step(&mut mb, MailCmd::Deliver { ids: vec![format!("n{i}")], at: now - 10 });
        }
        let mut kept_waits = Waits::default();
        kept_waits.replay("waits-snapshot", &json!({ "waits": waits.retained(now) })).unwrap();
        assert!(kept_waits.waits.contains_key("w1") && !kept_waits.waits.contains_key("w3"));
        let referenced: HashSet<String> = kept_waits.waits.keys().map(|id| wake_id(id)).collect();
        let mut kept_mail = Mailbox::default();
        kept_mail.replay("mail-snapshot", &mb.snapshot(now, &referenced)).unwrap();
        assert!(unsent_wakes(&kept_waits, |id| kept_mail.messages.contains_key(id)).is_empty());
    }

    use std::collections::HashSet;

    #[test]
    fn an_agent_log_snapshot_replays_to_the_same_agent() {
        use crate::agents::reducer::{ApplyResult, Cmd, State};
        let d = dir("agentlog");
        let id = "a1234567";
        let path = crate::agents::store::events_path(&d, id);
        crate::agents::store::ensure_agent_dir(&d, id).unwrap();
        let mut live = State::default();
        let mut seq = 0;
        let mut commit = |live: &mut State, cmd: Cmd| {
            if let ApplyResult::Ok(kind, data) = live.apply(&cmd) {
                seq += 1;
                crate::agents::store::append_event(&path, &EventRecord { seq, at: 1, kind: kind.clone(), data: data.clone() }).unwrap();
                live.replay(&kind, &data).unwrap();
            }
        };
        commit(&mut live, Cmd::Create { agent: crate::agents::reducer::tests::agent(id, "rev"), known_profiles: vec!["claude".into()] });
        for i in 0..30u64 {
            commit(&mut live, Cmd::Start { id: id.into(), at: 10 + i, live: false, started_by: Some("user".into()) });
            commit(&mut live, Cmd::BindSession { id: id.into(), agent: "claude".into(), session_id: format!("s{i}"), at: 11 + i });
            commit(&mut live, Cmd::Stop { id: id.into(), at: 12 + i, by: "user".into(), force: false, crashed: false });
        }
        let mut full = State::default();
        crate::agents::store::load_into(&mut full, &path, id);
        compact_log(&path, full.seqs[id], "agent-snapshot", serde_json::to_value(&full.agents[id]).unwrap()).unwrap();
        let mut compacted = State::default();
        crate::agents::store::load_into(&mut compacted, &path, id);
        assert_eq!(compacted.agents, full.agents);
        assert_eq!(compacted.seqs, full.seqs, "the next append continues the sequence");
        assert_eq!(compacted.snaps[id], full.seqs[id]);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn boot_restores_an_agent_log_left_only_as_its_backup() {
        let d = dir("bootrestore");
        let id = "a7654321";
        crate::agents::store::ensure_agent_dir(&d, id).unwrap();
        let path = crate::agents::store::events_path(&d, id);
        let record = EventRecord { seq: 1, at: 1, kind: "agent-create".into(), data: serde_json::to_value(crate::agents::reducer::tests::agent(id, "rev")).unwrap() };
        crate::agents::store::append_event(&backup_path(&path), &record).unwrap();
        std::fs::write(tmp_path(&path), "half").unwrap();
        let mut state = crate::agents::reducer::State::default();
        crate::agents::store::load_all(&mut state, &d);
        assert!(state.agents.contains_key(id), "the backup is restored before the load");
        assert!(path.exists() && !tmp_path(&path).exists());
        std::fs::remove_dir_all(d).ok();
    }
}