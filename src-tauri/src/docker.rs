use std::fs;

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct Container {
    id: String,
    name: String,
    image: String,
    state: String,
    status: String,
    ports: String,
}

#[derive(Deserialize)]
struct PsLine {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Names")]
    names: String,
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "State")]
    state: String,
    #[serde(rename = "Status")]
    status: String,
    #[serde(rename = "Ports")]
    ports: String,
}

fn run_docker(args: &[&str]) -> Result<String, String> {
    let out = crate::hidden_command("docker")
        .args(args)
        .output()
        .map_err(|e| format!("docker: {}", e))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[tauri::command]
pub async fn docker_available() -> bool {
    run_docker(&["--version"]).is_ok()
}

#[tauri::command]
pub async fn docker_ps() -> Result<Vec<Container>, String> {
    let out = run_docker(&["ps", "-a", "--no-trunc", "--format", "{{json .}}"])?;
    let mut list: Vec<Container> = out
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<PsLine>(line).ok())
        .map(|raw| Container {
            id: raw.id,
            name: raw.names.split(',').next().unwrap_or_default().trim_start_matches('/').to_string(),
            image: raw.image,
            state: raw.state.to_lowercase(),
            status: raw.status,
            ports: raw.ports,
        })
        .collect();
    list.sort_by(|a, b| {
        let running = (b.state == "running").cmp(&(a.state == "running"));
        running.then_with(|| a.name.cmp(&b.name))
    });
    Ok(list)
}

fn suppress_desktop_ui() {
    let Some(dir) = dirs::config_dir() else { return };
    let path = dir.join("Docker").join("settings-store.json");
    let Ok(text) = fs::read_to_string(&path) else { return };
    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&text) else { return };
    let Some(obj) = json.as_object_mut() else { return };
    if obj.get("OpenUIOnStartupDisabled").and_then(|v| v.as_bool()) == Some(true) {
        return;
    }
    obj.insert("OpenUIOnStartupDisabled".into(), serde_json::Value::Bool(true));
    let _ = fs::write(&path, json.to_string());
}

#[tauri::command]
pub async fn docker_engine(action: String) -> Result<(), String> {
    let verb = match action.as_str() {
        "start" | "stop" => action.as_str(),
        _ => return Err(format!("unknown action: {}", action)),
    };
    if verb == "start" {
        suppress_desktop_ui();
    }
    crate::hidden_command("docker")
        .args(["desktop", verb])
        .spawn()
        .map_err(|e| format!("docker desktop {}: {}", verb, e))?;
    Ok(())
}

#[tauri::command]
pub async fn docker_action(id: String, action: String) -> Result<(), String> {
    let verb = match action.as_str() {
        "start" | "stop" | "restart" => action.as_str(),
        _ => return Err(format!("unknown action: {}", action)),
    };
    run_docker(&[verb, &id])?;
    Ok(())
}
