use std::path::Path;
use serde::Deserialize;
use crate::git_glance::git;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LinkEntry {
    pub path: String,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoConfig {
    #[serde(default)]
    pub link: Vec<LinkEntry>,
    #[serde(default)]
    pub copy: Vec<String>,
    #[serde(default)]
    pub exclusive: Vec<String>,
}

impl RepoConfig {
    pub fn load(repo: &str) -> Result<Option<Self>, String> {
        let path = Path::new(repo).join(".panorama").join("worktree.json");
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("read .panorama/worktree.json: {e}"))?;
        let cfg: Self = serde_json::from_str(&text)
            .map_err(|e| format!("parse .panorama/worktree.json: {e}"))?;
        Ok(Some(cfg))
    }

    pub fn validate_exclusive_names(&self, names: &[String]) -> Result<(), String> {
        for name in names {
            if name == "main" {
                return Err(format!("exclusive name 'main' is reserved for in-place tasks"));
            }
            if !self.exclusive.iter().any(|n| n == name) {
                return Err(format!("unknown exclusive name '{name}'; add it to .panorama/worktree.json exclusive list"));
            }
        }
        Ok(())
    }
}

fn validate_path(repo: &str, path: &str, kind: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.is_absolute() || path.starts_with('/') || path.starts_with('\\') {
        return Err(format!("{kind} path '{path}' must be relative"));
    }
    for component in p.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(format!("{kind} path '{path}' must not contain '..'"));
        }
    }
    if !Path::new(repo).join(path).exists() {
        return Err(format!("{kind} path '{path}' does not exist in the main checkout"));
    }
    if git(repo, &["check-ignore", "-q", path]).is_err() {
        return Err(format!("'{path}' is not git-ignored in the main checkout; add it to .gitignore"));
    }
    Ok(())
}

pub struct MaterializeOpts<'a> {
    pub repo: &'a str,
    pub worktree: &'a str,
    pub dispatch_dir: &'a Path,
    pub config: &'a RepoConfig,
}

#[derive(Debug)]
pub struct MaterializeResult {
    pub link_paths: Vec<String>,
    pub read_only_paths: Vec<String>,
}

pub fn materialize(opts: MaterializeOpts<'_>) -> Result<MaterializeResult, String> {
    let MaterializeOpts { repo, worktree, dispatch_dir, config } = opts;
    let mut link_paths: Vec<String> = Vec::new();
    let mut read_only_paths: Vec<String> = Vec::new();

    for entry in &config.link {
        let path = &entry.path;
        validate_path(repo, path, "link")?;

        let target = Path::new(repo).join(path);
        if !target.is_dir() {
            return Err(format!("link path '{path}' is a file; use 'copy' for files"));
        }

        let link = Path::new(worktree).join(path);
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create parent for link '{path}': {e}"))?;
        }

        create_link(&target, &link)?;

        if git(worktree, &["check-ignore", "-q", path]).is_err() {
            let _ = remove_link_path(&link);
            return Err(format!("'{path}' is not ignored as a link; ignore it without a trailing slash"));
        }

        link_paths.push(path.to_string());
        if entry.read_only {
            read_only_paths.push(path.to_string());
        }
    }

    for path in &config.copy {
        validate_path(repo, path, "copy")?;
        let src = Path::new(repo).join(path);
        let dst = Path::new(worktree).join(path);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create parent for copy '{path}': {e}"))?;
        }
        copy_recursive(&src, &dst)?;
    }

    let links_json = serde_json::to_string(&link_paths)
        .map_err(|e| format!("serialize links.json: {e}"))?;
    std::fs::write(dispatch_dir.join("links.json"), links_json)
        .map_err(|e| format!("write links.json: {e}"))?;

    Ok(MaterializeResult { link_paths, read_only_paths })
}

pub fn unlink_recorded(dispatch_dir: &Path, worktree: &str) -> Result<(), String> {
    let links_path = dispatch_dir.join("links.json");
    if !links_path.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&links_path)
        .map_err(|e| format!("read links.json: {e}"))?;
    let links: Vec<String> = serde_json::from_str(&text)
        .map_err(|e| format!("parse links.json: {e}"))?;

    let mut still_present: Vec<String> = Vec::new();
    for rel_path in &links {
        let link = Path::new(worktree).join(rel_path);
        if is_link(&link) {
            remove_link_path(&link)?;
        }
        if is_link(&link) {
            still_present.push(rel_path.clone());
        }
    }
    if !still_present.is_empty() {
        return Err(format!(
            "links still present after removal: {}",
            still_present.join(", ")
        ));
    }
    Ok(())
}

pub fn is_link(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Err(_) => false,
        Ok(meta) => {
            if meta.is_symlink() {
                return true;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                return (meta.file_attributes() & 0x400) != 0;
            }
            #[allow(unreachable_code)]
            false
        }
    }
}

fn remove_link_path(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::fs::remove_dir(path)
            .map_err(|e| format!("remove junction '{}': {e}", path.display()))
    }
    #[cfg(not(windows))]
    {
        std::fs::remove_file(path)
            .map_err(|e| format!("remove symlink '{}': {e}", path.display()))
    }
}

fn create_link(target: &Path, link: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                link.to_str().ok_or_else(|| "non-UTF8 link path".to_string())?,
                target.to_str().ok_or_else(|| "non-UTF8 target path".to_string())?,
            ])
            .status()
            .map_err(|e| format!("mklink: {e}"))?;
        if !status.success() {
            return Err(format!("mklink /J failed for '{}'", link.display()));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, link).map_err(|e| {
            format!(
                "symlink '{}' -> '{}': {e}",
                link.display(),
                target.display()
            )
        })
    }
}

