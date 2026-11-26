use serde_json::{Map, Value};

use crate::orch::cli::{http_post, parse_response_body};

const USAGE: &str = "usage: panorama-cli agent <create|list|show|update|delete|start|stop|send|reply|read|inbox|wait|waits|coordinate|compact|memory|routine> [args] [--flags] [--json]";
const PREVIEW_CHARS: usize = 40;

#[derive(Debug, Default)]
struct CliEnv {
    agent: Option<String>,
    tile: Option<String>,
    worker: bool,
}

impl CliEnv {
    fn from_process() -> Self {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Self { agent: var("PANORAMA_AGENT_ID"), tile: var("PANORAMA_TILE_ID"), worker: var("PANORAMA_DISPATCH_ID").is_some() }
    }
}
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
        "send" => "expects-reply file",
        "reply" => "file",
        "read" => "",
        "inbox" => "pending limit",
        "wait" => "on timeout note cancel cancel-all",
        "waits" => "all status",
        "coordinate" => "run off",
        "compact" => "",
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
        "expects-reply" => "expectsReply",
        "file" => "text",
        "pending" => "pending",
        "limit" => "limit",
        "on" => "on",
        "timeout" => "timeoutMs",
        "note" => "note",
        "cancel" => "id",
        "cancel-all" => "all",
        "all" => "all",
        "status" => "status",
        "run" => "runId",
        "off" => "off",
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

fn positional_fields(command: &str) -> &'static [&'static str] {
    match command {
        "create" | "list" | "wait" | "waits" | "compact" => &[],
        "send" | "reply" => &["to", "text"],
        "inbox" | "coordinate" => &["agent"],
        _ => &["id"],
    }
}

fn parse_args<R>(args: &[String], env: &CliEnv, read_file: R) -> Result<Parsed, String>
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
            let Some(field) = positional_fields(&command).iter().find(|f| !body.contains_key(**f)) else {
                return Err(format!("unexpected argument {arg}"));
            };
            body.insert((*field).into(), arg.clone().into());
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
        if flag == "on" {
            let raw = args.get(i + 1).ok_or("--on requires a target")?;
            if let Some(Value::Array(list)) = body.get_mut("on") {
                list.push(raw.clone().into());
            } else {
                body.insert("on".into(), Value::Array(vec![raw.clone().into()]));
            }
            i += 2;
            continue;
        }
        if matches!(flag, "fresh" | "force" | "expects-reply" | "pending" | "cancel-all" | "all" | "off") {
            body.insert(field.into(), true.into());
            i += 1;
            continue;
        }
        let raw = args.get(i + 1).ok_or_else(|| format!("--{flag} requires a value"))?;
        let value: Value = match flag {
            "auto-start" => parse_bool(flag, raw)?.into(),
            "tail" | "limit" => raw.parse::<u64>().map_err(|_| format!("--{flag} requires a number"))?.into(),
            "timeout" => crate::agents::waits::parse_duration(raw).ok_or("--timeout takes a duration like 30m or 2h")?.into(),
            "instructions-file" | "file" => read_file(raw)?.into(),
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
    } else if matches!(command.as_str(), "send" | "reply") {
        if !body.contains_key("to") || !body.contains_key("text") {
            return Err(format!("{command} requires a recipient and a text (or --file)"));
        }
        if let Some(agent) = &env.agent { body.insert("fromAgent".into(), agent.clone().into()); }
        if let Some(tile) = &env.tile { body.insert("fromTile".into(), tile.clone().into()); }
        if env.worker { body.insert("worker".into(), true.into()); }
    } else if matches!(command.as_str(), "wait" | "waits") {
        let cancel = body.contains_key("id") || body.contains_key("all") && command == "wait";
        if command == "wait" && !cancel && !body.contains_key("on") {
            return Err("wait requires --on <target> (or --cancel <waitId> / --cancel-all)".into());
        }
        if command == "wait" && cancel && body.contains_key("on") {
            return Err("use either --on or --cancel".into());
        }
        if command == "waits" && body.remove("all").is_none() {
            body.insert("mine".into(), true.into());
        }
        if let Some(agent) = &env.agent { body.insert("fromAgent".into(), agent.clone().into()); }
        if let Some(tile) = &env.tile { body.insert("fromTile".into(), tile.clone().into()); }
        if env.worker { body.insert("worker".into(), true.into()); }
    } else if command == "coordinate" {
        if !body.contains_key("agent") || !body.contains_key("runId") {
            return Err("coordinate requires an agent name and --run <runId>".into());
        }
        let off = body.remove("off").is_some();
        body.insert("on".into(), (!off).into());
    } else if command == "compact" {
        if let Some(agent) = &env.agent { body.insert("fromAgent".into(), agent.clone().into()); }
        if let Some(tile) = &env.tile { body.insert("fromTile".into(), tile.clone().into()); }
        if env.worker { body.insert("worker".into(), true.into()); }
    } else if command == "read" && !body.contains_key("id") {
        return Err("read requires a message id".into());
    } else if !matches!(command.as_str(), "list" | "inbox" | "read") && !body.contains_key("id") {
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

fn message_line(m: &Value) -> String {
    let field = |k: &str| m[k].as_str().unwrap_or("").to_string();
    let flag = if m["expectsReply"] == true {
        if m["repliedBy"].is_string() { "  answered" } else { "  reply expected" }
    } else if m["replyTo"].is_string() {
        "  reply"
    } else {
        ""
    };
    let text = field("text").replace(char::is_control, " ");
    let mut preview: String = text.chars().take(PREVIEW_CHARS).collect();
    if text.chars().count() > PREVIEW_CHARS { preview.push_str("..."); }
    format!("{}  {} -> {}  {}{}  \"{}\"", field("id"), field("fromLabel"), field("toLabel"), field("status"), flag, preview)
}

fn route<'a>(command: &'a str, body: &Map<String, Value>) -> &'a str {
    match command {
        "inbox" => "messages",
        "wait" if body.contains_key("id") || body.contains_key("all") => "wait-cancel",
        _ => command,
    }
}

