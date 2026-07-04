use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

mod store;

const SIDECAR_PORT: u16 = 9777;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            spawn_sidecar();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            store::store_read,
            store::store_write,
            store::store_delete,
            store::store_list
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
