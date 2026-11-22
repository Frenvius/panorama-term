use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

const TTL: Duration = Duration::from_secs(600);
const PROCESS_TIMEOUT: Duration = Duration::from_secs(15);

struct Entry {
    fetched_at: Instant,
    models: Vec<String>,
}

fn mem_cache() -> &'static Mutex<HashMap<String, Entry>> {
    static C: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn catalog_path(base: &Path) -> PathBuf {
    base.join("orch").join("model-catalog.json")
}

fn load_disk_cache(base: &Path) -> HashMap<String, Vec<String>> {
    std::fs::read_to_string(catalog_path(base))
        .ok()
        .and_then(|s| serde_json::from_str::<HashMap<String, Vec<String>>>(&s).ok())
        .unwrap_or_default()
}

fn save_disk_cache(base: &Path, map: &HashMap<String, Vec<String>>) {
    let Ok(json) = serde_json::to_string(map) else { return };
    if let Some(dir) = catalog_path(base).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(catalog_path(base), json);
}

fn cli_command(exe: &str, args: &[&str]) -> std::process::Command {
    let mut c = crate::orch::cli_env::cli_command(exe);
    c.args(args);
    c
}

fn run_cmd(mut cmd: std::process::Command) -> Result<String, String> {
    use std::io::Read;
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + PROCESS_TIMEOUT;
    while child.try_wait().map_err(|e| e.to_string())?.is_none() {
        if Instant::now() >= deadline {
            crate::orch::cli_env::kill_tree(&mut child);
            return Err("timeout".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let buf = reader.join().map_err(|_| "stdout reader panicked".to_string())?;
    String::from_utf8(buf).map_err(|e| e.to_string())
}

fn codex_home() -> Option<PathBuf> {
    if let Some(dir) = std::env::var("CODEX_HOME").ok().filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let home = std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())?;
    Some(Path::new(&home).join(".codex"))
}

fn user_home() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())
        .map(PathBuf::from)
}

pub fn parse_codex_models(json: &str) -> Vec<String> {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let Some(arr) = v["models"].as_array() else { return vec![] };
    arr.iter()
        .filter(|m| m["visibility"].as_str() == Some("list"))
        .filter_map(|m| m["slug"].as_str().map(str::to_string))
        .collect()
}

pub fn parse_pi_models(output: &str) -> Vec<String> {
    output.lines()
        .skip(1)
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let provider = parts.next()?;
            let model = parts.next()?;
            Some(format!("{provider}/{model}"))
        })
        .collect()
}

pub fn parse_opencode_models(output: &str) -> Vec<String> {
    output.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && l.contains('/') && !l.contains(char::is_whitespace))
        .map(str::to_string)
        .collect()
}

pub fn parse_agy_models(output: &str) -> Vec<String> {
    output.lines()
        .filter(|l| l.contains('\t'))
        .filter_map(|l| l.split('\t').next().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string))
        .collect()
}

pub fn parse_kimi_config(toml: &str) -> Vec<String> {
    let mut result = Vec::new();
    for line in toml.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("[models.\"") {
            if let Some(id) = rest.strip_suffix("\"]") {
                result.push(id.to_string());
            }
        }
    }
    result
}

fn seed(profile_id: &str) -> Vec<String> {
    if profile_id == "claude" {
        vec!["haiku".into(), "sonnet".into(), "opus".into()]
    } else {
        vec![]
    }
}

