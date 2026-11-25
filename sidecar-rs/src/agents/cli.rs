use serde_json::{Map, Value};

use crate::orch::cli::{http_post, parse_response_body};

const USAGE: &str = "usage: panorama-cli agent <create|list|show|update|delete|start|stop> [name-or-id] [--flags] [--json]";
const EXIT_USAGE: i32 = 2;
const EXIT_API: i32 = 3;
const EXIT_UNREACHABLE: i32 = 4;

#[derive(Debug)]
struct Parsed {
    command: String,
    body: Map<String, Value>,
    json: bool,
}

fn allowed_flags(command: &str) -> Option<&'static str> {
    Some(match command {
        "create" => "name title description profile model effort cwd instructions instructions-file auto-start",
        "update" => "id name title description profile model effort cwd instructions instructions-file auto-start",
        "list" => "project",
        "show" => "id tail",
        "delete" => "id",
        "start" => "id fresh",
        "stop" => "id force",
        _ => return None,
    })
}

fn field_for(flag: &str) -> &'static str {
    match flag {
        "auto-start" => "autoStart",
        "instructions-file" => "instructions",
        "id" => "id",
        "name" => "name",
        "title" => "title",
        "description" => "description",
        "profile" => "profile",
        "model" => "model",
        "effort" => "effort",
        "cwd" => "cwd",
        "instructions" => "instructions",
        "project" => "project",
        "tail" => "tail",
        "fresh" => "fresh",
        "force" => "force",
        _ => "",
    }
}

fn parse_bool(flag: &str, raw: &str) -> Result<bool, String> {
    match raw {
        "true" | "on" | "yes" => Ok(true),
        "false" | "off" | "no" => Ok(false),
        _ => Err(format!("--{flag} requires true or false")),
    }
}

fn parse_args<R>(args: &[String], read_file: R) -> Result<Parsed, String>
where
    R: Fn(&str) -> Result<String, String>,
{
    let command = args.first().ok_or(USAGE)?.clone();
    let allowed = allowed_flags(&command).ok_or_else(|| format!("unknown command: {command}"))?;
    let mut body = Map::new();
    let mut json = false;
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        let Some(flag) = arg.strip_prefix("--") else {
            if body.contains_key("id") || command == "create" || command == "list" {
                return Err(format!("unexpected argument {arg}"));
            }
            body.insert("id".into(), arg.clone().into());
            i += 1;
            continue;
        };
        if flag == "json" {
            json = true;
            i += 1;
            continue;
        }
        if !allowed.split_whitespace().any(|f| f == flag) {
            return Err(format!("unknown flag --{flag} for {command}"));
        }
        let field = field_for(flag);
        if matches!(flag, "fresh" | "force") {
            body.insert(field.into(), true.into());
            i += 1;
            continue;
        }
        let raw = args.get(i + 1).ok_or_else(|| format!("--{flag} requires a value"))?;
        let value: Value = match flag {
            "auto-start" => parse_bool(flag, raw)?.into(),
            "tail" => raw.parse::<u64>().map_err(|_| "--tail requires a number".to_string())?.into(),
            "instructions-file" => read_file(raw)?.into(),
            _ => raw.clone().into(),
        };
        body.insert(field.into(), value);
        i += 2;
    }
    if command == "create" {
        if !body.contains_key("name") || !body.contains_key("profile") {
            return Err("create requires --name and --profile".into());
        }
        if !body.contains_key("cwd") {
            let cwd = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
            body.insert("cwd".into(), cwd.to_string_lossy().into_owned().into());
        }
    } else if command != "list" && !body.contains_key("id") {
        return Err(format!("{command} requires an agent name or id"));
    }
    Ok(Parsed { command, body, json })
}

fn read_file(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))
}

fn list_line(agent: &Value) -> String {
    let field = |k: &str| agent[k].as_str().unwrap_or("").to_string();
    let model = agent["model"].as_str().map(|m| format!(":{m}")).unwrap_or_default();
    let project = agent["project"].as_str().unwrap_or("-");
    format!("{}  {}  {}{}  {}", field("name"), field("status"), field("profile"), model, project)
}

