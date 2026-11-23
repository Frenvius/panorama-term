use crate::orch::git;
use crate::orch::model::{Dispatch, Trigger};
use crate::orch::reducer::RunEntry;

pub(crate) struct Checkpoint {
    pub head: String,
    pub diff_stat: String,
}

pub(crate) struct CheckpointInput<'a> {
    pub previous: &'a Dispatch,
    pub fallback_start_ref: &'a str,
}

fn class_label(dispatch: &Dispatch) -> String {
    dispatch.failure_class
        .and_then(|c| serde_json::to_value(c).ok())
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".into())
}

fn candidate_label(dispatch: &Dispatch) -> String {
    let profile = dispatch.profile.as_deref().unwrap_or("unknown");
    match dispatch.model.as_deref() {
        Some(model) => format!("{profile}/{model}"),
        None => profile.to_string(),
    }
}

pub(crate) fn previous_for_carry(entry: &RunEntry, next: &Dispatch) -> Option<Dispatch> {
    if entry.tasks.get(&next.task_id)?.routing != 1 {
        return None;
    }
    let previous = entry.dispatches.values()
        .filter(|d| d.task_id == next.task_id && d.attempt < next.attempt)
        .max_by_key(|d| d.attempt)?;
    previous.worktree.as_ref()?;
    previous.branch.as_ref()?;
    let carries = match next.trigger {
        Some(Trigger::Fallback) => true,
        Some(Trigger::Resume) => previous.profile != next.profile || previous.model != next.model,
        _ => false,
    };
    carries.then(|| previous.clone())
}

pub(crate) fn checkpoint_previous(input: CheckpointInput) -> Result<Option<Checkpoint>, String> {
    let CheckpointInput { previous, fallback_start_ref } = input;
    let Some(worktree) = previous.worktree.as_deref() else { return Ok(None) };
    if !std::path::Path::new(worktree).exists() {
        return Ok(None);
    }
    let start = previous.start_ref.as_deref().unwrap_or(fallback_start_ref);
    let dirty = !git::status_porcelain(worktree)?.trim().is_empty();
    if !dirty && !git::has_commits_beyond(worktree, start)? {
        return Ok(None);
    }
    if dirty {
        let message = format!("checkpoint: {} {}", previous.id, class_label(previous));
        if let Err(e) = git::commit_leftovers(worktree, &message) {
            if !git::status_porcelain(worktree)?.trim().is_empty() {
                return Err(e);
            }
        }
    }
    let head = git::head_sha(worktree)?;
    let diff_stat = git::diff_stat(worktree, start, &head)?;
    Ok(Some(Checkpoint { head, diff_stat }))
}

