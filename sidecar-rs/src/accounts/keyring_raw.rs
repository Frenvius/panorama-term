use std::path::{Path, PathBuf};

pub const MAX_KEYRING_BLOB: usize = 2560;

pub trait SecretIo: Send + Sync {
    fn live_get(&self) -> Result<Option<Vec<u8>>, String>;
    fn live_set(&self, blob: &[u8]) -> Result<(), String>;
    fn live_delete(&self) -> Result<(), String>;
    fn snapshot_get(&self, id: &str) -> Result<Option<Vec<u8>>, String>;
    fn snapshot_set(&self, id: &str, blob: &[u8]) -> Result<(), String>;
    fn snapshot_delete(&self, id: &str);
}

pub struct OsSecrets {
    root: PathBuf,
}

impl OsSecrets {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn file_path(&self, id: &str) -> PathBuf {
        self.root.join("agy").join(format!("{id}.json"))
    }

    fn file_get(&self, id: &str) -> Result<Option<Vec<u8>>, String> {
        match std::fs::read(self.file_path(id)) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("read snapshot file: {e}")),
        }
    }

    fn file_set(&self, id: &str, blob: &[u8]) -> Result<(), String> {
        let path = self.file_path(id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
        }
        write_private(&path, blob)
    }
}

fn write_private(path: &Path, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let tmp = path.with_extension("json.tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp).map_err(|e| format!("create tmp: {e}"))?;
    file.write_all(data).map_err(|e| format!("write tmp: {e}"))?;
    drop(file);
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))
}

fn snapshot_service() -> &'static str {
    if cfg!(debug_assertions) { "panorama-accounts-dev" } else { "panorama-accounts" }
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
mod kr {
    fn entry(id: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(super::snapshot_service(), &format!("agy:{id}")).map_err(|e| format!("keyring entry: {e}"))
    }

    pub fn get(id: &str) -> Result<Option<Vec<u8>>, String> {
        match entry(id)?.get_secret() {
            Ok(b) => Ok(Some(b)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("keyring get: {e}")),
        }
    }

    pub fn set(id: &str, blob: &[u8]) -> Result<(), String> {
        entry(id)?.set_secret(blob).map_err(|e| format!("keyring set: {e}"))
    }

    pub fn delete(id: &str) {
        if let Ok(e) = entry(id) {
            let _ = e.delete_credential();
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod kr {
    pub fn get(_: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }
    pub fn set(_: &str, _: &[u8]) -> Result<(), String> {
        Err("keyring unsupported".into())
    }
    pub fn delete(_: &str) {}
}

impl SecretIo for OsSecrets {
    fn snapshot_get(&self, id: &str) -> Result<Option<Vec<u8>>, String> {
        match kr::get(id) {
            Ok(Some(b)) => Ok(Some(b)),
            Ok(None) => self.file_get(id),
            Err(e) => self.file_get(id)?.map(Some).ok_or(e),
        }
    }

    fn snapshot_set(&self, id: &str, blob: &[u8]) -> Result<(), String> {
        if blob.len() <= MAX_KEYRING_BLOB {
            match kr::set(id, blob) {
                Ok(()) => {
                    let _ = std::fs::remove_file(self.file_path(id));
                    return Ok(());
                }
                Err(e) if !cfg!(target_os = "linux") => return Err(e),
                Err(_) => {}
            }
        }
        self.file_set(id, blob)?;
        kr::delete(id);
        Ok(())
    }

    fn snapshot_delete(&self, id: &str) {
        kr::delete(id);
        let _ = std::fs::remove_file(self.file_path(id));
    }

    fn live_get(&self) -> Result<Option<Vec<u8>>, String> {
        live::get()
    }

    fn live_set(&self, blob: &[u8]) -> Result<(), String> {
        live::set(blob)
    }

    fn live_delete(&self) -> Result<(), String> {
        live::delete()
    }
}

#[cfg(any(windows, target_os = "linux"))]
mod live {
    fn entry() -> Result<keyring::Entry, String> {
        #[cfg(windows)]
        let e = keyring::Entry::new_with_target("gemini:antigravity", "gemini", "antigravity");
        #[cfg(not(windows))]
        let e = keyring::Entry::new("gemini", "antigravity");
        e.map_err(|e| format!("keyring entry: {e}"))
    }

    pub fn get() -> Result<Option<Vec<u8>>, String> {
        match entry()?.get_secret() {
            Ok(b) => Ok(Some(b)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("keyring get: {e}")),
        }
    }

    pub fn set(blob: &[u8]) -> Result<(), String> {
        entry()?.set_secret(blob).map_err(|e| format!("keyring set: {e}"))
    }

    pub fn delete() -> Result<(), String> {
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("keyring delete: {e}")),
        }
    }
}

#[cfg(target_os = "macos")]
mod live {
    use std::io::Write;
    use std::process::{Command, Stdio};

    const SECURITY: &str = "/usr/bin/security";

    pub fn get() -> Result<Option<Vec<u8>>, String> {
        let out = Command::new(SECURITY)
            .args(["find-generic-password", "-s", "gemini", "-a", "antigravity", "-w"])
            .output()
            .map_err(|e| format!("security: {e}"))?;
        if !out.status.success() {
            return if out.status.code() == Some(44) { Ok(None) } else { Err("security read failed".into()) };
        }
        let mut b = out.stdout;
        if b.last() == Some(&b'\n') {
            b.pop();
        }
        Ok(Some(b))
    }

    fn shell_quote(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            if c == '"' || c == '\\' {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
        out
    }

    pub fn set(blob: &[u8]) -> Result<(), String> {
        let secret = std::str::from_utf8(blob).map_err(|_| "secret is not utf-8".to_string())?;
        if secret.contains('\n') || secret.contains('\r') {
            return Err("secret contains a newline".into());
        }
        let line = format!(
            "add-generic-password -U -s \"gemini\" -a \"antigravity\" -w {}\n",
            shell_quote(secret)
        );
        let mut child = Command::new(SECURITY)
            .arg("-i")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("security: {e}"))?;
        child
            .stdin
            .take()
            .ok_or("security stdin")?
            .write_all(line.as_bytes())
            .map_err(|e| format!("security write: {e}"))?;
        let status = child.wait().map_err(|e| format!("security wait: {e}"))?;
        if status.success() { Ok(()) } else { Err("security write failed".into()) }
    }

    pub fn delete() -> Result<(), String> {
        let status = Command::new(SECURITY)
            .args(["delete-generic-password", "-s", "gemini", "-a", "antigravity"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("security: {e}"))?;
        match status.code() {
            Some(0) | Some(44) => Ok(()),
            _ => Err("security delete failed".into()),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod live {
    pub fn get() -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }
    pub fn set(_: &[u8]) -> Result<(), String> {
        Err("keyring unsupported".into())
    }
    pub fn delete() -> Result<(), String> {
        Ok(())
    }
}
