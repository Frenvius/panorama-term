use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Copy, Clone)]
pub enum Flavor {
    Dev,
    Release,
}

impl Flavor {
    pub fn current() -> Flavor {
        if cfg!(debug_assertions) { Flavor::Dev } else { Flavor::Release }
    }
}

pub fn root(panorama_dir: &Path) -> PathBuf {
    panorama_dir.join("runtime")
}

pub fn current_file(root: &Path, host_port: u16) -> PathBuf {
    root.join(format!("current-{}.json", host_port))
}

fn host_exe_name(flavor: Flavor) -> String {
    match flavor {
        Flavor::Dev => format!("panorama-host-dev{}", std::env::consts::EXE_SUFFIX),
        Flavor::Release => format!("panorama-host{}", std::env::consts::EXE_SUFFIX),
    }
}

fn brain_exe_name(flavor: Flavor) -> String {
    match flavor {
        Flavor::Dev => format!("panorama-brain-dev{}", std::env::consts::EXE_SUFFIX),
        Flavor::Release => format!("panorama-brain{}", std::env::consts::EXE_SUFFIX),
    }
}

pub fn host_exe(dir: &Path, flavor: Flavor) -> PathBuf {
    dir.join(host_exe_name(flavor))
}

pub fn brain_exe(dir: &Path, flavor: Flavor) -> PathBuf {
    dir.join(brain_exe_name(flavor))
}

fn fnv1a_feed(hash: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *hash ^= b as u64;
        *hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
}

struct FileEntry {
    name: String,
    content: Vec<u8>,
}

fn build_entries(source_bin: &Path, flavor: Flavor) -> io::Result<Vec<FileEntry>> {
    let source_content = std::fs::read(source_bin)?;
    let src_dir = source_bin.parent();

    let maybe_conpty = src_dir.and_then(|d| std::fs::read(d.join("conpty.dll")).ok());
    let maybe_openconsole = src_dir.and_then(|d| std::fs::read(d.join("OpenConsole.exe")).ok());

    let host_name = host_exe_name(flavor);
    let brain_name = brain_exe_name(flavor);

    let mut entries = vec![
        FileEntry { name: host_name, content: source_content.clone() },
        FileEntry { name: brain_name, content: source_content },
    ];
    if let Some(c) = maybe_conpty {
        entries.push(FileEntry { name: "conpty.dll".into(), content: c });
    }
    if let Some(o) = maybe_openconsole {
        entries.push(FileEntry { name: "OpenConsole.exe".into(), content: o });
    }
    Ok(entries)
}

fn build_id(entries: &[FileEntry]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for e in entries {
        fnv1a_feed(&mut hash, e.name.as_bytes());
        fnv1a_feed(&mut hash, &e.content);
    }
    format!("{:016x}", hash)
}

fn is_valid(dir: &Path, entries: &[FileEntry]) -> bool {
    entries.iter().all(|e| {
        dir.join(&e.name)
            .metadata()
            .map(|m| m.len() == e.content.len() as u64)
            .unwrap_or(false)
    })
}

pub fn prepare(source_bin: &Path, root: &Path, flavor: Flavor) -> io::Result<PathBuf> {
    let entries = build_entries(source_bin, flavor)?;
    let id = build_id(&entries);
    let primary = root.join(&id);

    if is_valid(&primary, &entries) {
        return Ok(primary);
    }

    // a damaged dir may still be the live host's, whose idle OpenConsole.exe must not be deleted
    let target = if primary.exists() {
        root.join(format!("{}-{}", id, std::process::id()))
    } else {
        primary
    };

    if is_valid(&target, &entries) {
        return Ok(target);
    }

    std::fs::create_dir_all(root)?;

    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .subsec_nanos();
    let tmp = root.join(format!(".tmp-{}-{}", std::process::id(), nanos));
    std::fs::create_dir_all(&tmp)?;

    for e in &entries {
        std::fs::write(tmp.join(&e.name), &e.content)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::Permissions::from_mode(0o755);
        let _ = std::fs::set_permissions(tmp.join(host_exe_name(flavor)), mode.clone());
        let _ = std::fs::set_permissions(tmp.join(brain_exe_name(flavor)), mode.clone());
        let _ = std::fs::set_permissions(tmp.join("OpenConsole.exe"), mode);
    }

    match std::fs::rename(&tmp, &target) {
        Ok(()) => Ok(target),
        Err(e) => {
            if is_valid(&target, &entries) {
                let _ = std::fs::remove_dir_all(&tmp);
                Ok(target)
            } else {
                let _ = std::fs::remove_dir_all(&tmp);
                Err(e)
            }
        }
    }
}

