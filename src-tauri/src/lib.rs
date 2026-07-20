use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use tauri::{LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

mod git;
mod store;
mod docker;
mod notes;
mod claude;

const SIDECAR_PORT: u16 = 9777;
const HOST_PORT: u16 = 9778;
const NOTIF_WIDTH: f64 = 448.0;

fn host_alive() -> bool {
    let addr = format!("127.0.0.1:{HOST_PORT}").parse().unwrap();
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

fn host_bin(brain: &Path) -> PathBuf {
    if !cfg!(debug_assertions) {
        return brain.to_path_buf();
    }
    let dst = std::env::temp_dir().join("panorama-host.exe");
    if std::fs::copy(brain, &dst).is_ok() {
        dst
    } else {
        brain.to_path_buf()
    }
}

fn sidecar_bin() -> PathBuf {
    let name = if cfg!(windows) { "sidecar.exe" } else { "sidecar" };

    if cfg!(debug_assertions) {
        return PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("sidecar-rs")
            .join("target")
            .join("debug")
            .join(name);
    }

    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn spawn_sidecar() {
    if host_alive() {
        return;
    }
    let bin = sidecar_bin();
    if !bin.exists() {
        eprintln!("[panorama] sidecar binary missing: {}", bin.display());
        return;
    }
    let host = host_bin(&bin);
    let make = || {
        let mut cmd = Command::new(&host);
        cmd.arg("host")
            .env("PANORAMA_BRAIN_BIN", &bin)
            .env("PANORAMA_HOST_PORT", HOST_PORT.to_string())
            .env("PANORAMA_SIDECAR_PORT", SIDECAR_PORT.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd
    };

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let base = CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
        let mut cmd = make();
        cmd.creation_flags(base | CREATE_BREAKAWAY_FROM_JOB);
        if cmd.spawn().is_ok() {
            return;
        }
        let mut fallback = make();
        fallback.creation_flags(base);
        if let Err(e) = fallback.spawn() {
            eprintln!("[panorama] failed to spawn sidecar: {e}");
        }
        return;
    }

    #[cfg(not(windows))]
    {
        let mut cmd = make();
        if let Err(e) = cmd.spawn() {
            eprintln!("[panorama] failed to spawn sidecar: {e}");
        }
    }
}

#[tauri::command]
fn write_temp_image(request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let name = request
        .headers()
        .get("x-image-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let safe: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    if safe.is_empty() {
        return Err("invalid file name".into());
    }
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected raw image body".into());
    };
    let path = std::env::temp_dir().join("panorama-paste").join(safe);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn read_temp_image(path: String) -> Result<tauri::ipc::Response, String> {
    let base = std::env::temp_dir().join("panorama-paste");
    let target = PathBuf::from(&path);
    if !target.starts_with(&base) {
        return Err("path not allowed".into());
    }
    let bytes = std::fs::read(&target).map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(bytes))
}

fn create_notif_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, "notif", WebviewUrl::App("index.html".into()))
        .title("Panorama Notifications")
        .inner_size(NOTIF_WIDTH, 120.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .visible(false)
        .content_protected(true)
        .build()?;
    Ok(())
}

fn spawn_notif_topmost_guard(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(700));
        match app.get_webview_window("notif") {
            Some(notif) => {
                if notif.is_visible().unwrap_or(false) {
                    let _ = notif.set_always_on_top(true);
                }
            }
            None => break,
        }
    });
}

#[tauri::command]
fn notif_layout(app: tauri::AppHandle, height: f64) -> Result<(), String> {
    let win = app.get_webview_window("notif").ok_or("no overlay window")?;
    if height <= 0.0 {
        return win.hide().map_err(|e| e.to_string());
    }
    let monitor = app
        .get_webview_window("main")
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| win.primary_monitor().ok().flatten())
        .ok_or("no monitor")?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let size = area.size.to_logical::<f64>(scale);
    let pos = area.position.to_logical::<f64>(scale);
    let h = height.clamp(1.0, size.height);
    let current = win
        .inner_size()
        .map_err(|e| e.to_string())?
        .to_logical::<f64>(scale);
    let target_size = LogicalSize::new(NOTIF_WIDTH, h);
    let target_pos = LogicalPosition::new(pos.x + size.width - NOTIF_WIDTH, pos.y + size.height - h);

    if h > current.height {
        win.set_position(target_pos).map_err(|e| e.to_string())?;
        win.set_size(target_size).map_err(|e| e.to_string())?;
    } else {
        win.set_size(target_size).map_err(|e| e.to_string())?;
        win.set_position(target_pos).map_err(|e| e.to_string())?;
    }

    if !win.is_visible().map_err(|e| e.to_string())? {
        win.show().map_err(|e| e.to_string())?;
    }
    win.set_always_on_top(true).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn focus_main(app: tauri::AppHandle) -> Result<(), String> {
    let win = app.get_webview_window("main").ok_or("no main window")?;
    let _ = win.unminimize();
    win.show().map_err(|e| e.to_string())?;
    win.set_focus().map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
const BADGE_FONT: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b010, 0b010, 0b010],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

