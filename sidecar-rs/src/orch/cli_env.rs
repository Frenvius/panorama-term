use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const PATH_START: &str = "__PANORAMA_PATH_START__";
const PATH_END: &str = "__PANORAMA_PATH_END__";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
fn hidden(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut c = Command::new(program);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

#[cfg(not(windows))]
fn hidden(program: &str) -> Command {
    Command::new(program)
}

#[cfg(windows)]
fn path_probe() -> Command {
    let mut c = hidden(&crate::resolve_powershell());
    c.args([
        "-NoLogo",
        "-NonInteractive",
        "-Command",
        &format!("Write-Output ('{PATH_START}' + $env:PATH + '{PATH_END}')"),
    ]);
    c
}

#[cfg(not(windows))]
fn path_probe() -> Command {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let mut c = Command::new(shell);
    c.args(["-ilc", &format!("printf '%s%s%s' '{PATH_START}' \"$PATH\" '{PATH_END}'")]);
    c
}

fn capture_stdout(mut cmd: Command, timeout: Duration) -> Option<String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        buf
    });
    let deadline = Instant::now() + timeout;
    while child.try_wait().ok()?.is_none() {
        if Instant::now() >= deadline {
            kill_tree(&mut child);
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    reader.join().ok()
}

fn probe_login_path() -> Option<String> {
    let out = capture_stdout(path_probe(), PROBE_TIMEOUT)?;
    let start = out.find(PATH_START)? + PATH_START.len();
    let end = out[start..].find(PATH_END)? + start;
    let path = out[start..end].trim();
    (!path.is_empty()).then(|| path.to_string())
}

fn login_path() -> Option<String> {
    static CACHE: OnceLock<String> = OnceLock::new();
    if let Some(path) = CACHE.get() {
        return Some(path.clone());
    }
    let path = probe_login_path()?;
    Some(CACHE.get_or_init(|| path).clone())
}

pub fn prewarm() {
    std::thread::spawn(login_path);
}

pub fn cli_path() -> Option<String> {
    let login = login_path();
    let current = std::env::var("PATH").ok();
    let sep = if cfg!(windows) { ";" } else { ":" };
    match (login, current) {
        (Some(l), Some(c)) => Some(merge_paths(&l, &c, sep)),
        (l, c) => l.or(c),
    }
}

fn merge_paths(first: &str, second: &str, sep: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    first
        .split(sep)
        .chain(second.split(sep))
        .filter(|entry| !entry.is_empty())
        .filter(|entry| {
            let key = entry.trim_end_matches(['\\', '/']);
            seen.insert(if cfg!(windows) { key.to_lowercase() } else { key.to_string() })
        })
        .collect::<Vec<_>>()
        .join(sep)
}

#[cfg(windows)]
pub fn cli_command(program: &str) -> Command {
    let mut c = hidden("cmd.exe");
    c.args(["/d", "/c", program]);
    if let Some(path) = cli_path() {
        c.env("PATH", path);
    }
    c
}

#[cfg(not(windows))]
pub fn cli_command(program: &str) -> Command {
    let mut c = Command::new(program);
    if let Some(path) = cli_path() {
        c.env("PATH", path);
    }
    c
}

#[cfg(windows)]
pub fn kill_tree(child: &mut std::process::Child) {
    let _ = hidden("taskkill")
        .args(["/T", "/F", "/PID", &child.id().to_string()])
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(windows))]
pub fn kill_tree(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::merge_paths;

    #[test]
    fn merge_paths_drops_duplicates_keeping_first_order() {
        let merged = merge_paths("/a;/b/;/c", "/b;/d;;/a", ";");
        assert_eq!(merged, "/a;/b/;/c;/d");
    }

    #[cfg(windows)]
    #[test]
    fn merge_paths_is_case_insensitive_on_windows() {
        assert_eq!(merge_paths(r"C:\Bin", r"c:\bin\;D:\x", ";"), r"C:\Bin;D:\x");
    }
}