pub struct Current {
    pub dir: PathBuf,
    pub source: PathBuf,
    pub previous: Option<PathBuf>,
}

pub fn read_current(file: &Path) -> Option<Current> {
    let raw = std::fs::read_to_string(file).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let dir = PathBuf::from(v.get("dir")?.as_str()?);
    let source = PathBuf::from(v.get("source")?.as_str()?);
    let previous = v.get("previous").and_then(|p| p.as_str()).map(PathBuf::from);
    Some(Current { dir, source, previous })
}

pub fn publish(file: &Path, dir: &Path, source: &Path) -> io::Result<()> {
    let old = read_current(file);
    let previous = old.and_then(|c| {
        if c.dir != dir { Some(c.dir) } else { c.previous }
    });

    let new_content = serde_json::json!({
        "dir": dir.to_string_lossy(),
        "source": source.to_string_lossy(),
        "previous": previous.as_deref().map(|p| p.to_string_lossy().into_owned()),
    })
    .to_string();

    let existing = std::fs::read_to_string(file).ok();
    if existing.as_deref() == Some(new_content.as_str()) {
        return Ok(());
    }

    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_name = format!(
        "{}.tmp-{}",
        file.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    );
    let tmp = file.with_file_name(tmp_name);
    std::fs::write(&tmp, &new_content)?;
    std::fs::rename(&tmp, file)?;
    Ok(())
}

