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

fn sidecar_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("sidecar")
        .join("index.ts")
}

fn spawn_sidecar() {
    if sidecar_alive() {
        return;
    }
    let mut cmd = Command::new("node");
    cmd.arg("--experimental-strip-types").arg(sidecar_script());
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    if let Err(e) = cmd.spawn() {
        eprintln!("[panorama] failed to spawn sidecar: {e}");
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
