use std::collections::BTreeMap;
use std::path::Path;
use serde::{Deserialize, Serialize};

fn default_coordinator() -> String { "claude:opus".into() }
fn default_workers() -> Vec<String> { vec!["claude:haiku".into(), "claude:sonnet".into()] }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OrchestrateHint {
    Off,
    Suggest,
    Auto,
}

impl Default for OrchestrateHint {
    fn default() -> Self { OrchestrateHint::Suggest }
}

fn default_explore_chain() -> Vec<String> { vec!["claude:haiku".into()] }
fn default_implement_chain() -> Vec<String> { vec!["claude:haiku".into(), "claude:sonnet".into()] }
fn default_test_chain() -> Vec<String> { vec!["claude:haiku".into(), "claude:sonnet".into()] }
fn default_review_chain() -> Vec<String> { vec!["claude:sonnet".into()] }
fn default_mechanical_chain() -> Vec<String> { vec!["claude:haiku".into()] }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleChains {
    #[serde(default = "default_explore_chain")]
    pub explore: Vec<String>,
    #[serde(default = "default_implement_chain")]
    pub implement: Vec<String>,
    #[serde(default = "default_test_chain")]
    pub test: Vec<String>,
    #[serde(default = "default_review_chain")]
    pub review: Vec<String>,
    #[serde(default = "default_mechanical_chain")]
    pub mechanical: Vec<String>,
}

