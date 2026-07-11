use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

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

#[tauri::command]
pub fn git_branches(path: String) -> Result<BranchSnapshot, String> {
    snapshot(&path)
}

#[tauri::command]
pub fn git_checkout(path: String, branch: String) -> Result<BranchSnapshot, String> {
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
pub fn git_fetch(path: String) -> Result<BranchSnapshot, String> {
    run_git(&path, &["fetch", "--prune", "--all"])?;
    snapshot(&path)
}

#[tauri::command]
pub fn git_create_branch(
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
pub fn git_rename_branch(
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
pub fn git_delete_branch(
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
pub fn git_merge_branch(path: String, branch: String) -> Result<BranchSnapshot, String> {
    run_git(&path, &["merge", branch.trim()])?;
    snapshot(&path)
}

#[tauri::command]
pub fn git_rebase_onto(path: String, branch: String) -> Result<BranchSnapshot, String> {
    run_git(&path, &["rebase", branch.trim()])?;
    snapshot(&path)
}

#[tauri::command]
pub fn git_update_branch(path: String, rebase: bool) -> Result<BranchSnapshot, String> {
    if rebase {
        run_git(&path, &["pull", "--rebase"])?;
    } else {
        run_git(&path, &["pull", "--ff"])?;
    }
    snapshot(&path)
}

#[tauri::command]
pub fn git_push_current(path: String) -> Result<BranchSnapshot, String> {
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
pub fn git_set_upstream(
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
pub fn git_compare_with_current(path: String, branch: String) -> Result<Vec<CommitInfo>, String> {
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
pub fn git_toggle_branch_favorite(
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
