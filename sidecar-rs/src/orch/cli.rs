use std::collections::HashMap;
use std::io::{Read, Write};
use std::time::Duration;

pub fn main() {
    let args: Vec<String> = std::env::args().skip(2).collect();
    if args.is_empty() {
        eprintln!("usage: panorama-cli orch <command> [--flags]\n       panorama-cli orch plan-apply --file <path.json>");
        std::process::exit(2);
    }
    let command = args[0].clone();
    let rest = &args[1..];
    if command == "exec" {
        exec_run(rest, crate::port());
    }
    let (mut body, pretty) = parse_flags(rest, &command).unwrap_or_else(|msg| {
        eprintln!("[orch] {msg}");
        std::process::exit(2);
    });
    if command != "plan-apply" {
        apply_env_defaults(&command, &mut body);
    }
    if command == "worker-send" && body.get("keys").and_then(|v| v.as_bool()) == Some(true) {
        if let Some(text) = body.get("text").and_then(|v| v.as_str()) {
            match decode_keys(text) {
                Ok(bytes) => { body.insert("keysBytes".into(), serde_json::json!(bytes)); }
                Err(e) => { eprintln!("[orch] {e}"); std::process::exit(2); }
            }
        }
    }

    if let Err(msg) = validate_required(&command, &body) {
        eprintln!("[orch] {msg}");
        std::process::exit(2);
    }

    if command == "plan-apply" {
        body = read_plan(std::path::Path::new(body["file"].as_str().unwrap_or_default()))
            .unwrap_or_else(|msg| {
                eprintln!("[orch] {msg}");
                std::process::exit(2);
            });
    }

    let wait = body.get("wait").and_then(|v| v.as_bool()).unwrap_or(false);
    let timeout_ms = body.get("timeoutMs").and_then(|v| v.as_u64()).unwrap_or(30_000);
    let read_timeout_ms = if wait {
        timeout_ms.min(600_000).saturating_add(5_000)
    } else {
        10_000
    };

    let port = crate::port();
    let json_body = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
    let path = format!("/orch/{command}");

    let format_markdown = command == "report"
        && body.get("format").and_then(|v| v.as_str()) == Some("markdown");

    match http_post(port, &path, &json_body, read_timeout_ms) {
        Ok(response) => {
            let body_text = parse_response_body(&response);
            let v = match serde_json::from_str::<serde_json::Value>(body_text) {
                Ok(v) => v,
                Err(_) => {
                    let status_line = response.lines().next().unwrap_or("HTTP error");
                    eprintln!("[orch] unexpected response: {status_line}");
                    std::process::exit(4);
                }
            };
            let ok = v["ok"].as_bool().unwrap_or(false);
            if ok && format_markdown {
                if let Some(text) = v["text"].as_str() {
                    print!("{text}");
                    return;
                }
            }
            if pretty {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_else(|_| body_text.to_string()));
            } else {
                println!("{body_text}");
            }
            if !ok {
                let code = v["error"]["code"].as_str().unwrap_or("error");
                eprintln!("[orch] {}: {}", code, v["error"]["message"].as_str().unwrap_or(""));
                std::process::exit(3);
            }
        }
        Err(e) => {
            eprintln!("[orch] brain unreachable: {e}");
            std::process::exit(4);
        }
    }
}

fn decode_keys(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut bytes = text.bytes();
    while let Some(b) = bytes.next() {
        if b != b'\\' { out.push(b); continue; }
        match bytes.next() {
            Some(b'r') => out.push(b'\r'),
            Some(b'n') => out.push(b'\n'),
            Some(b't') => out.push(b'\t'),
            Some(b'e') => out.push(27),
            Some(b'\\') => out.push(b'\\'),
            Some(b'x') => {
                let hi = bytes.next().and_then(|b| (b as char).to_digit(16));
                let lo = bytes.next().and_then(|b| (b as char).to_digit(16));
                match (hi, lo) { (Some(h), Some(l)) => out.push((h * 16 + l) as u8), _ => return Err("invalid hex key escape".into()) }
            }
            Some(other) => { out.push(b'\\'); out.push(other); }
            None => out.push(b'\\'),
        }
    }
    Ok(out)
}

#[cfg(test)]
#[test]
fn key_escapes() {
    assert_eq!(decode_keys(r"a\r\n\t\e\xFF\\").unwrap(), b"a\r\n\t\x1b\xff\\");
    assert_eq!(decode_keys("é").unwrap(), "é".as_bytes());
    assert!(decode_keys(r"\xG0").is_err());
}

