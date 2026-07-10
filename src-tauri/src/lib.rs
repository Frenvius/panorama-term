use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use tauri::{LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

mod store;

const SIDECAR_PORT: u16 = 9777;
const NOTIF_WIDTH: f64 = 440.0;
const NOTIF_MARGIN: f64 = 16.0;

fn sidecar_alive() -> bool {
    let addr = format!("127.0.0.1:{SIDECAR_PORT}").parse().unwrap();
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

fn sidecar_bin() -> PathBuf {
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let name = if cfg!(windows) { "sidecar.exe" } else { "sidecar" };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("sidecar-rs")
        .join("target")
        .join(profile)
        .join(name)
}

fn spawn_sidecar() {
    if sidecar_alive() {
        return;
    }
    let bin = sidecar_bin();
    if !bin.exists() {
        eprintln!("[panorama] sidecar binary missing: {}", bin.display());
        return;
    }
    let make = || {
        let mut cmd = Command::new(&bin);
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
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
    WebviewWindowBuilder::new(
        app,
        "notif",
        WebviewUrl::App("index.html?overlay=notif".into()),
    )
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

#[tauri::command]
fn notif_layout(app: tauri::AppHandle, height: f64) -> Result<(), String> {
    let win = app.get_webview_window("notif").ok_or("no overlay window")?;
    if height <= 0.0 {
        return win.hide().map_err(|e| e.to_string());
    }
    let monitor = win
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("no primary monitor")?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let size = area.size.to_logical::<f64>(scale);
    let pos = area.position.to_logical::<f64>(scale);
    let h = height.clamp(1.0, size.height - NOTIF_MARGIN * 2.0);
    win.set_size(LogicalSize::new(NOTIF_WIDTH, h))
        .map_err(|e| e.to_string())?;
    win.set_position(LogicalPosition::new(
        pos.x + size.width - NOTIF_WIDTH - NOTIF_MARGIN,
        pos.y + size.height - h - NOTIF_MARGIN,
    ))
    .map_err(|e| e.to_string())?;
    win.show().map_err(|e| e.to_string())?;
    win.set_always_on_top(true).map_err(|e| e.to_string())
}

#[tauri::command]
fn focus_main(app: tauri::AppHandle) -> Result<(), String> {
    let win = app.get_webview_window("main").ok_or("no main window")?;
    let _ = win.unminimize();
    win.show().map_err(|e| e.to_string())?;
    win.set_focus().map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            spawn_sidecar();
            create_notif_window(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            write_temp_image,
            read_temp_image,
            notif_layout,
            focus_main,
            store::store_read,
            store::store_write,
            store::store_delete,
            store::store_list
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
