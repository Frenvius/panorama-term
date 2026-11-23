use std::collections::{HashMap, VecDeque};

use serde_json::{json, Value};

use super::{
    commit, created_run, err_from_str, err_resp, gen_run_id, ok_resp, parse_body, parse_run_bool,
    run_create_cmd, strict_str_arr, task_create_cmd, worker_start_inner, Cmd, CommitResult,
};

pub(super) fn task_label(index: usize, task: &Value) -> String {
    let id = format!("t{}", index + 1);
    match task["key"].as_str().or_else(|| task["title"].as_str()) {
        Some(name) => format!("task {id} ({name})"),
        None => format!("task {id}"),
    }
}

fn task_error(index: usize, task: &Value, error: String) -> String {
    let response: Value = serde_json::from_str(&error).unwrap_or_default();
    let code = response["error"]["code"].as_str().unwrap_or("bad_request");
    let message = response["error"]["message"].as_str().unwrap_or(&error);
    err_resp(code, &format!("{}: {message}", task_label(index, task)))
}

struct Tasks {
    commands: Vec<Cmd>,
    order: Vec<usize>,
    roots: Vec<usize>,
    summaries: Vec<Value>,
}

fn prepare_tasks(run_id: &str, tasks: &[Value]) -> Result<Tasks, String> {
    let ids: Vec<_> = (1..=tasks.len()).map(|n| format!("t{n}")).collect();
    let mut references: HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    for (i, task) in tasks.iter().enumerate() {
        let Some(key) = task.get("key") else { continue };
        let Some(key) = key.as_str().filter(|s| !s.trim().is_empty()) else {
            return Err(err_resp(
                "bad_request",
                &format!("{}: key must be a non-empty string", task_label(i, task)),
            ));
        };
        if references
            .insert(key, i)
            .is_some_and(|previous| previous != i)
        {
            return Err(err_resp(
                "bad_request",
                &format!("{}: duplicate key or task id {key}", task_label(i, task)),
            ));
        }
    }

    let mut commands = Vec::with_capacity(tasks.len());
    let mut summaries = Vec::with_capacity(tasks.len());
    let mut remaining_deps = Vec::with_capacity(tasks.len());
    let mut dependents = vec![Vec::new(); tasks.len()];
    let mut roots = Vec::new();
    for (i, task) in tasks.iter().enumerate() {
        let context = |e| task_error(i, task, e);
        for field in ["title", "spec"] {
            if !task[field].as_str().is_some_and(|s| !s.trim().is_empty()) {
                return Err(context(err_resp(
                    "bad_request",
                    &format!("{field} required"),
                )));
            }
        }
        let deps = strict_str_arr(task, "deps").map_err(context)?;
        remaining_deps.push(deps.len());
        if deps.is_empty() {
            roots.push(i);
        }
        let deps = deps
            .iter()
            .map(|dep| {
                let &index = references.get(dep.as_str()).ok_or_else(|| {
                    context(err_resp("bad_request", &format!("unknown dep: {dep}")))
                })?;
                dependents[index].push(i);
                Ok(ids[index].clone())
            })
            .collect::<Result<Vec<_>, String>>()?;
        let make_cmd = task_create_cmd(task, run_id.to_owned(), deps).map_err(context)?;
        commands.push(make_cmd(ids[i].clone()));
        summaries.push(json!({"key": task["key"], "id": ids[i], "title": task["title"]}));
    }

    let mut order = Vec::with_capacity(tasks.len());
    let mut ready = VecDeque::from(roots.clone());
    while let Some(i) = ready.pop_front() {
        order.push(i);
        for &dependent in &dependents[i] {
            remaining_deps[dependent] -= 1;
            if remaining_deps[dependent] == 0 {
                ready.push_back(dependent);
            }
        }
    }
    if let Some(i) = remaining_deps.iter().position(|&count| count != 0) {
        return Err(err_resp(
            "bad_request",
            &format!("{}: dependency cycle", task_label(i, &tasks[i])),
        ));
    }
    Ok(Tasks {
        commands,
        order,
        roots,
        summaries,
    })
}

pub(super) async fn handle(body: &[u8]) -> String {
    let body = match parse_body(body) {
        Ok(body) => body,
        Err(e) => return e,
    };
    apply(body, gen_run_id()).await.unwrap_or_else(|e| e)
}