fn apply_env_defaults(command: &str, body: &mut serde_json::Map<String, serde_json::Value>) {
    if let Ok(run_id) = std::env::var("PANORAMA_RUN_ID") {
        if !run_id.is_empty() && !body.contains_key("runId") {
            body.insert("runId".into(), run_id.into());
        }
    }
    let dispatch_id = std::env::var("PANORAMA_DISPATCH_ID").ok().filter(|s| !s.is_empty());
    let from_default = dispatch_id.clone().unwrap_or_else(|| "coordinator".into());
    if !body.contains_key("from") {
        body.insert("from".into(), from_default.clone().into());
    }
    if let Some(ref did) = dispatch_id {
        if !body.contains_key("dispatchId") {
            body.insert("dispatchId".into(), did.clone().into());
        }
    }
    if command == "check" && !body.contains_key("recipient") {
        body.insert("recipient".into(), from_default.clone().into());
    }
    if command == "run-create" {
        apply_coordinator_tile(body);
    }
    if command == "send" && !body.contains_key("to") {
        body.insert("to".into(), "coordinator".into());
    }
}

fn apply_coordinator_tile(body: &mut serde_json::Map<String, serde_json::Value>) {
    if !body.contains_key("coordinatorTile") {
        if let Ok(tile) = std::env::var("PANORAMA_TILE_ID").map(|t| t.trim().to_string()) {
            if !tile.is_empty() {
                body.insert("coordinatorTile".into(), tile.into());
            }
        }
    }
}

fn read_plan(path: &std::path::Path) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    read_plan_inner(path, true)
}

fn read_plan_inner(path: &std::path::Path, fill_coordinator: bool) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read plan {}: {e}", path.display()))?;
    let mut plan: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&contents)
        .map_err(|e| format!("invalid plan {}: {e}", path.display()))?;
    if fill_coordinator {
        if let Some(run) = plan.get_mut("run").and_then(serde_json::Value::as_object_mut) {
            apply_coordinator_tile(run);
        }
    }
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    if let Some(tasks) = plan.get_mut("tasks").and_then(serde_json::Value::as_array_mut) {
        for (index, task) in tasks.iter_mut().enumerate() {
            let label = super::plan::task_label(index, task);
            let Some(file) = task.as_object_mut().and_then(|obj| obj.remove("specFile")) else {
                continue;
            };
            let file = file.as_str().filter(|s| !s.is_empty())
                .ok_or_else(|| format!("{label}: specFile must be a non-empty path"))?;
            let spec_path = dir.join(file);
            let spec = std::fs::read_to_string(&spec_path)
                .map_err(|e| format!("{label}: cannot read specFile {}: {e}", spec_path.display()))?;
            task["spec"] = spec.into();
        }
    }
    Ok(plan)
}

struct ExecOpts {
    file: String,
    timeout_min: u64,
}

fn parse_exec_args(args: &[String]) -> Result<ExecOpts, String> {
    let mut file: Option<String> = None;
    let mut timeout_min = 240u64;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--file" => {
                i += 1;
                file = Some(args.get(i)
                    .ok_or_else(|| "--file requires a value".to_string())?
                    .clone());
            }
            "--timeout-min" => {
                i += 1;
                timeout_min = args.get(i)
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| "--timeout-min requires a number".to_string())?;
            }
            f if f.starts_with("--") => return Err(format!("unknown flag {f} for exec")),
            _ => {}
        }
        i += 1;
    }
    Ok(ExecOpts {
        file: file.ok_or_else(|| "exec requires --file <plan.json>".to_string())?,
        timeout_min,
    })
}

fn exec_exit_code(msgs: &[serde_json::Value], tasks: &serde_json::Value) -> i32 {
    let any_failed = tasks.as_array().is_some_and(|a| {
        a.iter().any(|t| t["status"].as_str() == Some("failed"))
    });
    let finished = msgs.iter().any(|m| m["type"].as_str() == Some("run_finished"));
    let only_idle = msgs.iter().all(|m| m["type"].as_str() == Some("run_idle"));
    match (finished || only_idle, any_failed, finished) {
        (true, true, _) => 1,
        (_, false, true) => 0,
        _ => 2,
    }
}

fn exec_fetch_report(port: u16, run_id: &str) -> Option<String> {
    let body = serde_json::json!({"runId": run_id, "format": "markdown"}).to_string();
    let response = http_post(port, "/orch/report", &body, 15_000).ok()?;
    let v: serde_json::Value = serde_json::from_str(parse_response_body(&response)).ok()?;
    if v["ok"].as_bool() != Some(true) { return None; }
    v["text"].as_str().map(str::to_string)
}

fn exec_fetch_tasks(port: u16, run_id: &str) -> serde_json::Value {
    let body = serde_json::json!({"runId": run_id, "brief": true}).to_string();
    match http_post(port, "/orch/run-show", &body, 10_000) {
        Ok(r) => {
            let v: serde_json::Value = serde_json::from_str(parse_response_body(&r))
                .unwrap_or_default();
            v["tasks"].clone()
        }
        Err(_) => serde_json::Value::Array(vec![]),
    }
}

