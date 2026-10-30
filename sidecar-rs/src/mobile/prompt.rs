use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use serde_json::{json, Map, Value};
use tokio::sync::Notify;

use super::{hex, random_bytes};
use crate::chat;

const PLAN_CAP: usize = 20000;
const FEEDBACK_CAP: usize = 4000;
const WAIT_SLICE: Duration = Duration::from_secs(25);
const PLAN_MODES: &[&str] = &["auto", "acceptEdits", "default"];
const DENY_MESSAGE: &str = "The user denied this from Panorama mobile.";

struct Pending {
    id: String,
    view: Value,
    input: Value,
    suggestions: Value,
}

#[derive(Default)]
struct Book {
    pending: HashMap<String, Pending>,
    answers: HashMap<String, (String, Value)>,
}

pub enum Wait {
    Answer(Value),
    Pending,
    Gone,
}

fn book() -> MutexGuard<'static, Book> {
    static BOOK: OnceLock<Mutex<Book>> = OnceLock::new();
    BOOK.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn answered() -> &'static Notify {
    static NOTIFY: OnceLock<Notify> = OnceLock::new();
    NOTIFY.get_or_init(Notify::new)
}

fn questions(input: &Value) -> Value {
    let list = input["questions"].as_array().cloned().unwrap_or_default();
    list.iter()
        .map(|q| {
            let options: Vec<Value> = q["options"]
                .as_array()
                .map(|opts| {
                    opts.iter()
                        .map(|o| json!({ "label": o["label"], "description": o["description"] }))
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "question": q["question"],
                "header": q["header"],
                "multiSelect": q["multiSelect"].as_bool().unwrap_or(false),
                "options": options,
            })
        })
        .collect()
}

fn rule_label(rule: &Value) -> Option<String> {
    let tool = rule["toolName"].as_str()?;
    Some(match rule["ruleContent"].as_str() {
        Some(content) => format!("{tool}({content})"),
        None => tool.to_string(),
    })
}

fn suggestion_label(s: &Value) -> Option<String> {
    match s["type"].as_str()? {
        "addRules" | "replaceRules" if s["behavior"] == "allow" => {
            let rules: Vec<String> = s["rules"].as_array()?.iter().filter_map(rule_label).collect();
            (!rules.is_empty()).then(|| rules.join(", "))
        }
        "setMode" => Some(format!("{} mode", s["mode"].as_str()?)),
        "addDirectories" => {
            let dirs: Vec<&str> = s["directories"].as_array()?.iter().filter_map(Value::as_str).collect();
            (!dirs.is_empty()).then(|| dirs.join(", "))
        }
        _ => None,
    }
}

fn always_label(suggestions: &Value) -> Value {
    let labels: Vec<String> = suggestions
        .as_array()
        .map(|list| list.iter().filter_map(suggestion_label).collect())
        .unwrap_or_default();
    if labels.is_empty() {
        Value::Null
    } else {
        Value::from(labels.join("; "))
    }
}

fn view(id: &str, tool: &str, input: &Value, suggestions: &Value) -> Value {
    match tool {
        "AskUserQuestion" => json!({ "id": id, "kind": "question", "tool": tool, "questions": questions(input) }),
        "ExitPlanMode" => {
            let plan = chat::clip(input["plan"].as_str().unwrap_or_default().trim(), PLAN_CAP);
            json!({ "id": id, "kind": "plan", "tool": tool, "plan": plan })
        }
        _ => json!({
            "id": id,
            "kind": "tool",
            "tool": tool,
            "summary": chat::tool_summary(input),
            "always": always_label(suggestions),
        }),
    }
}

pub fn open(tile_id: &str, evt: &Value) -> Value {
    let id = hex(&random_bytes::<8>());
    let tool = evt["tool_name"].as_str().unwrap_or("a tool");
    let input = evt["tool_input"].clone();
    let suggestions = evt["permission_suggestions"].clone();
    let view = view(&id, tool, &input, &suggestions);
    let mut b = book();
    b.answers.retain(|_, (tile, _)| tile != tile_id);
    b.pending.insert(tile_id.to_string(), Pending { id, view: view.clone(), input, suggestions });
    drop(b);
    answered().notify_waiters();
    view
}