pub fn gc(root: &Path, extra_keep: &[PathBuf]) {
    let mut keep = std::collections::HashSet::new();

    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let n = name.to_string_lossy();
            if n.starts_with("current-") && n.ends_with(".json") {
                if let Some(c) = read_current(&entry.path()) {
                    keep.insert(c.dir);
                    if let Some(p) = c.previous {
                        keep.insert(p);
                    }
                }
            }
        }
    }

    for p in extra_keep {
        keep.insert(p.clone());
    }

    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let n = name.to_string_lossy();

        if n.starts_with(".tmp-") {
            let old_enough = entry
                .metadata()
                .and_then(|m| m.modified())
                .map(|t| t.elapsed().unwrap_or(Duration::ZERO) > Duration::from_secs(600))
                .unwrap_or(false);
            if old_enough {
                let _ = std::fs::remove_dir_all(&path);
            }
            continue;
        }

        if keep.contains(&path) {
            continue;
        }
        let entry_name = entry.file_name();
        if keep
            .iter()
            .any(|k| k.file_name().map(|kn| kn == entry_name).unwrap_or(false))
        {
            continue;
        }

        let _ = std::fs::remove_dir_all(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_root() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join(format!("prt-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn dummy_bin(dir: &Path, content: &[u8]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("src{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn prepare_idempotent() {
        let root = unique_root();
        let src_dir = root.join("src");
        let bin = dummy_bin(&src_dir, b"fake binary data");
        let rt = root.join("rt");

        let dir1 = prepare(&bin, &rt, Flavor::Dev).unwrap();
        let dir2 = prepare(&bin, &rt, Flavor::Dev).unwrap();
        assert_eq!(dir1, dir2);

        let tmps: Vec<_> = std::fs::read_dir(&rt)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp-"))
            .collect();
        assert!(tmps.is_empty(), "no .tmp dirs should remain");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prepare_recovers_deleted_file() {
        let root = unique_root();
        let src_dir = root.join("src");
        let bin = dummy_bin(&src_dir, b"content xyz");
        let rt = root.join("rt");

        let dir1 = prepare(&bin, &rt, Flavor::Dev).unwrap();
        let b = brain_exe(&dir1, Flavor::Dev);
        std::fs::remove_file(&b).unwrap();

        let dir2 = prepare(&bin, &rt, Flavor::Dev).unwrap();
        assert!(brain_exe(&dir2, Flavor::Dev).exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prepare_different_content_different_dir() {
        let root = unique_root();
        let src_dir = root.join("src");
        let rt = root.join("rt");

        let bin1 = dummy_bin(&src_dir, b"binary v1");
        let dir1 = prepare(&bin1, &rt, Flavor::Dev).unwrap();

        let bin2 = dummy_bin(&src_dir, b"binary v2");
        let dir2 = prepare(&bin2, &rt, Flavor::Dev).unwrap();

        assert_ne!(dir1, dir2);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn publish_tracks_previous_not_self() {
        let root = unique_root();
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("current-9778.json");

        let dir_a = root.join("aaaa");
        let dir_b = root.join("bbbb");
        let src = root.join("sidecar.exe");

        publish(&file, &dir_a, &src).unwrap();
        let c1 = read_current(&file).unwrap();
        assert_eq!(c1.dir, dir_a);
        assert!(c1.previous.is_none());

        publish(&file, &dir_a, &src).unwrap();
        let c2 = read_current(&file).unwrap();
        assert_eq!(c2.dir, dir_a);
        assert!(c2.previous.is_none(), "same dir must not set previous to itself");

        publish(&file, &dir_b, &src).unwrap();
        let c3 = read_current(&file).unwrap();
        assert_eq!(c3.dir, dir_b);
        assert_eq!(c3.previous, Some(dir_a.clone()));

        publish(&file, &dir_b, &src).unwrap();
        let c4 = read_current(&file).unwrap();
        assert_eq!(c4.dir, dir_b);
        assert_eq!(c4.previous, Some(dir_a), "previous must not change when dir is unchanged");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gc_keeps_current_previous_extra_removes_unreferenced() {
        let root = unique_root();
        let rt = root.join("rt");
        std::fs::create_dir_all(&rt).unwrap();

        let dir_current = rt.join("current");
        let dir_previous = rt.join("previous");
        let dir_extra = rt.join("extra");
        let dir_unref = rt.join("unreferenced");

        for d in [&dir_current, &dir_previous, &dir_extra, &dir_unref] {
            std::fs::create_dir_all(d).unwrap();
        }

        let src = root.join("src.exe");
        let cf = rt.join("current-9778.json");
        let json = serde_json::json!({
            "dir": dir_current.to_string_lossy(),
            "source": src.to_string_lossy(),
            "previous": dir_previous.to_string_lossy(),
        })
        .to_string();
        std::fs::write(&cf, json).unwrap();

        gc(&rt, &[dir_extra.clone()]);

        assert!(dir_current.exists(), "current dir must be kept");
        assert!(dir_previous.exists(), "previous dir must be kept");
        assert!(dir_extra.exists(), "extra_keep dir must be kept");
        assert!(!dir_unref.exists(), "unreferenced dir must be removed");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn gc_windows_locked_exe_stays_then_removed() {
        let root = unique_root();
        let rt = root.join("rt");
        std::fs::create_dir_all(&rt).unwrap();

        let unref = rt.join("unreferenced");
        std::fs::create_dir_all(&unref).unwrap();

        let ping_src = Path::new("C:\\Windows\\System32\\PING.EXE");
        let brain = brain_exe(&unref, Flavor::Release);
        std::fs::copy(ping_src, &brain).unwrap();

        let mut child = std::process::Command::new(&brain)
            .args(["-n", "20", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        gc(&rt, &[]);
        assert!(brain.exists(), "gc must leave a locked exe in place");

        let _ = child.kill();
        let _ = child.wait();

        gc(&rt, &[]);
        assert!(!unref.exists(), "gc must remove the dir after the process exits");

        let _ = std::fs::remove_dir_all(&root);
    }
}