fn exec_print_decisions(msgs: &[serde_json::Value], run_id: &str, cli: &str) {
    println!("\n## Decisions needed\n");
    for m in msgs {
        let msg_type = m["type"].as_str().unwrap_or("unknown");
        let task_id = m["taskId"].as_str().unwrap_or("");
        if task_id.is_empty() {
            println!("- {msg_type}");
        } else {
            println!("- {msg_type} (task {task_id})");
        }
    }
    println!("\nTo handle: `{cli} orch check --run-id {run_id} --decisions`");
}

fn exec_run(args: &[String], port: u16) -> ! {
    let opts = parse_exec_args(args).unwrap_or_else(|e| {
        eprintln!("[orch] {e}");
        std::process::exit(2);
    });
    let mut plan = read_plan_inner(std::path::Path::new(&opts.file), false)
        .unwrap_or_else(|e| {
            eprintln!("[orch] {e}");
            std::process::exit(2);
        });
    if let Some(run) = plan.get_mut("run").and_then(serde_json::Value::as_object_mut) {
        run.remove("detach");
        run.insert("detached".into(), serde_json::Value::Bool(true));
    }
    let plan_body = serde_json::to_string(&plan).unwrap_or_default();
    let raw = http_post(port, "/orch/plan-apply", &plan_body, 30_000)
        .unwrap_or_else(|e| {
            eprintln!("[orch] brain unreachable: {e}");
            std::process::exit(4);
        });
    let start: serde_json::Value = serde_json::from_str(parse_response_body(&raw))
        .unwrap_or_default();
    if start["ok"].as_bool() != Some(true) {
        let code = start["error"]["code"].as_str().unwrap_or("error");
        eprintln!("[orch] {}: {}", code, start["error"]["message"].as_str().unwrap_or(""));
        std::process::exit(4);
    }
    let run_id = match start["run"]["id"].as_str() {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => {
            eprintln!("[orch] no run id in plan-apply response");
            std::process::exit(4);
        }
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(opts.timeout_min * 60);
    let cli = crate::panorama_cli_path();
    loop {
        std::thread::sleep(Duration::from_secs(2));
        if std::time::Instant::now() >= deadline {
            if let Some(text) = exec_fetch_report(port, &run_id) {
                print!("{text}");
            }
            eprintln!("[orch] exec timed out after {} min", opts.timeout_min);
            std::process::exit(3);
        }
        let check_body = serde_json::json!({
            "runId": run_id,
            "decisions": true,
            "peek": true,
            "recipient": "coordinator",
        }).to_string();
        let response = match http_post(port, "/orch/check", &check_body, 10_000) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[orch] brain unreachable: {e}");
                std::process::exit(4);
            }
        };
        let v: serde_json::Value = serde_json::from_str(parse_response_body(&response))
            .unwrap_or_default();
        if v["ok"].as_bool() != Some(true) {
            eprintln!("[orch] check: {}", v["error"]["message"].as_str().unwrap_or("unreachable"));
            std::process::exit(4);
        }
        let Some(msgs) = v["messages"].as_array() else { continue };
        if msgs.is_empty() { continue; }
        let tasks = exec_fetch_tasks(port, &run_id);
        let exit_code = exec_exit_code(msgs, &tasks);
        if let Some(text) = exec_fetch_report(port, &run_id) {
            print!("{text}");
        }
        if exit_code != 0 {
            exec_print_decisions(msgs, &run_id, &cli);
        }
        std::process::exit(exit_code);
    }
}

fn validate_required(command: &str, body: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    let required: &[&str] = match command {
        "run-create" => &["repo"],
        "plan-apply" => &["file"],
        "run-show" => &["runId"],
        "run-set-status" => &["runId", "status"],
        "run-update" => &["runId"],
        "run-set-coordinator" => &["runId"],
        "task-create" => &["runId", "title", "spec"],
        "task-update" => &["runId", "taskId"],
        "task-list" => &["runId"],
        "dispatch" => &["runId", "taskId"],
        "send" => &["runId", "type"],
        "check" => &["runId"],
        "gate-create" => &["runId", "taskId", "question"],
        "gate-resolve" => &["runId", "gateId", "resolution"],
        "worker-start" => &["runId", "taskId"],
        "worker-show" => &["runId", "dispatchId"],
        "worker-read" | "worker-transcript" => &["runId", "dispatchId"],
        "worker-send" => &["runId", "dispatchId", "text"],
        "worker-stop" => &["runId", "dispatchId"],
        "worker-release" => &["runId", "dispatchId"],
        "run-list" => &[],
        "integrate" => &["runId", "taskId"],
        "worktree-clean" => &["runId"],
        "verify" => &["runId", "taskId"],
        "retry" => &["runId", "taskId"],
        "conflict-resolve" => &["runId", "taskId"],
        "report" => &["runId"],
        "dispatch-adopt" => &["runId", "dispatchId"],
        "candidate-list" | "metrics" => &[],
        "candidate-clear" => &["agent"],
        "usage-report" => &["agent", "accountId"],
        _ => return Err(format!("unknown command: {command}")),
    };
    for field in required {
        let missing = !body.contains_key(*field)
            || body[*field].is_null()
            || body[*field].as_str().map(|s| s.is_empty()).unwrap_or(false);
        if missing {
            return Err(format!("missing required field: {field}"));
        }
    }
    Ok(())
}

