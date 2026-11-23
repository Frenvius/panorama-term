use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::{json, Value};

use super::reducer::{CoordinatorWake, RunEntry};

type Tokens = [Option<u64>; 4];
type Window = (u64, Option<u64>);

const FIELDS: [&str; 4] = ["input", "output", "cacheRead", "cacheCreation"];
const CLAUDE_FIELDS: [&str; 4] = [
    "input_tokens",
    "output_tokens",
    "cache_read_input_tokens",
    "cache_creation_input_tokens",
];
const CODEX_FIELDS: [&str; 4] = [
    "input_tokens",
    "output_tokens",
    "cached_input_tokens",
    "cache_write_input_tokens",
];

struct WorkerUsage {
    profile: String,
    tile: Option<String>,
    window: Window,
}

#[derive(Default)]
struct ProfileUsage {
    tokens: [Option<u128>; 4],
    partial: [bool; 4],
}

impl ProfileUsage {
    fn record(&mut self, tokens: Tokens) {
        for (i, value) in tokens.into_iter().enumerate() {
            if let Some(value) = value {
                self.tokens[i] = Some(self.tokens[i].unwrap_or_default() + u128::from(value));
            } else {
                self.partial[i] = true;
            }
        }
    }

    fn metrics(&self) -> Value {
        token_metrics(
            self.tokens
                .map(|value| value.and_then(|n| n.try_into().ok())),
            self.partial,
        )
    }
}

pub(super) struct RunUsage {
    run_id: String,
    coordinator_tile: Option<String>,
    created_at: u64,
    wakes: Vec<CoordinatorWake>,
    workers: Vec<WorkerUsage>,
}

impl RunUsage {
    pub(super) fn snapshot(entry: &RunEntry) -> Self {
        Self {
            run_id: entry.run.id.clone(),
            coordinator_tile: entry.run.coordinator_tile.clone(),
            created_at: entry.run.created_at,
            wakes: entry.coordinator_wakes.clone(),
            workers: entry
                .dispatches
                .values()
                .map(|d| WorkerUsage {
                    profile: d.profile.clone().unwrap_or_default(),
                    tile: d.tile_id.clone(),
                    window: (d.started_at, d.ended_at),
                })
                .collect(),
        }
    }

    pub(super) fn read(self) -> Value {
        let coordinator = tile_tokens(
            "claude",
            self.coordinator_tile.as_deref(),
            (self.created_at, None),
        );
        let mut providers: BTreeMap<_, ProfileUsage> = BTreeMap::new();
        for worker in self.workers {
            let tokens = tile_tokens(&worker.profile, worker.tile.as_deref(), worker.window);
            providers.entry(worker.profile).or_default().record(tokens);
        }
        let workers: BTreeMap<_, _> = providers
            .into_iter()
            .map(|(profile, usage)| (profile, usage.metrics()))
            .collect();
        json!({
            "runId": self.run_id,
            "coordinator": {
                "wakes": {
                    "count": self.wakes.len(), "events": self.wakes,
                    "provenance": "reported", "source": "event_log",
                },
                "tokens": token_metrics(coordinator, [false; 4]),
            },
            "workersByProfile": workers,
        })
    }
}

fn token_metrics(tokens: Tokens, partial: [bool; 4]) -> Value {
    Value::Object(
        FIELDS
            .into_iter()
            .zip(tokens)
            .zip(partial)
            .map(|((field, value), partial)| {
                let provenance = if value.is_some() {
                    "reported"
                } else {
                    "unavailable"
                };
                let mut metric = json!({"value": value, "provenance": provenance});
                if partial && value.is_some() {
                    metric["partial"] = true.into();
                }
                (field.to_string(), metric)
            })
            .collect(),
    )
}

fn add(left: Tokens, right: Tokens) -> Tokens {
    std::array::from_fn(|i| left[i]?.checked_add(right[i]?))
}

fn tile_tokens(agent: &str, tile: Option<&str>, window: Window) -> Tokens {
    let path = tile
        .filter(|_| matches!(agent, "claude" | "codex"))
        .and_then(crate::read_binding_rec)
        .filter(|binding| binding["agent"].as_str().is_none_or(|bound| bound == agent))
        .and_then(|binding| {
            let id = binding["agentSessionId"].as_str()?;
            if Path::new(id).file_name()?.to_str()? != id {
                return None;
            }
            crate::find_agent_transcript(agent, id, binding["cwd"].as_str())
        });
    path.map_or_else(Tokens::default, |path| {
        transcript_tokens(agent, &path, window)
    })
}

