use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::agents::envelope;
use crate::agents::mail::{MailCmd, Message, Party};
use crate::agents::messages::{cause_of, commit_mail, set_cause, with_mailbox};
use crate::agents::model::{LiveStatus, StatusSource};
use crate::{ClientInput, Confirm};

pub const TYPING_QUIET_MS: u64 = 10_000;
pub const RETRY_MS: u64 = 5_000;
pub const PENDING_STALE_MS: u64 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub tile: String,
    pub live: bool,
    pub starting: bool,
    pub status: LiveStatus,
    pub source: StatusSource,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    NotIdle,
    WaitTyping(u64),
}

pub fn readiness(view: &View, input: ClientInput, now: u64) -> Readiness {
    if !view.live || view.starting || view.status != LiveStatus::Idle || view.source == StatusSource::None {
        return Readiness::NotIdle;
    }
    let since_input = now.saturating_sub(input.last_ms);
    if since_input < TYPING_QUIET_MS {
        return Readiness::WaitTyping(TYPING_QUIET_MS - since_input);
    }
    if input.pending > 0 && since_input < PENDING_STALE_MS {
        return Readiness::WaitTyping(RETRY_MS);
    }
    Readiness::Ready
}

pub(crate) trait DeliveryBackend {
    fn view(&self, party: &Party) -> Option<View>;
    fn input(&self, tile: &str) -> ClientInput;
    fn now(&self) -> u64;
    fn bracketed(&self, tile: &str) -> bool;
    fn prefix(&self, party: &Party) -> Option<String>;
    fn prefix_delivered(&self, party: &Party);
    fn last_busy(&self, party: &Party) -> Option<u64>;
    fn orch_delivered(&self, run: &str);
    type Lock;
    async fn lock(&self, tile: &str) -> Self::Lock;
    async fn paste(&self, tile: &str, text: &str) -> Result<Confirm, String>;
    async fn clear(&self, tile: &str);
    fn retry_later(&self, party: &Party, delay_ms: u64);
}

fn in_flight() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

fn stuck_once() -> &'static Mutex<HashMap<String, Option<u64>>> {
    static SET: OnceLock<Mutex<HashMap<String, Option<u64>>>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

fn retry_pending() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

pub(crate) fn delivering(party: &Party) -> bool {
    guard(in_flight()).contains(&party.key())
}

pub(crate) fn claim_retry(key: &str) -> bool {
    guard(retry_pending()).insert(key.to_string())
}

pub(crate) fn release_retry(key: &str) {
    guard(retry_pending()).remove(key);
}