fn parse_flags(args: &[String], command: &str) -> Result<(serde_json::Map<String, serde_json::Value>, bool), String> {
    let mut body: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    let mut arrays: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    let mut pretty = false;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if !arg.starts_with("--") {
            i += 1;
            continue;
        }
        let flag = &arg[2..];

        let allowed = match command {
            "run-create" => "repo title base-ref setup coordinator-tile max-workers max-attempts dispatch-timeout-min verify-timeout-min stall-quiet-min max-verify-rounds autonomy detach corrective-retry worker-summary-cap allow-in-place",
            "plan-apply" => "file",
            "run-show" => "run-id full brief last-status",
            "run-list" => "",
            "run-set-status" => "run-id status",
            "run-update" => "run-id title autonomy max-operational-attempts max-verify-rounds detach corrective-retry worker-summary-cap",
            "candidate-list" | "metrics" => "",
            "candidate-clear" => "agent model account",
            "usage-report" => "agent account-id windows",
            "run-set-coordinator" => "run-id tile-id",
            "task-create" => "run-id title spec spec-file acceptance dep profile escalation verify exclusive role review owner by workspace timeout-min max-cost-usd",
            "task-update" => "run-id task-id status result spec spec-file acceptance verify profile escalation exclusive review owner by reason timeout-min max-cost-usd",
            "task-list" => "run-id status",
            "dispatch" => "run-id task-id profile model retry-of tile-id",
            "send" => "run-id type from to task-id dispatch-id body outcome summary file report report-file reply-to",
            "check" => "run-id recipient type peek wait timeout-ms decisions",
            "gate-create" => "run-id task-id question option",
            "gate-resolve" => "run-id gate-id resolution",
            "worker-start" => "run-id task-id by profile model effort retry-of stop-live stopLive worktree",
            "worker-show" | "worker-stop" | "worker-release" => "run-id dispatch-id",
            "dispatch-adopt" => "run-id dispatch-id summary",
            "worker-read" => "run-id dispatch-id lines all",
            "worker-transcript" => "run-id dispatch-id tail",
            "worker-send" => "run-id dispatch-id text keys",
            "integrate" => "run-id task-id dispatch-id",
            "worktree-clean" => "run-id",
            "verify" => "run-id task-id",
            "conflict-resolve" => "run-id task-id profile model effort",
            "retry" => "run-id task-id by profile model effort",
            "report" => "run-id format",
            _ => return Err(format!("unknown command: {command}")),
        };
        if flag != "pretty" && !allowed.split_whitespace().any(|item| item == flag) {
            return Err(format!("unknown flag --{flag} for {command}"));
        }

        if flag == "pretty" {
            pretty = true;
            i += 1;
            continue;
        }

        if flag == "spec-file" || flag == "report-file" {
            if i + 1 < args.len() {
                let path = &args[i + 1];
                match std::fs::read_to_string(path) {
                    Ok(content) => { body.insert(if flag == "spec-file" { "spec" } else { "report" }.into(), content.into()); }
                    Err(e) => { eprintln!("cannot read {flag}: {e}"); std::process::exit(2); }
                }
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        let field = flag_to_field(flag, command);
        let is_bool = is_bool_flag(flag) || (flag == "detach" && command == "run-create");
        let is_arr = is_array_field(field, command);

        if is_bool {
            body.insert(field.into(), true.into());
            i += 1;
            continue;
        }

        if i + 1 >= args.len() {
            if matches!(flag, "detach" | "corrective-retry") {
                return Err(format!("--{flag} requires on or off"));
            }
            if flag == "worker-summary-cap" {
                return Err("--worker-summary-cap requires a positive integer".into());
            }
            i += 1;
            continue;
        }
        let raw = &args[i + 1];

        if field == "body" && raw == "-" {
            let mut stdin_content = String::new();
            if std::io::stdin().read_to_string(&mut stdin_content).is_ok() {
                body.insert("body".into(), stdin_content.into());
            }
            i += 2;
            continue;
        }

        let json_val = if matches!(flag, "detach" | "corrective-retry") {
            match raw.as_str() {
                "on" => true.into(),
                "off" => false.into(),
                _ => return Err(format!("--{flag} requires on or off")),
            }
        } else {
            coerce_value(field, raw)
        };
        if is_arr {
            arrays.entry(field.into()).or_default().push(json_val);
        } else {
            body.insert(field.into(), json_val);
        }
        i += 2;
    }

    for (key, vals) in arrays {
        body.insert(key, serde_json::Value::Array(vals));
    }
    Ok((body, pretty))
}

fn flag_to_field<'a>(flag: &'a str, command: &str) -> &'a str {
    match flag {
        "dep" => "deps",
        "file" if command == "plan-apply" => "file",
        "file" => "filesModified",
        "option" => "options",
        "type" if command == "check" => "types",
        "type" => "type",
        "run-id" => "runId",
        "task-id" => "taskId",
        "gate-id" => "gateId",
        "dispatch-id" => "dispatchId",
        "retry-of" => "retryOf",
        "tile-id" => "tileId",
        "base-ref" => "baseRef",
        "coordinator-tile" => "coordinatorTile",
        "detach" => "detached",
        "timeout-ms" => "timeoutMs",
        "max-workers" => "maxWorkers",
        "max-attempts" => "maxAttempts",
        "dispatch-timeout-min" => "dispatchTimeoutMin",
        "verify-timeout-min" => "verifyTimeoutMin",
        "stall-quiet-min" => "stallQuietMin",
        "reply-to" => "replyTo",
        other => camel_cache(other),
    }
}