fn copy_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)
            .map_err(|e| format!("mkdir '{}': {e}", dst.display()))?;
        for item in std::fs::read_dir(src)
            .map_err(|e| format!("readdir '{}': {e}", src.display()))?
        {
            let item = item.map_err(|e| format!("readdir entry: {e}"))?;
            copy_recursive(&item.path(), &dst.join(item.file_name()))?;
        }
    } else {
        std::fs::copy(src, dst)
            .map_err(|e| format!("copy '{}': {e}", src.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_dir(label: &str) -> PathBuf {
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("orch-cfg-{label}-{ns}"));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn git_cmd(dir: &PathBuf, args: &[&str]) {
        std::process::Command::new("git")
            .args(["-C", dir.to_str().unwrap()])
            .args(args)
            .status()
            .expect("git");
    }

    fn init_git(dir: &PathBuf) {
        git_cmd(dir, &["init"]);
        git_cmd(dir, &["config", "user.email", "t@t.local"]);
        git_cmd(dir, &["config", "user.name", "T"]);
    }

    fn init_git_with_ignore(dir: &PathBuf, pattern: &str) {
        init_git(dir);
        std::fs::write(dir.join(".gitignore"), format!("{pattern}\n")).unwrap();
        git_cmd(dir, &["add", ".gitignore"]);
        git_cmd(dir, &["commit", "-m", "init"]);
    }

    #[test]
    fn parse_valid_config() {
        let cfg: RepoConfig = serde_json::from_str(
            r#"{"link":[{"path":"build","readOnly":true}],"copy":[".env"],"exclusive":["rail-build"]}"#,
        )
        .unwrap();
        assert_eq!(cfg.link.len(), 1);
        assert!(cfg.link[0].read_only);
        assert_eq!(cfg.copy, vec![".env"]);
        assert_eq!(cfg.exclusive, vec!["rail-build"]);
    }

    #[test]
    fn reject_unknown_key() {
        let result: Result<RepoConfig, _> = serde_json::from_str(r#"{"unknown": true}"#);
        assert!(result.is_err(), "unknown keys must be rejected");
    }

    #[test]
    fn reject_absolute_path() {
        let dir = tmp_dir("abs-path");
        init_git(&dir);
        let result = validate_path(dir.to_str().unwrap(), "/absolute/path", "link");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must be relative"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reject_dotdot_path() {
        let dir = tmp_dir("dotdot");
        init_git(&dir);
        let result = validate_path(dir.to_str().unwrap(), "../escape", "link");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("'..'"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reject_file_in_link() {
        let main_repo = tmp_dir("file-in-link-main");
        init_git_with_ignore(&main_repo, "some_file.txt");
        std::fs::write(main_repo.join("some_file.txt"), "content").unwrap();
        let wt = tmp_dir("file-in-link-wt");
        init_git_with_ignore(&wt, "some_file.txt");
        let dispatch_dir = tmp_dir("file-in-link-dispatch");
        let config = RepoConfig {
            link: vec![LinkEntry { path: "some_file.txt".to_string(), read_only: false }],
            copy: vec![],
            exclusive: vec![],
        };
        let result = materialize(MaterializeOpts {
            repo: main_repo.to_str().unwrap(),
            worktree: wt.to_str().unwrap(),
            dispatch_dir: &dispatch_dir,
            config: &config,
        });
        assert!(result.is_err(), "file in link must be rejected");
        assert!(result.unwrap_err().contains("use 'copy'"));
        std::fs::remove_dir_all(&main_repo).ok();
        std::fs::remove_dir_all(&wt).ok();
        std::fs::remove_dir_all(&dispatch_dir).ok();
    }

    #[test]
    fn reject_reserved_main_exclusive() {
        let cfg = RepoConfig { link: vec![], copy: vec![], exclusive: vec!["main".to_string()] };
        let result = cfg.validate_exclusive_names(&["main".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("reserved"));
    }

    #[test]
    fn reject_unknown_exclusive_name() {
        let cfg = RepoConfig { link: vec![], copy: vec![], exclusive: vec!["rail-build".to_string()] };
        let result = cfg.validate_exclusive_names(&["typo-name".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown exclusive"));
    }

    #[test]
    fn no_links_json_is_ok() {
        let dir = tmp_dir("no-links-json");
        let result = unlink_recorded(&dir, "/fake/worktree");
        assert!(result.is_ok(), "no links.json must be a no-op");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn junction_or_symlink_survives_unlink() {
        let main_repo = tmp_dir("jct-main");
        init_git_with_ignore(&main_repo, "shared");
        let shared = main_repo.join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("data.txt"), "important data").unwrap();

        let wt = tmp_dir("jct-wt");
        init_git_with_ignore(&wt, "shared");
        let dispatch_dir = tmp_dir("jct-dispatch");

        let config = RepoConfig {
            link: vec![LinkEntry { path: "shared".to_string(), read_only: false }],
            copy: vec![],
            exclusive: vec![],
        };
        let result = materialize(MaterializeOpts {
            repo: main_repo.to_str().unwrap(),
            worktree: wt.to_str().unwrap(),
            dispatch_dir: &dispatch_dir,
            config: &config,
        });
        assert!(result.is_ok(), "materialize failed: {:?}", result.err());

        let link_path = wt.join("shared");
        assert!(link_path.exists(), "link must exist after materialize");
        assert!(is_link(&link_path), "must be detected as link/junction");

        unlink_recorded(&dispatch_dir, wt.to_str().unwrap()).expect("unlink");

        assert!(!is_link(&link_path), "link must be gone after unlink");
        assert!(
            shared.join("data.txt").exists(),
            "target data.txt must survive unlink"
        );

        std::fs::remove_dir_all(&main_repo).ok();
        std::fs::remove_dir_all(&wt).ok();
        std::fs::remove_dir_all(&dispatch_dir).ok();
    }
}