fn timestamp_ms(value: &Value) -> Option<u64> {
    let stamp = value.as_str()?;
    let (date, clock) = stamp.split_once('T')?;
    let (year, rest) = date.split_once('-')?;
    let (month, day) = rest.split_once('-')?;
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return None;
    }
    let days =
        super::failure::date_days(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)?;
    let (clock, offset) = if let Some(clock) = clock.strip_suffix('Z') {
        (clock, 0)
    } else {
        let index = clock.find(['+', '-'])?;
        let (clock, offset) = clock.split_at(index);
        let (hour, minute) = offset.get(1..)?.split_once(':')?;
        let minutes = clock_part(hour, 24)? * 60 + clock_part(minute, 60)?;
        (
            clock,
            if offset.starts_with('-') {
                -minutes
            } else {
                minutes
            },
        )
    };
    let (clock, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let millis = fraction
        .bytes()
        .chain(b"000".iter().copied())
        .take(3)
        .fold(0, |n, byte| n * 10 + i64::from(byte - b'0'));
    let (hour, rest) = clock.split_once(':')?;
    let (minute, second) = rest.split_once(':')?;
    let minutes = days * 1440 + clock_part(hour, 24)? * 60 + clock_part(minute, 60)? - offset;
    (minutes * 60_000 + clock_part(second, 61)? * 1000 + millis)
        .try_into()
        .ok()
}

fn clock_part(part: &str, limit: i64) -> Option<i64> {
    if part.len() != 2 || !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let n = part.parse().ok()?;
    (n < limit).then_some(n)
}

fn transcript_tokens(agent: &str, path: &Path, (start, end): Window) -> Tokens {
    let Ok(file) = std::fs::File::open(path) else {
        return Tokens::default();
    };
    let entries = BufReader::new(file)
        .split(b'\n')
        .map_while(Result::ok)
        .filter_map(|line| {
            let entry: Value = serde_json::from_slice(&line).ok()?;
            let at = timestamp_ms(&entry["timestamp"])?;
            end.is_none_or(|end| at <= end).then_some((at, entry))
        });
    match agent {
        "claude" => claude_tokens(
            entries
                .filter(|(at, _)| *at >= start)
                .map(|(_, entry)| entry),
        ),
        "codex" => codex_tokens(entries, start),
        _ => Tokens::default(),
    }
}

fn claude_tokens(entries: impl Iterator<Item = Value>) -> Tokens {
    let mut messages: HashMap<String, Tokens> = HashMap::new();
    for entry in entries {
        let message = &entry["message"];
        if message["role"] != "assistant" {
            continue;
        }
        let Some(id) = message["id"].as_str().filter(|id| !id.is_empty()) else {
            continue;
        };
        let usage = CLAUDE_FIELDS.map(|key| message["usage"][key].as_u64());
        if usage.iter().any(Option::is_some) {
            let tokens = messages.entry(id.to_string()).or_default();
            *tokens = std::array::from_fn(|i| usage[i].or(tokens[i]));
        }
    }
    messages.into_values().reduce(add).unwrap_or_default()
}

fn codex_tokens(entries: impl Iterator<Item = (u64, Value)>, start: u64) -> Tokens {
    let mut previous = [Some(0); 4];
    let mut total = None;
    for (at, entry) in entries {
        if entry["type"] != "event_msg" || entry["payload"]["type"] != "token_count" {
            continue;
        }
        let usage = &entry["payload"]["info"]["total_token_usage"];
        let counters = CODEX_FIELDS.map(|key| usage[key].as_u64());
        if counters.iter().all(Option::is_none) {
            continue;
        }
        let delta: Tokens = std::array::from_fn(|i| counters[i]?.checked_sub(previous[i]?));
        previous = counters;
        if at >= start {
            total = Some(total.map_or(delta, |sum| add(sum, delta)));
        }
    }
    total.unwrap_or_default()
}