pub(crate) fn handoff_block(previous: &Dispatch, checkpoint: &Checkpoint) -> String {
    let mut block = String::from("## Handoff from the previous worker\n\n");
    block.push_str(&format!(
        "Fallback reason: {} on {} (dispatch {}).\n\n",
        class_label(previous),
        candidate_label(previous),
        previous.id,
    ));
    if let Some(summary) = previous.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        block.push_str(&format!("Previous dispatch summary:\n{summary}\n\n"));
    }
    let stat = if checkpoint.diff_stat.is_empty() { "(no file changes)" } else { checkpoint.diff_stat.as_str() };
    block.push_str(&format!("Changes so far (git diff --stat):\n```\n{stat}\n```\n\n"));
    block.push_str("Continue this work or redo it; the previous worker stopped for an operational reason, not because its work was wrong.");
    block
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orch::model::{DispatchStatus, FailureClass};
    use std::path::{Path, PathBuf};

    fn tmp_dir(label: &str) -> PathBuf {
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("orch-carry-{label}-{ns}"));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn git_cmd(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(["-C", dir.to_str().unwrap()])
            .args(args)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo(label: &str) -> (PathBuf, String) {
        let repo = tmp_dir(label);
        git_cmd(&repo, &["init"]);
        git_cmd(&repo, &["config", "user.email", "test@test.local"]);
        git_cmd(&repo, &["config", "user.name", "Test"]);
        std::fs::write(repo.join("init.txt"), "init").unwrap();
        git_cmd(&repo, &["add", "."]);
        git_cmd(&repo, &["commit", "-m", "init"]);
        let base = git::head_sha(repo.to_str().unwrap()).unwrap();
        (repo, base)
    }

    fn old_dispatch(repo: &str, base: &str) -> Dispatch {
        let path = git::worktree_path(repo, "r1", "t1", 1);
        let branch = git::worktree_branch("r1", "t1", 1);
        git::worktree_add(repo, &path, &branch, base).unwrap();
        let mut d = crate::orch::tests::mk_dispatch_entry("r1.t1.1", "t1", 1, None, None, None, 0);
        d.status = DispatchStatus::Failed;
        d.worktree = Some(path);
        d.branch = Some(branch);
        d.failure_class = Some(FailureClass::QuotaExhausted);
        d.profile = Some("claude".into());
        d.model = Some("opus".into());
        d
    }

    fn checkpoint_for(previous: &Dispatch, base: &str) -> Option<Checkpoint> {
        checkpoint_previous(CheckpointInput { previous, fallback_start_ref: base }).unwrap()
    }

    fn commit_count(path: &str) -> usize {
        let out = std::process::Command::new("git")
            .args(["-C", path, "rev-list", "--count", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    }

    #[test]
    fn dirty_old_worktree_gets_checkpoint_and_new_worktree_has_file() {
        let (repo, base) = init_repo("dirty");
        let repo_s = repo.to_str().unwrap();
        let old = old_dispatch(repo_s, &base);
        let old_wt = old.worktree.clone().unwrap();
        std::fs::write(Path::new(&old_wt).join("work.txt"), "partial").unwrap();

        let cp = checkpoint_for(&old, &base).expect("checkpoint");
        let message = String::from_utf8_lossy(
            &std::process::Command::new("git").args(["-C", &old_wt, "log", "-1", "--format=%s"]).output().unwrap().stdout,
        ).trim().to_string();
        assert_eq!(message, "checkpoint: r1.t1.1 quota_exhausted");
        assert!(git::status_porcelain(&old_wt).unwrap().trim().is_empty());
        assert!(cp.diff_stat.contains("work.txt"));

        let new_path = git::worktree_path(repo_s, "r1", "t1", 2);
        git::worktree_add(repo_s, &new_path, &git::worktree_branch("r1", "t1", 2), &cp.head).unwrap();
        assert!(Path::new(&new_path).join("work.txt").exists());
        assert!(Path::new(&old_wt).exists(), "old worktree stays");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn clean_old_worktree_with_commits_carries_head_without_checkpoint_commit() {
        let (repo, base) = init_repo("clean");
        let repo_s = repo.to_str().unwrap();
        let old = old_dispatch(repo_s, &base);
        let old_wt = old.worktree.clone().unwrap();
        std::fs::write(Path::new(&old_wt).join("done.txt"), "done").unwrap();
        git_cmd(Path::new(&old_wt), &["add", "."]);
        git_cmd(Path::new(&old_wt), &["commit", "-m", "worker commit"]);
        let before = commit_count(&old_wt);

        let cp = checkpoint_for(&old, &base).expect("checkpoint");
        assert_eq!(commit_count(&old_wt), before, "no checkpoint commit");
        assert_eq!(cp.head, git::head_sha(&old_wt).unwrap());
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn unchanged_old_worktree_does_not_carry() {
        let (repo, base) = init_repo("none");
        let old = old_dispatch(repo.to_str().unwrap(), &base);
        assert!(checkpoint_for(&old, &base).is_none());
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn chained_carry_uses_recorded_start_ref() {
        let (repo, base) = init_repo("chain");
        let repo_s = repo.to_str().unwrap();
        let mut old = old_dispatch(repo_s, &base);
        let old_wt = old.worktree.clone().unwrap();
        std::fs::write(Path::new(&old_wt).join("a.txt"), "a").unwrap();
        git_cmd(Path::new(&old_wt), &["add", "."]);
        git_cmd(Path::new(&old_wt), &["commit", "-m", "a"]);
        let head = git::head_sha(&old_wt).unwrap();
        old.start_ref = Some(head);
        assert!(checkpoint_for(&old, &base).is_none(), "nothing beyond its own start ref");
        std::fs::remove_dir_all(&repo).ok();
    }

    fn entry_with(routing: u32, old: &Dispatch, next: &Dispatch) -> RunEntry {
        let mut entry = crate::orch::tests::mk_run_entry("r1");
        let mut task = crate::orch::tests::mk_task_entry("t1", crate::orch::model::TaskStatus::Failed, 1);
        task.routing = routing;
        entry.tasks.insert("t1".into(), task);
        entry.dispatches.insert(old.id.clone(), old.clone());
        entry.dispatches.insert(next.id.clone(), next.clone());
        entry
    }

    fn next_dispatch(trigger: Trigger) -> Dispatch {
        let mut d = crate::orch::tests::mk_dispatch_entry("r1.t1.2", "t1", 2, None, None, None, 0);
        d.trigger = Some(trigger);
        d.profile = Some("codex".into());
        d
    }

    #[test]
    fn escalation_and_legacy_never_carry() {
        let (repo, base) = init_repo("gate");
        let old = old_dispatch(repo.to_str().unwrap(), &base);
        let fallback = next_dispatch(Trigger::Fallback);
        assert!(previous_for_carry(&entry_with(1, &old, &fallback), &fallback).is_some());
        let escalation = next_dispatch(Trigger::Escalation);
        assert!(previous_for_carry(&entry_with(1, &old, &escalation), &escalation).is_none());
        assert!(previous_for_carry(&entry_with(0, &old, &fallback), &fallback).is_none());
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn resume_carries_only_on_a_different_candidate() {
        let (repo, base) = init_repo("resume");
        let old = old_dispatch(repo.to_str().unwrap(), &base);
        let mut same = next_dispatch(Trigger::Resume);
        same.profile = old.profile.clone();
        same.model = old.model.clone();
        assert!(previous_for_carry(&entry_with(1, &old, &same), &same).is_none());
        let other = next_dispatch(Trigger::Resume);
        assert!(previous_for_carry(&entry_with(1, &old, &other), &other).is_some());
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn brief_contains_handoff_block() {
        let (repo, base) = init_repo("brief");
        let repo_s = repo.to_str().unwrap();
        let mut old = old_dispatch(repo_s, &base);
        old.summary = Some("wired the parser".into());
        let old_wt = old.worktree.clone().unwrap();
        std::fs::write(Path::new(&old_wt).join("work.txt"), "partial").unwrap();
        let cp = checkpoint_for(&old, &base).expect("checkpoint");
        let handoff = handoff_block(&old, &cp);

        let dir = tmp_dir("brief-out");
        let task = crate::orch::tests::mk_task_entry("t1", crate::orch::model::TaskStatus::Failed, 1);
        let next = next_dispatch(Trigger::Fallback);
        let path = crate::orch::worker::write_brief(crate::orch::worker::BriefInput {
            base: &dir, run_id: "r1", dispatch_id: "r1.t1.2", task: &task, dispatch: &next,
            deps_results: &[], cli_path: "/cli", cwd: "/tmp", merge_note: None, handoff: Some(&handoff),
        }).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("quota_exhausted on claude/opus"));
        assert!(text.contains("wired the parser"));
        assert!(text.contains("work.txt"));
        assert!(text.contains("Continue this work or redo it; the previous worker stopped for an operational reason, not because its work was wrong."));
        std::fs::remove_dir_all(&repo).ok();
        std::fs::remove_dir_all(&dir).ok();
    }
}