pub fn current(tile_id: &str) -> Value {
    book().pending.get(tile_id).map_or(Value::Null, |p| p.view.clone())
}

pub fn clear(tile_id: &str) {
    if book().pending.remove(tile_id).is_some() {
        answered().notify_waiters();
    }
}

fn tool_decision(p: &Pending, answer: &Value) -> Result<Value, String> {
    match answer["action"].as_str() {
        Some("allow") => Ok(json!({ "behavior": "allow" })),
        Some("always") if p.view["always"].is_string() => {
            Ok(json!({ "behavior": "allow", "updatedPermissions": p.suggestions }))
        }
        Some("always") => Err("this request has no rule to remember".into()),
        Some("deny") => Ok(json!({ "behavior": "deny", "message": DENY_MESSAGE, "interrupt": true })),
        _ => Err("unknown action".into()),
    }
}

fn question_decision(p: &Pending, answer: &Value) -> Result<Value, String> {
    let mut answers = Map::new();
    for q in p.view["questions"].as_array().into_iter().flatten() {
        let question = q["question"].as_str().unwrap_or_default();
        let reply = answer["answers"][question].as_str().map(str::trim).unwrap_or_default();
        if reply.is_empty() {
            return Err("answer every question".into());
        }
        answers.insert(question.to_string(), Value::from(chat::clip(reply, FEEDBACK_CAP)));
    }
    let mut input = p.input.clone();
    if let Some(obj) = input.as_object_mut() {
        obj.insert("answers".into(), Value::Object(answers));
    }
    Ok(json!({ "behavior": "allow", "updatedInput": input }))
}

fn plan_decision(p: &Pending, answer: &Value) -> Result<Value, String> {
    match answer["action"].as_str() {
        Some("approve") => {
            let mode = answer["mode"].as_str().filter(|m| PLAN_MODES.contains(m)).ok_or("unknown mode")?;
            Ok(json!({
                "behavior": "allow",
                "updatedInput": p.input,
                "updatedPermissions": [{ "type": "setMode", "mode": mode, "destination": "session" }],
            }))
        }
        Some("revise") => {
            let feedback = answer["feedback"].as_str().map(str::trim).unwrap_or_default();
            if feedback.is_empty() {
                return Err("say what to change".into());
            }
            Ok(json!({ "behavior": "deny", "message": chat::clip(feedback, FEEDBACK_CAP) }))
        }
        _ => Err("unknown action".into()),
    }
}

pub fn answer(tile_id: &str, prompt_id: &str, answer: &Value) -> Result<(), String> {
    let mut b = book();
    let p = b
        .pending
        .get(tile_id)
        .filter(|p| p.id == prompt_id)
        .ok_or("already answered on the computer")?;
    let decision = match p.view["kind"].as_str() {
        Some("question") => question_decision(p, answer)?,
        Some("plan") => plan_decision(p, answer)?,
        _ => tool_decision(p, answer)?,
    };
    b.pending.remove(tile_id);
    b.answers.insert(prompt_id.to_string(), (tile_id.to_string(), decision));
    drop(b);
    answered().notify_waiters();
    Ok(())
}

fn poll(tile_id: &str, prompt_id: &str) -> Wait {
    let mut b = book();
    if let Some((_, decision)) = b.answers.remove(prompt_id) {
        return Wait::Answer(decision);
    }
    if b.pending.get(tile_id).is_some_and(|p| p.id == prompt_id) {
        Wait::Pending
    } else {
        Wait::Gone
    }
}