fn camel_cache(flag: &str) -> &'static str {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    if let Some(v) = guard.get(flag) {
        return v;
    }
    let camel = kebab_to_camel(flag);
    let leaked: &'static str = Box::leak(camel.into_boxed_str());
    guard.insert(flag.to_string(), leaked);
    leaked
}

fn kebab_to_camel(s: &str) -> String {
    let mut result = String::new();
    let mut upper = false;
    for c in s.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            result.extend(c.to_uppercase());
            upper = false;
        } else {
            result.push(c);
        }
    }
    result
}

fn is_bool_flag(flag: &str) -> bool {
    matches!(flag, "all" | "full" | "review" | "peek" | "wait" | "decisions" | "pretty" | "keys" | "brief" | "stopLive" | "stop-live" | "allow-in-place")
}

fn is_array_field(field: &str, command: &str) -> bool {
    matches!(field, "deps" | "filesModified" | "verify" | "escalation" | "exclusive" | "options")
        || (field == "types" && command == "check")
}

fn coerce_value(field: &str, raw: &str) -> serde_json::Value {
    if is_numeric_field(field) {
        if let Ok(n) = raw.parse::<u64>() {
            return serde_json::Value::Number(n.into());
        }
    }
    serde_json::Value::String(raw.to_string())
}

fn is_numeric_field(field: &str) -> bool {
    matches!(field, "timeoutMs" | "maxWorkers" | "maxAttempts" | "dispatchTimeoutMin" | "verifyTimeoutMin" | "stallQuietMin" | "maxOperationalAttempts" | "workerSummaryCap" | "replyTo" | "lines" | "tail")
}

fn http_post(port: u16, path: &str, body: &str, read_timeout_ms: u64) -> Result<String, String> {
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}")
        .parse()
        .map_err(|e: std::net::AddrParseError| e.to_string())?;
    let mut stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(Duration::from_millis(read_timeout_ms)))
        .map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    Ok(read_http_response(&mut stream)?)
}

fn read_http_response(stream: &mut std::net::TcpStream) -> Result<String, String> {
    use std::io::Read;
    let mut buf = Vec::new();
    match stream.read_to_end(&mut buf) {
        Ok(_) => {}
        Err(e) => {
            // Windows WSAECONNRESET (10054): server closed with RST after sending data.
            // If we already have data, treat it as EOF rather than an error.
            if buf.is_empty() {
                return Err(e.to_string());
            }
        }
    }
    String::from_utf8(buf).map_err(|e| e.to_string())
}