fn discover(profile_id: &str) -> Result<Vec<String>, String> {
    match profile_id {
        "claude" => Ok(seed("claude")),
        "codex" => {
            let path = codex_home().ok_or("no codex home")?.join("models_cache.json");
            let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let models = parse_codex_models(&content);
            if models.is_empty() {
                return Err("no models with visibility=list in models_cache.json".into());
            }
            Ok(models)
        }
        "pi" => {
            let output = run_cmd(cli_command("pi", &["--list-models"]))?;
            let models = parse_pi_models(&output);
            if models.is_empty() {
                return Err("empty output from pi --list-models".into());
            }
            Ok(models)
        }
        "opencode" => {
            let output = run_cmd(cli_command("opencode", &["models"]))?;
            let models = parse_opencode_models(&output);
            if models.is_empty() {
                return Err("empty output from opencode models".into());
            }
            Ok(models)
        }
        "kimi" => {
            let home = user_home().ok_or("no home dir")?;
            let content = std::fs::read_to_string(home.join(".kimi-code").join("config.toml"))
                .or_else(|_| std::fs::read_to_string(home.join(".kimi").join("config.toml")))
                .map_err(|e| e.to_string())?;
            let models = parse_kimi_config(&content);
            if models.is_empty() {
                return Err("no [models.\"...\"] sections in kimi config".into());
            }
            Ok(models)
        }
        "antigravity" => {
            let output = run_cmd(cli_command("agy", &["models"]))?;
            let models = parse_agy_models(&output);
            if models.is_empty() {
                return Err("empty output from agy models".into());
            }
            Ok(models)
        }
        _ => Ok(vec![]),
    }
}

fn get_cached(profile_id: &str, base: &Path) -> Option<(Vec<String>, bool)> {
    {
        let guard = mem_cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.get(profile_id) {
            let fresh = entry.fetched_at.elapsed() < TTL;
            return Some((entry.models.clone(), fresh));
        }
    }
    let disk = load_disk_cache(base);
    disk.get(profile_id).filter(|m| !m.is_empty()).map(|m| (m.clone(), false))
}

fn store_cache(profile_id: &str, models: Vec<String>, base: &Path) {
    {
        let mut guard = mem_cache().lock().unwrap_or_else(|e| e.into_inner());
        guard.insert(profile_id.to_string(), Entry { fetched_at: Instant::now(), models: models.clone() });
    }
    static DISK_LOCK: Mutex<()> = Mutex::new(());
    let _disk = DISK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut disk = load_disk_cache(base);
    disk.insert(profile_id.to_string(), models);
    save_disk_cache(base, &disk);
}

fn mark_attempted(profile_id: &str, base: &Path) {
    let models = get_cached(profile_id, base).map(|(m, _)| m).unwrap_or_else(|| seed(profile_id));
    let mut guard = mem_cache().lock().unwrap_or_else(|e| e.into_inner());
    guard.insert(profile_id.to_string(), Entry { fetched_at: Instant::now(), models });
}

fn refresh_one(profile_id: &str, base: &Path) {
    match discover(profile_id) {
        Ok(models) if !models.is_empty() => store_cache(profile_id, models, base),
        Ok(_) => mark_attempted(profile_id, base),
        Err(e) => {
            eprintln!("[orch] model discovery failed for {profile_id}: {e}");
            mark_attempted(profile_id, base);
        }
    }
}

fn refresh_parallel(ids: &[String], base: &Path) {
    std::thread::scope(|s| {
        for id in ids {
            s.spawn(move || refresh_one(id, base));
        }
    });
}

pub fn prepare(ids: Vec<String>, base: PathBuf, force: bool) {
    let mut now = Vec::new();
    let mut later = Vec::new();
    for id in ids {
        match get_cached(&id, &base) {
            Some((_, true)) if !force => {}
            Some((_, false)) if !force => later.push(id),
            _ => now.push(id),
        }
    }
    if !later.is_empty() {
        for id in &later {
            mark_attempted(id, &base);
        }
        let base_bg = base.clone();
        std::thread::spawn(move || refresh_parallel(&later, &base_bg));
    }
    refresh_parallel(&now, &base);
}