fn wait_line(w: &Value) -> String {
    let on: Vec<&str> = w["on"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let status = w["status"].as_str().unwrap_or("");
    let fired = w["fired"]["summary"].as_str().map(|s| format!("  -> {s}")).unwrap_or_default();
    format!("{}  {status}  {}{fired}", w["id"].as_str().unwrap_or(""), on.join(" | "))
}

fn exit_with(message: &str, code: i32) -> ! {
    eprintln!("[agent] {message}");
    std::process::exit(code);
}

const ROUTINE_USAGE: &str = "usage: panorama-cli agent routine <create|list|show|update|enable|disable|delete|run|report|token|payload> ...";

fn routine_flag(flag: &str) -> Option<(&'static str, &'static str)> {
    Some(match flag {
        "name" => ("name", "text"),
        "at" => ("at", "text"),
        "days" => ("days", "text"),
        "cron" => ("cron", "text"),
        "every" => ("every", "text"),
        "hook" => ("hook", "bool"),
        "manual" => ("manual", "bool"),
        "text" => ("text", "text"),
        "file" => ("text", "file"),
        "missed" => ("missed", "text"),
        "catch-up-window" => ("catchUpWindowMs", "dur"),
        "overlap" => ("overlap", "text"),
        "fresh" => ("fresh", "bool"),
        "keep-session" => ("fresh", "false"),
        "report-to" => ("reportTo", "text"),
        "max-per-day" => ("maxPerDay", "int"),
        "max-cost-day" => ("maxCostDay", "float"),
        "run-timeout" => ("runTimeoutMs", "dur"),
        "if-changed" => ("ifChanged", "text"),
        "disabled" => ("disabled", "bool"),
        "runs" => ("limit", "int"),
        "payload-file" => ("payload", "file"),
        "no-update" => ("noUpdate", "bool"),
        "failed" => ("failed", "bool"),
        "rotate" => ("rotate", "bool"),
        _ => return None,
    })
}

fn routine_shape(sub: &str) -> Option<(&'static str, &'static [&'static str], &'static str)> {
    Some(match sub {
        "create" => ("routine-create", &["agent"], "name at days cron every hook manual text file missed catch-up-window overlap fresh keep-session report-to max-per-day max-cost-day run-timeout if-changed disabled"),
        "update" => ("routine-update", &["id"], "name at days cron every hook manual text file missed catch-up-window overlap fresh keep-session report-to max-per-day max-cost-day run-timeout if-changed"),
        "list" => ("routines", &["agent"], ""),
        "show" => ("routine-runs", &["id"], "runs"),
        "enable" => ("routine-enable", &["id"], ""),
        "disable" => ("routine-disable", &["id"], ""),
        "delete" => ("routine-delete", &["id"], ""),
        "run" => ("routine-run", &["id"], "payload-file"),
        "report" => ("routine-report", &["runId", "text"], "no-update failed"),
        "token" => ("routine-token", &["id"], "rotate"),
        "payload" => ("routine-payload", &["runId"], ""),
        _ => return None,
    })
}

fn parse_routine<R>(args: &[String], env: &CliEnv, read_file: R) -> Result<(String, Map<String, Value>), String>
where
    R: Fn(&str) -> Result<String, String>,
{
    let sub = args.first().ok_or(ROUTINE_USAGE)?;
    let (route, positional, allowed) = routine_shape(sub).ok_or(ROUTINE_USAGE)?;
    let mut body = Map::new();
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        let Some(flag) = arg.strip_prefix("--") else {
            let field = positional.iter().find(|f| !body.contains_key(**f)).ok_or_else(|| format!("unexpected argument {arg}"))?;
            body.insert((*field).into(), arg.clone().into());
            i += 1;
            continue;
        };
        if !allowed.split_whitespace().any(|f| f == flag) {
            return Err(format!("unknown flag --{flag} for routine {sub}"));
        }
        let (field, kind) = routine_flag(flag).ok_or_else(|| format!("unknown flag --{flag}"))?;
        if kind == "bool" || kind == "false" {
            body.insert(field.into(), (kind == "bool").into());
            i += 1;
            continue;
        }
        let raw = args.get(i + 1).ok_or_else(|| format!("--{flag} requires a value"))?;
        let value: Value = match kind {
            "file" => read_file(raw)?.into(),
            "int" => raw.parse::<u64>().map_err(|_| format!("--{flag} requires a number"))?.into(),
            "float" => raw.parse::<f64>().map_err(|_| format!("--{flag} requires a number"))?.into(),
            "dur" => crate::agents::waits::parse_duration(raw).ok_or_else(|| format!("--{flag} takes a duration like 30m or 12h"))?.into(),
            _ => raw.clone().into(),
        };
        body.insert(field.into(), value);
        i += 2;
    }
    let needed: &[&str] = match sub.as_str() {
        "list" => &[],
        "report" => &["runId"],
        "create" => &["agent", "name"],
        _ => &[positional[0]],
    };
    if let Some(missing) = needed.iter().find(|f| !body.contains_key(**f)) {
        return Err(format!("routine {sub} requires {missing}"));
    }
    if sub == "create" && !body.contains_key("text") {
        return Err("routine create requires --text or --file".into());
    }
    if let Some(agent) = &env.agent { body.insert("fromAgent".into(), agent.clone().into()); }
    if let Some(tile) = &env.tile { body.insert("fromTile".into(), tile.clone().into()); }
    if env.worker { body.insert("worker".into(), true.into()); }
    Ok((route.to_string(), body))
}

fn routine_line(r: &Value) -> String {
    let s = |k: &str| r[k].as_str().unwrap_or("-").to_string();
    let state = if r["pausedReason"].is_string() { format!("paused ({})", s("pausedReason")) } else if r["enabled"] == true { "on".into() } else { "off".into() };
    let last = r["last"]["outcome"].as_str().unwrap_or("-");
    format!("{}  {}  {}  {}  next {}  last {}", s("id"), s("name"), s("triggerText"), state, s("nextRunLocal"), last)
}

fn routine_main(args: &[String]) -> ! {
    let json = args.iter().any(|a| a == "--json");
    let args: Vec<String> = args.iter().filter(|a| *a != "--json").cloned().collect();
    let (route, body) = parse_routine(&args, &CliEnv::from_process(), read_file).unwrap_or_else(|e| exit_with(&e, EXIT_USAGE));
    let text_body = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
    let response = http_post(crate::port(), &format!("/agents/{route}"), &text_body, 15_000)
        .unwrap_or_else(|e| exit_with(&format!("brain unreachable: {e}"), EXIT_UNREACHABLE));
    let text = parse_response_body(&response);
    let v: Value = serde_json::from_str(text).unwrap_or_else(|_| exit_with("unexpected response", EXIT_UNREACHABLE));
    if v["ok"].as_bool() != Some(true) {
        exit_with(&format!("{}: {}", v["error"]["code"].as_str().unwrap_or("error"), v["error"]["message"].as_str().unwrap_or("")), EXIT_API);
    }
    if json {
        println!("{text}");
        std::process::exit(0);
    }
    match route.as_str() {
        "routines" => v["routines"].as_array().into_iter().flatten().for_each(|r| println!("{}", routine_line(r))),
        "routine-create" | "routine-update" | "routine-enable" | "routine-disable" => {
            println!("{}", routine_line(&v["routine"]));
            if let Some(token) = v["token"].as_str() {
                println!("Webhook token (shown once): {token}");
            }
        }
        "routine-token" => println!("Webhook token (shown once): {}", v["token"].as_str().unwrap_or("")),
        "routine-run" => match v["runId"].as_str() {
            Some(run) => println!("Started run {run}."),
            None => println!("Skipped: {}", v["skipped"].as_str().unwrap_or("")),
        },
        "routine-report" => println!("{}", v["text"].as_str().unwrap_or("")),
        "routine-payload" => println!("{}", v["payload"].as_str().unwrap_or("")),
        "routine-delete" => println!("Deleted."),
        _ => println!("{}", serde_json::to_string_pretty(&v).unwrap_or_else(|_| text.to_string())),
    }
    std::process::exit(0);
}

const MEMORY_USAGE: &str = "usage: panorama-cli agent memory <remember|edit|set|forget|search|list> [args] [--essential|--searchable] [--agent <name>] [--json]";

fn parse_memory(args: &[String], env: &CliEnv) -> Result<(String, Map<String, Value>), String> {
    let sub = args.first().ok_or(MEMORY_USAGE)?;
    let (route, positional, flags): (&str, &[&str], &str) = match sub.as_str() {
        "remember" => ("memory-remember", &["text"], "essential agent"),
        "edit" => ("memory-edit", &["id", "text"], "agent"),
        "set" => ("memory-set", &["id"], "essential searchable agent"),
        "forget" => ("memory-forget", &["id"], "agent"),
        "search" => ("memory-search", &["query"], "agent"),
        "list" => ("memory", &[], "essential agent"),
        _ => return Err(MEMORY_USAGE.into()),
    };
    let mut body = Map::new();
    let mut words: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        let Some(flag) = arg.strip_prefix("--") else {
            words.push(arg.clone());
            i += 1;
            continue;
        };
        if !flags.split_whitespace().any(|f| f == flag) {
            return Err(format!("unknown flag --{flag} for memory {sub}"));
        }
        match flag {
            "essential" | "searchable" => {
                body.insert("essential".into(), (flag == "essential").into());
                i += 1;
            }
            _ => {
                let raw = args.get(i + 1).ok_or_else(|| format!("--{flag} requires a value"))?;
                body.insert(flag.into(), raw.clone().into());
                i += 2;
            }
        }
    }
    let last = positional.len().saturating_sub(1);
    for (n, field) in positional.iter().enumerate() {
        let value = if n == last { words[n.min(words.len())..].join(" ") } else { words.get(n).cloned().unwrap_or_default() };
        if value.trim().is_empty() {
            return Err(format!("memory {sub} requires {field}"));
        }
        body.insert((*field).into(), value.into());
    }
    if positional.is_empty() && !words.is_empty() {
        return Err(format!("unexpected argument {}", words[0]));
    }
    if sub == "set" && !body.contains_key("essential") {
        return Err("memory set requires --essential or --searchable".into());
    }
    if let Some(agent) = &env.agent { body.insert("fromAgent".into(), agent.clone().into()); }
    if let Some(tile) = &env.tile { body.insert("fromTile".into(), tile.clone().into()); }
    if env.worker { body.insert("worker".into(), true.into()); }
    Ok((route.to_string(), body))
}