pub async fn wait(tile_id: &str, prompt_id: &str) -> Wait {
    let deadline = tokio::time::Instant::now() + WAIT_SLICE;
    loop {
        let notified = answered().notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        match poll(tile_id, prompt_id) {
            Wait::Pending => {}
            done => return done,
        }
        if tokio::time::timeout_at(deadline, notified).await.is_err() {
            return Wait::Pending;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evt(tool: &str, input: Value, suggestions: Value) -> Value {
        json!({ "tool_name": tool, "tool_input": input, "permission_suggestions": suggestions })
    }

    #[test]
    fn tool_prompt_offers_always_only_with_a_rule() {
        let rule = json!([{ "type": "addRules", "behavior": "allow", "destination": "localSettings",
            "rules": [{ "toolName": "Bash", "ruleContent": "npm test" }] }]);
        let with = open("t-always", &evt("Bash", json!({ "command": "npm test" }), rule.clone()));
        assert_eq!(with["always"], "Bash(npm test)");
        let id = with["id"].as_str().unwrap().to_string();
        answer("t-always", &id, &json!({ "action": "always" })).unwrap();
        let Wait::Answer(d) = poll("t-always", &id) else { panic!("no answer") };
        assert_eq!(d["updatedPermissions"], rule);

        let without = open("t-always", &evt("Bash", json!({ "command": "ls" }), Value::Null));
        assert!(without["always"].is_null());
        let id = without["id"].as_str().unwrap();
        assert!(answer("t-always", id, &json!({ "action": "always" })).is_err());
    }

    #[test]
    fn question_answers_ride_in_updated_input() {
        let input = json!({ "questions": [{ "question": "Color?", "header": "C", "multiSelect": false,
            "options": [{ "label": "Red", "description": "r" }, { "label": "Blue", "description": "b" }] }] });
        let v = open("t-q", &evt("AskUserQuestion", input, Value::Null));
        assert_eq!(v["kind"], "question");
        let id = v["id"].as_str().unwrap().to_string();
        assert!(answer("t-q", &id, &json!({ "answers": {} })).is_err());
        answer("t-q", &id, &json!({ "answers": { "Color?": "Blue" } })).unwrap();
        let Wait::Answer(d) = poll("t-q", &id) else { panic!("no answer") };
        assert_eq!(d["updatedInput"]["answers"]["Color?"], "Blue");
        assert_eq!(d["updatedInput"]["questions"][0]["header"], "C");
    }

    #[test]
    fn plan_approve_sets_mode_and_revise_denies_with_feedback() {
        let plan = evt("ExitPlanMode", json!({ "plan": "1. do it" }), Value::Null);
        let v = open("t-p", &plan);
        assert_eq!(v["plan"], "1. do it");
        let id = v["id"].as_str().unwrap().to_string();
        assert!(answer("t-p", &id, &json!({ "action": "approve", "mode": "yolo" })).is_err());
        answer("t-p", &id, &json!({ "action": "approve", "mode": "acceptEdits" })).unwrap();
        let Wait::Answer(d) = poll("t-p", &id) else { panic!("no answer") };
        assert_eq!(d["updatedInput"]["plan"], "1. do it");
        assert_eq!(d["updatedPermissions"][0]["mode"], "acceptEdits");

        let id = open("t-p", &plan)["id"].as_str().unwrap().to_string();
        answer("t-p", &id, &json!({ "action": "revise", "feedback": " add tests " })).unwrap();
        let Wait::Answer(d) = poll("t-p", &id) else { panic!("no answer") };
        assert_eq!(d, json!({ "behavior": "deny", "message": "add tests" }));
    }

    #[test]
    fn cleared_or_replaced_prompts_are_gone() {
        let first = open("t-gone", &evt("Bash", json!({}), Value::Null))["id"].as_str().unwrap().to_string();
        let second = open("t-gone", &evt("Bash", json!({}), Value::Null))["id"].as_str().unwrap().to_string();
        assert!(matches!(poll("t-gone", &first), Wait::Gone));
        assert!(matches!(poll("t-gone", &second), Wait::Pending));
        assert!(answer("t-gone", &first, &json!({ "action": "allow" })).is_err());
        clear("t-gone");
        assert!(matches!(poll("t-gone", &second), Wait::Gone));
    }

    #[tokio::test]
    async fn wait_wakes_on_answer() {
        let id = open("t-wait", &evt("Bash", json!({}), Value::Null))["id"].as_str().unwrap().to_string();
        let answer_id = id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            answer("t-wait", &answer_id, &json!({ "action": "deny" })).unwrap();
        });
        let Wait::Answer(d) = wait("t-wait", &id).await else { panic!("no answer") };
        assert_eq!(d["behavior"], "deny");
    }
}