pub fn models_for(profile_id: &str, base: &Path) -> Vec<String> {
    get_cached(profile_id, base).map(|(m, _)| m).unwrap_or_else(|| seed(profile_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_visibility_filter() {
        let json = r#"{"models":[
            {"slug":"gpt-6.1-sol","visibility":"list"},
            {"slug":"gpt-5.1-hidden","visibility":"hidden"},
            {"slug":"gpt-6.2-astra","visibility":"list"}
        ]}"#;
        let models = parse_codex_models(json);
        assert_eq!(models, vec!["gpt-6.1-sol", "gpt-6.2-astra"]);
    }

    #[test]
    fn codex_empty_on_bad_json() {
        assert!(parse_codex_models("not json").is_empty());
        assert!(parse_codex_models("{}").is_empty());
        assert!(parse_codex_models(r#"{"models":[]}"#).is_empty());
    }

    #[test]
    fn codex_no_list_models_gives_empty() {
        let json = r#"{"models":[{"slug":"x","visibility":"hidden"}]}"#;
        assert!(parse_codex_models(json).is_empty());
    }

    #[test]
    fn pi_table_parse() {
        let output = "provider  model  context  max-out  thinking  images\n\
                      openai    gpt-5    128k     16k      no        yes\n\
                      openrouter  google/gemini-3.8-flash  200k  8k  yes  no\n";
        let models = parse_pi_models(output);
        assert_eq!(models, vec!["openai/gpt-5", "openrouter/google/gemini-3.8-flash"]);
    }

    #[test]
    fn pi_skips_header_and_empty_lines() {
        let output = "provider  model  context\n\n  anthropic  claude-sonnet  200k\n";
        let models = parse_pi_models(output);
        assert_eq!(models, vec!["anthropic/claude-sonnet"]);
    }

    #[test]
    fn pi_empty_output_gives_empty() {
        assert!(parse_pi_models("").is_empty());
        assert!(parse_pi_models("provider  model  context\n").is_empty());
    }

    #[test]
    fn opencode_keeps_slash_lines() {
        let output = "openai/gpt-5\nanthropic/claude-opus\n\n# comment\nno-slash\n";
        let models = parse_opencode_models(output);
        assert_eq!(models, vec!["openai/gpt-5", "anthropic/claude-opus"]);
    }

    #[test]
    fn opencode_rejects_lines_with_whitespace() {
        let output = "openai/gpt-5\nsome model/with space\n";
        let models = parse_opencode_models(output);
        assert_eq!(models, vec!["openai/gpt-5"]);
    }

    #[test]
    fn kimi_config_section_headers() {
        let toml = r#"
[settings]
key = "value"

[models."openai-codex/gpt-5.6-sol"]
price = 0.01

[models."anthropic/claude-haiku-5"]
price = 0.001
"#;
        let models = parse_kimi_config(toml);
        assert_eq!(models, vec!["openai-codex/gpt-5.6-sol", "anthropic/claude-haiku-5"]);
    }

    #[test]
    fn kimi_config_no_sections_gives_empty() {
        assert!(parse_kimi_config("[settings]\nfoo = bar\n").is_empty());
        assert!(parse_kimi_config("").is_empty());
    }

    #[test]
    fn agy_tab_parse() {
        let output = "Fetching available models...\ngemini-3.8-flash-high\tGemini 3.8 Flash (High)\ngemini-2.5-pro\tGemini 2.5 Pro\n";
        let models = parse_agy_models(output);
        assert_eq!(models, vec!["gemini-3.8-flash-high", "gemini-2.5-pro"]);
    }

    #[test]
    fn agy_skips_lines_without_tab() {
        let output = "no-tab-line\nslug\tlabel\n";
        let models = parse_agy_models(output);
        assert_eq!(models, vec!["slug"]);
    }

    #[test]
    fn agy_empty_output_gives_empty() {
        assert!(parse_agy_models("").is_empty());
        assert!(parse_agy_models("Fetching available models...\n").is_empty());
    }

    #[test]
    fn claude_seed() {
        let models = discover("claude").unwrap();
        assert_eq!(models, vec!["haiku", "sonnet", "opus"]);
    }

    #[test]
    fn unknown_profile_gives_empty() {
        let models = discover("unknown-agent-xyz").unwrap();
        assert!(models.is_empty());
    }
}