fn memory_main(args: &[String]) -> ! {
    let json = args.iter().any(|a| a == "--json");
    let args: Vec<String> = args.iter().filter(|a| *a != "--json").cloned().collect();
    let (route, body) = parse_memory(&args, &CliEnv::from_process()).unwrap_or_else(|e| exit_with(&e, EXIT_USAGE));
    let text_body = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
    let response = http_post(crate::port(), &format!("/agents/{route}"), &text_body, 15_000)
        .unwrap_or_else(|e| exit_with(&format!("brain unreachable: {e}"), EXIT_UNREACHABLE));
    let text = parse_response_body(&response);
    let v: Value = serde_json::from_str(text).unwrap_or_else(|_| exit_with("unexpected response", EXIT_UNREACHABLE));
    if v["ok"].as_bool() != Some(true) {
        exit_with(&format!("{}: {}", v["error"]["code"].as_str().unwrap_or("error"), v["error"]["message"].as_str().unwrap_or("")), EXIT_API);
    }
    if json {
        println!("{text}");
    } else if let Some(lines) = v["lines"].as_array() {
        if lines.is_empty() {
            println!("No memories found.");
        }
        lines.iter().filter_map(Value::as_str).for_each(|l| println!("{l}"));
    } else {
        println!("{}", v["text"].as_str().unwrap_or(""));
    }
    std::process::exit(0);
}