fn guard<T>(m: &'static Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct Flight(String);

impl Flight {
    fn take(party: &Party) -> Option<Self> {
        let key = party.key();
        let inserted = guard(in_flight()).insert(key.clone());
        inserted.then(|| Flight(key))
    }
}

impl Drop for Flight {
    fn drop(&mut self) {
        guard(in_flight()).remove(&self.0);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    NotReady,
    Waiting(u64),
    Delivered(Vec<String>),
    StuckRetry,
    Failed(Vec<String>),
    Error,
    Busy,
}

pub(crate) async fn drain<B: DeliveryBackend>(backend: &B, party: &Party) -> Outcome {
    let Some(_flight) = Flight::take(party) else { return Outcome::Busy };
    let mut queue: Vec<Message> = with_mailbox(|mb| mb.queue_for(party).into_iter().cloned().collect());
    if party.agent_id().is_some_and(crate::agents::rollover::paused) {
        queue.retain(|m| m.from_brain() && m.kind.as_deref() == Some(crate::agents::mail::KIND_ROLLOVER));
    }
    if queue.is_empty() {
        return Outcome::Nothing;
    }
    let Some(view) = backend.view(party) else { return Outcome::NotReady };
    if let Some(outcome) = not_ready(backend, party, &view) {
        return outcome;
    }
    let proven = proven_delivered(backend, party, &view.tile, &queue);
    if !proven.is_empty() {
        if let Err(e) = commit_mail(MailCmd::Deliver { ids: proven.clone(), at: backend.now() }) {
            eprintln!("[agents] delivery of {proven:?} not recorded: {}", e.message);
        }
        let mut stuck = guard(stuck_once());
        for id in &proven {
            stuck.remove(id);
        }
        return Outcome::Delivered(proven);
    }
    let bracketed = backend.bracketed(&view.tile);
    let prefix = backend.prefix(party);
    let refs: Vec<&Message> = queue.iter().collect();
    let Some(batch) = envelope::batch(&refs, &crate::panorama_cli_path(), bracketed, prefix.as_deref()) else {
        return Outcome::Nothing;
    };
    let key = party.key();
    let previous_cause = cause_of(&key);
    let paste_started = backend.now();
    let result = {
        let _writer = backend.lock(&view.tile).await;
        let Some(current) = backend.view(party) else { return Outcome::NotReady };
        if let Some(outcome) = not_ready(backend, party, &current) {
            return outcome;
        }
        if let Some(cause) = queue.iter().filter(|m| batch.ids.contains(&m.id)).max_by_key(|m| m.hops) {
            set_cause(&key, &cause.thread, cause.hops);
        }
        backend.paste(&view.tile, &batch.text).await
    };
    if !matches!(result, Ok(Confirm::Submitted)) {
        restore_cause(&key, previous_cause);
    }
    match result {
        Ok(Confirm::Submitted) => {
            let still_queued: Vec<String> = with_mailbox(|mb| batch.ids.iter()
                .filter(|id| mb.messages.get(*id).is_some_and(|m| m.status == crate::agents::mail::MsgStatus::Queued))
                .cloned()
                .collect());
            if !still_queued.is_empty() {
                if let Err(e) = commit_mail(MailCmd::Deliver { ids: still_queued, at: backend.now() }) {
                    eprintln!("[agents] delivery of {:?} not recorded: {}", batch.ids, e.message);
                }
            }
            if prefix.is_some() {
                backend.prefix_delivered(party);
            }
            crate::agents::routines::on_delivered(&batch.ids, paste_started, backend.now());
            for m in queue.iter().filter(|m| batch.ids.contains(&m.id) && m.kind.as_deref() == Some(crate::agents::mail::KIND_ORCH)) {
                if let Some(run) = m.key.as_deref().and_then(crate::agents::mail::orch_run) {
                    backend.orch_delivered(run);
                }
            }
            let mut stuck = guard(stuck_once());
            for id in &batch.ids {
                stuck.remove(id);
            }
            Outcome::Delivered(batch.ids)
        }
        Ok(confirm @ (Confirm::Stuck | Confirm::Unconfirmed)) => {
            if backend.input(&view.tile).last_ms > paste_started {
                backend.retry_later(party, RETRY_MS);
                return Outcome::StuckRetry;
            }
            let second = {
                let stuck = guard(stuck_once());
                batch.ids.iter().all(|id| stuck.contains_key(id))
            };
            backend.clear(&view.tile).await;
            if second {
                let reason = if confirm == Confirm::Stuck { "stuck" } else { "unconfirmed" };
                for id in &batch.ids {
                    guard(stuck_once()).remove(id);
                    if let Err(e) = commit_mail(MailCmd::Fail { id: id.clone(), at: backend.now(), reason: reason.into() }) {
                        eprintln!("[agents] fail {id}: {}", e.message);
                    }
                }
                return Outcome::Failed(batch.ids);
            }
            let unconfirmed_at = (confirm == Confirm::Unconfirmed).then_some(paste_started);
            guard(stuck_once()).extend(batch.ids.iter().map(|id| (id.clone(), unconfirmed_at)));
            backend.retry_later(party, RETRY_MS);
            Outcome::StuckRetry
        }
        Err(_) => {
            backend.retry_later(party, RETRY_MS);
            Outcome::Error
        }
    }
}

fn not_ready<B: DeliveryBackend>(backend: &B, party: &Party, view: &View) -> Option<Outcome> {
    match readiness(view, backend.input(&view.tile), backend.now()) {
        Readiness::Ready => None,
        Readiness::NotIdle => Some(Outcome::NotReady),
        Readiness::WaitTyping(ms) => {
            backend.retry_later(party, ms);
            Some(Outcome::Waiting(ms))
        }
    }
}

fn proven_delivered<B: DeliveryBackend>(backend: &B, party: &Party, tile: &str, queue: &[Message]) -> Vec<String> {
    let Some(busy_at) = backend.last_busy(party) else { return vec![] };
    let typed_at = backend.input(tile).last_ms;
    let stuck = guard(stuck_once());
    queue.iter()
        .filter(|m| stuck.get(&m.id).copied().flatten().is_some_and(|at| busy_at >= at && typed_at <= at))
        .map(|m| m.id.clone())
        .collect()
}

fn restore_cause(key: &str, previous: Option<crate::agents::messages::Cause>) {
    match previous {
        Some(cause) => crate::agents::messages::put_cause(key, cause),
        None => crate::agents::messages::clear_cause(key),
    }
}
struct LiveBackend;

fn tile_view(tile: &str) -> View {
    let session = crate::lookup_live(tile);
    let status = session.as_ref()
        .and_then(|s| crate::cached_agent_state(s))
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["lastStatus"].as_str().map(str::to_string));
    let (status, source) = match status.as_deref() {
        Some("idle") => (LiveStatus::Idle, StatusSource::Hook),
        Some("busy") => (LiveStatus::Busy, StatusSource::Hook),
        Some("waiting") => (LiveStatus::Waiting, StatusSource::Hook),
        _ => match crate::agents::runtime::quiet_status(tile) {
            Some(status) => (status, StatusSource::Quiet),
            None => (LiveStatus::Idle, StatusSource::None),
        },
    };
    View { tile: tile.to_string(), live: session.is_some(), starting: false, status, source }
}

impl DeliveryBackend for LiveBackend {
    fn view(&self, party: &Party) -> Option<View> {
        match party {
            Party::Agent { id } => crate::agents::delivery_view(id),
            Party::Tile { id } => Some(tile_view(id)),
            Party::User | Party::Brain => None,
        }
    }
    fn input(&self, tile: &str) -> ClientInput { crate::client_input(tile) }
    fn now(&self) -> u64 { crate::agents::unix_ms() }
    fn bracketed(&self, tile: &str) -> bool { crate::tile_bracketed_paste(tile) }
    fn prefix(&self, party: &Party) -> Option<String> {
        party.agent_id().and_then(crate::agents::pending_block_for_paste)
    }
    fn prefix_delivered(&self, party: &Party) {
        if let Some(id) = party.agent_id() {
            crate::agents::mark_block_pasted(id);
        }
    }
    fn last_busy(&self, party: &Party) -> Option<u64> { crate::agents::messages::last_busy_ms(&party.key()) }
    fn orch_delivered(&self, run: &str) { crate::orch::note_agent_wake_delivered(run) }
    type Lock = Option<tokio::sync::OwnedMutexGuard<()>>;
    async fn lock(&self, tile: &str) -> Self::Lock { crate::lock_tile_writer(tile).await }
    async fn paste(&self, tile: &str, text: &str) -> Result<Confirm, String> { crate::paste_to_locked_tile(tile, text).await }
    async fn clear(&self, tile: &str) { crate::clear_tile_input(tile).await }
    fn retry_later(&self, party: &Party, delay_ms: u64) {
        if !claim_retry(&party.key()) {
            return;
        }
        let party = party.clone();
        crate::agents::runtime::spawn_background(async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            release_retry(&party.key());
            kick(&party);
        });
    }
}

