use crate::git_glance::git;

fn worktrees_dir_entry(repo: &str, name: &str) -> String {
    std::path::Path::new(repo).join(".claude").join("worktrees").join(name).to_string_lossy().into_owned()
}

pub(crate) fn worktree_path(repo: &str, run_id: &str, task_id: &str, attempt: u32) -> String {
    worktrees_dir_entry(repo, &format!("orch-{run_id}-{task_id}-{attempt}"))
}

pub(crate) fn worktree_branch(run_id: &str, task_id: &str, attempt: u32) -> String {
    format!("orch/{run_id}/{task_id}-{attempt}")
}

pub(crate) fn integration_worktree_path(repo: &str, run_id: &str) -> String {
    worktrees_dir_entry(repo, &format!("orch-{run_id}-integration"))
}

pub(crate) fn integration_branch(run_id: &str) -> String {
    format!("orch/{run_id}/integration")
}

pub(crate) fn rev_parse(repo: &str, rev: &str) -> Result<String, String> {
    let raw = git(repo, &["rev-parse", rev])?;
    Ok(String::from_utf8_lossy(&raw).trim().to_string())
}

pub(crate) fn worktree_add(repo: &str, path: &str, branch: &str, base: &str) -> Result<(), String> {
    if std::path::Path::new(path).exists() {
        return Err(format!("worktree path already exists: {path}"));
    }
    git(repo, &["worktree", "add", "-b", branch, path, base]).map(|_| ())
}

const REMOVE_ATTEMPTS: u32 = 5;
const REMOVE_RETRY_MS: u64 = 1000;