pub fn main() {
    let args: Vec<String> = std::env::args().skip(2).collect();
    if args.first().map(String::as_str) == Some("memory") {
        memory_main(&args[1..]);
    }    if args.first().map(String::as_str) == Some("routine") {
        routine_main(&args[1..]);
    }
    let parsed = parse_args(&args, &CliEnv::from_process(), read_file).unwrap_or_else(|e| exit_with(&e, EXIT_USAGE));
    let body = serde_json::to_string(&parsed.body).unwrap_or_else(|_| "{}".into());
    let path = format!("/agents/{}", route(&parsed.command, &parsed.body));
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
        return;
    }
    match parsed.command.as_str() {
        "send" | "reply" => println!("{}", v["text"].as_str().unwrap_or_default()),
        "wait" if v["cancelled"].is_number() => println!("Cancelled {} wait(s).", v["cancelled"]),
        "wait" => println!("{}", v["text"].as_str().unwrap_or_default()),
        "waits" => v["waits"].as_array().into_iter().flatten().for_each(|w| println!("{}", wait_line(w))),
        "coordinate" => println!("{}", v["text"].as_str().unwrap_or_default()),
        "compact" => println!("Compacted the logs: {} KB -> {} KB", v["before"].as_u64().unwrap_or(0) / 1024, v["after"].as_u64().unwrap_or(0) / 1024),
        "inbox" => v["messages"].as_array().into_iter().flatten().for_each(|m| println!("{}", message_line(m))),
        "read" => {
            println!("{}", message_line(&v["message"]));
            println!("{}", v["message"]["text"].as_str().unwrap_or_default());
        }
        _ => println!("{}", serde_json::to_string_pretty(&v).unwrap_or_else(|_| text.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_with(args: &[&str], env: &CliEnv) -> Result<Parsed, String> {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse_args(&owned, env, |path| if path == "inst.md" { Ok("from file".into()) } else { Err("missing".into()) })
    }

    fn parse(args: &[&str]) -> Result<Parsed, String> {
        parse_with(args, &CliEnv::default())
    }

    #[test]
    fn send_takes_recipient_and_text_and_the_sender_from_env() {
        let env = CliEnv { agent: Some("a1234567".into()), tile: Some("agent-a1234567".into()), worker: false };
        let p = parse_with(&["send", "rev", "hello there", "--expects-reply"], &env).unwrap();
        assert_eq!(Value::Object(p.body), serde_json::json!({
            "to": "rev", "text": "hello there", "expectsReply": true, "fromAgent": "a1234567", "fromTile": "agent-a1234567",
        }));
        let worker = CliEnv { tile: Some("t1".into()), worker: true, ..Default::default() };
        let p = parse_with(&["send", "rev", "--file", "inst.md"], &worker).unwrap();
        assert_eq!(p.body["text"], "from file");
        assert_eq!(p.body["worker"], true);
        assert!(p.body.get("fromAgent").is_none());
        let user = parse(&["reply", "m123456", "ok"]).unwrap();
        assert_eq!(Value::Object(user.body), serde_json::json!({ "to": "m123456", "text": "ok" }));
    }

    #[test]
    fn messaging_usage_errors_and_routes() {
        assert!(parse(&["send", "rev"]).is_err());
        assert!(parse(&["send", "rev", "a", "b"]).is_err());
        assert!(parse(&["reply", "m1", "x", "--expects-reply"]).is_err(), "a reply cannot expect a reply");
        assert!(parse(&["read"]).is_err());
        assert_eq!(parse(&["read", "m1"]).unwrap().body["id"], "m1");
        let inbox = parse(&["inbox", "rev", "--pending", "--limit", "5"]).unwrap();
        assert_eq!(Value::Object(inbox.body), serde_json::json!({ "agent": "rev", "pending": true, "limit": 5 }));
        assert!(parse(&["inbox"]).is_ok());
        assert_eq!(route("inbox", &Map::new()), "messages");
        assert_eq!(route("send", &Map::new()), "send");
    }

    #[test]
    fn wait_collects_targets_and_cancels_through_its_own_route() {
        let env = CliEnv { agent: Some("a1234567".into()), tile: Some("agent-a1234567".into()), worker: false };
        let p = parse_with(&["wait", "--on", "in:2m", "--on", "session:build@D:/x", "--timeout", "30m", "--note", "fix build"], &env).unwrap();
        assert_eq!(Value::Object(p.body.clone()), serde_json::json!({
            "on": ["in:2m", "session:build@D:/x"], "timeoutMs": 1_800_000, "note": "fix build", "fromAgent": "a1234567", "fromTile": "agent-a1234567",
        }));
        assert_eq!(route("wait", &p.body), "wait");
        let c = parse_with(&["wait", "--cancel", "w123"], &env).unwrap();
        assert_eq!(c.body["id"], "w123");
        assert_eq!(route("wait", &c.body), "wait-cancel");
        let all = parse_with(&["wait", "--cancel-all"], &env).unwrap();
        assert_eq!(route("wait", &all.body), "wait-cancel");
        assert!(parse(&["wait"]).is_err());
        assert!(parse(&["wait", "--on", "in:2m", "--cancel", "w1"]).is_err());
        assert!(parse(&["wait", "--on", "in:2m", "--timeout", "soon"]).is_err());
        assert_eq!(parse(&["waits"]).unwrap().body["mine"], true);
        assert!(parse(&["waits", "--all"]).unwrap().body.get("mine").is_none());
        let worker = CliEnv { tile: Some("t1".into()), worker: true, ..Default::default() };
        assert_eq!(parse_with(&["wait", "--on", "in:2m"], &worker).unwrap().body["worker"], true);
    }

    #[test]
    fn routine_commands_parse() {
        let user = CliEnv::default();
        let read = |p: &str| if p == "inst.md" { Ok("from file".to_string()) } else { Err("missing".to_string()) };
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (route, body) = parse_routine(&args(&["create", "k", "--name", "Check", "--at", "08:30", "--days", "mon-fri", "--file", "inst.md", "--overlap", "queue", "--run-timeout", "30m", "--max-per-day", "4", "--fresh"]), &user, read).unwrap();
        assert_eq!(route, "routine-create");
        assert_eq!(Value::Object(body), serde_json::json!({
            "agent": "k", "name": "Check", "at": "08:30", "days": "mon-fri", "text": "from file", "overlap": "queue", "runTimeoutMs": 1_800_000, "maxPerDay": 4, "fresh": true,
        }));
        let (route, body) = parse_routine(&args(&["report", "rt1-3", "--failed", "disk full"]), &CliEnv { agent: Some("a1".into()), ..Default::default() }, read).unwrap();
        assert_eq!(route, "routine-report");
        assert_eq!(Value::Object(body), serde_json::json!({ "runId": "rt1-3", "failed": true, "text": "disk full", "fromAgent": "a1" }));
        assert_eq!(parse_routine(&args(&["list"]), &user, read).unwrap().0, "routines");
        assert_eq!(parse_routine(&args(&["show", "rt1", "--runs", "5"]), &user, read).unwrap().1["limit"], 5);
        assert!(parse_routine(&args(&["create", "k", "--name", "x"]), &user, read).is_err(), "needs a text");
        assert!(parse_routine(&args(&["run"]), &user, read).is_err());
        assert!(parse_routine(&args(&["enable", "rt1", "--fresh"]), &user, read).is_err());
        assert!(parse_routine(&args(&["nope"]), &user, read).is_err());
    }

    #[test]
    fn coordinate_takes_an_agent_and_a_run() {
        let on = parse(&["coordinate", "planner", "--run", "r6un"]).unwrap();
        assert_eq!(Value::Object(on.body), serde_json::json!({ "agent": "planner", "runId": "r6un", "on": true }));
        let off = parse(&["coordinate", "planner", "--run", "r6un", "--off"]).unwrap();
        assert_eq!(off.body["on"], false);
        assert!(parse(&["coordinate", "planner"]).is_err());
    }

    #[test]
    fn message_line_format() {
        let m = serde_json::json!({ "id": "m4k2p9", "fromLabel": "planner", "toLabel": "rev", "status": "delivered",
            "expectsReply": true, "text": "Can you review src/foo.rs for the off-by-one in parse()?" });
        assert_eq!(message_line(&m), "m4k2p9  planner -> rev  delivered  reply expected  \"Can you review src/foo.rs for the off-by...\"");
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

    #[test]
    fn memory_commands_parse_with_the_caller_from_env() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let env = CliEnv { agent: Some("a1".into()), tile: Some("agent-a1".into()), worker: false };
        let (route, body) = parse_memory(&args(&["remember", "Reply", "in", "Portuguese.", "--essential"]), &env).unwrap();
        assert_eq!(route, "memory-remember");
        assert_eq!((body["text"].as_str(), body["essential"].as_bool(), body["fromAgent"].as_str()), (Some("Reply in Portuguese."), Some(true), Some("a1")));
        assert!(!body.contains_key("by"), "by is set by the brain");
        let (route, body) = parse_memory(&args(&["set", "m1", "--searchable"]), &env).unwrap();
        assert_eq!((route.as_str(), body["essential"].as_bool()), ("memory-set", Some(false)));
        let (_, body) = parse_memory(&args(&["search", "deploy", "staging"]), &env).unwrap();
        assert_eq!(body["query"], "deploy staging");
        let (_, body) = parse_memory(&args(&["edit", "m1", "new", "text"]), &env).unwrap();
        assert_eq!((body["id"].as_str(), body["text"].as_str()), (Some("m1"), Some("new text")));
        assert!(parse_memory(&args(&["set", "m1"]), &env).is_err());
        assert!(parse_memory(&args(&["remember"]), &env).is_err());
        assert!(parse_memory(&args(&["remember", "x", "--by", "user"]), &env).is_err());
        assert!(parse_memory(&args(&["list", "extra"]), &env).is_err());
    }
}