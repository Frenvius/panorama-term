mod agy;
mod cli_login;
mod jwt;
mod links;
mod keyring_raw;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use agy::Pending;
use keyring_raw::{OsSecrets, SecretIo};
use links::Sources;

const AGENTS: &[&str] = &["antigravity", "claude", "codex"];
const SYSTEM: &str = "system";

#[derive(Debug)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

fn fail(code: &'static str, message: impl Into<String>) -> ApiError {
    ApiError { code, message: message.into() }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AccountRec {
    pub agent: String,
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct Account {
    pub agent: String,
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    pub system: bool,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
}

impl AccountRec {
    pub fn into_account(self) -> Account {
        Account {
            agent: self.agent,
            id: self.id,
            label: self.label,
            email: self.email,
            system: false,
            created_at: self.created_at,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Defaults {
    pub claude: String,
    pub codex: String,
}

impl Default for Defaults {
    fn default() -> Self {
        Self { claude: SYSTEM.into(), codex: SYSTEM.into() }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct TileSel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub accounts: Vec<AccountRec>,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub tiles: BTreeMap<String, TileSel>,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn new_id() -> String {
    let mut bytes = [0u8; 8];
    if getrandom::getrandom(&mut bytes).is_err() {
        let n = now_ms();
        bytes.iter_mut().enumerate().for_each(|(i, b)| *b = (n >> (i * 5)) as u8);
    }
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    bytes.iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect()
}

fn store_path(root: &Path) -> PathBuf {
    root.join("accounts.json")
}

fn load_store(root: &Path) -> Store {
    std::fs::read_to_string(store_path(root))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_store(root: &Path, store: &Store) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| format!("create dir: {e}"))?;
    let json = serde_json::to_string_pretty(store).map_err(|e| format!("serialize: {e}"))?;
    crate::write_atomic(store_path(root), json.as_bytes()).map_err(|e| format!("write store: {e}"))
}

fn valid_agent(agent: &str) -> bool {
    AGENTS.contains(&agent)
}

fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

pub struct Service {
    pub(super) root: PathBuf,
    pub(super) io: Arc<dyn SecretIo>,
    pub(super) store: Mutex<Store>,
    pub(super) pending: Mutex<HashMap<String, Pending>>,
    pub(super) settings_path: Option<PathBuf>,
    pub(super) sources: Sources,
    pub(super) poll_every: Duration,
    pub(super) poll_timeout: Duration,
}

impl Service {
    pub fn new(root: PathBuf, io: Arc<dyn SecretIo>, settings_path: Option<PathBuf>) -> Self {
        let store = load_store(&root);
        Self {
            root,
            io,
            store: Mutex::new(store),
            pending: Mutex::new(HashMap::new()),
            settings_path,
            sources: Sources::from_env(),
            poll_every: Duration::from_secs(2),
            poll_timeout: Duration::from_secs(600),
        }
    }

    fn mutate<T>(&self, f: impl FnOnce(&mut Store) -> Result<T, ApiError>) -> Result<T, ApiError> {
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = store.clone();
        let out = f(&mut next)?;
        save_store(&self.root, &next).map_err(|e| fail("store", e))?;
        *store = next;
        Ok(out)
    }

    pub fn list(&self) -> Value {
        let store = self.store.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut accounts: Vec<Account> = ["claude", "codex"]
            .iter()
            .map(|agent| Account {
                agent: (*agent).into(),
                id: SYSTEM.into(),
                label: "System".into(),
                email: system_email(agent),
                system: true,
                created_at: 0,
            })
            .collect();
        accounts.extend(store.accounts.into_iter().map(AccountRec::into_account));
        json!({ "accounts": accounts, "defaults": store.defaults, "liveAgy": self.live_agy() })
    }

    fn find_idx(store: &Store, agent: &str, id: &str) -> Option<usize> {
        store.accounts.iter().position(|a| a.agent == agent && a.id == id)
    }

    fn selectable(store: &Store, agent: &str, id: &str) -> bool {
        (id == SYSTEM && agent != "antigravity") || Self::find_idx(store, agent, id).is_some()
    }

    pub fn remove(&self, agent: &str, id: &str) -> Result<(), ApiError> {
        if id == SYSTEM {
            return Err(fail("forbidden", "the system account cannot be removed"));
        }
        if !valid_id(id) {
            return Err(fail("invalid", "invalid account id"));
        }
        self.mutate(|store| {
            let idx = Self::find_idx(store, agent, id).ok_or_else(|| fail("not_found", "account not found"))?;
            store.accounts.remove(idx);
            if store.defaults.claude == id && agent == "claude" {
                store.defaults.claude = SYSTEM.into();
            }
            if store.defaults.codex == id && agent == "codex" {
                store.defaults.codex = SYSTEM.into();
            }
            for sel in store.tiles.values_mut() {
                if agent == "claude" && sel.claude.as_deref() == Some(id) {
                    sel.claude = None;
                }
                if agent == "codex" && sel.codex.as_deref() == Some(id) {
                    sel.codex = None;
                }
            }
            Ok(())
        })?;
        match agent {
            "antigravity" => self.io.snapshot_delete(id),
            _ => {
                links::remove_account_dir(&self.account_dir(agent, id));
            }
        }
        Ok(())
    }

    pub fn rename(&self, agent: &str, id: &str, label: &str) -> Result<(), ApiError> {
        let label = label.trim();
        if label.is_empty() {
            return Err(fail("invalid", "label is empty"));
        }
        self.mutate(|store| {
            let idx = Self::find_idx(store, agent, id).ok_or_else(|| fail("not_found", "account not found"))?;
            store.accounts[idx].label = label.to_string();
            Ok(())
        })
    }

    pub fn set_default(&self, agent: &str, id: &str) -> Result<(), ApiError> {
        self.mutate(|store| {
            if !Self::selectable(store, agent, id) {
                return Err(fail("not_found", "account not found"));
            }
            match agent {
                "claude" => store.defaults.claude = id.into(),
                "codex" => store.defaults.codex = id.into(),
                _ => return Err(fail("invalid", "agent has no default")),
            }
            Ok(())
        })
    }

    pub fn tile_get(&self, tile_id: &str) -> Value {
        let (claude, codex) = self.effective_ids(tile_id);
        json!({ "claude": claude, "codex": codex })
    }

    pub fn tile_set(&self, tile_id: &str, agent: &str, id: &str) -> Result<(), ApiError> {
        if tile_id.is_empty() {
            return Err(fail("invalid", "tileId is empty"));
        }
        self.mutate(|store| {
            if !Self::selectable(store, agent, id) {
                return Err(fail("not_found", "account not found"));
            }
            let sel = store.tiles.entry(tile_id.to_string()).or_default();
            match agent {
                "claude" => sel.claude = Some(id.into()),
                "codex" => sel.codex = Some(id.into()),
                _ => return Err(fail("invalid", "agent has no per-tile account")),
            }
            Ok(())
        })
    }

    pub fn dispatch(self: &Arc<Self>, cmd: &str, body: &[u8]) -> Result<Value, ApiError> {
        let req: Value = if body.is_empty() {
            json!({})
        } else {
            serde_json::from_slice(body).map_err(|_| fail("invalid", "body is not json"))?
        };
        let s = |key: &str| -> Result<&str, ApiError> {
            req.get(key)
                .and_then(Value::as_str)
                .ok_or_else(|| fail("invalid", format!("missing {key}")))
        };
        let agent = || -> Result<&str, ApiError> {
            let a = s("agent")?;
            if valid_agent(a) { Ok(a) } else { Err(fail("invalid", "unknown agent")) }
        };
        let id = || -> Result<&str, ApiError> {
            let id = s("id")?;
            if id == SYSTEM || valid_id(id) { Ok(id) } else { Err(fail("invalid", "invalid account id")) }
        };
        match cmd {
            "list" => Ok(self.list()),
            "add-start" => {
                let tile_id = s("tileId")?;
                match agent()? {
                    "antigravity" => {
                        let (pending_id, command) = self.agy_add_start()?;
                        self.spawn_poll(pending_id.clone());
                        Ok(json!({ "pendingId": pending_id, "command": command }))
                    }
                    cli => {
                        let (pending_id, command) = self.cli_add_start(cli, tile_id)?;
                        self.spawn_poll(pending_id.clone());
                        Ok(json!({ "pendingId": pending_id, "command": command }))
                    }
                }
            }
            "add-status" => Ok(self.agy_add_status(s("pendingId")?)),
            "add-cancel" => {
                self.agy_add_cancel(s("pendingId")?);
                Ok(json!({}))
            }
            "remove" => self.remove(agent()?, id()?).map(|_| json!({})),
            "rename" => self.rename(agent()?, id()?, s("label")?).map(|_| json!({})),
            "set-default" => self.set_default(agent()?, id()?).map(|_| json!({})),
            "activate" => match agent()? {
                "antigravity" => self.agy_activate(id()?),
                _ => Err(fail("unsupported", "only antigravity supports activate")),
            },
            "tile-get" => Ok(self.tile_get(s("tileId")?)),
            "tile-set" => self.tile_set(s("tileId")?, agent()?, id()?).map(|_| json!({})),
            _ => Err(fail("unknown_command", format!("unknown accounts command: {cmd}"))),
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

fn system_email(agent: &str) -> Option<String> {
    match agent {
        "claude" => {
            let path = match std::env::var_os("CLAUDE_CONFIG_DIR") {
                Some(dir) => PathBuf::from(dir).join(".claude.json"),
                None => home_dir()?.join(".claude.json"),
            };
            let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
            v.pointer("/oauthAccount/emailAddress")?.as_str().map(str::to_string)
        }
        "codex" => {
            let dir = match std::env::var_os("CODEX_HOME") {
                Some(dir) => PathBuf::from(dir),
                None => home_dir()?.join(".codex"),
            };
            let v: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("auth.json")).ok()?).ok()?;
            let claims = jwt::jwt_claims(v.pointer("/tokens/id_token")?.as_str()?)?;
            claims.get("email")?.as_str().map(str::to_string)
        }
        _ => None,
    }
}

pub fn tile_env(tile_id: &str) -> Vec<(String, String)> {
    service().map(|s| s.tile_env(tile_id)).unwrap_or_default()
}

pub fn claude_dir_for_tile(tile_id: &str) -> Option<PathBuf> {
    service().ok()?.claude_dir_for_tile(tile_id)
}

fn service() -> Result<&'static Arc<Service>, ApiError> {
    static SERVICE: OnceLock<Arc<Service>> = OnceLock::new();
    if let Some(s) = SERVICE.get() {
        return Ok(s);
    }
    let root = crate::panorama_dir()
        .ok_or_else(|| fail("no_home", "home directory not found"))?
        .join("accounts");
    let svc = Arc::new(Service::new(
        root.clone(),
        Arc::new(OsSecrets::new(root)),
        crate::orch::agy::settings_path(),
    ));
    Ok(SERVICE.get_or_init(|| svc))
}

fn reply(result: Result<Value, ApiError>) -> String {
    match result {
        Ok(mut v) => {
            if let Some(obj) = v.as_object_mut() {
                obj.insert("ok".into(), Value::Bool(true));
            }
            v.to_string()
        }
        Err(e) => json!({ "ok": false, "error": { "code": e.code, "message": e.message } }).to_string(),
    }
}

pub async fn handle(cmd: &str, body: &[u8]) -> String {
    let cmd = cmd.to_string();
    let body = body.to_vec();
    let joined = tokio::task::spawn_blocking(move || service().and_then(|s| s.dispatch(&cmd, &body))).await;
    reply(joined.unwrap_or_else(|_| Err(fail("internal", "accounts handler failed"))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeIo {
        live: Mutex<Option<Vec<u8>>>,
        snaps: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SecretIo for FakeIo {
        fn live_get(&self) -> Result<Option<Vec<u8>>, String> {
            Ok(self.live.lock().unwrap().clone())
        }
        fn live_set(&self, blob: &[u8]) -> Result<(), String> {
            *self.live.lock().unwrap() = Some(blob.to_vec());
            Ok(())
        }
        fn live_delete(&self) -> Result<(), String> {
            *self.live.lock().unwrap() = None;
            Ok(())
        }
        fn snapshot_get(&self, id: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(self.snaps.lock().unwrap().get(id).cloned())
        }
        fn snapshot_set(&self, id: &str, blob: &[u8]) -> Result<(), String> {
            self.snaps.lock().unwrap().insert(id.into(), blob.to_vec());
            Ok(())
        }
        fn snapshot_delete(&self, id: &str) {
            self.snaps.lock().unwrap().remove(id);
        }
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("accounts-test-{tag}-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn setup(tag: &str) -> (Service, Arc<FakeIo>, PathBuf) {
        let dir = tmp_dir(tag);
        let io = Arc::new(FakeIo::default());
        let svc = Service::new(dir.join("accounts"), io.clone(), Some(dir.join("settings.json")));
        (svc, io, dir)
    }

    fn cred(email: &str, token: &str) -> Vec<u8> {
        json!({ "email": email, "refresh_token": token }).to_string().into_bytes()
    }

    fn add_rec(svc: &Service, agent: &str, id: &str) {
        svc.mutate(|s| {
            s.accounts.push(AccountRec {
                agent: agent.into(),
                id: id.into(),
                label: id.into(),
                email: None,
                created_at: 1,
                identity: None,
            });
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn store_round_trip() {
        let (svc, _io, dir) = setup("store");
        add_rec(&svc, "claude", "abc12345");
        svc.rename("claude", "abc12345", "Work").unwrap();
        svc.set_default("claude", "abc12345").unwrap();
        svc.tile_set("t1", "codex", "system").unwrap();
        svc.tile_set("t1", "claude", "abc12345").unwrap();

        let reloaded = Service::new(dir.join("accounts"), Arc::new(FakeIo::default()), None);
        let store = reloaded.store.lock().unwrap().clone();
        assert_eq!(store.accounts[0].label, "Work");
        assert_eq!(store.defaults.claude, "abc12345");
        assert_eq!(store.tiles["t1"].claude.as_deref(), Some("abc12345"));

        svc.remove("claude", "abc12345").unwrap();
        let store = svc.store.lock().unwrap().clone();
        assert!(store.accounts.is_empty());
        assert_eq!(store.defaults.claude, "system");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tile_get_falls_back_to_defaults() {
        let (svc, _io, dir) = setup("tile");
        add_rec(&svc, "codex", "cx000001");
        svc.set_default("codex", "cx000001").unwrap();
        assert_eq!(svc.tile_get("unknown"), json!({"claude": "system", "codex": "cx000001"}));
        svc.tile_set("t2", "codex", "system").unwrap();
        assert_eq!(svc.tile_get("t2"), json!({"claude": "system", "codex": "system"}));
        svc.tile_set("t3", "codex", "cx000001").unwrap();
        svc.remove("codex", "cx000001").unwrap();
        assert_eq!(svc.tile_get("t3"), json!({"claude": "system", "codex": "system"}));
        assert!(svc.tile_set("t3", "codex", "missing").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn system_account_is_protected() {
        let (svc, _io, dir) = setup("sys");
        assert_eq!(svc.remove("claude", "system").unwrap_err().code, "forbidden");
        assert!(svc.set_default("antigravity", "system").is_err());
        let list = svc.list();
        let accounts = list["accounts"].as_array().unwrap();
        assert!(accounts.iter().any(|a| a["agent"] == "claude" && a["id"] == "system" && a["system"] == true));
        assert!(accounts.iter().all(|a| a["agent"] != "antigravity"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn agy_add_flow_saves_account_and_restores_previous() {
        let (svc, io, dir) = setup("addflow");
        let svc = Arc::new(svc);
        io.live_set(&cred("old@x.com", "r1")).unwrap();
        let (pid, command) = svc.agy_add_start().unwrap();
        assert_eq!(command, "agy");
        assert!(io.live_get().unwrap().is_none());
        assert!(svc.poll_step(&pid, false));
        assert_eq!(svc.agy_add_status(&pid)["state"], "waiting");

        io.live_set(&cred("New@X.com", "r2")).unwrap();
        assert!(!svc.poll_step(&pid, false));
        let status = svc.agy_add_status(&pid);
        assert_eq!(status["state"], "done");
        assert_eq!(status["account"]["email"], "new@x.com");
        assert_eq!(svc.store.lock().unwrap().accounts.len(), 1);
        let live = io.live_get().unwrap().unwrap();
        assert_eq!(agy::identity(&live).as_deref(), Some("old@x.com"));
        assert_eq!(svc.live_agy(), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn agy_add_existing_identity_updates_snapshot() {
        let (svc, io, dir) = setup("addexisting");
        io.live_set(&cred("a@x.com", "r1")).unwrap();
        let (pid, _) = svc.agy_add_start().unwrap();
        io.live_set(&cred("a@x.com", "r2")).unwrap();
        svc.poll_step(&pid, false);
        let id = svc.store.lock().unwrap().accounts[0].id.clone();

        let (pid2, _) = svc.agy_add_start().unwrap();
        io.live_set(&cred("a@x.com", "r3")).unwrap();
        svc.poll_step(&pid2, false);
        assert_eq!(svc.store.lock().unwrap().accounts.len(), 1);
        let snap = io.snapshot_get(&id).unwrap().unwrap();
        assert!(String::from_utf8(snap).unwrap().contains("r3"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn agy_add_cancel_and_timeout_restore_previous() {
        let (svc, io, dir) = setup("cancel");
        io.live_set(&cred("keep@x.com", "r1")).unwrap();
        let (pid, _) = svc.agy_add_start().unwrap();
        svc.agy_add_cancel(&pid);
        assert_eq!(agy::identity(&io.live_get().unwrap().unwrap()).as_deref(), Some("keep@x.com"));

        let (pid, _) = svc.agy_add_start().unwrap();
        assert!(!svc.poll_step(&pid, true));
        assert_eq!(svc.agy_add_status(&pid)["state"], "failed");
        assert_eq!(agy::identity(&io.live_get().unwrap().unwrap()).as_deref(), Some("keep@x.com"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn agy_activate_syncs_live_and_strips_model_provider() {
        let (svc, io, dir) = setup("activate");
        io.live_set(&cred("a@x.com", "a1")).unwrap();
        let (pid, _) = svc.agy_add_start().unwrap();
        io.live_set(&cred("a@x.com", "a1")).unwrap();
        svc.poll_step(&pid, false);
        io.live_set(&cred("b@x.com", "b1")).unwrap();
        let (pid, _) = svc.agy_add_start().unwrap();
        io.live_set(&cred("b@x.com", "b1")).unwrap();
        svc.poll_step(&pid, false);
        let (a, b) = {
            let s = svc.store.lock().unwrap();
            let id = |e: &str| s.accounts.iter().find(|x| x.email.as_deref() == Some(e)).unwrap().id.clone();
            (id("a@x.com"), id("b@x.com"))
        };

        std::fs::write(
            dir.join("settings.json"),
            r#"{"modelProvider":"x","trustedWorkspaces":["/w"]}"#,
        )
        .unwrap();
        io.live_set(&cred("a@x.com", "a2-rotated")).unwrap();
        let out = svc.agy_activate(&b).unwrap();
        assert!(out["tiles"].is_array());
        assert_eq!(svc.live_agy().as_deref(), Some(b.as_str()));
        let snap_a = String::from_utf8(io.snapshot_get(&a).unwrap().unwrap()).unwrap();
        assert!(snap_a.contains("a2-rotated"));
        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert!(settings.get("modelProvider").is_none());
        assert_eq!(settings["trustedWorkspaces"][0], "/w");
        assert_eq!(svc.agy_activate("nope").unwrap_err().code, "not_found");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn strip_model_provider_cases() {
        let dir = tmp_dir("strip");
        let path = dir.join("settings.json");
        assert_eq!(crate::orch::agy::strip_model_provider(&path), Ok(false));
        std::fs::write(&path, r#"{"a":1}"#).unwrap();
        assert_eq!(crate::orch::agy::strip_model_provider(&path), Ok(false));
        std::fs::write(&path, r#"{"a":1,"modelProvider":"p"}"#).unwrap();
        assert_eq!(crate::orch::agy::strip_model_provider(&path), Ok(true));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v, json!({"a": 1}));
        std::fs::remove_dir_all(&dir).ok();
    }

    fn shared_fixture(dir: &Path) -> Sources {
        let claude = dir.join("src-claude");
        std::fs::create_dir_all(claude.join("skills")).unwrap();
        std::fs::write(claude.join("skills").join("a.md"), "skill").unwrap();
        std::fs::write(claude.join("settings.json"), "{}").unwrap();
        std::fs::write(claude.join(".credentials.json"), "secret").unwrap();
        std::fs::write(
            dir.join("src.claude.json"),
            json!({"oauthAccount": {"emailAddress": "a@x.com"}, "userID": "u", "primaryApiKey": "k",
                   "customApiKeyResponses": {}, "theme": "dark"})
            .to_string(),
        )
        .unwrap();
        Sources { claude_dir: claude, claude_json: dir.join("src.claude.json"), codex_dir: dir.join("src-codex") }
    }

    #[test]
    fn ensure_links_is_idempotent_and_never_clobbers() {
        let dir = tmp_dir("links");
        let src = shared_fixture(&dir).claude_dir;
        let dest = dir.join("acc");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("settings.json"), "mine").unwrap();
        links::ensure_links(&src, &dest, links::CLAUDE_SHARED);
        links::ensure_links(&src, &dest, links::CLAUDE_SHARED);
        assert_eq!(std::fs::read_to_string(dest.join("settings.json")).unwrap(), "mine");
        assert_eq!(std::fs::read_to_string(dest.join("skills").join("a.md")).unwrap(), "skill");
        assert!(links::is_link(&dest.join("skills")));
        assert!(!dest.join(".credentials.json").exists());
        assert!(!dest.join("agents").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn seed_strips_identity_keys_once() {
        let dir = tmp_dir("seed");
        let srcs = shared_fixture(&dir);
        let dest = dir.join("acc");
        std::fs::create_dir_all(&dest).unwrap();
        links::seed_claude_json(&srcs.claude_json, &dest);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(dest.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(v, json!({"theme": "dark"}));
        std::fs::write(dest.join(".claude.json"), "{\"kept\":1}").unwrap();
        links::seed_claude_json(&srcs.claude_json, &dest);
        assert_eq!(std::fs::read_to_string(dest.join(".claude.json")).unwrap(), "{\"kept\":1}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn removing_account_keeps_shared_source() {
        let (mut svc, _io, dir) = setup("rmlinks");
        svc.sources = shared_fixture(&dir);
        add_rec(&svc, "claude", "acc00001");
        svc.ensure_account_dir("claude", "acc00001");
        let acc = svc.account_dir("claude", "acc00001");
        assert!(links::is_link(&acc.join("skills")));
        svc.remove("claude", "acc00001").unwrap();
        assert!(!acc.exists());
        assert_eq!(std::fs::read_to_string(dir.join("src-claude").join("skills").join("a.md")).unwrap(), "skill");
        assert!(dir.join("src-claude").join("settings.json").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tile_env_resolution() {
        let (mut svc, _io, dir) = setup("env");
        svc.sources = shared_fixture(&dir);
        assert!(svc.tile_env("t1").is_empty());
        add_rec(&svc, "claude", "acc00001");
        add_rec(&svc, "codex", "cod00001");
        svc.set_default("claude", "acc00001").unwrap();
        let env = svc.tile_env("t1");
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "CLAUDE_CONFIG_DIR");
        assert!(env[0].1.ends_with("acc00001"));
        svc.tile_set("t1", "claude", "system").unwrap();
        svc.tile_set("t1", "codex", "cod00001").unwrap();
        let env = svc.tile_env("t1");
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "CODEX_HOME");
        assert!(svc.account_dir("codex", "cod00001").is_dir());
        assert_eq!(svc.claude_dir_for_tile("t1"), None);
        assert_eq!(svc.claude_dir_for_tile("other"), Some(svc.account_dir("claude", "acc00001")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn claude_login_flow_and_duplicate() {
        let (mut svc, _io, dir) = setup("clogin");
        svc.sources = shared_fixture(&dir);
        let svc = Arc::new(svc);
        let (pid, command) = svc.cli_add_start("claude", "tile-a").unwrap();
        assert_eq!(command, "claude");
        assert_eq!(svc.tile_get("tile-a")["claude"], pid.as_str());
        assert_eq!(svc.tile_env("tile-a")[0].0, "CLAUDE_CONFIG_DIR");
        assert!(svc.poll_step(&pid, false));
        assert_eq!(svc.agy_add_status(&pid)["state"], "waiting");

        let cfg = svc.account_dir("claude", &pid).join(".claude.json");
        std::fs::write(&cfg, json!({"oauthAccount": {"emailAddress": "Me@x.com"}}).to_string()).unwrap();
        assert!(!svc.poll_step(&pid, false));
        let status = svc.agy_add_status(&pid);
        assert_eq!(status["state"], "done");
        assert_eq!(status["account"]["email"], "me@x.com");
        assert_eq!(svc.tile_get("tile-a")["claude"], pid.as_str());

        let (pid2, _) = svc.cli_add_start("claude", "tile-b").unwrap();
        let dir2 = svc.account_dir("claude", &pid2);
        std::fs::write(dir2.join(".claude.json"), json!({"oauthAccount": {"emailAddress": "me@x.com"}}).to_string()).unwrap();
        svc.poll_step(&pid2, false);
        let status = svc.agy_add_status(&pid2);
        assert_eq!(status["state"], "failed");
        assert_eq!(status["error"], "duplicate");
        assert!(!dir2.exists());
        assert_eq!(svc.tile_get("tile-b")["claude"], "system");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn codex_login_flow_and_cancel() {
        let (mut svc, _io, dir) = setup("xlogin");
        svc.sources = shared_fixture(&dir);
        let (pid, command) = svc.cli_add_start("codex", "tile-a").unwrap();
        assert_eq!(command, "codex login");
        let payload = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, json!({"email": "c@x.com"}).to_string());
        let token = format!("aaa.{payload}.ccc");
        let auth = svc.account_dir("codex", &pid).join("auth.json");
        std::fs::write(&auth, json!({"tokens": {"id_token": token}}).to_string()).unwrap();
        assert!(!svc.poll_step(&pid, false));
        assert_eq!(svc.agy_add_status(&pid)["account"]["email"], "c@x.com");

        let (pid2, _) = svc.cli_add_start("codex", "tile-b").unwrap();
        let dir2 = svc.account_dir("codex", &pid2);
        svc.agy_add_cancel(&pid2);
        assert!(!dir2.exists());
        assert_eq!(svc.tile_get("tile-b")["codex"], "system");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reply_shapes() {
        let ok: Value = serde_json::from_str(&reply(Ok(json!({"x": 1})))).unwrap();
        assert_eq!(ok, json!({"ok": true, "x": 1}));
        let bad: Value = serde_json::from_str(&reply(Err(fail("c", "m")))).unwrap();
        assert_eq!(bad, json!({"ok": false, "error": {"code": "c", "message": "m"}}));
    }

    #[test]
    fn dispatch_rejects_path_like_ids() {
        let (svc, _io, dir) = setup("ids");
        let svc = Arc::new(svc);
        for id in ["../x", "..\\x", "a/b", "ABC", ""] {
            let body = json!({ "agent": "claude", "id": id }).to_string();
            assert_eq!(svc.dispatch("remove", body.as_bytes()).unwrap_err().code, "invalid");
        }
        let body = json!({ "agent": "claude", "id": "system", "tileId": "t1" }).to_string();
        assert!(svc.dispatch("tile-set", body.as_bytes()).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn removing_linked_account_dir_keeps_target() {
        let dir = tmp_dir("rmjunction");
        let real = dir.join("real");
        std::fs::create_dir_all(real.join("sub")).unwrap();
        std::fs::write(real.join("sub").join("f"), "keep").unwrap();
        let acc = dir.join("acc");
        std::os::unix::fs::symlink(&real, &acc).unwrap();
        links::remove_account_dir(&acc);
        assert!(!acc.exists());
        assert_eq!(std::fs::read_to_string(real.join("sub").join("f")).unwrap(), "keep");
        std::fs::remove_dir_all(&dir).ok();
    }
}
