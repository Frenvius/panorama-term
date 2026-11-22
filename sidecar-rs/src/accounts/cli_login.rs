use std::path::PathBuf;

use serde_json::Value;

use super::agy::{Pending, PendingState};
use super::links::{self, CLAUDE_SHARED, CODEX_SHARED};
use super::{fail, jwt, new_id, now_ms, save_store, valid_id, AccountRec, ApiError, Service, SYSTEM};

#[derive(Clone)]
pub struct CliLogin {
    pub agent: &'static str,
    pub id: String,
    pub tile_id: String,
}

impl Service {
    pub(super) fn account_dir(&self, agent: &str, id: &str) -> PathBuf {
        self.root.join(agent).join(id)
    }

    pub(super) fn ensure_account_dir(&self, agent: &str, id: &str) {
        let dir = self.account_dir(agent, id);
        match agent {
            "claude" => {
                links::ensure_links(&self.sources.claude_dir, &dir, CLAUDE_SHARED);
                links::seed_claude_json(&self.sources.claude_json, &dir);
            }
            "codex" => links::ensure_links(&self.sources.codex_dir, &dir, CODEX_SHARED),
            _ => {}
        }
    }

    fn drop_tile_override(&self, tile_id: &str, agent: &str) {
        let _ = self.mutate(|store| {
            if let Some(sel) = store.tiles.get_mut(tile_id) {
                match agent {
                    "claude" => sel.claude = None,
                    _ => sel.codex = None,
                }
            }
            Ok(())
        });
    }

    pub fn cli_add_start(&self, agent: &str, tile_id: &str) -> Result<(String, &'static str), ApiError> {
        if tile_id.is_empty() {
            return Err(fail("invalid", "tileId is empty"));
        }
        let (agent, command): (&'static str, &'static str) = match agent {
            "claude" => ("claude", "claude"),
            "codex" => ("codex", "codex login"),
            _ => return Err(fail("invalid", "agent has no cli login")),
        };
        let id = new_id();
        std::fs::create_dir_all(self.account_dir(agent, &id)).map_err(|e| fail("io", format!("create account dir: {e}")))?;
        self.ensure_account_dir(agent, &id);
        let tile = tile_id.to_string();
        let set = self.mutate(|store| {
            let sel = store.tiles.entry(tile.clone()).or_default();
            match agent {
                "claude" => sel.claude = Some(id.clone()),
                _ => sel.codex = Some(id.clone()),
            }
            Ok(())
        });
        if let Err(e) = set {
            links::remove_account_dir(&self.account_dir(agent, &id));
            return Err(e);
        }
        let login = CliLogin { agent, id: id.clone(), tile_id: tile };
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(
            id.clone(),
            Pending { previous: None, state: PendingState::Waiting, login: Some(login) },
        );
        Ok((id, command))
    }

    fn read_identity(&self, login: &CliLogin) -> Option<(String, String)> {
        let dir = self.account_dir(login.agent, &login.id);
        match login.agent {
            "claude" => {
                let v: Value = serde_json::from_str(&std::fs::read_to_string(dir.join(".claude.json")).ok()?).ok()?;
                let email = v.pointer("/oauthAccount/emailAddress")?.as_str()?.trim().to_lowercase();
                (!email.is_empty()).then(|| (email.clone(), email))
            }
            _ => {
                let v: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("auth.json")).ok()?).ok()?;
                v.pointer("/tokens/id_token")?.as_str().filter(|t| !t.is_empty())?;
                let ident = jwt::identity_of(&v)?;
                let email = jwt::email_of(&v).unwrap_or_else(|| ident.clone());
                Some((ident, email))
            }
        }
    }

    fn discard_login(&self, login: &CliLogin) {
        links::remove_account_dir(&self.account_dir(login.agent, &login.id));
        self.drop_tile_override(&login.tile_id, login.agent);
    }

    pub(super) fn cli_poll(&self, p: &mut Pending, login: &CliLogin, timed_out: bool) -> bool {
        let Some((ident, email)) = self.read_identity(login) else {
            if timed_out {
                self.discard_login(login);
                p.state = PendingState::Failed("login timed out".into());
                return false;
            }
            return true;
        };
        let duplicate = {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            store
                .accounts
                .iter()
                .any(|a| a.agent == login.agent && a.identity.as_deref() == Some(ident.as_str()))
        };
        if duplicate {
            self.discard_login(login);
            p.state = PendingState::Failed("duplicate".into());
            return false;
        }
        let rec = AccountRec {
            agent: login.agent.into(),
            id: login.id.clone(),
            label: email.clone(),
            email: Some(email),
            created_at: now_ms(),
            identity: Some(ident),
        };
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        store.accounts.push(rec.clone());
        p.state = match save_store(&self.root, &store) {
            Ok(()) => PendingState::Done(rec.into_account()),
            Err(e) => {
                store.accounts.pop();
                drop(store);
                self.discard_login(login);
                PendingState::Failed(e)
            }
        };
        false
    }

    pub(super) fn cli_cancel(&self, p: &Pending, login: &CliLogin) {
        if matches!(p.state, PendingState::Waiting) {
            self.discard_login(login);
        }
    }

    pub fn effective_ids(&self, tile_id: &str) -> (String, String) {
        let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let sel = store.tiles.get(tile_id);
        let pick = |agent: &str, chosen: Option<&String>, default: &String| {
            chosen
                .filter(|id| {
                    Self::selectable(&store, agent, id)
                        || (valid_id(id) && id.as_str() != SYSTEM && self.account_dir(agent, id).is_dir())
                })
                .unwrap_or(default)
                .clone()
        };
        (
            pick("claude", sel.and_then(|s| s.claude.as_ref()), &store.defaults.claude),
            pick("codex", sel.and_then(|s| s.codex.as_ref()), &store.defaults.codex),
        )
    }

    pub fn tile_env(&self, tile_id: &str) -> Vec<(String, String)> {
        let (claude, codex) = self.effective_ids(tile_id);
        let mut env = Vec::new();
        for (agent, id, var) in [("claude", claude, "CLAUDE_CONFIG_DIR"), ("codex", codex, "CODEX_HOME")] {
            if id == SYSTEM || !valid_id(&id) {
                continue;
            }
            self.ensure_account_dir(agent, &id);
            env.push((var.to_string(), self.account_dir(agent, &id).to_string_lossy().into_owned()));
        }
        env
    }

    pub fn claude_dir_for_tile(&self, tile_id: &str) -> Option<PathBuf> {
        let (claude, _) = self.effective_ids(tile_id);
        (claude != SYSTEM && valid_id(&claude)).then(|| self.account_dir("claude", &claude))
    }
}
