use std::path::PathBuf;
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

fn claude_request() -> Result<Vec<(&'static str, String)>, String> {
    let dir = std::env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .ok()
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

fn codex_request() -> Result<Vec<(&'static str, String)>, String> {
    let dir = std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .ok()
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

#[tauri::command]
pub async fn agent_usage(provider: String) -> Result<Value, String> {
    let (url, headers) = match provider.as_str() {
        "claude" => ("https://api.anthropic.com/api/oauth/usage", claude_request()?),
        "codex" => ("https://chatgpt.com/backend-api/wham/usage", codex_request()?),
        _ => return Err(format!("unknown provider {provider}")),
    };
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
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