pub(super) fn markdown(usage: &Value) -> String {
    let tokens = |metrics: &Value| {
        FIELDS
            .into_iter()
            .zip(["input", "output", "cache read", "cache creation"])
            .map(|(field, label)| match metrics[field]["value"].as_u64() {
                Some(value) if metrics[field]["partial"] == true => {
                    format!("{label} {value} (reported subtotal)")
                }
                Some(value) => format!("{label} {value} (reported)"),
                None => format!("{label} unavailable"),
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let coordinator = &usage["coordinator"];
    let mut section = format!(
        "## Usage\n\n- Coordinator wakes: {} (event log)\n- Coordinator tokens: {}\n",
        coordinator["wakes"]["count"],
        tokens(&coordinator["tokens"])
    );
    if let Some(workers) = usage["workersByProfile"].as_object() {
        for (profile, metrics) in workers {
            let profile = if profile.is_empty() {
                "unknown profile"
            } else {
                profile
            };
            section.push_str(&format!(
                "- Worker tokens ({profile}): {}\n",
                tokens(metrics)
            ));
        }
    }
    section.push('\n');
    section
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_fixture(agent: &str, lines: &[Value], window: Window) -> Tokens {
        let path = std::env::temp_dir().join(format!(
            "orch-usage-{agent}-{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let jsonl = lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, jsonl).unwrap();
        let tokens = transcript_tokens(agent, &path, window);
        std::fs::remove_file(path).unwrap();
        tokens
    }

    #[test]
    fn claude_deduplicates_messages_and_respects_the_window() {
        let assistant = |id: &str, timestamp: &str, input: u64| {
            json!({
                "type": "assistant", "timestamp": timestamp,
                "message": {"role": "assistant", "id": id, "usage": {
                    "input_tokens": input, "output_tokens": 7,
                    "cache_read_input_tokens": 12, "cache_creation_input_tokens": 0,
                }},
            })
        };
        let lines = [
            assistant("early", "2026-01-01T00:00:00.999Z", 100),
            assistant("first", "2026-01-01T00:00:01Z", 10),
            assistant("first", "2026-01-01T00:00:01.100Z", 10),
            assistant("second", "2025-12-31T21:00:02.2345-03:00", 20),
            assistant("late", "2026-01-01T00:00:03Z", 100),
        ];
        let start = 1_767_225_601_000;
        assert_eq!(
            read_fixture("claude", &lines, (start, Some(start + 1234))),
            [Some(30), Some(14), Some(24), Some(0)]
        );
        assert_eq!(
            read_fixture("claude", &lines, (start, None)),
            [Some(130), Some(21), Some(36), Some(0)]
        );
    }

    #[test]
    fn codex_uses_cumulative_deltas_within_the_dispatch() {
        let snapshot = |time: &str, input: u64, output: u64, cached: u64| {
            json!({
                "timestamp": format!("2026-01-01T00:00:{time}Z"),
                "type": "event_msg", "payload": {"type": "token_count", "info": {"total_token_usage": {
                    "input_tokens": input, "output_tokens": output, "cached_input_tokens": cached,
                }}},
            })
        };
        let lines = [
            snapshot("00", 90, 50, 30),
            snapshot("01", 90, 50, 30),
            snapshot("02", 110, 52, 40),
            snapshot("03", 110, 52, 40),
            snapshot("04", 150, 60, 50),
        ];
        assert_eq!(
            read_fixture(
                "codex",
                &lines,
                (1_767_225_601_000, Some(1_767_225_603_000))
            ),
            [Some(20), Some(2), Some(10), None]
        );
    }

    #[test]
    fn missing_and_unrecognized_transcripts_are_unavailable() {
        let missing = Path::new("");
        let unavailable = json!({"value": null, "provenance": "unavailable"});
        assert_eq!(
            token_metrics(transcript_tokens("claude", missing, (0, None)), [false; 4])["input"],
            unavailable
        );
        assert_eq!(
            token_metrics(
                read_fixture("codex", &[json!({"type": "session_meta"})], (0, None)),
                [false; 4]
            )["input"],
            unavailable
        );
        assert_eq!(
            token_metrics([None, Some(0), None, None], [false; 4])["output"],
            json!({"value": 0, "provenance": "reported"})
        );
    }

    #[test]
    fn profile_totals_flag_reported_subtotals_when_a_dispatch_is_missing() {
        let mut usage = ProfileUsage::default();
        usage.record([Some(2), Some(0), None, None]);
        usage.record([None; 4]);
        usage.record([Some(3), Some(4), None, None]);
        let metrics = usage.metrics();
        assert_eq!(
            metrics["input"],
            json!({"value": 5, "provenance": "reported", "partial": true})
        );
        assert_eq!(
            metrics["cacheRead"],
            json!({"value": null, "provenance": "unavailable"})
        );
    }
}