impl Default for RoleChains {
    fn default() -> Self {
        Self {
            explore: default_explore_chain(),
            implement: default_implement_chain(),
            test: default_test_chain(),
            review: default_review_chain(),
            mechanical: default_mechanical_chain(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchSettings {
    #[serde(default = "default_coordinator")]
    pub coordinator: String,
    #[serde(default = "default_workers", skip_serializing)]
    pub workers: Vec<String>,
    #[serde(default)]
    pub roles: RoleChains,
    #[serde(default)]
    pub orchestrate_hint: OrchestrateHint,
    #[serde(default)]
    pub hidden_agents: Vec<String>,
    #[serde(default)]
    pub providers: Vec<OrchProvider>,
    #[serde(default)]
    pub show_worker_tiles: bool,
    #[serde(default)]
    pub custom_models: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub ai_actions: AiActions,
}

impl Default for OrchSettings {
    fn default() -> Self {
        Self {
            coordinator: default_coordinator(),
            workers: Vec::new(),
            roles: RoleChains::default(),
            orchestrate_hint: OrchestrateHint::default(),
            hidden_agents: Vec::new(),
            providers: Vec::new(),
            show_worker_tiles: false,
            custom_models: BTreeMap::new(),
            ai_actions: AiActions::default(),
        }
    }
}

fn default_ai_provider() -> String { "claude".into() }
fn default_ai_model() -> String { "haiku".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTarget {
    #[serde(default = "default_ai_provider")]
    pub provider: String,
    #[serde(default = "default_ai_model")]
    pub model: String,
}

impl Default for AiTarget {
    fn default() -> Self {
        Self { provider: default_ai_provider(), model: default_ai_model() }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiActionOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_message: Option<AiTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tidy: Option<AiTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiActions {
    #[serde(default)]
    pub default: AiTarget,
    #[serde(default)]
    pub overrides: AiActionOverrides,
}

impl Default for AiActions {
    fn default() -> Self {
        Self { default: AiTarget::default(), overrides: AiActionOverrides::default() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchProvider {
    pub id: String,
    pub preset: String,
    pub label: String,
    pub base_url: String,
    pub main_model: String,
    pub fast_model: String,
}

pub struct ProviderPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub base_url: &'static str,
    pub main_model: &'static str,
    pub fast_model: &'static str,
    pub extra_env: &'static [(&'static str, &'static str)],
}

pub fn presets() -> &'static [ProviderPreset] {
    static PRESETS: &[ProviderPreset] = &[
        ProviderPreset {
            id: "deepseek",
            label: "DeepSeek",
            base_url: "https://api.deepseek.com/anthropic",
            main_model: "deepseek-flash[1m]",
            fast_model: "deepseek-flash",
            extra_env: &[],
        },
        ProviderPreset {
            id: "zai",
            label: "Z.AI GLM",
            base_url: "https://api.z.ai/api/anthropic",
            main_model: "glm-5.3[1m]",
            fast_model: "glm-5.3-flash[1m]",
            extra_env: &[("API_TIMEOUT_MS", "3000000")],
        },
        ProviderPreset {
            id: "kimi",
            label: "Kimi",
            base_url: "https://api.moonshot.ai/anthropic",
            main_model: "kimi-k3[1m]",
            fast_model: "kimi-k3[1m]",
            extra_env: &[],
        },
        ProviderPreset {
            id: "minimax",
            label: "MiniMax",
            base_url: "https://api.minimax.io/anthropic",
            main_model: "MiniMax-M3.1-Flash[1m]",
            fast_model: "MiniMax-M3.1-Flash[1m]",
            extra_env: &[],
        },
        ProviderPreset {
            id: "openrouter",
            label: "OpenRouter",
            base_url: "https://openrouter.ai/api",
            main_model: "~anthropic/claude-sonnet-latest[1m]",
            fast_model: "~anthropic/claude-haiku-latest",
            extra_env: &[],
        },
        ProviderPreset {
            id: "custom",
            label: "Custom",
            base_url: "",
            main_model: "",
            fast_model: "",
            extra_env: &[],
        },
    ];
    PRESETS
}

pub fn settings_path(base: &Path) -> std::path::PathBuf {
    base.join("orch").join("settings.json")
}

pub fn load(base: &Path) -> OrchSettings {
    let path = settings_path(base);
    let raw: Option<serde_json::Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let Some(v) = raw else { return OrchSettings::default() };
    let roles_absent = v.get("roles").is_none();
    let mut s: OrchSettings = serde_json::from_value(v).unwrap_or_default();
    if roles_absent && !s.workers.is_empty() {
        s.roles.implement = s.workers.clone();
    }
    s
}

pub fn save(base: &Path, settings: &OrchSettings) -> Result<(), String> {
    let path = settings_path(base);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| format!("serialize: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename: {e}"))
}

pub fn built_in_ids() -> &'static [&'static str] {
    &["claude", "codex", "opencode", "kimi", "pi"]
}

pub fn provider_id_for_label(label: &str, existing_ids: &[String]) -> String {
    let raw: String = label.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = raw.split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    let base = if built_in_ids().contains(&slug.as_str()) {
        format!("{slug}-api")
    } else {
        slug
    };

    if !existing_ids.contains(&base) {
        return base;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if !existing_ids.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_dir(suffix: &str) -> PathBuf {
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("orch-settings-{suffix}-{ns}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn defaults_when_file_missing() {
        let dir = tmp_dir("defaults");
        let settings = load(&dir);
        assert_eq!(settings.coordinator, "claude:opus");
        assert_eq!(settings.roles.implement, vec!["claude:haiku", "claude:sonnet"]);
        assert_eq!(settings.orchestrate_hint, OrchestrateHint::Suggest);
        assert!(settings.hidden_agents.is_empty());
        assert!(settings.providers.is_empty());
        assert!(!settings.show_worker_tiles);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn provider_id_kimi_collision() {
        let id = provider_id_for_label("Kimi", &[]);
        assert_eq!(id, "kimi-api");
    }

    #[test]
    fn provider_id_deepseek_no_collision() {
        let id = provider_id_for_label("DeepSeek", &[]);
        assert_eq!(id, "deepseek");
    }

    #[test]
    fn provider_id_uniqueness() {
        let existing = vec!["deepseek".to_string(), "deepseek-2".to_string()];
        let id = provider_id_for_label("DeepSeek", &existing);
        assert_eq!(id, "deepseek-3");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tmp_dir("roundtrip");
        let mut roles = RoleChains::default();
        roles.implement = vec!["claude:haiku".into()];
        let settings = OrchSettings {
            coordinator: "claude:opus".into(),
            workers: Vec::new(),
            roles,
            orchestrate_hint: OrchestrateHint::Auto,
            hidden_agents: vec!["codex".into()],
            show_worker_tiles: true,
            custom_models: BTreeMap::new(),
            providers: vec![OrchProvider {
                id: "myds".into(),
                preset: "deepseek".into(),
                label: "My DeepSeek".into(),
                base_url: "https://example.com".into(),
                main_model: "ds-fast".into(),
                fast_model: "ds-fast".into(),
            }],
            ai_actions: AiActions::default(),
        };
        save(&dir, &settings).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.coordinator, "claude:opus");
        assert_eq!(loaded.roles.implement, vec!["claude:haiku"]);
        assert_eq!(loaded.orchestrate_hint, OrchestrateHint::Auto);
        assert_eq!(loaded.providers.len(), 1);
        assert_eq!(loaded.providers[0].id, "myds");
        assert!(loaded.show_worker_tiles);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn migration_workers_to_roles_implement() {
        let dir = tmp_dir("migration");
        let json = serde_json::json!({
            "coordinator": "claude:opus",
            "workers": ["claude:haiku", "claude:sonnet"],
            "hiddenAgents": [],
            "providers": [],
            "showWorkerTiles": false
        });
        let path = settings_path(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, json.to_string()).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.roles.implement, vec!["claude:haiku", "claude:sonnet"],
            "workers should migrate to roles.implement");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn migration_no_workers_keeps_default_roles() {
        let dir = tmp_dir("no-workers");
        let json = serde_json::json!({
            "coordinator": "claude:opus",
            "workers": [],
            "hiddenAgents": [],
            "providers": [],
            "showWorkerTiles": false
        });
        let path = settings_path(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, json.to_string()).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.roles.implement, default_implement_chain(),
            "empty workers should not override default roles");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn roles_present_wins_over_workers() {
        let dir = tmp_dir("roles-present");
        let json = serde_json::json!({
            "coordinator": "claude:opus",
            "workers": ["claude:haiku"],
            "roles": { "implement": ["claude:sonnet"] },
            "hiddenAgents": [],
            "providers": [],
            "showWorkerTiles": false
        });
        let path = settings_path(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, json.to_string()).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.roles.implement, vec!["claude:sonnet"],
            "explicit roles.implement should win over workers migration");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn orchestrate_hint_roundtrip() {
        let dir = tmp_dir("hint");
        let settings = OrchSettings {
            orchestrate_hint: OrchestrateHint::Off,
            ..OrchSettings::default()
        };
        save(&dir, &settings).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.orchestrate_hint, OrchestrateHint::Off);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn custom_models_roundtrip_and_default() {
        let dir = tmp_dir("custom-models");
        let loaded_empty = load(&dir);
        assert!(loaded_empty.custom_models.is_empty(), "default must be empty");

        let mut s = OrchSettings::default();
        s.custom_models.insert("codex".into(), vec!["test-model-x".into(), "other".into()]);
        save(&dir, &s).unwrap();
        let loaded = load(&dir);
        let got = loaded.custom_models.get("codex").map(|v| v.as_slice()).unwrap_or(&[]);
        assert_eq!(got, &["test-model-x", "other"]);
        assert!(loaded.custom_models.get("claude").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