#[cfg(target_os = "windows")]
const BADGE_PLUS: [u8; 5] = [0b000, 0b010, 0b111, 0b010, 0b000];

#[cfg(target_os = "windows")]
fn badge_icon(count: u32) -> tauri::image::Image<'static> {
    const SIZE: usize = 32;
    const SCALE: usize = 3;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    let center = (SIZE as f32 - 1.0) / 2.0;
    let radius = center;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            if (dx * dx + dy * dy).sqrt() <= radius {
                let i = (y * SIZE + x) * 4;
                rgba[i..i + 4].copy_from_slice(&[0xd9, 0x77, 0x57, 0xff]);
            }
        }
    }
    let glyphs: Vec<[u8; 5]> = if count > 9 {
        vec![BADGE_FONT[9], BADGE_PLUS]
    } else {
        vec![BADGE_FONT[count.max(1) as usize % 10]]
    };
    let glyph_w = 3 * SCALE;
    let glyph_h = 5 * SCALE;
    let gap = SCALE;
    let total_w = glyphs.len() * glyph_w + (glyphs.len() - 1) * gap;
    let x0 = (SIZE - total_w) / 2;
    let y0 = (SIZE - glyph_h) / 2;
    for (g, glyph) in glyphs.iter().enumerate() {
        let gx = x0 + g * (glyph_w + gap);
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..3 {
                if bits & (0b100 >> col) == 0 {
                    continue;
                }
                for sy in 0..SCALE {
                    for sx in 0..SCALE {
                        let px = gx + col * SCALE + sx;
                        let py = y0 + row * SCALE + sy;
                        let i = (py * SIZE + px) * 4;
                        rgba[i..i + 4].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
                    }
                }
            }
        }
    }
    tauri::image::Image::new_owned(rgba, SIZE as u32, SIZE as u32)
}

#[tauri::command]
fn set_pending_count(app: tauri::AppHandle, count: u32) -> Result<(), String> {
    let win = app.get_webview_window("main").ok_or("no main window")?;
    let title = if count > 0 {
        format!("Panorama ({count})")
    } else {
        "Panorama".to_string()
    };
    let _ = win.set_title(&title);
    #[cfg(target_os = "windows")]
    {
        if count == 0 {
            let _ = win.set_overlay_icon(None);
        } else {
            let _ = win.set_overlay_icon(Some(badge_icon(count)));
        }
    }
    Ok(())
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    let spawn = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    #[cfg(target_os = "windows")]
    return spawn("rundll32", &["url.dll,FileProtocolHandler", &url]);
    #[cfg(target_os = "macos")]
    return spawn("open", &[&url]);
    #[cfg(all(unix, not(target_os = "macos")))]
    return spawn("xdg-open", &[&url]);
}

#[tauri::command]
fn reveal_path(path: String) -> Result<(), String> {
    let spawn = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    #[cfg(target_os = "windows")]
    return spawn("cmd", &["/C", "start", "", &path]);
    #[cfg(target_os = "macos")]
    return spawn("open", &[&path]);
    #[cfg(all(unix, not(target_os = "macos")))]
    return spawn("xdg-open", &[&path]);
}

#[derive(serde::Serialize)]
struct DirEntry {
    name: String,
    path: String,
    dir: bool,
}

#[tauri::command]
fn read_dir(path: String) -> Result<Vec<DirEntry>, String> {
    let mut out: Vec<DirEntry> = std::fs::read_dir(&path)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "node_modules" {
                return None;
            }
            let dir = entry.file_type().ok()?.is_dir();
            let path = entry.path().to_string_lossy().into_owned();
            Some(DirEntry { name, path, dir })
        })
        .collect();
    out.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

fn run_watchers() -> &'static std::sync::Mutex<std::collections::HashMap<u32, notify::RecommendedWatcher>> {
    static W: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<u32, notify::RecommendedWatcher>>> =
        std::sync::OnceLock::new();
    W.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

static NEXT_RUN_WATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

#[tauri::command]
fn run_watch_manifests(app: tauri::AppHandle, path: String) -> Result<u32, String> {
    use notify::Watcher;
    use tauri::Emitter;
    let dir = PathBuf::from(&path);
    let root = path.clone();
    let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        let Ok(event) = res else { return };
        if event.kind.is_access() {
            return;
        }
        let relevant = event.paths.iter().any(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n == "package.json" || n == "Cargo.toml" || n.ends_with(".sln") || n.ends_with(".csproj")
            })
        });
        if relevant {
            let _ = app.emit("run:manifests", serde_json::json!({ "path": root }));
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(&dir, notify::RecursiveMode::NonRecursive)
        .map_err(|e| e.to_string())?;
    let id = NEXT_RUN_WATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    run_watchers().lock().unwrap().insert(id, watcher);
    Ok(id)
}

#[tauri::command]
fn run_unwatch_manifests(id: u32) {
    run_watchers().lock().unwrap().remove(&id);
}