async fn apply(mut body: Value, run_id: String) -> Result<String, String> {
    let start = parse_run_bool(&body, "start")?.unwrap_or(true);
    let run = body
        .get_mut("run")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| err_resp("bad_request", "run must be an object"))?;
    if let Some(detach) = run.remove("detach") {
        run.insert("detached".into(), detach);
    }
    run.entry("detached").or_insert(Value::Bool(true));

    let tasks = body["tasks"]
        .as_array()
        .ok_or_else(|| err_resp("bad_request", "tasks must be an array"))?;
    let Tasks {
        commands,
        order,
        roots,
        summaries,
    } = prepare_tasks(&run_id, tasks)?;
    let run = run_create_cmd(&body["run"], run_id.clone()).await?;
    let cmd = Cmd::PlanApply {
        run: Box::new(run),
        tasks: commands,
        order,
    };
    match commit(move |_| Ok(cmd), run_id.clone()).await {
        CommitResult::Ok(_) => {}
        CommitResult::Stale => return Err(err_resp("stale", "plan apply stale")),
        CommitResult::Err(e) => return Err(err_from_str(&e)),
    }
    let run = created_run(&run_id).await;
    let mut started = Vec::new();
    if start {
        let max_workers = run["limits"]["maxWorkers"].as_u64().unwrap_or_default() as usize;
        for i in roots {
            if started.len() >= max_workers {
                break;
            }
            let task_id = &summaries[i]["id"];
            let response =
                worker_start_inner(json!({"runId": run_id, "taskId": task_id}), None).await;
            let response: Value = serde_json::from_str(&response).unwrap_or_default();
            if let Some(id) = response["dispatch"]["id"].as_str() {
                started.push(id.to_owned());
            } else if response["ok"] != true {
                eprintln!(
                    "[orch] plan-apply {run_id}/{task_id}: {}",
                    response["error"]
                );
            }
        }
    }
    Ok(ok_resp(
        json!({"run": run, "tasks": summaries, "started": started}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orch::{load_into, run_events_path, state, tests::f12_test_home, State};

    fn plan(tasks: Value) -> Value {
        f12_test_home();
        json!({
            "run": {"repo": std::env::current_dir().unwrap()},
            "tasks": tasks,
            "start": false,
        })
    }

    #[tokio::test]
    async fn plan_apply_commits_in_plan_order_and_replays_forward_dependencies() {
        let mut request = plan(json!([
            {"key":"dependent","title":"dependent","spec":"do it","deps":["root","t3"],"profile":"codex"},
            {"key":"root","title":"root","spec":"do it","profile":"codex"},
            {"title":"other","spec":"do it","profile":"codex"},
        ]));
        request["run"]["coordinatorTile"] = json!("plan-test-tile");
        let response = crate::orch::handle("plan-apply", request.to_string().as_bytes()).await;
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(
            response["tasks"],
            json!([
                {"key":"dependent","id":"t1","title":"dependent"},
                {"key":"root","id":"t2","title":"root"},
                {"key":null,"id":"t3","title":"other"},
            ])
        );
        assert_eq!(response["started"], json!([]));
        assert_eq!(response["run"]["detached"], true);
        assert_eq!(response["run"]["coordinatorTile"], "plan-test-tile");
        assert_eq!(
            response["run"]["autonomy"],
            json!({
                "operational": true, "execution": true, "integration": true,
            })
        );
        let run_id = response["run"]["id"].as_str().unwrap();
        let base = crate::panorama_dir().unwrap();
        let mut replayed = State::default();
        load_into(&mut replayed, &run_events_path(&base, run_id), run_id);
        let live = state().lock().unwrap_or_else(|e| e.into_inner());
        for entry in [&live.runs[run_id], &replayed.runs[run_id]] {
            assert_eq!(entry.tasks["t1"].deps, ["t2", "t3"]);
            assert_eq!(entry.next_task_id(), "t4");
            assert!(entry.dispatches.is_empty());
        }
    }

    #[tokio::test]
    async fn plan_apply_honors_explicit_detach_and_autonomy() {
        for (fields, detached, operational) in [
            (json!({"detach": false}), false, false),
            (json!({"autonomy": "operational"}), true, true),
        ] {
            let mut request = plan(json!([]));
            request["run"]
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            let response = apply(request, gen_run_id()).await.unwrap();
            let response: Value = serde_json::from_str(&response).unwrap();
            assert_eq!(response["run"]["detached"], detached);
            let autonomy = &response["run"]["autonomy"];
            assert_eq!(
                autonomy["operational"].as_bool().unwrap_or(false),
                operational
            );
            assert_eq!(autonomy["execution"].as_bool().unwrap_or(false), false);
            assert_eq!(autonomy["integration"].as_bool().unwrap_or(false), false);
        }
    }

    #[tokio::test]
    async fn plan_apply_rejects_invalid_tasks_without_events() {
        for (fields, message) in [
            (json!({"deps":["t2"]}), "dependency cycle"),
            (json!({"deps":["unknown"]}), "unknown dep"),
            (json!({"role":"unknown"}), "role must be one of"),
            (json!({"title":null}), "title required"),
            (json!({"spec":null}), "spec required"),
        ] {
            let mut request = plan(json!([
                {"key":"anchor","title":"anchor","spec":"do it","profile":"codex"},
                {"key":"invalid","title":"invalid","spec":"do it","profile":"codex"},
            ]));
            request["tasks"][1]
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            let run_id = gen_run_id();
            let response = apply(request, run_id.clone()).await.unwrap_err();
            assert!(response.contains("task t2 (invalid)"), "{response}");
            assert!(response.contains(message), "{response}");
            let base = crate::panorama_dir().unwrap();
            assert!(!run_events_path(&base, &run_id).exists());
            assert!(!state()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .runs
                .contains_key(&run_id));
        }
    }

    #[tokio::test]
    async fn plan_apply_starts_only_roots_up_to_max_workers_by_default() {
        let mut request = plan(json!([
            {"title":"dependent","spec":"do it","deps":["t2"],"profile":"codex"},
            {"title":"first root","spec":"do it","profile":"codex"},
            {"title":"second root","spec":"do it","profile":"codex"},
        ]));
        request.as_object_mut().unwrap().remove("start");
        request["run"]["limits"] = json!({"maxWorkers": 1});
        let run_id = gen_run_id();
        let response = apply(request, run_id.clone()).await.unwrap();
        let response: Value = serde_json::from_str(&response).unwrap();
        let dispatch_id = format!("{run_id}.t2.1");
        assert_eq!(response["started"], json!([dispatch_id]));
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = &guard.runs[&run_id];
        assert_eq!(entry.dispatches.len(), 1);
        assert_eq!(entry.dispatches[&dispatch_id].task_id, "t2");
    }
}
