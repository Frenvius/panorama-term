use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tauri::Emitter;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Serialize)]
pub struct LocalBranch {
    name: String,
    is_current: bool,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    is_favorite: bool,
}

#[derive(Serialize)]
pub struct RemoteBranch {
    remote: String,
    branch: String,
    is_favorite: bool,
}

#[derive(Serialize)]
pub struct BranchSnapshot {
    current: Option<String>,
    local: Vec<LocalBranch>,
    remotes: Vec<RemoteBranch>,
    recent: Vec<String>,
}

#[derive(Serialize)]
pub struct CommitInfo {
    hash: String,
    short: String,
    subject: String,
    author: String,
    date: String,
}

#[derive(Serialize)]
pub struct LogRow {
    short: String,
    parents: Vec<String>,
    author: String,
    committer: String,
    date: String,
    refs: String,
    message: String,
}

#[derive(Serialize)]
pub struct FileChange {
    path: String,
    name: String,
    dir: String,
    status_index: String,
    status_worktree: String,
    is_untracked: bool,
    rename_from: Option<String>,
}

#[derive(Serialize)]
pub struct StatusSnapshot {
    changes: Vec<FileChange>,
    unversioned: Vec<FileChange>,
}

#[derive(Serialize)]
pub struct CommitMessageEntry {
    short: String,
    subject: String,
    body: String,
    date: String,
}

fn run_git(repo: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("git: {}", e))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn favorites_path() -> Result<PathBuf, String> {
    let dir = dirs::config_dir().ok_or("no config dir")?.join("panorama");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("branch-favorites.json"))
}

fn read_favorites() -> HashMap<String, Vec<String>> {
    let Ok(path) = favorites_path() else {
        return HashMap::new();
    };
    fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_favorites(map: &HashMap<String, Vec<String>>) -> Result<(), String> {
    let path = favorites_path()?;
    let text = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| e.to_string())
}