// Windows releases a killed shell's cwd handle late; git may drop the worktree metadata and
// still fail to delete the folder, which leaves an orphan only the filesystem can remove.
pub(crate) fn worktree_remove(repo: &str, path: &str) -> Result<(), String> {
    let mut last_err = String::new();
    for attempt in 0..REMOVE_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(REMOVE_RETRY_MS));
        }
        let result = if is_registered_worktree(repo, path) {
            git(repo, &["worktree", "remove", path]).map(|_| ())
        } else {
            remove_orphan_dir(path)
        };
        match result {
            Ok(()) => return Ok(()),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

fn remove_orphan_dir(path: &str) -> Result<(), String> {
    if !std::path::Path::new(path).exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(path).map_err(|e| format!("remove {path}: {e}"))
}

fn normalize_path(path: &str) -> String {
    let resolved = std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().trim_start_matches(r"\\?\").to_string())
        .unwrap_or_else(|_| path.to_string());
    let p = resolved.replace('\\', "/").trim_end_matches('/').to_string();
    if cfg!(windows) { p.to_lowercase() } else { p }
}

pub(crate) fn is_registered_worktree(repo: &str, path: &str) -> bool {
    registered_worktree(repo, path).unwrap_or(true)
}

fn registered_worktree(repo: &str, path: &str) -> Result<bool, String> {
    let raw = git(repo, &["worktree", "list", "--porcelain"])?;
    let wanted = normalize_path(path);
    Ok(String::from_utf8_lossy(&raw)
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .any(|p| normalize_path(p) == wanted))
}

pub(crate) fn can_adopt_worktree(repo: &str, path: &str, branch: &str) -> bool {
    std::path::Path::new(path).join(".git").is_file()
        && registered_worktree(repo, path) == Ok(true)
        && crate::git_branch(path).as_deref() == Some(branch)
}

pub(crate) fn status_porcelain(path: &str) -> Result<String, String> {
    let raw = git(path, &["status", "--porcelain"])?;
    Ok(String::from_utf8_lossy(&raw).to_string())
}

fn git_committing(path: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let has_identity = git(path, &["config", "user.name"])
        .map(|r| !String::from_utf8_lossy(&r).trim().is_empty())
        .unwrap_or(false);
    if has_identity {
        return git(path, args);
    }
    let mut full = vec!["-c", "user.name=Panorama", "-c", "user.email=orch@panorama.local"];
    full.extend_from_slice(args);
    git(path, &full)
}

pub(crate) fn commit_leftovers(path: &str, message: &str) -> Result<(), String> {
    git(path, &["add", "-A"])?;
    git_committing(path, &["commit", "-m", message]).map(|_| ())
}

pub(crate) fn head_sha(path: &str) -> Result<String, String> {
    rev_parse(path, "HEAD")
}

pub(crate) fn diff_name_only(path: &str, base_ref: &str) -> Result<Vec<String>, String> {
    let range = format!("{base_ref}..HEAD");
    let raw = git(path, &["diff", "--name-only", &range])?;
    Ok(String::from_utf8_lossy(&raw)
        .lines()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .collect())
}

pub(crate) fn diff_stat(path: &str, from: &str, to: &str) -> Result<String, String> {
    let range = format!("{from}..{to}");
    let raw = git(path, &["diff", "--stat", &range])?;
    Ok(String::from_utf8_lossy(&raw).trim_end().to_string())
}

pub(crate) enum MergeOutcome {
    Success(String),
    Conflict(Vec<String>),
}

pub(crate) fn merge_no_ff(path: &str, branch: &str) -> Result<MergeOutcome, String> {
    match git_committing(path, &["merge", "--no-ff", "--no-edit", branch]) {
        Ok(_) => {
            let sha = head_sha(path)?;
            Ok(MergeOutcome::Success(sha))
        }
        Err(merge_err) => {
            let conflicted: Vec<String> = git(path, &["diff", "--name-only", "--diff-filter=U"])
                .map(|raw| {
                    String::from_utf8_lossy(&raw)
                        .lines()
                        .map(str::to_string)
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let _ = git(path, &["merge", "--abort"]);
            if conflicted.is_empty() {
                Err(format!("merge failed: {merge_err}"))
            } else {
                Ok(MergeOutcome::Conflict(conflicted))
            }
        }
    }
}

pub(crate) fn merge_no_commit(path: &str, branch: &str) -> Result<Vec<String>, String> {
    match git(path, &["merge", "--no-ff", "--no-commit", branch]) {
        Ok(_) => Ok(vec![]),
        Err(merge_err) => {
            let conflicted: Vec<String> = git(path, &["diff", "--name-only", "--diff-filter=U"])
                .map(|raw| String::from_utf8_lossy(&raw).lines().map(str::to_string).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default();
            if conflicted.is_empty() {
                let _ = git(path, &["merge", "--abort"]);
                Err(format!("merge failed: {merge_err}"))
            } else {
                Ok(conflicted)
            }
        }
    }
}

pub(crate) fn reset_hard(path: &str, rev: &str) -> Result<(), String> {
    git(path, &["reset", "--hard", rev]).map(|_| ())
}

pub(crate) fn is_merged_into(repo: &str, branch: &str, into_branch: &str) -> bool {
    git(repo, &["merge-base", "--is-ancestor", branch, into_branch]).is_ok()
}

pub(crate) fn has_commits_beyond(path: &str, base_sha: &str) -> Result<bool, String> {
    let range = format!("{base_sha}..HEAD");
    let raw = git(path, &["rev-list", "--count", &range])?;
    let count: u64 = String::from_utf8_lossy(&raw).trim().parse().unwrap_or(0);
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn tmp_dir(label: &str) -> PathBuf {
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("orch-git-{label}-{ns}"));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn git_cmd(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(["-C", dir.to_str().unwrap()])
            .args(args)
            .status()
            .expect("git");
        assert!(status.success(), "git {:?} failed in {}", args, dir.display());
    }

    fn init_repo(dir: &Path) {
        git_cmd(dir, &["init"]);
        git_cmd(dir, &["config", "user.email", "test@test.local"]);
        git_cmd(dir, &["config", "user.name", "Test"]);
    }

    fn write_commit(dir: &Path, file: &str, content: &str) -> String {
        std::fs::write(dir.join(file), content).unwrap();
        git_cmd(dir, &["add", file]);
        git_cmd(dir, &["commit", "-m", &format!("add {file}")]);
        let out = std::process::Command::new("git")
            .args(["-C", dir.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write_commit_in(dir: &str, file: &str, content: &str) -> String {
        write_commit(Path::new(dir), file, content)
    }

    #[test]
    fn orphan_worktree_folder_is_detected_and_removed() {
        let repo = tmp_dir("wt-orphan");
        init_repo(&repo);
        let base = write_commit(&repo, "init.txt", "init");
        let repo_s = repo.to_str().unwrap();
        let path = worktree_path(repo_s, "r1", "t1", 1);
        worktree_add(repo_s, &path, &worktree_branch("r1", "t1", 1), &base).expect("worktree_add");
        assert!(is_registered_worktree(repo_s, &path));

        let leftover = std::env::temp_dir().join(format!("orch-git-leftover-{}", std::process::id()));
        std::fs::create_dir_all(&leftover).unwrap();
        std::fs::rename(Path::new(&path).join(".git"), leftover.join(".git")).unwrap();
        git_cmd(&repo, &["worktree", "prune"]);
        assert!(!is_registered_worktree(repo_s, &path));
        assert!(Path::new(&path).exists());

        worktree_remove(repo_s, &path).expect("orphan removal");
        assert!(!Path::new(&path).exists());
        std::fs::remove_dir_all(&leftover).ok();
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn worktree_create_from_base_sha() {
        let repo = tmp_dir("wt-base");
        init_repo(&repo);
        let base = write_commit(&repo, "init.txt", "init");
        let repo_s = repo.to_str().unwrap();

        let path = worktree_path(repo_s, "r1", "t1", 1);
        let branch = worktree_branch("r1", "t1", 1);
        worktree_add(repo_s, &path, &branch, &base).expect("worktree_add");

        assert!(Path::new(&path).exists(), "worktree path must exist");
        assert!(Path::new(&format!("{path}/init.txt")).exists(), "base file visible in worktree");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn worktree_create_from_integration_tip() {
        let repo = tmp_dir("wt-int-tip");
        init_repo(&repo);
        let base = write_commit(&repo, "init.txt", "init");
        let repo_s = repo.to_str().unwrap();

        let int_path = integration_worktree_path(repo_s, "r1");
        let int_branch = integration_branch("r1");
        worktree_add(repo_s, &int_path, &int_branch, &base).expect("integration worktree_add");

        write_commit_in(&int_path, "dep.txt", "dep work");

        let wt_path = worktree_path(repo_s, "r1", "t2", 1);
        let wt_branch = worktree_branch("r1", "t2", 1);
        worktree_add(repo_s, &wt_path, &wt_branch, &int_branch).expect("worker worktree_add");

        assert!(Path::new(&format!("{wt_path}/dep.txt")).exists(), "dep work visible from integration tip");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn worktree_add_refuses_existing_path() {
        let repo = tmp_dir("wt-exist-path");
        init_repo(&repo);
        let base = write_commit(&repo, "init.txt", "init");
        let repo_s = repo.to_str().unwrap();

        let path = worktree_path(repo_s, "r1", "t1", 1);
        std::fs::create_dir_all(&path).unwrap();

        let result = worktree_add(repo_s, &path, "orch/r1/t1-1", &base);
        assert!(result.is_err(), "must refuse existing path");
        assert!(result.unwrap_err().contains("already exists"), "error must mention path");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn worktree_add_refuses_existing_branch() {
        let repo = tmp_dir("wt-exist-branch");
        init_repo(&repo);
        let base = write_commit(&repo, "init.txt", "init");
        let repo_s = repo.to_str().unwrap();

        let path1 = worktree_path(repo_s, "r1", "t1", 1);
        let branch = worktree_branch("r1", "t1", 1);
        worktree_add(repo_s, &path1, &branch, &base).expect("first worktree_add");

        let path2 = worktree_path(repo_s, "r1", "t1", 2);
        let result = worktree_add(repo_s, &path2, &branch, &base);
        assert!(result.is_err(), "must refuse existing branch");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn auto_commit_leftovers_cleans_dirty_tree() {
        let repo = tmp_dir("wt-autocommit");
        init_repo(&repo);
        let base = write_commit(&repo, "init.txt", "init");
        let repo_s = repo.to_str().unwrap();

        let path = worktree_path(repo_s, "r1", "t1", 1);
        let branch = worktree_branch("r1", "t1", 1);
        worktree_add(repo_s, &path, &branch, &base).expect("worktree_add");

        std::fs::write(format!("{path}/new.txt"), "uncommitted").unwrap();

        let status_before = status_porcelain(&path).unwrap();
        assert!(!status_before.trim().is_empty(), "must be dirty before commit");

        commit_leftovers(&path, "orch(t1): test task").expect("commit_leftovers");

        let status_after = status_porcelain(&path).unwrap();
        assert!(status_after.trim().is_empty(), "must be clean after commit");

        let sha = head_sha(&path).unwrap();
        assert!(!sha.is_empty(), "head sha present after commit");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn integrate_clean_merge_succeeds() {
        let repo = tmp_dir("wt-merge-clean");
        init_repo(&repo);
        let base = write_commit(&repo, "init.txt", "init");
        let repo_s = repo.to_str().unwrap();

        let wt = worktree_path(repo_s, "r1", "t1", 1);
        let br = worktree_branch("r1", "t1", 1);
        worktree_add(repo_s, &wt, &br, &base).expect("wt add");
        write_commit_in(&wt, "work.txt", "work output");

        let int_path = integration_worktree_path(repo_s, "r1");
        let int_branch = integration_branch("r1");
        worktree_add(repo_s, &int_path, &int_branch, &base).expect("int add");

        let outcome = merge_no_ff(&int_path, &br).expect("merge");
        assert!(matches!(outcome, MergeOutcome::Success(_)), "clean merge should succeed");
        assert!(Path::new(&format!("{int_path}/work.txt")).exists(), "work.txt visible after merge");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn integrate_conflict_aborts_and_returns_files() {
        let repo = tmp_dir("wt-merge-conflict");
        init_repo(&repo);
        let base = write_commit(&repo, "shared.txt", "original");
        let repo_s = repo.to_str().unwrap();

        let wt1 = worktree_path(repo_s, "r1", "t1", 1);
        let br1 = worktree_branch("r1", "t1", 1);
        worktree_add(repo_s, &wt1, &br1, &base).expect("wt1 add");
        write_commit_in(&wt1, "shared.txt", "worker1 version");

        let int_path = integration_worktree_path(repo_s, "r1");
        let int_branch = integration_branch("r1");
        worktree_add(repo_s, &int_path, &int_branch, &base).expect("int add");
        merge_no_ff(&int_path, &br1).expect("first merge");

        let wt2 = worktree_path(repo_s, "r1", "t2", 1);
        let br2 = worktree_branch("r1", "t2", 1);
        worktree_add(repo_s, &wt2, &br2, &base).expect("wt2 add");
        write_commit_in(&wt2, "shared.txt", "worker2 version");

        let outcome = merge_no_ff(&int_path, &br2).expect("conflict merge should not error");
        match outcome {
            MergeOutcome::Conflict(files) => {
                assert!(files.contains(&"shared.txt".to_string()), "shared.txt in conflict list");
            }
            MergeOutcome::Success(_) => panic!("expected conflict, got success"),
        }

        let status = status_porcelain(&int_path).unwrap();
        assert!(status.trim().is_empty(), "integration worktree dirty after conflict abort: {status}");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn merge_no_commit_leaves_conflict_in_progress() {
        let repo = tmp_dir("wt-merge-nocommit");
        init_repo(&repo);
        let base = write_commit(&repo, "shared.txt", "original");
        let repo_s = repo.to_str().unwrap();

        let wt1 = worktree_path(repo_s, "r1", "t1", 1);
        let br1 = worktree_branch("r1", "t1", 1);
        worktree_add(repo_s, &wt1, &br1, &base).expect("wt1 add");
        write_commit_in(&wt1, "shared.txt", "worker1 version");

        let int_path = integration_worktree_path(repo_s, "r1");
        let int_branch = integration_branch("r1");
        worktree_add(repo_s, &int_path, &int_branch, &base).expect("int add");
        write_commit_in(&int_path, "shared.txt", "integration version");

        let wt2 = worktree_path(repo_s, "r1", "t1", 2);
        let br2 = worktree_branch("r1", "t1", 2);
        let tip = rev_parse(repo_s, &int_branch).unwrap();
        worktree_add(repo_s, &wt2, &br2, &tip).expect("wt2 add");

        let files = merge_no_commit(&wt2, &br1).expect("merge in progress");
        assert_eq!(files, vec!["shared.txt".to_string()]);
        let content = std::fs::read_to_string(format!("{wt2}/shared.txt")).unwrap();
        assert!(content.contains("<<<<<<<"), "conflict markers present");

        std::fs::write(format!("{wt2}/shared.txt"), "resolved").unwrap();
        commit_leftovers(&wt2, "resolve").expect("commit merge");
        assert!(is_merged_into(repo_s, &br1, &br2), "resolved branch contains the task branch");
        assert!(matches!(merge_no_ff(&int_path, &br2).expect("merge"), MergeOutcome::Success(_)));
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn worktree_clean_keeps_dirty_worktree() {
        let repo = tmp_dir("wt-clean-dirty");
        init_repo(&repo);
        let base = write_commit(&repo, "init.txt", "init");
        let repo_s = repo.to_str().unwrap();

        let path = worktree_path(repo_s, "r1", "t1", 1);
        let branch = worktree_branch("r1", "t1", 1);
        worktree_add(repo_s, &path, &branch, &base).expect("wt add");

        std::fs::write(format!("{path}/dirty.txt"), "not committed").unwrap();

        let status = status_porcelain(&path).unwrap();
        assert!(!status.trim().is_empty(), "must be dirty");

        let int_branch = integration_branch("r1");
        let merged = is_merged_into(repo_s, &branch, &int_branch);
        assert!(!merged, "unintegrated branch must not appear merged");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn worktree_clean_keeps_unmerged_worktree() {
        let repo = tmp_dir("wt-clean-unmerged");
        init_repo(&repo);
        let base = write_commit(&repo, "init.txt", "init");
        let repo_s = repo.to_str().unwrap();

        let path = worktree_path(repo_s, "r1", "t1", 1);
        let branch = worktree_branch("r1", "t1", 1);
        worktree_add(repo_s, &path, &branch, &base).expect("wt add");
        write_commit_in(&path, "work.txt", "work");

        let int_branch = integration_branch("r1");
        let merged = is_merged_into(repo_s, &branch, &int_branch);
        assert!(!merged, "must not appear merged before integration");

        let has = has_commits_beyond(&path, &base).unwrap();
        assert!(has, "must have commits beyond base");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn rev_parse_fails_for_non_git_dir() {
        let dir = tmp_dir("non-git");
        let result = rev_parse(dir.to_str().unwrap(), "HEAD");
        assert!(result.is_err(), "rev_parse must fail for non-git dir");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reset_hard_undoes_merge() {
        let repo = tmp_dir("wt-reset-hard");
        init_repo(&repo);
        let base = write_commit(&repo, "init.txt", "init");
        let repo_s = repo.to_str().unwrap();

        let wt = worktree_path(repo_s, "r1", "t1", 1);
        let br = worktree_branch("r1", "t1", 1);
        worktree_add(repo_s, &wt, &br, &base).expect("wt add");
        write_commit_in(&wt, "work.txt", "work");

        let int_path = integration_worktree_path(repo_s, "r1");
        let int_branch = integration_branch("r1");
        worktree_add(repo_s, &int_path, &int_branch, &base).expect("int add");
        merge_no_ff(&int_path, &br).expect("merge");

        assert!(std::path::Path::new(&format!("{int_path}/work.txt")).exists(), "work.txt present after merge");

        reset_hard(&int_path, "ORIG_HEAD").expect("reset_hard");

        assert!(!std::path::Path::new(&format!("{int_path}/work.txt")).exists(), "work.txt gone after reset");
        let sha_after = head_sha(&int_path).unwrap();
        assert_eq!(sha_after, base, "head back to base after reset");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn is_merged_into_returns_true_after_merge() {
        let repo = tmp_dir("wt-is-merged");
        init_repo(&repo);
        let base = write_commit(&repo, "init.txt", "init");
        let repo_s = repo.to_str().unwrap();

        let wt = worktree_path(repo_s, "r1", "t1", 1);
        let br = worktree_branch("r1", "t1", 1);
        worktree_add(repo_s, &wt, &br, &base).expect("wt add");
        write_commit_in(&wt, "work.txt", "work");

        let int_path = integration_worktree_path(repo_s, "r1");
        let int_branch = integration_branch("r1");
        worktree_add(repo_s, &int_path, &int_branch, &base).expect("int add");
        merge_no_ff(&int_path, &br).expect("merge");

        assert!(is_merged_into(repo_s, &br, &int_branch), "must be merged after merge");
        std::fs::remove_dir_all(&repo).ok();
    }
}
