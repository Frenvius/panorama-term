use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;

fn home() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())
        .map(PathBuf::from)
}

fn read_json(path: PathBuf) -> Result<Value, String> {
    let raw = std::fs::read_to_string(&path).map_err(|_| "signed-out".to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

fn account_dir(provider: &str, account_id: Option<&str>) -> Result<Option<PathBuf>, String> {
    let Some(id) = account_id.filter(|id| *id != "system") else {
        return Ok(None);
    };
    let valid = (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if !valid {
        return Err("invalid account id".into());
    }
    let base = crate::panorama_base_dir().ok_or("signed-out")?;
    Ok(Some(base.join("accounts").join(provider).join(id)))
}

fn claude_request(dir: Option<PathBuf>) -> Result<Vec<(&'static str, String)>, String> {
    let dir = dir
        .or_else(|| std::env::var("CLAUDE_CONFIG_DIR").map(PathBuf::from).ok())
        .or_else(|| home().map(|h| h.join(".claude")))
        .ok_or("signed-out")?;
    let creds = read_json(dir.join(".credentials.json"))?;
    let token = creds["claudeAiOauth"]["accessToken"].as_str().ok_or("signed-out")?;
    Ok(vec![
        ("Authorization", format!("Bearer {token}")),
        ("anthropic-beta", "oauth-2025-04-20".into()),
        ("User-Agent", "claude-code/2.1.0".into()),
    ])
}

fn codex_request(dir: Option<PathBuf>) -> Result<Vec<(&'static str, String)>, String> {
    let dir = dir
        .or_else(|| std::env::var("CODEX_HOME").map(PathBuf::from).ok())
        .or_else(|| home().map(|h| h.join(".codex")))
        .ok_or("signed-out")?;
    let auth = read_json(dir.join("auth.json"))?;
    let token = auth["tokens"]["access_token"].as_str().ok_or("signed-out")?;
    let mut headers = vec![
        ("Authorization", format!("Bearer {token}")),
        ("User-Agent", "codex-cli".into()),
        ("OpenAI-Beta", "codex-1".into()),
        ("originator", "Codex Desktop".into()),
    ];
    if let Some(account) = auth["tokens"]["account_id"].as_str() {
        headers.push(("ChatGPT-Account-Id", account.into()));
    }
    Ok(headers)
}

const AGY_MIN_VERSION: [u32; 3] = [1, 1, 11];

static AGY_UNSUPPORTED: AtomicBool = AtomicBool::new(false);

fn agy_command() -> std::process::Command {
    let mut cmd = crate::hidden_command("agy");
    cmd.env("AGY_CLI_DISABLE_AUTO_UPDATE", "true");
    if let Some(h) = home() {
        cmd.current_dir(h);
    }
    cmd
}

fn parse_version(raw: &str) -> Option<[u32; 3]> {
    let token = raw.split_whitespace().find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let mut parts = token.split('.').map(|p| p.trim_matches(|c: char| !c.is_ascii_digit()).parse::<u32>().ok());
    Some([parts.next()??, parts.next().flatten().unwrap_or(0), parts.next().flatten().unwrap_or(0)])
}

fn agy_usage_result(stdout: &str) -> Result<Value, String> {
    let line = stdout.lines().rev().find(|l| l.trim_start().starts_with('{')).ok_or("no usage output")?;
    let json: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
    if json["status"] != "SUCCESS" {
        return Err(json["error"].as_str().unwrap_or("usage failed").to_string());
    }
    if json["command"]["name"] != "usage" || !json["command"]["data"]["groups"].is_array() {
        AGY_UNSUPPORTED.store(true, Ordering::Relaxed);
        return Err("unsupported".into());
    }
    Ok(json["command"]["data"].clone())
}

fn antigravity_usage() -> Result<Value, String> {
    if AGY_UNSUPPORTED.load(Ordering::Relaxed) {
        return Err("unsupported".into());
    }
    let version = agy_command().arg("--version").output().map_err(|_| "not-installed".to_string())?;
    let version = parse_version(&String::from_utf8_lossy(&version.stdout)).ok_or("not-installed")?;
    if version < AGY_MIN_VERSION {
        return Err("unsupported".into());
    }
    let out = agy_command()
        .args(["-p", "/usage", "--output-format", "json", "--print-timeout", "20s"])
        .output()
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
    if stderr.contains("sign in") || stderr.contains("login") || stderr.contains("not authenticated") {
        return Err("signed-out".into());
    }
    agy_usage_result(&stdout)
}

fn extra_ca_certs() -> Vec<reqwest::Certificate> {
    let Some(path) = std::env::var_os("NODE_EXTRA_CA_CERTS") else {
        return Vec::new();
    };
    let Ok(pem) = std::fs::read(&path) else {
        eprintln!("[usage] cannot read NODE_EXTRA_CA_CERTS {}", PathBuf::from(path).display());
        return Vec::new();
    };
    reqwest::Certificate::from_pem_bundle(&pem).unwrap_or_else(|e| {
        eprintln!("[usage] invalid NODE_EXTRA_CA_CERTS: {e}");
        Vec::new()
    })
}

#[tauri::command]
pub async fn agent_usage(provider: String, account_id: Option<String>) -> Result<Value, String> {
    if provider == "antigravity" {
        return tauri::async_runtime::spawn_blocking(antigravity_usage)
            .await
            .map_err(|e| e.to_string())?;
    }
    let (url, headers) = match provider.as_str() {
        "claude" => (
            "https://api.anthropic.com/api/oauth/usage",
            claude_request(account_dir("claude", account_id.as_deref())?)?,
        ),
        "codex" => (
            "https://chatgpt.com/backend-api/wham/usage",
            codex_request(account_dir("codex", account_id.as_deref())?)?,
        ),
        _ => return Err(format!("unknown provider {provider}")),
    };
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .tls_certs_merge(extra_ca_certs())
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.get(url);
    for (name, value) in headers {
        req = req.header(name, value);
    }
    let res = req.send().await.map_err(|e| e.to_string())?;
    let status = res.status();
    if status == 401 || status == 403 {
        return Err("signed-out".into());
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let body = res.text().await.map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_agy_version() {
        assert_eq!(parse_version("1.5.14
"), Some([1, 5, 14]));
        assert_eq!(parse_version("agy 1.1"), Some([1, 1, 0]));
        assert!(parse_version("").is_none());
    }

    #[test]
    fn reads_usage_command_data() {
        let ok = r#"{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[]}}}"#;
        assert_eq!(agy_usage_result(ok).unwrap()["groups"], serde_json::json!([]));
        let failed = r#"{"status":"ERROR","error":"boom"}"#;
        assert_eq!(agy_usage_result(failed).unwrap_err(), "boom");
    }
}
