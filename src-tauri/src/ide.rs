use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone)]
pub struct IdeInfo {
    id: String,
    label: String,
    command: String,
    family: &'static str,
    recommended: bool,
}

struct VscodeDef {
    id: &'static str,
    label: &'static str,
    command: &'static str,
}

const VSCODE_IDES: &[VscodeDef] = &[
    VscodeDef { id: "vscode", label: "VS Code", command: "code" },
    VscodeDef { id: "cursor", label: "Cursor", command: "cursor" },
    VscodeDef { id: "windsurf", label: "Windsurf", command: "windsurf" },
    VscodeDef { id: "vscode-insiders", label: "VS Code Insiders", command: "code-insiders" },
    VscodeDef { id: "vscodium", label: "VSCodium", command: "codium" },
    VscodeDef { id: "zed", label: "Zed", command: "zed" },
];

const JB_LAUNCHERS: &[(&str, &str, &str)] = &[
    ("intellij", "IntelliJ IDEA", "idea"),
    ("pycharm", "PyCharm", "pycharm"),
    ("webstorm", "WebStorm", "webstorm"),
    ("rustrover", "RustRover", "rustrover"),
    ("goland", "GoLand", "goland"),
    ("clion", "CLion", "clion"),
    ("rider", "Rider", "rider"),
    ("phpstorm", "PhpStorm", "phpstorm"),
    ("rubymine", "RubyMine", "rubymine"),
    ("datagrip", "DataGrip", "datagrip"),
];

#[derive(Deserialize)]
struct ToolboxState {
    tools: Vec<ToolboxTool>,
}

#[derive(Deserialize)]
struct ToolboxTool {
    #[serde(rename = "productCode")]
    product_code: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "launchCommand")]
    launch_command: Option<String>,
}

fn jb_id(product_code: &str) -> Option<&'static str> {
    Some(match product_code {
        "IU" | "IC" => "intellij",
        "PY" | "PC" => "pycharm",
        "WS" => "webstorm",
        "RR" => "rustrover",
        "GO" => "goland",
        "CL" => "clion",
        "RD" => "rider",
        "PS" => "phpstorm",
        "RM" => "rubymine",
        "DB" => "datagrip",
        "AI" => "androidstudio",
        _ => return None,
    })
}

fn command_exists(cmd: &str) -> bool {
    let finder = if cfg!(windows) { "where.exe" } else { "which" };
    crate::hidden_command(finder)
        .arg(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn toolbox_config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA")?;
        Some(PathBuf::from(base).join("JetBrains/Toolbox"))
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Application Support/JetBrains/Toolbox"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join(".local/share/JetBrains/Toolbox"))
    }
}

fn toolbox_ides() -> Vec<IdeInfo> {
    let Some(dir) = toolbox_config_dir() else { return Vec::new() };
    let Ok(raw) = std::fs::read_to_string(dir.join("state.json")) else { return Vec::new() };
    let Ok(state) = serde_json::from_str::<ToolboxState>(&raw) else { return Vec::new() };
    state
        .tools
        .into_iter()
        .filter_map(|tool| {
            let command = tool.launch_command?;
            let id = jb_id(&tool.product_code)?;
            Some(IdeInfo {
                id: id.into(),
                label: tool.display_name,
                command,
                family: "jetbrains",
                recommended: false,
            })
        })
        .collect()
}

fn dir_has_ext(dir: &Path, exts: &[&str]) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries.flatten().any(|entry| {
        entry
            .path()
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| exts.iter().any(|x| x.eq_ignore_ascii_case(ext)))
    })
}

fn project_ide_id(path: &str) -> Option<&'static str> {
    let dir = Path::new(path);
    let has = |f: &str| dir.join(f).exists();
    if has("Cargo.toml") {
        return Some("rustrover");
    }
    if has("go.mod") {
        return Some("goland");
    }
    if has("pom.xml") || has("build.gradle") || has("build.gradle.kts") || has("settings.gradle") {
        return Some("intellij");
    }
    if has("composer.json") {
        return Some("phpstorm");
    }
    if has("Gemfile") {
        return Some("rubymine");
    }
    if has("requirements.txt") || has("pyproject.toml") || has("setup.py") || has("Pipfile") {
        return Some("pycharm");
    }
    if has("CMakeLists.txt") {
        return Some("clion");
    }
    if dir_has_ext(dir, &["sln", "csproj", "fsproj"]) {
        return Some("rider");
    }
    if has("package.json") {
        return Some("webstorm");
    }
    None
}

fn scan_ides() -> Vec<IdeInfo> {
    let mut out: Vec<IdeInfo> = Vec::new();

    for def in VSCODE_IDES {
        if command_exists(def.command) {
            out.push(IdeInfo {
                id: def.id.into(),
                label: def.label.into(),
                command: def.command.into(),
                family: "vscode",
                recommended: false,
            });
        }
    }

    out.extend(toolbox_ides());

    for (id, label, launcher) in JB_LAUNCHERS {
        if out.iter().any(|x| x.id == *id) {
            continue;
        }
        if command_exists(launcher) {
            out.push(IdeInfo {
                id: (*id).into(),
                label: (*label).into(),
                command: (*launcher).into(),
                family: "jetbrains",
                recommended: false,
            });
        }
    }

    out
}

#[tauri::command]
pub fn detect_ides(path: Option<String>) -> Vec<IdeInfo> {
    static CACHE: OnceLock<Vec<IdeInfo>> = OnceLock::new();
    let mut out = CACHE.get_or_init(scan_ides).clone();

    if let Some(path) = path {
        if let Some(rid) = project_ide_id(&path) {
            if let Some(i) = out.iter().position(|x| x.id == rid) {
                out[i].recommended = true;
                let rec = out.remove(i);
                out.insert(0, rec);
            }
        }
    }

    out
}

#[tauri::command]
pub fn open_in_ide(path: String, command: String, family: String) -> Result<(), String> {
    let is_exe_path = command.contains('/') || command.contains('\\');
    let jetbrains = family == "jetbrains";

    if is_exe_path {
        let mut cmd = crate::hidden_command(&command);
        if !jetbrains {
            cmd.arg("-n");
        }
        return cmd.arg(&path).spawn().map(|_| ()).map_err(|e| e.to_string());
    }

    #[cfg(windows)]
    {
        let mut cmd = crate::hidden_command("cmd");
        cmd.arg("/C").arg(&command);
        if !jetbrains {
            cmd.arg("-n");
        }
        return cmd.arg(&path).spawn().map(|_| ()).map_err(|e| e.to_string());
    }
    #[cfg(not(windows))]
    {
        let mut cmd = crate::hidden_command(&command);
        if !jetbrains {
            cmd.arg("-n");
        }
        cmd.arg(&path).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
}
