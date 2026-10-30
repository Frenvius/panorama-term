use std::path::{Path, PathBuf};

use serde_json::{json, Value};

const BUILD_WORDS: &[&str] = &["build", "compile", "bundle", "dist", "tsc"];

pub fn detect_json(cwd: &str) -> Value {
    let list: Vec<Value> = detect(Path::new(cwd))
        .into_iter()
        .map(|cmd| json!({ "kind": kind_of(&cmd), "cmd": cmd }))
        .collect();
    Value::Array(list)
}

pub fn kind_of(cmd: &str) -> &'static str {
    let is_build = cmd
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|word| BUILD_WORDS.iter().any(|b| word.eq_ignore_ascii_case(b)));
    if is_build {
        "build"
    } else {
        "run"
    }
}

fn detect(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(raw) = std::fs::read_to_string(dir.join("package.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
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
    task_commands(dir, &mut out);
    dotnet_commands(dir, &mut out);
    out
}

fn task_commands(dir: &Path, out: &mut Vec<String>) {
    let file = ["Taskfile.yml", "Taskfile.yaml", "taskfile.yml", "taskfile.yaml"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.exists());
    let Some(path) = file else { return };
    let Ok(raw) = std::fs::read_to_string(path) else { return };
    let mut lines = raw.lines();
    for line in lines.by_ref() {
        if line.starts_with("tasks:") {
            break;
        }
    }
    let mut indent: Option<usize> = None;
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let lead = line.len() - trimmed.len();
        if lead == 0 {
            break;
        }
        let depth = *indent.get_or_insert(lead);
        if lead != depth {
            continue;
        }
        if let Some(name) = trimmed.strip_suffix(':').or_else(|| trimmed.split_once(':').map(|(k, _)| k)) {
            let name = name.trim().trim_matches(|c| c == '"' || c == '\'');
            if !name.is_empty() {
                out.push(format!("task {name}"));
            }
        }
    }
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

fn dotnet_commands(dir: &Path, out: &mut Vec<String>) {
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

#[cfg(test)]
mod tests {
    use super::kind_of;

    #[test]
    fn build_words_match_whole_words_only() {
        assert_eq!(kind_of("cargo build"), "build");
        assert_eq!(kind_of("bun run build:vite"), "build");
        assert_eq!(kind_of("dotnet build"), "build");
        assert_eq!(kind_of("bun run TSC"), "build");
        assert_eq!(kind_of("bun run dev"), "run");
        assert_eq!(kind_of("bun run rebuild"), "run");
        assert_eq!(kind_of("bun run build_all"), "run");
        assert_eq!(kind_of("cargo run"), "run");
    }
}