fn paths_equal(a: &str, b: &str) -> bool {
    if cfg!(target_os = "windows") {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn favorites_for(repo: &str) -> Vec<String> {
    read_favorites()
        .into_iter()
        .find(|(k, _)| paths_equal(k, repo))
        .map(|(_, v)| v)
        .unwrap_or_default()
}

fn remote_names(repo: &str) -> Vec<String> {
    run_git(repo, &["remote"])
        .map(|out| {
            out.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn head_name(repo: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn parse_track(s: &str) -> (u32, u32) {
    let mut ahead = 0;
    let mut behind = 0;
    for part in s.split(',') {
        let p = part.trim();
        if let Some(n) = p.strip_prefix("ahead ") {
            ahead = n.trim().parse().unwrap_or(0);
        } else if let Some(n) = p.strip_prefix("behind ") {
            behind = n.trim().parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

fn recent_branches(repo: &str, locals: &[String]) -> Vec<String> {
    let Ok(out) = run_git(repo, &["reflog", "--format=%gs", "-n", "300"]) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut recent = Vec::new();
    for line in out.lines() {
        let Some(rest) = line.strip_prefix("checkout: moving from ") else {
            continue;
        };
        let Some((_, to)) = rest.split_once(" to ") else {
            continue;
        };
        let to = to.trim().to_string();
        if locals.contains(&to) && seen.insert(to.clone()) {
            recent.push(to);
            if recent.len() >= 5 {
                break;
            }
        }
    }
    recent
}

fn snapshot(repo: &str) -> Result<BranchSnapshot, String> {
    let favorites = favorites_for(repo);
    let is_fav = |full: &str| favorites.iter().any(|f| f == full);

    let local_out = run_git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:short)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(HEAD)",
            "refs/heads",
        ],
    )?;

    let mut local = Vec::new();
    let mut current: Option<String> = None;
    for line in local_out.lines() {
        let parts: Vec<&str> = line.splitn(4, '\u{0}').collect();
        if parts.len() < 4 {
            continue;
        }
        let name = parts[0].to_string();
        let (ahead, behind) = parse_track(parts[2].trim());
        let is_current = parts[3].trim() == "*";
        if is_current {
            current = Some(name.clone());
        }
        let upstream = Some(parts[1].trim().to_string()).filter(|u| !u.is_empty());
        let is_favorite = is_fav(&name);
        local.push(LocalBranch {
            name,
            is_current,
            upstream,
            ahead,
            behind,
            is_favorite,
        });
    }

    if current.is_none() {
        if let Some(head) = head_name(repo) {
            let is_favorite = is_fav(&head);
            local.insert(
                0,
                LocalBranch {
                    name: head.clone(),
                    is_current: true,
                    upstream: None,
                    ahead: 0,
                    behind: 0,
                    is_favorite,
                },
            );
            current = Some(head);
        }
    }

    let remote_out = run_git(
        repo,
        &["for-each-ref", "--format=%(refname:short)", "refs/remotes"],
    )?;
    let mut remotes = Vec::new();
    for line in remote_out.lines() {
        let line = line.trim();
        if line.is_empty() || line.ends_with("/HEAD") {
            continue;
        }
        if let Some((remote, branch)) = line.split_once('/') {
            remotes.push(RemoteBranch {
                remote: remote.to_string(),
                branch: branch.to_string(),
                is_favorite: is_fav(line),
            });
        }
    }

    let names: Vec<String> = local.iter().map(|b| b.name.clone()).collect();
    let recent = recent_branches(repo, &names);

    Ok(BranchSnapshot {
        current,
        local,
        remotes,
        recent,
    })
}

fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let (mut ai, mut bi) = (0usize, 0usize);
    while ai < a_bytes.len() && bi < b_bytes.len() {
        let ac = a_bytes[ai];
        let bc = b_bytes[bi];
        if ac.is_ascii_digit() && bc.is_ascii_digit() {
            let a_start = ai;
            while ai < a_bytes.len() && a_bytes[ai].is_ascii_digit() {
                ai += 1;
            }
            let b_start = bi;
            while bi < b_bytes.len() && b_bytes[bi].is_ascii_digit() {
                bi += 1;
            }
            let a_num = &a_bytes[a_start..ai];
            let b_num = &b_bytes[b_start..bi];
            let a_trim = a_num
                .iter()
                .position(|&c| c != b'0')
                .map_or(&a_num[a_num.len()..], |i| &a_num[i..]);
            let b_trim = b_num
                .iter()
                .position(|&c| c != b'0')
                .map_or(&b_num[b_num.len()..], |i| &b_num[i..]);
            match a_trim.len().cmp(&b_trim.len()).then_with(|| a_trim.cmp(b_trim)) {
                Ordering::Equal => {}
                other => return other,
            }
        } else {
            match ac.to_ascii_lowercase().cmp(&bc.to_ascii_lowercase()) {
                Ordering::Equal => {
                    ai += 1;
                    bi += 1;
                }
                other => return other,
            }
        }
    }
    a_bytes.len().cmp(&b_bytes.len())
}

fn split_name_dir(p: &str) -> (String, String) {
    match p.rfind('/') {
        Some(idx) => (p[idx + 1..].to_string(), p[..idx].to_string()),
        None => (p.to_string(), String::new()),
    }
}

fn basename(p: &str) -> String {
    let trimmed = p.trim_end_matches(['\\', '/']);
    if trimmed.is_empty() {
        return p.to_string();
    }
    trimmed
        .rsplit(|c: char| c == '\\' || c == '/')
        .next()
        .map(|s| s.to_string())
        .unwrap_or_else(|| p.to_string())
}

fn parse_porcelain_z(out: &[u8]) -> Vec<FileChange> {
    let mut entries = Vec::new();
    let mut i = 0;
    while i + 3 <= out.len() {
        let x = out[i] as char;
        let y = out[i + 1] as char;
        let start = i + 3;
        let mut j = start;
        while j < out.len() && out[j] != 0 {
            j += 1;
        }
        if j >= out.len() {
            break;
        }
        let path = String::from_utf8_lossy(&out[start..j]).into_owned();
        i = j + 1;

        let mut rename_from = None;
        if x == 'R' || x == 'C' {
            let r_start = i;
            let mut k = r_start;
            while k < out.len() && out[k] != 0 {
                k += 1;
            }
            rename_from = Some(String::from_utf8_lossy(&out[r_start..k]).into_owned());
            i = k + 1;
        }

        let (name, dir) = split_name_dir(&path);
        let is_untracked = x == '?' && y == '?';
        entries.push(FileChange {
            path,
            name,
            dir,
            status_index: x.to_string(),
            status_worktree: y.to_string(),
            is_untracked,
            rename_from,
        });
    }
    entries
}

fn parse_commit_entries(raw: &str) -> Vec<CommitMessageEntry> {
    let mut entries = Vec::new();
    for chunk in raw.split('\x1e') {
        let chunk = chunk.trim_start_matches('\n');
        if chunk.is_empty() {
            continue;
        }
        let parts: Vec<&str> = chunk.splitn(3, '\x1f').collect();
        if parts.len() < 3 {
            continue;
        }
        let short = parts[0].trim().to_string();
        let date = parts[1].trim().to_string();
        let body = parts[2].trim_end().to_string();
        let subject = body.lines().next().unwrap_or("").to_string();
        entries.push(CommitMessageEntry {
            short,
            subject,
            body,
            date,
        });
    }
    entries
}

fn repo_root(path: &str) -> Result<String, String> {
    run_git(path, &["rev-parse", "--show-toplevel"]).map(|out| out.trim().to_string())
}

#[tauri::command]
pub async fn git_status(path: String) -> Result<StatusSnapshot, String> {
    let path = repo_root(&path)?;
    let out = Command::new("git")
        .arg("-C")
        .arg(&path)
        .args(["status", "--porcelain=v1", "-uall", "-z"])
        .output()
        .map_err(|e| format!("git: {}", e))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    let repo_name = basename(&path);
    let mut changes = Vec::new();
    let mut unversioned = Vec::new();
    for mut entry in parse_porcelain_z(&out.stdout) {
        if entry.dir.is_empty() {
            entry.dir = repo_name.clone();
        }
        if entry.is_untracked {
            unversioned.push(entry);
        } else {
            changes.push(entry);
        }
    }

    let by_name =
        |a: &FileChange, b: &FileChange| natural_cmp(&a.name, &b.name).then_with(|| natural_cmp(&a.path, &b.path));
    changes.sort_by(by_name);
    unversioned.sort_by(by_name);

    Ok(StatusSnapshot {
        changes,
        unversioned,
    })
}

#[tauri::command]
pub async fn git_commit(path: String, files: Vec<String>, message: String, amend: bool) -> Result<(), String> {
    if files.is_empty() && !amend {
        return Err("No files selected".into());
    }
    if message.trim().is_empty() && !amend {
        return Err("Commit message is empty".into());
    }
    let path = repo_root(&path)?;

    if !files.is_empty() {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&path).arg("add").arg("--");
        for f in &files {
            cmd.arg(f);
        }
        let out = cmd.output().map_err(|e| format!("git: {}", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
    }

    let mut args = vec!["commit", "-m", &message];
    if amend {
        args.push("--amend");
    }
    run_git(&path, &args)?;
    Ok(())
}

#[tauri::command]
pub async fn git_log_messages(path: String, limit: Option<u32>) -> Result<Vec<CommitMessageEntry>, String> {
    let n = limit.unwrap_or(20).to_string();
    let out = run_git(
        &path,
        &["log", "--format=%x1e%h%x1f%ad%x1f%B", "--date=short", "-n", &n],
    )?;
    Ok(parse_commit_entries(&out))
}

fn parse_log_rows(raw: &str) -> Vec<LogRow> {
    let mut rows = Vec::new();
    for chunk in raw.split('\x1e') {
        let chunk = chunk.trim_start_matches('\n');
        if chunk.is_empty() {
            continue;
        }
        let parts: Vec<&str> = chunk.splitn(7, '\x1f').collect();
        if parts.len() < 7 {
            continue;
        }
        rows.push(LogRow {
            short: parts[0].to_string(),
            parents: parts[1].split_whitespace().map(str::to_string).collect(),
            author: parts[2].to_string(),
            committer: parts[3].to_string(),
            date: parts[4].to_string(),
            refs: parts[5].trim().to_string(),
            message: parts[6].trim_end().to_string(),
        });
    }
    rows
}

#[tauri::command]
pub async fn git_log_graph(path: String, limit: Option<u32>) -> Result<Vec<LogRow>, String> {
    let n = limit.unwrap_or(200).to_string();
    let out = run_git(
        &path,
        &[
            "log",
            "--format=%x1e%h%x1f%p%x1f%an%x1f%cn%x1f%ad%x1f%D%x1f%B",
            "--date=format:%Y-%m-%d %H:%M",
            "-n",
            &n,
        ],
    )?;
    Ok(parse_log_rows(&out))
}

#[tauri::command]
pub async fn git_remote_url(path: String) -> Result<String, String> {
    let names = remote_names(&path);
    let name = if names.iter().any(|n| n == "origin") {
        "origin".to_string()
    } else {
        names.into_iter().next().ok_or("no remote")?
    };
    Ok(run_git(&path, &["remote", "get-url", &name])?.trim().to_string())
}

#[tauri::command]
pub async fn git_unpushed_commits(path: String) -> Result<Vec<CommitMessageEntry>, String> {
    let fmt = "--format=%x1e%h%x1f%ad%x1f%B";
    let out = run_git(&path, &["log", "@{u}..HEAD", fmt, "--date=short"])
        .or_else(|_| run_git(&path, &["log", "-n", "10", fmt, "--date=short"]))?;
    Ok(parse_commit_entries(&out))
}

#[derive(Serialize)]
pub struct TrackCounts {
    ahead: u32,
    behind: u32,
}

#[tauri::command]
pub async fn git_ahead_behind(path: String) -> TrackCounts {
    let (ahead, behind) = run_git(&path, &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"])
        .ok()
        .and_then(|out| {
            let mut it = out.split_whitespace();
            let behind = it.next()?.parse().ok()?;
            let ahead = it.next()?.parse().ok()?;
            Some((ahead, behind))
        })
        .unwrap_or((0, 0));
    TrackCounts { ahead, behind }
}

#[tauri::command]
pub async fn git_branches(path: String) -> Result<BranchSnapshot, String> {
    snapshot(&path)
}

#[tauri::command]
pub async fn git_checkout(path: String, branch: String) -> Result<BranchSnapshot, String> {
    let target = match branch.split_once('/') {
        Some((remote, rest)) if remote_names(&path).iter().any(|r| r == remote) => rest.to_string(),
        _ => branch.clone(),
    };

    let snap = snapshot(&path)?;
    if snap.current.as_deref() == Some(target.as_str()) {
        return Ok(snap);
    }

    run_git(&path, &["switch", &target])?;
    snapshot(&path)
}

#[tauri::command]
pub async fn git_fetch(path: String) -> Result<BranchSnapshot, String> {
    run_git(&path, &["fetch", "--prune", "--all"])?;
    snapshot(&path)
}

#[tauri::command]
pub async fn git_create_branch(
    path: String,
    name: String,
    checkout: bool,
    overwrite: bool,
    start_point: Option<String>,
) -> Result<BranchSnapshot, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Branch name is empty".into());
    }

    let start = start_point
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut args: Vec<&str> = match (checkout, overwrite) {
        (true, true) => vec!["checkout", "-B", name],
        (true, false) => vec!["switch", "-c", name],
        (false, true) => vec!["branch", "-f", name],
        (false, false) => vec!["branch", name],
    };
    if let Some(s) = start {
        args.push(s);
    }
    run_git(&path, &args)?;
    snapshot(&path)
}

#[tauri::command]
pub async fn git_rename_branch(
    path: String,
    old_name: String,
    new_name: String,
) -> Result<BranchSnapshot, String> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err("Branch name is empty".into());
    }
    run_git(&path, &["branch", "-m", old_name.trim(), new_name])?;

    let mut map = read_favorites();
    if let Some((_, list)) = map.iter_mut().find(|(k, _)| paths_equal(k, &path)) {
        if let Some(slot) = list.iter_mut().find(|b| b.as_str() == old_name.trim()) {
            *slot = new_name.to_string();
            let _ = write_favorites(&map);
        }
    }
    snapshot(&path)
}

#[tauri::command]
pub async fn git_delete_branch(
    path: String,
    full_name: String,
    is_remote: bool,
) -> Result<BranchSnapshot, String> {
    if is_remote {
        let Some((remote, branch)) = full_name.split_once('/') else {
            return Err(format!("Invalid remote branch: {}", full_name));
        };
        run_git(&path, &["push", remote, "--delete", branch])?;
    } else {
        run_git(&path, &["branch", "-D", full_name.trim()])?;
    }

    let mut map = read_favorites();
    if let Some((_, list)) = map.iter_mut().find(|(k, _)| paths_equal(k, &path)) {
        list.retain(|b| b.as_str() != full_name.as_str());
        let _ = write_favorites(&map);
    }
    snapshot(&path)
}

#[tauri::command]
pub async fn git_merge_branch(path: String, branch: String) -> Result<BranchSnapshot, String> {
    run_git(&path, &["merge", branch.trim()])?;
    snapshot(&path)
}

#[tauri::command]
pub async fn git_rebase_onto(path: String, branch: String) -> Result<BranchSnapshot, String> {
    run_git(&path, &["rebase", branch.trim()])?;
    snapshot(&path)
}

#[tauri::command]
pub async fn git_update_branch(path: String, rebase: bool) -> Result<BranchSnapshot, String> {
    if rebase {
        run_git(&path, &["pull", "--rebase"])?;
    } else {
        run_git(&path, &["pull", "--ff"])?;
    }
    snapshot(&path)
}

#[tauri::command]
pub async fn git_push_current(path: String) -> Result<BranchSnapshot, String> {
    if let Err(e) = run_git(&path, &["push"]) {
        let needs_upstream = e.contains("no upstream") || e.contains("set-upstream");
        let Some(current) = head_name(&path).filter(|_| needs_upstream) else {
            return Err(e);
        };
        run_git(&path, &["push", "--set-upstream", "origin", &current])?;
    }
    snapshot(&path)
}

#[tauri::command]
pub async fn git_set_upstream(
    path: String,
    branch: String,
    upstream: Option<String>,
) -> Result<BranchSnapshot, String> {
    match upstream.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(up) => run_git(
            &path,
            &["branch", &format!("--set-upstream-to={}", up), branch.trim()],
        )?,
        None => run_git(&path, &["branch", "--unset-upstream", branch.trim()])?,
    };
    snapshot(&path)
}

#[tauri::command]
pub async fn git_compare_with_current(path: String, branch: String) -> Result<Vec<CommitInfo>, String> {
    let current = head_name(&path).unwrap_or_else(|| "HEAD".into());
    let range = format!("{}..{}", current, branch.trim());
    let out = run_git(
        &path,
        &[
            "log",
            "--format=%H%00%h%00%s%00%an%00%ad",
            "--date=short",
            &range,
        ],
    )?;

    let mut commits = Vec::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.splitn(5, '\u{0}').collect();
        if parts.len() < 5 {
            continue;
        }
        commits.push(CommitInfo {
            hash: parts[0].to_string(),
            short: parts[1].to_string(),
            subject: parts[2].to_string(),
            author: parts[3].to_string(),
            date: parts[4].to_string(),
        });
    }
    Ok(commits)
}

#[tauri::command]
pub async fn git_toggle_branch_favorite(
    path: String,
    full_name: String,
) -> Result<BranchSnapshot, String> {
    let mut map = read_favorites();
    let key = map
        .keys()
        .find(|k| paths_equal(k, &path))
        .cloned()
        .unwrap_or_else(|| path.clone());
    let entry = map.entry(key).or_default();
    match entry.iter().position(|b| b == &full_name) {
        Some(pos) => {
            entry.remove(pos);
        }
        None => entry.push(full_name),
    }
    write_favorites(&map)?;
    snapshot(&path)
}

#[derive(Serialize)]
pub struct FileDiff {
    old: String,
    new: String,
    binary: bool,
    crlf: bool,
}

fn is_binary(text: &[u8]) -> bool {
    text.iter().take(8000).any(|b| *b == 0)
}

fn rename_source(repo: &str, file: &str) -> Option<String> {
    let out = run_git(repo, &["diff", "-M", "--name-status", "HEAD", "--", file]).ok()?;
    out.lines().find_map(|line| {
        let mut cols = line.split('\t');
        let status = cols.next()?;
        if !status.starts_with('R') {
            return None;
        }
        let from = cols.next()?;
        let to = cols.next()?;
        (to == file).then(|| from.to_string())
    })
}

#[tauri::command]
pub async fn git_diff_file(path: String, file: String) -> Result<FileDiff, String> {
    let mut old = run_git(&path, &["show", &format!("HEAD:{}", file)]).unwrap_or_default();

    if old.is_empty() {
        if let Some(from) = rename_source(&path, &file) {
            old = run_git(&path, &["show", &format!("HEAD:{}", from)]).unwrap_or_default();
        }
    }

    let full = PathBuf::from(&path).join(&file);
    let bytes = fs::read(&full).unwrap_or_default();
    if is_binary(&bytes) || is_binary(old.as_bytes()) {
        return Ok(FileDiff {
            old: String::new(),
            new: String::new(),
            binary: true,
            crlf: false,
        });
    }

    let new = String::from_utf8_lossy(&bytes).into_owned();
    let crlf = new.contains("\r\n");

    Ok(FileDiff {
        old: old.replace("\r\n", "\n"),
        new: new.replace("\r\n", "\n"),
        binary: false,
        crlf,
    })
}

static NEXT_WATCH: AtomicU32 = AtomicU32::new(1);

fn watchers() -> &'static Mutex<HashMap<u32, RecommendedWatcher>> {
    static WATCHERS: OnceLock<Mutex<HashMap<u32, RecommendedWatcher>>> = OnceLock::new();
    WATCHERS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[tauri::command]
pub async fn git_watch_file(app: tauri::AppHandle, path: String, file: String) -> Result<u32, String> {
    let full = PathBuf::from(&path).join(&file);
    let dir = full.parent().ok_or("no parent dir")?.to_path_buf();
    let name = full.file_name().ok_or("no file name")?.to_os_string();
    let git_dir = PathBuf::from(&path).join(".git");

    let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        let Ok(event) = res else { return };
        if event.kind.is_access() {
            return;
        }
        let relevant = event
            .paths
            .iter()
            .any(|p| p.file_name().is_some_and(|n| n == name || n == "HEAD" || n == "index"));
        if relevant {
            let _ = app.emit(
                "diff:changed",
                serde_json::json!({ "root": path, "file": file }),
            );
        }
    })
    .map_err(|e| e.to_string())?;

    watcher
        .watch(&dir, RecursiveMode::NonRecursive)
        .map_err(|e| e.to_string())?;
    if git_dir.is_dir() {
        let _ = watcher.watch(&git_dir, RecursiveMode::NonRecursive);
    }

    let id = NEXT_WATCH.fetch_add(1, Ordering::Relaxed);
    watchers().lock().unwrap().insert(id, watcher);
    Ok(id)
}