fn exit_with(message: &str, code: i32) -> ! {
    eprintln!("[agent] {message}");
    std::process::exit(code);
}

pub fn main() {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let parsed = parse_args(&args, read_file).unwrap_or_else(|e| exit_with(&e, EXIT_USAGE));
    let body = serde_json::to_string(&parsed.body).unwrap_or_else(|_| "{}".into());
    let path = format!("/agents/{}", parsed.command);
    let response = http_post(crate::port(), &path, &body, 15_000)
        .unwrap_or_else(|e| exit_with(&format!("brain unreachable: {e}"), EXIT_UNREACHABLE));
    let text = parse_response_body(&response);
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        exit_with(&format!("unexpected response: {}", response.lines().next().unwrap_or("")), EXIT_UNREACHABLE);
    };
    if v["ok"].as_bool() != Some(true) {
        println!("{text}");
        exit_with(&format!("{}: {}", v["error"]["code"].as_str().unwrap_or("error"), v["error"]["message"].as_str().unwrap_or("")), EXIT_API);
    }
    if parsed.command == "list" && !parsed.json {
        for agent in v["agents"].as_array().into_iter().flatten() {
            println!("{}", list_line(agent));
        }
        return;
    }
    if parsed.json {
        println!("{text}");
    } else {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_else(|_| text.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Parsed, String> {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse_args(&owned, |path| if path == "inst.md" { Ok("from file".into()) } else { Err("missing".into()) })
    }

    #[test]
    fn create_flags_map_to_body() {
        let p = parse(&["create", "--name", "rev", "--profile", "claude", "--model", "haiku", "--cwd", "/repo",
            "--instructions-file", "inst.md", "--auto-start", "false", "--json"]).unwrap();
        assert_eq!(p.command, "create");
        assert!(p.json);
        assert_eq!(Value::Object(p.body), serde_json::json!({
            "name": "rev", "profile": "claude", "model": "haiku", "cwd": "/repo",
            "instructions": "from file", "autoStart": false,
        }));
    }

    #[test]
    fn create_defaults_cwd_and_requires_name_and_profile() {
        let p = parse(&["create", "--name", "rev", "--profile", "claude"]).unwrap();
        assert!(p.body["cwd"].as_str().is_some_and(|c| !c.is_empty()));
        assert!(parse(&["create", "--name", "rev"]).is_err());
        assert!(parse(&["create", "--name", "rev", "--profile", "claude", "--instructions-file", "nope"]).is_err());
    }

    #[test]
    fn positional_ref_and_bool_flags() {
        let p = parse(&["stop", "rev", "--force"]).unwrap();
        assert_eq!(Value::Object(p.body), serde_json::json!({ "id": "rev", "force": true }));
        let p = parse(&["show", "rev", "--tail", "20"]).unwrap();
        assert_eq!(p.body["tail"], 20);
        let p = parse(&["update", "rev", "--name", "reviewer", "--title", "Reviewer"]).unwrap();
        assert_eq!(p.body["id"], "rev");
        assert_eq!(p.body["name"], "reviewer");
    }

    #[test]
    fn usage_errors() {
        assert!(parse(&[]).is_err());
        assert!(parse(&["bogus"]).is_err());
        assert!(parse(&["start"]).is_err());
        assert!(parse(&["stop", "rev", "--fresh"]).is_err());
        assert!(parse(&["start", "a", "b"]).is_err());
        assert!(parse(&["show", "rev", "--tail", "x"]).is_err());
        assert!(parse(&["create", "--name", "rev", "--profile", "claude", "--auto-start", "maybe"]).is_err());
        assert!(parse(&["list"]).is_ok());
    }

    #[test]
    fn list_line_format() {
        let agent = serde_json::json!({"name":"rev","status":"idle","profile":"claude","model":"haiku","project":"D:/repo"});
        assert_eq!(list_line(&agent), "rev  idle  claude:haiku  D:/repo");
        let bare = serde_json::json!({"name":"rev","status":"stopped","profile":"kimi"});
        assert_eq!(list_line(&bare), "rev  stopped  kimi  -");
    }
}
