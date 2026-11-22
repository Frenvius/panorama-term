use std::path::{Path, PathBuf};

use serde_json::Value;

pub const CLAUDE_SHARED: &[&str] = &[
    "settings.json",
    "settings.local.json",
    "CLAUDE.md",
    "keybindings.json",
    "agents",
    "commands",
    "skills",
    "plugins",
    "hooks",
    "output-styles",
    "memory",
    "projects",
    "plans",
];

pub const CODEX_SHARED: &[&str] = &[
    "config.toml",
    "AGENTS.md",
    "hooks.json",
    "agents",
    "skills",
    "prompts",
    "instructions",
    "plugins",
    "sessions",
    "archived_sessions",
];

const IDENTITY_KEYS: &[&str] = &["oauthAccount", "userID", "primaryApiKey", "customApiKeyResponses"];
const COPIES_MARKER: &str = ".panorama-copies";

#[derive(Clone)]
pub struct Sources {
    pub claude_dir: PathBuf,
    pub claude_json: PathBuf,
    pub codex_dir: PathBuf,
}

impl Sources {
    pub fn from_env() -> Self {
        let home = super::home_dir().unwrap_or_default();
        let env_dir = |key: &str| std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from);
        let claude_env = env_dir("CLAUDE_CONFIG_DIR");
        Self {
            claude_json: claude_env.as_ref().map_or_else(|| home.join(".claude.json"), |d| d.join(".claude.json")),
            claude_dir: claude_env.unwrap_or_else(|| home.join(".claude")),
            codex_dir: env_dir("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
        }
    }
}

pub fn is_link(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        return meta.file_attributes() & REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    false
}

#[cfg(unix)]
fn link_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(windows)]
fn link_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(dst)
        .arg(src)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("mklink /J failed"))
    }
}

#[cfg(unix)]
fn link_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(windows)]
fn link_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(src, dst)
}

fn read_copies(dest: &Path) -> Vec<String> {
    std::fs::read_to_string(dest.join(COPIES_MARKER))
        .map(|raw| raw.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

fn write_copies(dest: &Path, names: &[String]) {
    let _ = std::fs::write(dest.join(COPIES_MARKER), names.join("\n"));
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

// Windows without Developer Mode cannot symlink files, so those are copies. Sync is one-way:
// a copy is refreshed only when the source is newer; edits made through the account dir are
// overwritten on the next ensure.
fn refresh_copy(src: &Path, dst: &Path) {
    if let (Some(s), Some(d)) = (modified(src), modified(dst)) {
        if s > d {
            let _ = std::fs::copy(src, dst);
        }
    }
}

pub fn ensure_links(source: &Path, dest: &Path, names: &[&str]) {
    if std::fs::create_dir_all(dest).is_err() {
        return;
    }
    let mut copies = read_copies(dest);
    let before = copies.len();
    for name in names {
        let src = source.join(name);
        let Ok(src_meta) = std::fs::metadata(&src) else {
            continue;
        };
        let dst = dest.join(name);
        if let Ok(dst_meta) = std::fs::symlink_metadata(&dst) {
            if !is_link(&dst) && dst_meta.is_file() && src_meta.is_file() && copies.iter().any(|c| c == name) {
                refresh_copy(&src, &dst);
            }
            continue;
        }
        if src_meta.is_dir() {
            let _ = link_dir(&src, &dst);
        } else if link_file(&src, &dst).is_err() && std::fs::copy(&src, &dst).is_ok() {
            copies.push((*name).to_string());
        }
    }
    if copies.len() != before {
        write_copies(dest, &copies);
    }
}

pub fn seed_claude_json(source_json: &Path, dest_dir: &Path) {
    let target = dest_dir.join(".claude.json");
    if target.exists() {
        return;
    }
    let Ok(raw) = std::fs::read_to_string(source_json) else {
        return;
    };
    let Ok(Value::Object(mut obj)) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    for key in IDENTITY_KEYS {
        obj.remove(*key);
    }
    let _ = std::fs::write(target, Value::Object(obj).to_string());
}

fn remove_link(path: &Path) {
    if std::fs::remove_dir(path).is_err() {
        let _ = std::fs::remove_file(path);
    }
}

pub fn remove_account_dir(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if is_link(&path) {
                remove_link(&path);
            } else if path.is_dir() {
                remove_account_dir(&path);
            }
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}