#[tauri::command]
fn run_commands(path: String) -> Vec<String> {
    let dir = std::path::Path::new(&path);
    let mut out = Vec::new();
    if let Ok(raw) = std::fs::read_to_string(dir.join("package.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(scripts) = v.get("scripts").and_then(|s| s.as_object()) {
                for name in scripts.keys() {
                    out.push(format!("bun run {name}"));
                }
            }
        }
    }
    if dir.join("Cargo.toml").exists() {
        out.push("cargo run".into());
        out.push("cargo build".into());
    }
    dotnet_commands(dir, &mut out);
    out
}

fn xml_tag(src: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let s = src.find(&open)? + open.len();
    let e = src[s..].find(&close)? + s;
    let v = src[s..e].trim();
    (!v.is_empty()).then(|| v.to_string())
}

fn msbuild_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("$(") {
        out.push_str(&rest[..i]);
        match rest[i..].find(')') {
            Some(j) => {
                out.push_str("$env:");
                out.push_str(&rest[i + 2..i + j]);
                rest = &rest[i + j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn dotnet_commands(dir: &std::path::Path, out: &mut Vec<String>) {
    let mut csprojs: Vec<PathBuf> = Vec::new();
    let mut has_sln = false;
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        match p.extension().and_then(|x| x.to_str()) {
            Some("sln") => {
                has_sln = true;
                if let Ok(s) = std::fs::read_to_string(&p) {
                    for line in s.lines().filter(|l| l.starts_with("Project(")) {
                        if let Some(rel) = line.split('"').find(|seg| seg.ends_with(".csproj")) {
                            csprojs.push(dir.join(rel));
                        }
                    }
                }
            }
            Some("csproj") => csprojs.push(p),
            _ => {}
        }
    }
    if !has_sln && csprojs.is_empty() {
        return;
    }
    out.push("dotnet build".into());
    csprojs.sort();
    csprojs.dedup();
    for p in csprojs {
        let Ok(src) = std::fs::read_to_string(&p) else { continue };
        if let Some(prog) = xml_tag(&src, "StartProgram") {
            let prog = msbuild_env(&prog);
            let args = xml_tag(&src, "StartArguments").unwrap_or_default();
            let launch = format!("& \"{prog}\" {args}").trim_end().to_string();
            out.push(match xml_tag(&src, "StartWorkingDirectory").map(|w| msbuild_env(&w)) {
                Some(wd) => format!("Set-Location \"{wd}\"; {launch}"),
                None => launch,
            });
        } else if src.contains("<Project Sdk=")
            && xml_tag(&src, "OutputType").is_some_and(|t| t.eq_ignore_ascii_case("exe"))
        {
            let rel = p.strip_prefix(dir).ok().and_then(|r| r.to_str().map(String::from));
            out.push(match rel {
                Some(r) if r.contains(['/', '\\']) => format!("dotnet run --project {r}"),
                _ => "dotnet run".into(),
            });
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());

    #[cfg(debug_assertions)]
    {
        builder = builder.plugin(
            tauri_plugin_mcp_bridge::Builder::new()
                .bind_address("127.0.0.1")
                .base_port(9523)
                .build(),
        );
    }

    builder
        .setup(|app| {
            spawn_sidecar();
            create_notif_window(app.handle())?;
            spawn_notif_topmost_guard(app.handle().clone());
            notes::start_notes_watch(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            match event {
                tauri::WindowEvent::Destroyed => window.app_handle().exit(0),
                tauri::WindowEvent::Focused(true) => {
                    if let Some(notif) = window.app_handle().get_webview_window("notif") {
                        if notif.is_visible().unwrap_or(false) {
                            let _ = notif.set_always_on_top(true);
                        }
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            write_temp_image,
            read_temp_image,
            notif_layout,
            focus_main,
            set_pending_count,
            reveal_path,
            read_dir,
            run_commands,
            run_watch_manifests,
            run_unwatch_manifests,
            open_url,
            store::store_read,
            store::store_write,
            store::store_delete,
            store::store_list,
            git::git_branches,
            git::git_ahead_behind,
            git::git_checkout,
            git::git_fetch,
            git::git_create_branch,
            git::git_rename_branch,
            git::git_delete_branch,
            git::git_merge_branch,
            git::git_rebase_onto,
            git::git_update_branch,
            git::git_push_current,
            git::git_set_upstream,
            git::git_compare_with_current,
            git::git_toggle_branch_favorite,
            git::git_status,
            git::git_commit,
            git::git_log_messages,
            git::git_log_graph,
            git::git_remote_url,
            git::git_unpushed_commits,
            git::git_diff_file,
            git::git_add_ignore,
            git::git_rollback_file,
            git::git_revert_hunk,
            git::git_watch_file,
            git::git_unwatch_file,
            docker::docker_available,
            docker::docker_ps,
            docker::docker_action,
            docker::docker_engine,
            notes::link_note,
            notes::unlink_note,
            notes::link_term,
            notes::unlink_term,
            notes::read_note,
            notes::write_note,
            notes::delete_note,
            claude::claude_session_summary
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
