use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::agents::envelope;
use crate::agents::mail::{MailCmd, Message, Party};
use crate::agents::messages::{commit_mail, set_cause, with_mailbox};
use crate::agents::model::{LiveStatus, StatusSource};
use crate::{ClientInput, Confirm};

pub const TYPING_QUIET_MS: u64 = 10_000;
pub const RETRY_MS: u64 = 5_000;

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
    if input.pending > 0 {
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
    async fn paste(&self, tile: &str, text: &str) -> Result<Confirm, String>;
    async fn clear(&self, tile: &str);
    fn retry_later(&self, party: &Party, delay_ms: u64);
}

fn in_flight() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

fn stuck_once() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(Default::default)
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
    let queue: Vec<Message> = with_mailbox(|mb| mb.queue_for(party).into_iter().cloned().collect());
    if queue.is_empty() {
        return Outcome::Nothing;
    }
    let Some(view) = backend.view(party) else { return Outcome::NotReady };
    match readiness(&view, backend.input(&view.tile), backend.now()) {
        Readiness::NotIdle => return Outcome::NotReady,
        Readiness::WaitTyping(ms) => {
            backend.retry_later(party, ms);
            return Outcome::Waiting(ms);
        }
        Readiness::Ready => {}
    }
    let bracketed = backend.bracketed(&view.tile);
    let prefix = backend.prefix(party);
    let refs: Vec<&Message> = queue.iter().collect();
    let Some(batch) = envelope::batch(&refs, &crate::panorama_cli_path(), bracketed, prefix.as_deref()) else {
        return Outcome::Nothing;
    };
    match backend.paste(&view.tile, &batch.text).await {
        Ok(Confirm::Submitted) => {
            if let Err(e) = commit_mail(MailCmd::Deliver { ids: batch.ids.clone(), at: backend.now() }) {
                eprintln!("[agents] delivery of {:?} not recorded: {}", batch.ids, e.message);
            }
            if prefix.is_some() {
                backend.prefix_delivered(party);
            }
            let delivered = queue.iter().filter(|m| batch.ids.contains(&m.id));
            if let (Some(agent), Some(cause)) = (party.agent_id(), delivered.max_by_key(|m| m.hops)) {
                set_cause(agent, &cause.thread, cause.hops);
            }
            let mut stuck = guard(stuck_once());
            for id in &batch.ids {
                stuck.remove(id);
            }
            Outcome::Delivered(batch.ids)
        }
        Ok(Confirm::Stuck) => {
            let second = {
                let stuck = guard(stuck_once());
                batch.ids.iter().all(|id| stuck.contains(id))
            };
            backend.clear(&view.tile).await;
            if second {
                for id in &batch.ids {
                    guard(stuck_once()).remove(id);
                    if let Err(e) = commit_mail(MailCmd::Fail { id: id.clone(), at: backend.now(), reason: "stuck".into() }) {
                        eprintln!("[agents] fail {id}: {}", e.message);
                    }
                }
                return Outcome::Failed(batch.ids);
            }
            guard(stuck_once()).extend(batch.ids.iter().cloned());
            backend.retry_later(party, RETRY_MS);
            Outcome::StuckRetry
        }
        Err(_) => {
            backend.retry_later(party, RETRY_MS);
            Outcome::Error
        }
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
        _ => (LiveStatus::Idle, StatusSource::None),
    };
    View { tile: tile.to_string(), live: session.is_some(), starting: false, status, source }
}

impl DeliveryBackend for LiveBackend {
    fn view(&self, party: &Party) -> Option<View> {
        match party {
            Party::Agent { id } => crate::agents::delivery_view(id),
            Party::Tile { id } => Some(tile_view(id)),
            Party::User => None,
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
    async fn paste(&self, tile: &str, text: &str) -> Result<Confirm, String> { crate::send_to_tile_confirmed(tile, text).await }
    async fn clear(&self, tile: &str) { crate::clear_tile_input(tile).await }
    fn retry_later(&self, party: &Party, delay_ms: u64) {
        let party = party.clone();
        crate::agents::runtime::spawn_background(async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            kick(&party);
        });
    }
}

pub fn kick(party: &Party) {
    if *party == Party::User || !has_mail(party) {
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
    }

    impl Fake {
        fn idle(tile: &str) -> Self {
            Self {
                view: Some(View { tile: tile.into(), live: true, starting: false, status: LiveStatus::Idle, source: StatusSource::Hook }),
                input: ClientInput::default(), now: 1_000_000_000, results: Mutex::new(vec![]), pasted: Mutex::new(vec![]),
                cleared: Mutex::new(0), retries: Mutex::new(vec![]), prefix: None, prefix_marked: Mutex::new(false),
            }
        }

        fn answering(self, results: Vec<Result<Confirm, String>>) -> Self {
            *self.results.lock().unwrap() = results;
            self
        }
    }

    impl DeliveryBackend for Fake {
        fn view(&self, _: &Party) -> Option<View> { self.view.clone() }
        fn input(&self, _: &str) -> ClientInput { self.input }
        fn now(&self) -> u64 { self.now }
        fn bracketed(&self, _: &str) -> bool { true }
        fn prefix(&self, _: &Party) -> Option<String> { self.prefix.clone() }
        fn prefix_delivered(&self, _: &Party) { *self.prefix_marked.lock().unwrap() = true; }
        async fn paste(&self, _: &str, text: &str) -> Result<Confirm, String> {
            self.pasted.lock().unwrap().push(text.to_string());
            let mut results = self.results.lock().unwrap();
            if results.is_empty() { Ok(Confirm::Submitted) } else { results.remove(0) }
        }
        async fn clear(&self, _: &str) { *self.cleared.lock().unwrap() += 1; }
        fn retry_later(&self, _: &Party, delay_ms: u64) { self.retries.lock().unwrap().push(delay_ms); }
    }

    fn queue(tag: &str, n: usize) -> (Party, Vec<String>) {
        crate::orch::tests::f12_test_home();
        let suffix = format!("{tag}{:07}", crate::agents::unix_ms() % 10_000_000);
        let to = Party::Agent { id: format!("ad{suffix}") };
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
        fake.input = ClientInput { last_ms: fake.now - 60_000, pending: 3 };
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
        let cause = crate::agents::messages::cause_of(to.agent_id().unwrap()).unwrap();
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

    #[test]
    fn user_mail_is_never_pasted() {
        assert!(LiveBackend.view(&Party::User).is_none());
    }
}