pub fn settle_for(m: &Message) -> u64 {
    if m.from_brain() { crate::agents::waits::SETTLE_MS } else { 0 }
}

pub fn kick_for(m: &Message) {
    match settle_for(m) {
        0 => kick(&m.to),
        ms => kick_after(&m.to, ms),
    }
}

pub fn kick_after(party: &Party, delay_ms: u64) {
    let party = party.clone();
    crate::agents::runtime::spawn_background(async move {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        kick(&party);
    });
}

pub fn kick(party: &Party) {
    if matches!(party, Party::User | Party::Brain) || !has_mail(party) {
        return;
    }
    let party = party.clone();
    crate::agents::runtime::spawn_background(async move {
        drain(&LiveBackend, &party).await;
    });
}

pub fn has_mail(party: &Party) -> bool {
    with_mailbox(|mb| !mb.queue_for(party).is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::mail::{MsgStatus, tests::msg};

    struct Fake {
        view: Option<View>,
        input: ClientInput,
        now: u64,
        results: Mutex<Vec<Result<Confirm, String>>>,
        pasted: Mutex<Vec<String>>,
        cleared: Mutex<usize>,
        retries: Mutex<Vec<u64>>,
        prefix: Option<String>,
        prefix_marked: Mutex<bool>,
        view_calls: Mutex<u32>,
        busy_after_first_view: bool,
        busy_during_paste: Option<String>,
        drop_during_paste: Option<String>,
        typed_during_paste: Mutex<bool>,
        type_during_paste: bool,
        last_busy: Option<u64>,
        orch_runs: Mutex<Vec<String>>,
    }

    impl Fake {
        fn idle(tile: &str) -> Self {
            Self {
                view: Some(View { tile: tile.into(), live: true, starting: false, status: LiveStatus::Idle, source: StatusSource::Hook }),
                input: ClientInput::default(), now: 1_000_000_000, results: Mutex::new(vec![]), pasted: Mutex::new(vec![]),
                cleared: Mutex::new(0), retries: Mutex::new(vec![]), prefix: None, prefix_marked: Mutex::new(false),
                view_calls: Mutex::new(0), busy_after_first_view: false, busy_during_paste: None, drop_during_paste: None,
                typed_during_paste: Mutex::new(false), type_during_paste: false, last_busy: None,
                orch_runs: Mutex::new(vec![]),
            }
        }

        fn answering(self, results: Vec<Result<Confirm, String>>) -> Self {
            *self.results.lock().unwrap() = results;
            self
        }
    }

    impl DeliveryBackend for Fake {
        fn view(&self, _: &Party) -> Option<View> {
            let mut calls = self.view_calls.lock().unwrap();
            *calls += 1;
            let mut view = self.view.clone()?;
            if self.busy_after_first_view && *calls > 1 {
                view.status = LiveStatus::Busy;
            }
            Some(view)
        }
        fn input(&self, _: &str) -> ClientInput {
            if *self.typed_during_paste.lock().unwrap() {
                return ClientInput { last_ms: self.now + 1, pending: 1 };
            }
            self.input
        }
        fn now(&self) -> u64 { self.now }
        fn bracketed(&self, _: &str) -> bool { true }
        fn prefix(&self, _: &Party) -> Option<String> { self.prefix.clone() }
        fn prefix_delivered(&self, _: &Party) { *self.prefix_marked.lock().unwrap() = true; }
        fn last_busy(&self, _: &Party) -> Option<u64> { self.last_busy }
        fn orch_delivered(&self, run: &str) { self.orch_runs.lock().unwrap().push(run.to_string()); }
        type Lock = ();
        async fn lock(&self, _: &str) {}
        async fn paste(&self, _: &str, text: &str) -> Result<Confirm, String> {
            self.pasted.lock().unwrap().push(text.to_string());
            if let Some(key) = &self.busy_during_paste {
                crate::agents::messages::note_status(key, true, false);
            }
            if let Some(id) = &self.drop_during_paste {
                commit_mail(MailCmd::Drop { id: id.clone(), at: self.now, reason: "recipient_deleted".into() }).unwrap();
            }
            if self.type_during_paste {
                *self.typed_during_paste.lock().unwrap() = true;
            }
            let mut results = self.results.lock().unwrap();
            if results.is_empty() { Ok(Confirm::Submitted) } else { results.remove(0) }
        }
        async fn clear(&self, _: &str) { *self.cleared.lock().unwrap() += 1; }
        fn retry_later(&self, _: &Party, delay_ms: u64) { self.retries.lock().unwrap().push(delay_ms); }
    }

    fn queue(tag: &str, n: usize) -> (Party, Vec<String>) {
        let suffix = format!("{tag}{:07}", crate::agents::unix_ms() % 10_000_000);
        queue_to(Party::Agent { id: format!("ad{suffix}") }, &suffix, n)
    }

    fn queue_to(to: Party, suffix: &str, n: usize) -> (Party, Vec<String>) {
        crate::orch::tests::f12_test_home();
        let ids: Vec<String> = (0..n).map(|i| format!("m{suffix}{i}")).collect();
        for (i, id) in ids.iter().enumerate() {
            let m = Message { hops: (i as u8).min(crate::agents::mail::MAX_HOPS), thread: format!("t{suffix}"), ..msg(id, Party::User, to.clone()) };
            commit_mail(MailCmd::Send { message: m, recipient_exists: true }).unwrap();
        }
        (to, ids)
    }

    fn status_of(id: &str) -> MsgStatus {
        with_mailbox(|mb| mb.messages[id].status)
    }

    #[tokio::test]
    async fn never_delivers_unless_live_and_idle_from_a_known_source() {
        let (to, ids) = queue("a", 1);
        for (status, source, starting, live) in [
            (LiveStatus::Idle, StatusSource::None, false, true),
            (LiveStatus::Busy, StatusSource::Hook, false, true),
            (LiveStatus::Waiting, StatusSource::Hook, false, true),
            (LiveStatus::Idle, StatusSource::Hook, true, true),
            (LiveStatus::Idle, StatusSource::Hook, false, false),
        ] {
            let mut fake = Fake::idle("agent-x");
            fake.view = Some(View { tile: "agent-x".into(), live, starting, status, source });
            assert_eq!(drain(&fake, &to).await, Outcome::NotReady, "{status:?} {source:?} starting={starting} live={live}");
            assert!(fake.pasted.lock().unwrap().is_empty());
        }
        assert_eq!(status_of(&ids[0]), MsgStatus::Queued);
        let mut quiet = Fake::idle("agent-x");
        quiet.view.as_mut().unwrap().source = StatusSource::Quiet;
        assert!(matches!(drain(&quiet, &to).await, Outcome::Delivered(_)));
    }

    #[tokio::test]
    async fn waits_out_typing_and_a_half_typed_line() {
        let (to, ids) = queue("b", 1);
        let mut fake = Fake::idle("agent-x");
        fake.input = ClientInput { last_ms: fake.now - 4_000, pending: 0 };
        assert_eq!(drain(&fake, &to).await, Outcome::Waiting(6_000));
        fake.input = ClientInput { last_ms: fake.now - 30_000, pending: 3 };
        assert_eq!(drain(&fake, &to).await, Outcome::Waiting(RETRY_MS));
        assert!(fake.pasted.lock().unwrap().is_empty());
        assert_eq!(status_of(&ids[0]), MsgStatus::Queued);
        fake.input = ClientInput { last_ms: fake.now - 60_000, pending: 0 };
        assert_eq!(drain(&fake, &to).await, Outcome::Delivered(ids.clone()));
    }

    #[tokio::test]
    async fn batches_in_one_paste_and_records_the_cause() {
        let (to, ids) = queue("c", 7);
        let fake = Fake::idle("agent-x");
        assert_eq!(drain(&fake, &to).await, Outcome::Delivered(ids[..5].to_vec()));
        assert_eq!(fake.pasted.lock().unwrap().len(), 1);
        assert_eq!(fake.pasted.lock().unwrap()[0].lines().count(), 5);
        assert!(ids[..5].iter().all(|id| status_of(id) == MsgStatus::Delivered));
        assert_eq!(status_of(&ids[5]), MsgStatus::Queued);
        let cause = crate::agents::messages::cause_of(&to.key()).unwrap();
        assert_eq!(cause.hops, 4, "the highest hop in the batch");
    }

    #[tokio::test]
    async fn stuck_clears_and_retries_once_then_fails() {
        let (to, ids) = queue("d", 1);
        let fake = Fake::idle("agent-x").answering(vec![Ok(Confirm::Stuck), Ok(Confirm::Stuck)]);
        assert_eq!(drain(&fake, &to).await, Outcome::StuckRetry);
        assert_eq!(status_of(&ids[0]), MsgStatus::Queued);
        assert_eq!(*fake.cleared.lock().unwrap(), 1);
        assert_eq!(drain(&fake, &to).await, Outcome::Failed(ids.clone()));
        assert_eq!(status_of(&ids[0]), MsgStatus::Failed);
        assert_eq!(fake.pasted.lock().unwrap().len(), 2, "pasted at most twice");
    }

    #[tokio::test]
    async fn a_paste_error_keeps_the_message_queued() {
        let (to, ids) = queue("e", 1);
        let fake = Fake::idle("agent-x").answering(vec![Err("no session".into())]);
        assert_eq!(drain(&fake, &to).await, Outcome::Error);
        assert_eq!(status_of(&ids[0]), MsgStatus::Queued);
        assert_eq!(*fake.retries.lock().unwrap(), vec![RETRY_MS]);
    }

    #[tokio::test]
    async fn concurrent_drains_are_single_flight() {
        let (to, _) = queue("f", 1);
        let held = Flight::take(&to).unwrap();
        let fake = Fake::idle("agent-x");
        assert_eq!(drain(&fake, &to).await, Outcome::Busy);
        drop(held);
        assert!(matches!(drain(&fake, &to).await, Outcome::Delivered(_)));
    }

    #[tokio::test]
    async fn a_pending_block_goes_in_front_of_the_batch() {
        let (to, _) = queue("g", 1);
        let fake = Fake { prefix: Some("[panorama agent] roster".into()), ..Fake::idle("agent-x") };
        assert!(matches!(drain(&fake, &to).await, Outcome::Delivered(_)));
        assert!(fake.pasted.lock().unwrap()[0].starts_with("[panorama agent] roster\n[panorama msg"));
        assert!(*fake.prefix_marked.lock().unwrap());
    }

    #[tokio::test]
    async fn rechecks_readiness_after_taking_the_writer_lock() {
        let (to, ids) = queue("h", 1);
        let fake = Fake { busy_after_first_view: true, ..Fake::idle("agent-x") };
        assert_eq!(drain(&fake, &to).await, Outcome::NotReady);
        assert!(fake.pasted.lock().unwrap().is_empty());
        assert_eq!(status_of(&ids[0]), MsgStatus::Queued);
    }

    #[tokio::test]
    async fn a_busy_reported_during_the_paste_still_ends_the_cause() {
        let (to, _) = queue("i", 1);
        let fake = Fake { busy_during_paste: Some(to.key()), ..Fake::idle("agent-x") };
        assert!(matches!(drain(&fake, &to).await, Outcome::Delivered(_)));
        crate::agents::messages::note_status(&to.key(), false, true);
        assert!(crate::agents::messages::cause_of(&to.key()).is_none(), "the turn that carried the mail is over");
    }

    #[tokio::test]
    async fn a_failed_paste_keeps_the_previous_cause() {
        let (to, _) = queue("j", 1);
        crate::agents::messages::set_cause(&to.key(), "older", 1);
        let fake = Fake::idle("agent-x").answering(vec![Err("gone".into())]);
        assert_eq!(drain(&fake, &to).await, Outcome::Error);
        assert_eq!(crate::agents::messages::cause_of(&to.key()).map(|c| c.thread).as_deref(), Some("older"));
    }

    #[tokio::test]
    async fn a_plain_tile_that_receives_mail_inherits_its_thread() {
        let suffix = format!("k{:07}", crate::agents::unix_ms() % 10_000_000);
        let tile = Party::Tile { id: format!("tile-{suffix}") };
        let (to, ids) = queue_to(tile.clone(), &suffix, 2);
        assert!(matches!(drain(&Fake::idle("tile-x"), &to).await, Outcome::Delivered(_)));
        let thread = with_mailbox(|mb| mb.messages[&ids[1]].thread.clone());
        assert_eq!(crate::agents::messages::inherited_thread(&tile), Some((thread, 2)));
    }

    #[tokio::test]
    async fn delivery_commits_the_messages_still_queued_when_one_was_dropped() {
        let (to, ids) = queue("l", 2);
        let fake = Fake { drop_during_paste: Some(ids[0].clone()), ..Fake::idle("agent-x") };
        assert!(matches!(drain(&fake, &to).await, Outcome::Delivered(_)));
        assert_eq!(status_of(&ids[0]), MsgStatus::Dropped);
        assert_eq!(status_of(&ids[1]), MsgStatus::Delivered);
    }

    #[tokio::test]
    async fn stuck_cleanup_never_erases_text_the_user_typed_meanwhile() {
        let (to, ids) = queue("n", 1);
        let fake = Fake { type_during_paste: true, ..Fake::idle("agent-x") }.answering(vec![Ok(Confirm::Stuck)]);
        drain(&fake, &to).await;
        assert_eq!(*fake.cleared.lock().unwrap(), 0);
        assert_eq!(status_of(&ids[0]), MsgStatus::Queued);
    }

    #[tokio::test]
    async fn an_unconfirmed_paste_is_retried_once_then_failed_as_unconfirmed() {
        let (to, ids) = queue("o", 1);
        let fake = Fake::idle("agent-x").answering(vec![Ok(Confirm::Unconfirmed), Ok(Confirm::Unconfirmed)]);
        assert_eq!(drain(&fake, &to).await, Outcome::StuckRetry);
        assert_eq!(drain(&fake, &to).await, Outcome::Failed(ids.clone()));
        assert_eq!(with_mailbox(|mb| mb.messages[&ids[0]].reason.clone()).as_deref(), Some("unconfirmed"));
    }

    #[tokio::test]
    async fn an_unconfirmed_paste_followed_by_a_turn_is_not_pasted_again() {
        let (to, ids) = queue("p", 1);
        let first = Fake::idle("agent-x").answering(vec![Ok(Confirm::Unconfirmed)]);
        assert_eq!(drain(&first, &to).await, Outcome::StuckRetry);
        let later = Fake { last_busy: Some(first.now + 3_500), ..Fake::idle("agent-x") };
        assert_eq!(drain(&later, &to).await, Outcome::Delivered(ids.clone()));
        assert!(later.pasted.lock().unwrap().is_empty(), "the late busy proves the first paste was submitted");
        assert_eq!(status_of(&ids[0]), MsgStatus::Delivered);
    }

    #[tokio::test]
    async fn a_turn_after_the_user_typed_proves_nothing() {
        let (to, _) = queue("q", 1);
        let first = Fake::idle("agent-x").answering(vec![Ok(Confirm::Unconfirmed)]);
        assert_eq!(drain(&first, &to).await, Outcome::StuckRetry);
        let mut later = Fake { last_busy: Some(first.now + 3_500), ..Fake::idle("agent-x") };
        later.now += 20_000;
        later.input = ClientInput { last_ms: first.now + 1_000, pending: 0 };
        assert!(matches!(drain(&later, &to).await, Outcome::Delivered(_)));
        assert_eq!(later.pasted.lock().unwrap().len(), 1, "the user's own prompt could explain the turn, so it pastes again");

        let (to, _) = queue("r", 1);
        let stuck = Fake::idle("agent-x").answering(vec![Ok(Confirm::Stuck)]);
        assert_eq!(drain(&stuck, &to).await, Outcome::StuckRetry);
        let later = Fake { last_busy: Some(stuck.now + 3_500), ..Fake::idle("agent-x") };
        drain(&later, &to).await;
        assert_eq!(later.pasted.lock().unwrap().len(), 1, "a paste still visible after Enter was not submitted");
    }

    #[test]
    fn a_half_typed_line_stops_blocking_after_a_minute() {
        let view = View { tile: "t".into(), live: true, starting: false, status: LiveStatus::Idle, source: StatusSource::Hook };
        let now = 10_000_000;
        assert_eq!(readiness(&view, ClientInput { last_ms: now - 30_000, pending: 3 }, now), Readiness::WaitTyping(RETRY_MS));
        assert_eq!(readiness(&view, ClientInput { last_ms: now - PENDING_STALE_MS - 1, pending: 3 }, now), Readiness::Ready);
    }

    #[test]
    fn one_retry_timer_per_party() {
        let key = format!("agent:retry{}", crate::agents::unix_ms());
        assert!(claim_retry(&key));
        assert!(!claim_retry(&key), "a second kick does not start another timer");
        release_retry(&key);
        assert!(claim_retry(&key));
        release_retry(&key);
    }

    #[tokio::test]
    async fn delivering_an_orch_wake_records_the_coordinator_wake() {
        let (to, _) = queue("w", 1);
        let suffix = format!("{:07}", crate::agents::unix_ms() % 10_000_000);
        let orch = Message {
            kind: Some(crate::agents::mail::KIND_ORCH.into()), key: Some(format!("orch:x:rwake{suffix}#3")),
            ..msg(&format!("owake{suffix}"), Party::Brain, to.clone())
        };
        commit_mail(MailCmd::Send { message: orch, recipient_exists: true }).unwrap();
        let fake = Fake::idle("agent-x");
        assert!(matches!(drain(&fake, &to).await, Outcome::Delivered(_)));
        assert_eq!(*fake.orch_runs.lock().unwrap(), vec![format!("rwake{suffix}")]);
        let (to, _) = queue("v", 1);
        let failed = Fake::idle("agent-x").answering(vec![Err("gone".into())]);
        drain(&failed, &to).await;
        assert!(failed.orch_runs.lock().unwrap().is_empty());
    }

    #[test]
    fn a_plain_tile_without_hook_status_uses_the_quiet_detector() {
        let tile = format!("tile-quiet-{}", crate::agents::unix_ms());
        assert_eq!(tile_view(&tile).source, StatusSource::None);
        crate::agents::runtime::tests::set_quiet(&tile, LiveStatus::Idle);
        let view = tile_view(&tile);
        assert_eq!((view.status, view.source), (LiveStatus::Idle, StatusSource::Quiet));
    }

    #[test]
    fn brain_messages_settle_before_delivery_and_mail_does_not() {
        let event = Message { kind: Some(crate::agents::mail::KIND_EVENT.into()), ..msg("e1", Party::Brain, Party::User) };
        assert_eq!(settle_for(&event), crate::agents::waits::SETTLE_MS);
        assert_eq!(settle_for(&msg("m1", Party::User, Party::User)), 0);
    }

    #[test]
    fn user_mail_is_never_pasted() {
        assert!(LiveBackend.view(&Party::User).is_none());
    }

    #[tokio::test]
    async fn mail_waits_during_a_rollover_and_follows_the_handoff_in_order() {
        let (to, ids) = queue("r", 3);
        let id = to.agent_id().unwrap().to_string();
        let mut agent = crate::agents::reducer::tests::agent(&id, &format!("r-{id}"));
        agent.rollover.open = Some(crate::agents::model::Rollover { n: 1, reason: "user".into(), phase: "open".into(), note: true, handoff: true, ..Default::default() });
        crate::agents::lock().agents.insert(id.clone(), agent);
        let brain = |mid: &str, kind: &str| Message { kind: Some(kind.into()), from_name: "panorama".into(), ..msg(mid, Party::Brain, to.clone()) };
        let prompt = format!("r{id}-1");
        commit_mail(MailCmd::Send { message: brain(&prompt, crate::agents::mail::KIND_ROLLOVER), recipient_exists: true }).unwrap();
        let fake = Fake::idle("agent-x");
        assert_eq!(drain(&fake, &to).await, Outcome::Delivered(vec![prompt.clone()]), "only the rollover prompt passes the pause");
        assert_eq!(drain(&fake, &to).await, Outcome::Nothing);
        assert!(ids.iter().all(|m| status_of(m) == MsgStatus::Queued));
        let handoff = format!("h{id}-1");
        commit_mail(MailCmd::Send { message: brain(&handoff, crate::agents::mail::KIND_HANDOFF), recipient_exists: true }).unwrap();
        crate::agents::lock().agents.get_mut(&id).unwrap().rollover.open = None;
        let Outcome::Delivered(sent) = drain(&fake, &to).await else { panic!() };
        crate::agents::lock().agents.remove(&id);
        let mut expected = vec![handoff];
        expected.extend(ids.iter().cloned());
        assert_eq!(sent, expected, "the handoff first, then the queued mail in order, each once");
        assert!(fake.pasted.lock().unwrap()[1].starts_with("[panorama handoff] "), "{:?}", fake.pasted.lock().unwrap());
    }
}