#[tauri::command]
pub async fn git_unwatch_file(id: u32) {
    watchers().lock().unwrap().remove(&id);
}

#[tauri::command]
pub async fn git_rollback_file(path: String, file: String) -> Result<(), String> {
    if run_git(&path, &["cat-file", "-e", &format!("HEAD:{}", file)]).is_ok() {
        run_git(&path, &["checkout", "HEAD", "--", &file])?;
        return Ok(());
    }
    let _ = run_git(&path, &["rm", "--cached", "--force", "--", &file]);
    let full = PathBuf::from(&path).join(&file);
    if full.is_dir() {
        fs::remove_dir_all(&full).map_err(|e| e.to_string())
    } else {
        fs::remove_file(&full).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn git_add_ignore(path: String, pattern: String, local: bool) -> Result<(), String> {
    let file = if local {
        let dir = run_git(&path, &["rev-parse", "--git-common-dir"])?;
        let mut base = PathBuf::from(dir.trim());
        if base.is_relative() {
            base = PathBuf::from(&path).join(base);
        }
        let info = base.join("info");
        fs::create_dir_all(&info).map_err(|e| e.to_string())?;
        info.join("exclude")
    } else {
        PathBuf::from(&path).join(".gitignore")
    };
    let existing = fs::read_to_string(&file).unwrap_or_default();
    let line = format!("/{}", pattern.trim_start_matches('/'));
    if existing.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&line);
    out.push('\n');
    fs::write(&file, out).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn git_revert_hunk(path: String, file: String, content: String, crlf: bool) -> Result<(), String> {
    let full = PathBuf::from(&path).join(&file);
    let text = if crlf {
        content.replace('\n', "\r\n")
    } else {
        content
    };
    fs::write(&full, text).map_err(|e| e.to_string())
}