fn parse_response_body(response: &str) -> &str {
    response.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str], command: &str) -> serde_json::Map<String, serde_json::Value> {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse_flags(&owned, command).unwrap().0
    }

    #[test]
    fn plan_apply_reads_specs_relative_to_the_plan_file() {
        let dir = std::env::temp_dir().join(format!("plan-cli-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("specs")).unwrap();
        let path = dir.join("plan.json");
        std::fs::write(dir.join("specs").join("task.md"), "task content").unwrap();
        std::fs::write(&path, r#"{
            "run":{"coordinatorTile":"explicit"},
            "tasks":[{"key":"first","specFile":"specs/task.md"},{"spec":"inline"}]
        }"#).unwrap();
        let args = parse(&["--file", path.to_str().unwrap()], "plan-apply");
        assert!(validate_required("plan-apply", &args).is_ok());
        assert!(validate_required("plan-apply", &Default::default()).is_err());
        let plan = read_plan(&path).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(plan["run"]["coordinatorTile"], "explicit");
        assert_eq!(plan["tasks"][0]["spec"], "task content");
        assert!(plan["tasks"][0].get("specFile").is_none());
        assert_eq!(plan["tasks"][1]["spec"], "inline");
    }

    #[test]
    fn run_commands_accept_corrective_retry_on_and_off() {
        for command in ["run-create", "run-update"] {
            assert_eq!(parse(&["--corrective-retry", "on"], command)["correctiveRetry"], true);
            assert_eq!(parse(&["--corrective-retry", "off"], command)["correctiveRetry"], false);
            assert!(parse_flags(&["--corrective-retry".into()], command).is_err());
        }
        let body = parse(&["--corrective-retry", "off"], "run-create");
        assert_eq!(crate::orch::run_create_fields(&body.into()).unwrap()["correctiveRetry"], false);
    }

    #[test]
    fn run_commands_parse_worker_summary_cap() {
        for command in ["run-create", "run-update"] {
            assert_eq!(parse(&["--worker-summary-cap", "42"], command)["workerSummaryCap"], 42);
            assert!(parse_flags(&["--worker-summary-cap".into()], command).is_err());
        }
    }

    #[test]
    fn documented_flags_parse() {
        let commands = [
            ("run-create", "repo title base-ref setup max-workers max-attempts dispatch-timeout-min verify-timeout-min stall-quiet-min"),
            ("task-create", "run-id title spec spec-file acceptance verify dep role escalation profile review"),
            ("task-update", "run-id task-id spec acceptance verify escalation profile status result owner reason"),
            ("worker-start", "run-id task-id profile model stop-live"),
            ("check", "run-id wait timeout-ms peek type"),
            ("worker-show", "run-id dispatch-id"),
            ("worker-read", "run-id dispatch-id lines all"),
            ("worker-transcript", "run-id dispatch-id tail"),
            ("worker-send", "run-id dispatch-id text keys"),
            ("worker-stop", "run-id dispatch-id"),
            ("worker-release", "run-id dispatch-id"),
            ("dispatch-adopt", "run-id dispatch-id summary"),
            ("retry", "run-id task-id profile model"),
            ("verify", "run-id task-id"),
            ("conflict-resolve", "run-id task-id profile model"),
            ("integrate", "run-id task-id dispatch-id"),
            ("send", "type to body report report-file"),
            ("report", "run-id format"),
            ("worktree-clean", "run-id"),
            ("run-set-status", "run-id status"),
            ("run-show", "run-id brief"),
            ("task-list", "run-id"),
            ("gate-create", "run-id task-id question option"),
            ("gate-resolve", "run-id gate-id resolution"),
            ("run-update", "run-id title"),
        ];
        let file = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/claude-mod/skills/orchestrate/SKILL.md");
        for (command, flags) in commands {
            for flag in flags.split_whitespace() {
                let value = if matches!(flag, "spec-file" | "report-file") { file } else { "1" };
                let args = [format!("--{flag}"), value.to_string()];
                let (body, _) = parse_flags(&args, command).unwrap_or_else(|e| panic!("{command}: {e}"));
                let field = match flag { "spec-file" => "spec", "report-file" => "report", _ => flag_to_field(flag, command) };
                assert!(body.contains_key(field), "{command} --{flag}");
            }
            assert!(parse_flags(&["--pretty".into()], command).unwrap().1);
        }
        let body = parse(&["--profile", "codex", "--model", "gpt-6-luna"], "conflict-resolve");
        assert_eq!(body["profile"], "codex");
        assert_eq!(body["model"], "gpt-6-luna");
    }

    #[test]
    fn owner_and_unknown_flags() {
        let args = vec!["--owner".into(), "coordinator".into(), "--reason".into(), "asked".into()];
        let body = parse_flags(&args, "task-update").unwrap().0;
        assert_eq!(body["owner"], "coordinator");
        assert_eq!(body["reason"], "asked");
        assert!(parse_flags(&["--owner".into(), "user".into()], "check").unwrap_err().contains("--owner"));
        assert!(parse_flags(&["--typo".into()], "task-update").unwrap_err().contains("--typo"));
    }

    #[test]
    fn report_flags_read_inline_and_file() {
        let path = std::env::temp_dir().join(format!("orch-report-{}.md", std::process::id()));
        std::fs::write(&path, "full report").unwrap();
        assert_eq!(parse(&["--report", "inline"], "send")["report"], "inline");
        assert_eq!(parse(&["--report-file", path.to_str().unwrap()], "send")["report"], "full report");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn run_update_autonomy_and_max_operational_flags() {
        let body = parse(&["--autonomy", "operational,execution", "--max-operational-attempts", "8"], "run-update");
        assert_eq!(body["autonomy"].as_str(), Some("operational,execution"));
        assert_eq!(body["maxOperationalAttempts"].as_u64(), Some(8));
        let mut with_run = body.clone();
        with_run.insert("runId".into(), "r1".into());
        assert!(validate_required("run-update", &with_run).is_ok());
    }

    #[test]
    fn run_create_detach_enables_autonomy_unless_explicit() {
        use super::super::{run_create_fields, ApplyResult, Cmd, Limits, State};

        for (flags, expected) in [
            (&["--detach"][..], [true, true, true]),
            (&["--detach", "--autonomy", "execution"][..], [false, true, false]),
            (&["--detach", "--autonomy", ""][..], [false, false, false]),
        ] {
            let mut state = State::default();
            let fields = run_create_fields(&parse(flags, "run-create").into()).unwrap();
            let cmd = Cmd::RunCreate {
                id: "r1".into(), title: "run".into(), repo: ".".into(), base_ref: None,
                base_sha: None, setup: None, coordinator_tile: None, limits: Limits::default(), now: 1,
            }.with_fields(fields);
            let ApplyResult::Ok(kind, data) = state.apply(&cmd) else { panic!("run-create failed") };
            state.replay(&kind, &data).unwrap();
            let run = &state.runs["r1"].run;
            assert!(run.detached);
            let a = run.autonomy;
            assert_eq!([a.operational, a.execution, a.integration], expected);
        }
        assert_eq!(parse(&["--detach", "on"], "run-update")["detached"], true);
        assert_eq!(parse(&["--detach", "off"], "run-update")["detached"], false);
        assert!(parse_flags(&["--detach".into()], "run-update").is_err());
        assert!(parse_flags(&["--detach".into(), "invalid".into()], "run-update").is_err());
    }

    #[test]
    fn check_decisions_parses_with_wait_and_type() {
        let body = parse(&["--run-id", "r1", "--decisions", "--wait", "--type", "question"], "check");
        assert_eq!(body["runId"], "r1");
        assert_eq!(body["decisions"], true);
        assert_eq!(body["wait"], true);
        assert_eq!(body["types"], serde_json::json!(["question"]));
    }

    #[test]
    fn candidate_clear_and_usage_report_flags() {
        let body = parse(&["--agent", "claude", "--model", "opus", "--account", "a1"], "candidate-clear");
        assert_eq!(body["agent"].as_str(), Some("claude"));
        assert_eq!(body["model"].as_str(), Some("opus"));
        assert_eq!(body["account"].as_str(), Some("a1"));
        assert!(validate_required("candidate-clear", &body).is_ok());
        assert!(validate_required("candidate-clear", &serde_json::Map::new()).is_err());
        let report = parse(&["--agent", "codex", "--account-id", "a", "--windows", "[]"], "usage-report");
        assert_eq!(report["accountId"].as_str(), Some("a"));
        assert!(validate_required("usage-report", &report).is_ok());
        assert!(validate_required("candidate-list", &serde_json::Map::new()).is_ok());
        assert!(validate_required("metrics", &serde_json::Map::new()).is_ok());
    }

    #[test]
    fn type_flag_send_maps_to_type_string() {
        let body = parse(&["--type", "worker_done"], "send");
        assert_eq!(body["type"].as_str(), Some("worker_done"));
        assert!(!body.contains_key("types"));
    }

    #[test]
    fn type_flag_check_maps_to_types_array() {
        let body = parse(&["--type", "progress", "--type", "worker_done"], "check");
        assert!(!body.contains_key("type"));
        let types = body["types"].as_array().unwrap();
        assert_eq!(types.len(), 2);
        assert_eq!(types[0].as_str(), Some("progress"));
        assert_eq!(types[1].as_str(), Some("worker_done"));
    }

    #[test]
    fn repeated_dep_flags_become_array() {
        let body = parse(&["--dep", "t1", "--dep", "t2"], "task-create");
        let deps = body["deps"].as_array().unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].as_str(), Some("t1"));
        assert_eq!(deps[1].as_str(), Some("t2"));
    }

    #[test]
    fn flag_value_starting_with_dashes_is_treated_as_value() {
        let body = parse(&["--summary", "--fixed-bug"], "send");
        assert_eq!(body["summary"].as_str(), Some("--fixed-bug"));
    }

    #[test]
    fn bool_flags_set_true_without_consuming_next_arg() {
        let body = parse(&["--wait", "--peek"], "check");
        assert_eq!(body["wait"].as_bool(), Some(true));
        assert_eq!(body["peek"].as_bool(), Some(true));
    }

    #[test]
    fn kebab_flag_converts_to_camel() {
        let body = parse(&["--run-id", "r1", "--task-id", "t1"], "task-update");
        assert_eq!(body["runId"].as_str(), Some("r1"));
        assert_eq!(body["taskId"].as_str(), Some("t1"));
    }

    #[test]
    fn timeout_ms_is_numeric() {
        let body = parse(&["--timeout-ms", "60000"], "check");
        assert_eq!(body["timeoutMs"].as_u64(), Some(60000));
    }

    #[test]
    fn validate_required_send_fails_without_type() {
        let mut body = serde_json::Map::new();
        body.insert("runId".into(), "r1".into());
        assert!(validate_required("send", &body).is_err());
    }

    #[test]
    fn validate_required_send_passes_with_type() {
        let mut body = serde_json::Map::new();
        body.insert("runId".into(), "r1".into());
        body.insert("type".into(), "worker_done".into());
        assert!(validate_required("send", &body).is_ok());
    }

    #[test]
    fn validate_required_unknown_command_is_error() {
        let body = serde_json::Map::new();
        let err = validate_required("unknown-cmd", &body).unwrap_err();
        assert!(err.contains("unknown command"));
    }

    #[test]
    fn validate_required_run_create_requires_repo() {
        let body = serde_json::Map::new();
        assert!(validate_required("run-create", &body).is_err());
        let mut body2 = serde_json::Map::new();
        body2.insert("repo".into(), "/tmp/r".into());
        assert!(validate_required("run-create", &body2).is_ok());
    }

    #[test]
    fn read_timeout_uses_saturating_add() {
        let timeout_ms: u64 = 600_000;
        let result = timeout_ms.min(600_000).saturating_add(5_000);
        assert_eq!(result, 605_000);

        let large: u64 = u64::MAX;
        let capped = large.min(600_000).saturating_add(5_000);
        assert_eq!(capped, 605_000);
    }

    #[test]
    fn exec_flags_parse() {
        let opts = parse_exec_args(&["--file".to_string(), "plan.json".to_string()]).unwrap();
        assert_eq!(opts.file, "plan.json");
        assert_eq!(opts.timeout_min, 240);

        let opts = parse_exec_args(&[
            "--file".to_string(), "p.json".to_string(),
            "--timeout-min".to_string(), "60".to_string(),
        ]).unwrap();
        assert_eq!(opts.file, "p.json");
        assert_eq!(opts.timeout_min, 60);

        assert!(parse_exec_args(&[]).is_err());
        assert!(parse_exec_args(&["--file".to_string()]).is_err());
        assert!(parse_exec_args(&["--unknown".to_string()]).is_err());
        assert!(parse_exec_args(&["--timeout-min".to_string()]).is_err());
    }

    #[test]
    fn exec_plan_no_coordinator_tile() {
        let dir = std::env::temp_dir().join(format!("exec-no-coord-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plan.json");
        std::fs::write(&path, r#"{"run":{"repo":"/tmp"},"tasks":[]}"#).unwrap();
        std::env::set_var("PANORAMA_TILE_ID", "tile-exec-test");
        let plan_no = read_plan_inner(&path, false).unwrap();
        let plan_yes = read_plan_inner(&path, true).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        std::env::remove_var("PANORAMA_TILE_ID");
        assert!(
            plan_no.get("run")
                .and_then(|r| r.as_object())
                .and_then(|r| r.get("coordinatorTile"))
                .is_none(),
            "exec must not set coordinatorTile"
        );
        assert_eq!(
            plan_yes.get("run")
                .and_then(|r| r.as_object())
                .and_then(|r| r.get("coordinatorTile"))
                .and_then(|v| v.as_str()),
            Some("tile-exec-test"),
            "read_plan (fill=true) must set coordinatorTile from env"
        );
    }

    #[test]
    fn exec_exit_code_mapping() {
        let finished = serde_json::json!([{"type": "run_finished"}]);
        let idle = serde_json::json!([{"type": "run_idle"}]);
        let gate = serde_json::json!([{"type": "gate"}]);
        let tasks_ok = serde_json::json!([{"status": "completed"}]);
        let tasks_fail = serde_json::json!([{"status": "failed"}]);

        assert_eq!(exec_exit_code(finished.as_array().unwrap(), &tasks_ok), 0);
        assert_eq!(exec_exit_code(finished.as_array().unwrap(), &tasks_fail), 1,
            "run_finished with a failed task is exit 1");
        assert_eq!(exec_exit_code(idle.as_array().unwrap(), &tasks_fail), 1,
            "run_idle only + failed tasks = exit 1");
        assert_eq!(exec_exit_code(idle.as_array().unwrap(), &tasks_ok), 2,
            "run_idle only + no failed tasks = exit 2");
        assert_eq!(exec_exit_code(gate.as_array().unwrap(), &tasks_fail), 2,
            "non-idle decision takes priority over failed task");
        assert_eq!(exec_exit_code(gate.as_array().unwrap(), &tasks_ok), 2);
    }
}
