use std::path::Path;
use serde::{Deserialize, Serialize};

fn default_coordinator() -> String { "claude:opus".into() }
fn default_workers() -> Vec<String> { vec!["claude:haiku".into(), "claude:sonnet".into()] }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchSettings {
    #[serde(default = "default_coordinator")]
    pub coordinator: String,
    #[serde(default = "default_workers")]
    pub workers: Vec<String>,
    #[serde(default)]
    pub hidden_agents: Vec<String>,
    #[serde(default)]
    pub providers: Vec<OrchProvider>,
    #[serde(default)]
    pub show_worker_tiles: bool,
}

impl Default for OrchSettings {
    fn default() -> Self {
        Self {
            coordinator: default_coordinator(),
            workers: default_workers(),
            hidden_agents: Vec::new(),
            providers: Vec::new(),
            show_worker_tiles: false,
        }
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
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
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
        assert_eq!(settings.workers, vec!["claude:haiku", "claude:sonnet"]);
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
        let settings = OrchSettings {
            coordinator: "claude:opus".into(),
            workers: vec!["claude:haiku".into()],
            hidden_agents: vec!["codex".into()],
            show_worker_tiles: true,
            providers: vec![OrchProvider {
                id: "myds".into(),
                preset: "deepseek".into(),
                label: "My DeepSeek".into(),
                base_url: "https://example.com".into(),
                main_model: "ds-fast".into(),
                fast_model: "ds-fast".into(),
            }],
        };
        save(&dir, &settings).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.coordinator, "claude:opus");
        assert_eq!(loaded.providers.len(), 1);
        assert_eq!(loaded.providers[0].id, "myds");
        assert!(loaded.show_worker_tiles);
        std::fs::remove_dir_all(&dir).ok();
    }
}
