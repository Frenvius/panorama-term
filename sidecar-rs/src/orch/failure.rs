use serde::{Deserialize, Serialize};
use crate::orch::model::FailureClass;

const TAIL_LINES: usize = 15;
const CONTINUATION_LINES: usize = 2;
const MS_PER_MIN: i64 = 60_000;
const MS_PER_DAY: i64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Account,
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Reliable,
    Partial,
}

pub struct ClassifyInput<'a> {
    pub agent: &'a str,
    pub reason_code: &'a str,
    pub tail: &'a [String],
    pub now_ms: i64,
    pub local_offset_min: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub class: FailureClass,
    pub scope: Scope,
    pub reset_at: Option<i64>,
    pub source: String,
    pub confidence: Confidence,
}

struct Rule {
    agents: &'static [&'static str],
    id: &'static str,
    starts: &'static [&'static str],
    contains: &'static [&'static str],
    class: FailureClass,
    scope: Scope,
    confidence: Confidence,
}

const CLAUDE: &[&str] = &["claude"];
const CODEX: &[&str] = &["codex"];
const PI: &[&str] = &["pi"];
const KIMI: &[&str] = &["kimi"];
const AGY: &[&str] = &["agy", "antigravity"];

use Confidence::{Partial, Reliable};
use FailureClass::{AuthFailed, ModelUnavailable, QuotaExhausted, RateLimited};
use Scope::{Account, Model};

const RULES: &[Rule] = &[
    Rule {
        agents: CLAUDE,
        id: "server_throttle",
        starts: &["Server is temporarily limiting requests"],
        contains: &[],
        class: RateLimited,
        scope: Account,
        confidence: Reliable,
    },
    Rule {
        agents: CLAUDE,
        id: "usage_limit",
        starts: &["You've hit your", "You've reached your"],
        contains: &["limit"],
        class: QuotaExhausted,
        scope: Account,
        confidence: Reliable,
    },
    Rule {
        agents: CLAUDE,
        id: "out_of_credits",
        starts: &[
            "You're out of usage credits",
            "You're out of extra usage",
            "Your org is out of usage",
            "Your organization is out of usage credits",
            "Usage is paused for your organization",
        ],
        contains: &[],
        class: QuotaExhausted,
        scope: Account,
        confidence: Reliable,
    },
    Rule {
        agents: CLAUDE,
        id: "credit_balance",
        starts: &[
            "Credit balance is too low",
            "Your credit balance is too low to access the Anthropic API",
        ],
        contains: &[],
        class: QuotaExhausted,
        scope: Account,
        confidence: Reliable,
    },
    Rule {
        agents: CLAUDE,
        id: "legacy_limit_banner",
        starts: &[
            "Claude usage limit reached",
            "5-hour limit reached",
            "Weekly limit reached",
            "Session limit reached",
            "Opus limit reached",
            "Sonnet limit reached",
        ],
        contains: &[],
        class: QuotaExhausted,
        scope: Account,
        confidence: Partial,
    },
    Rule {
        agents: CLAUDE,
        id: "high_load",
        starts: &[
            "Opus is experiencing high load",
            "Fable is experiencing high load",
            "Repeated 529 Overloaded errors",
        ],
        contains: &[],
        class: ModelUnavailable,
        scope: Model,
        confidence: Reliable,
    },
    Rule {
        agents: CLAUDE,
        id: "auth",
        starts: &[
            "Not logged in",
            "Invalid API key",
            "Invalid auth token",
            "Authentication required",
            "OAuth token revoked",
            "Login expired",
            "Login reached its time limit",
            "Failed to authenticate",
            "Session expired. Please run /login",
            "OAuth refresh token is no longer valid",
            "Please run /login",
        ],
        contains: &[],
        class: AuthFailed,
        scope: Account,
        confidence: Reliable,
    },
    Rule {
        agents: CODEX,
        id: "usage_limit_model",
        starts: &["You've hit your usage limit for "],
        contains: &[],
        class: QuotaExhausted,
        scope: Model,
        confidence: Reliable,
    },
    Rule {
        agents: CODEX,
        id: "usage_limit",
        starts: &["You've hit your usage limit"],
        contains: &[],
        class: QuotaExhausted,
        scope: Account,
        confidence: Reliable,
    },
    Rule {
        agents: CODEX,
        id: "workspace_credits",
        starts: &[
            "Your workspace is out of credits",
            "You hit your spend cap set",
        ],
        contains: &[],
        class: QuotaExhausted,
        scope: Account,
        confidence: Reliable,
    },
    Rule {
        agents: CODEX,
        id: "quota_exceeded",
        starts: &["Quota exceeded. Check your plan and billing details"],
        contains: &[],
        class: QuotaExhausted,
        scope: Account,
        confidence: Reliable,
    },
    Rule {
        agents: CODEX,
        id: "model_capacity",
        starts: &["Selected model is at capacity"],
        contains: &[],
        class: ModelUnavailable,
        scope: Model,
        confidence: Reliable,
    },
    Rule {
        agents: CODEX,
        id: "high_demand",
        starts: &["We're currently experiencing high demand"],
        contains: &[],
        class: RateLimited,
        scope: Model,
        confidence: Reliable,
    },
    Rule {
        agents: CODEX,
        id: "api_rate_limit",
        starts: &["Rate limit reached for "],
        contains: &["try again in"],
        class: RateLimited,
        scope: Model,
        confidence: Partial,
    },
    Rule {
        agents: CODEX,
        id: "auth",
        starts: &[
            "Your access token could not be refreshed",
            "Not signed in. Please run 'codex login'",
        ],
        contains: &[],
        class: AuthFailed,
        scope: Account,
        confidence: Reliable,
    },
    Rule {
        agents: PI,
        id: "rate_limit_retry",
        starts: &["rate limit reached; retry in"],
        contains: &[],
        class: RateLimited,
        scope: Account,
        confidence: Reliable,
    },
    Rule {
        agents: PI,
        id: "auth",
        starts: &["No API key found for", "Authentication failed for \""],
        contains: &[],
        class: AuthFailed,
        scope: Account,
        confidence: Reliable,
    },
    Rule {
        agents: KIMI,
        id: "provider_rate_limit",
        starts: &["Provider rate limit"],
        contains: &[],
        class: RateLimited,
        scope: Account,
        confidence: Partial,
    },
    Rule {
        agents: KIMI,
        id: "provider_auth",
        starts: &["Provider authentication error"],
        contains: &[],
        class: AuthFailed,
        scope: Account,
        confidence: Partial,
    },
    Rule {
        agents: AGY,
        id: "model_quota",
        starts: &["You have exhausted your quota on this model"],
        contains: &[],
        class: QuotaExhausted,
        scope: Model,
        confidence: Reliable,
    },
    Rule {
        agents: AGY,
        id: "model_capacity",
        starts: &["You have exhausted your capacity on this model"],
        contains: &[],
        class: QuotaExhausted,
        scope: Model,
        confidence: Partial,
    },
    Rule {
        agents: AGY,
        id: "not_signed_in",
        starts: &["Welcome back! You are currently not signed in"],
        contains: &[],
        class: AuthFailed,
        scope: Account,
        confidence: Reliable,
    },
];

const DECORATION: &[char] = &[
    '\u{23BF}', '\u{23FA}', '\u{25CF}', '\u{2502}', '\u{2503}', '\u{2022}', '\u{26A0}',
    '\u{2717}', '\u{2718}', '\u{2716}', '\u{274C}', '\u{25A0}', '!',
];
const ERROR_PREFIXES: &[&str] = &["API Error: ", "Error: ", "error: ", "ERROR: "];

pub fn classify(input: ClassifyInput) -> Classification {
    let agent = input.agent.trim().to_ascii_lowercase();
    if let Some(found) = match_tail(&agent, &input) {
        return found;
    }
    classify_reason_code(input.reason_code)
}

fn match_tail(agent: &str, input: &ClassifyInput) -> Option<Classification> {
    let start = input.tail.len().saturating_sub(TAIL_LINES);
    let lines: Vec<String> = input.tail[start..].iter().map(|l| clean_line(l)).collect();
    for index in (0..lines.len()).rev() {
        let line = core_text(&lines[index]);
        let Some(rule) = RULES.iter().find(|r| rule_matches(r, agent, line)) else {
            continue;
        };
        let context = continuation_context(&lines, index);
        return Some(Classification {
            class: rule.class,
            scope: rule.scope,
            reset_at: parse_reset(&context, input.now_ms, input.local_offset_min),
            source: format!("classifier:{agent}:{}", rule.id),
            confidence: rule.confidence,
        });
    }
    None
}

fn rule_matches(rule: &Rule, agent: &str, line: &str) -> bool {
    rule.agents.contains(&agent)
        && rule.starts.iter().any(|s| line.starts_with(s))
        && rule.contains.iter().all(|c| line.contains(c))
}

fn continuation_context(lines: &[String], index: usize) -> String {
    let end = (index + 1 + CONTINUATION_LINES).min(lines.len());
    lines[index..end]
        .iter()
        .map(|l| core_text(l))
        .collect::<Vec<_>>()
        .join(" ")
}

fn core_text(line: &str) -> &str {
    let mut text = line.trim_start_matches(|c: char| c.is_whitespace() || DECORATION.contains(&c));
    for prefix in ERROR_PREFIXES {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest;
            break;
        }
    }
    text.trim_end()
}

fn clean_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(straight_quote(c));
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

fn straight_quote(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{02bc}' => '\'',
        '\u{201c}' | '\u{201d}' => '"',
        _ => c,
    }
}

fn classify_reason_code(reason_code: &str) -> Classification {
    let (class, confidence) = match reason_code {
        "spawn_failed" | "worktree_failed" | "brief_failed" => {
            (FailureClass::LaunchFailed, Partial)
        }
        "session_gone" | "agent_exited" => (FailureClass::ExecutionFailed, Partial),
        "setup_failed" => (FailureClass::SetupFailed, Partial),
        "worker_failed_report" | "verification_failed" => (FailureClass::QualityFailed, Partial),
        _ => (FailureClass::Unknown, Partial),
    };
    Classification {
        class,
        scope: Model,
        reset_at: None,
        source: format!("reason_code:{reason_code}"),
        confidence,
    }
}

fn parse_reset(text: &str, now_ms: i64, local_offset_min: i32) -> Option<i64> {
    let lower = text.to_ascii_lowercase();
    parse_relative_reset(text, &lower, now_ms)
        .or_else(|| parse_absolute_reset(text, &lower, now_ms, local_offset_min))
}

fn text_after<'a>(text: &'a str, lower: &str, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| lower.find(key).map(|at| &text[at + key.len()..]))
        .next()
}

const RELATIVE_KEYS: &[&str] = &[
    "try again in ",
    "retry in ",
    "resets in ",
    "reset in ",
    "reset after ",
    "retry after ",
];
const ABSOLUTE_KEYS: &[&str] = &["try again at ", "resets at ", "reset at ", "resets "];

fn parse_relative_reset(text: &str, lower: &str, now_ms: i64) -> Option<i64> {
    let rest = text_after(text, lower, RELATIVE_KEYS)?;
    parse_duration_ms(rest).map(|ms| now_ms + ms)
}

fn parse_duration_ms(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let mut pos = 0;
    let mut total = 0.0_f64;
    let mut groups = 0;
    loop {
        while pos < bytes.len() && matches!(bytes[pos], b' ' | b',') {
            pos += 1;
        }
        if text[pos..].to_ascii_lowercase().starts_with("and ") {
            pos += 4;
            continue;
        }
        let num_start = pos;
        while pos < bytes.len() && (bytes[pos].is_ascii_digit() || bytes[pos] == b'.') {
            pos += 1;
        }
        let Ok(value) = text[num_start..pos].parse::<f64>() else {
            break;
        };
        while pos < bytes.len() && bytes[pos] == b' ' {
            pos += 1;
        }
        let unit_start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_alphabetic() {
            pos += 1;
        }
        let Some(unit_ms) = unit_millis(&text[unit_start..pos]) else {
            break;
        };
        total += value * unit_ms;
        groups += 1;
    }
    (groups > 0).then(|| total.round() as i64)
}

fn unit_millis(unit: &str) -> Option<f64> {
    let ms = match unit.to_ascii_lowercase().as_str() {
        "ms" => 1.0,
        "s" | "sec" | "secs" | "second" | "seconds" => 1_000.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 60_000.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000.0,
        "d" | "day" | "days" => 86_400_000.0,
        _ => return None,
    };
    Some(ms)
}

struct ClockPhrase {
    month_day: Option<(u32, u32)>,
    year: Option<i64>,
    hour: u32,
    minute: u32,
    zone: Option<String>,
}

fn parse_absolute_reset(text: &str, lower: &str, now_ms: i64, local_offset_min: i32) -> Option<i64> {
    let rest = text_after(text, lower, ABSOLUTE_KEYS)?;
    let phrase = parse_clock_phrase(rest)?;
    let zone = match &phrase.zone {
        Some(name) => Zone::Named(find_zone(name)?),
        None => Zone::Fixed(local_offset_min),
    };
    resolve_clock(&phrase, &zone, now_ms)
}

fn parse_clock_phrase(text: &str) -> Option<ClockPhrase> {
    let words: Vec<&str> = text
        .split(|c: char| c == ' ' || c == ',')
        .filter(|w| !w.is_empty())
        .collect();
    let mut i = 0;
    let mut month_day = None;
    let mut year = None;
    if let Some(month) = words.first().and_then(|w| month_number(w)) {
        let day = parse_day(words.get(1)?)?;
        month_day = Some((month, day));
        i = 2;
        if let Some(y) = words.get(i).and_then(|w| parse_year(w)) {
            year = Some(y);
            i += 1;
        }
        if words.get(i).is_some_and(|w| w.eq_ignore_ascii_case("at")) {
            i += 1;
        }
    }
    let (hour, minute, used) = parse_time(words.get(i)?, words.get(i + 1).copied())?;
    i += used;
    let zone = words
        .get(i)
        .filter(|w| w.starts_with('('))
        .map(|w| w.trim_matches(|c| c == '(' || c == ')' || c == '.').to_string());
    Some(ClockPhrase {
        month_day,
        year,
        hour,
        minute,
        zone,
    })
}

fn month_number(word: &str) -> Option<u32> {
    let lower = word.to_ascii_lowercase();
    const MONTHS: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    MONTHS
        .iter()
        .position(|m| lower.len() >= 3 && m.starts_with(lower.as_str()))
        .map(|p| p as u32 + 1)
}

fn parse_day(word: &str) -> Option<u32> {
    let digits = word.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '.');
    let day: u32 = digits.parse().ok()?;
    (1..=31).contains(&day).then_some(day)
}

fn parse_year(word: &str) -> Option<i64> {
    (word.len() == 4 && word.bytes().all(|b| b.is_ascii_digit()))
        .then(|| word.parse().ok())
        .flatten()
}

fn parse_time(word: &str, next: Option<&str>) -> Option<(u32, u32, usize)> {
    let word = word.trim_end_matches(['.', ')']);
    let lower = word.to_ascii_lowercase();
    let (clock, mut meridiem, mut used) = match lower.strip_suffix("am") {
        Some(c) => (c.to_string(), Some(false), 1),
        None => match lower.strip_suffix("pm") {
            Some(c) => (c.to_string(), Some(true), 1),
            None => (lower.clone(), None, 1),
        },
    };
    if meridiem.is_none() {
        let next = next?.trim_end_matches(['.', ')']).to_ascii_lowercase();
        meridiem = match next.as_str() {
            "am" => Some(false),
            "pm" => Some(true),
            _ => None,
        };
        if meridiem.is_some() {
            used = 2;
        } else if !clock.contains(':') {
            return None;
        }
    }
    let (hour_text, minute_text) = clock.split_once(':').unwrap_or((clock.as_str(), "0"));
    let mut hour: u32 = hour_text.parse().ok()?;
    let minute: u32 = minute_text.parse().ok()?;
    if minute > 59 {
        return None;
    }
    match meridiem {
        Some(pm) => {
            if !(1..=12).contains(&hour) {
                return None;
            }
            hour %= 12;
            if pm {
                hour += 12;
            }
        }
        None if hour > 23 => return None,
        None => {}
    }
    Some((hour, minute, used))
}

enum Zone {
    Fixed(i32),
    Named(&'static ZoneRule),
}

impl Zone {
    fn offset_min_at(&self, utc_ms: i64) -> i32 {
        match self {
            Zone::Fixed(minutes) => *minutes,
            Zone::Named(rule) => rule.offset_min_at(utc_ms),
        }
    }

    fn local_to_utc(&self, local_ms: i64) -> i64 {
        let first = self.offset_min_at(local_ms - i64::from(self.standard_min()) * MS_PER_MIN);
        let utc = local_ms - i64::from(first) * MS_PER_MIN;
        let second = self.offset_min_at(utc);
        if second == first {
            return utc;
        }
        local_ms - i64::from(second) * MS_PER_MIN
    }

    fn standard_min(&self) -> i32 {
        match self {
            Zone::Fixed(minutes) => *minutes,
            Zone::Named(rule) => rule.standard_min,
        }
    }
}

#[derive(Clone, Copy)]
enum Dst {
    None,
    Us,
    Eu,
}

struct ZoneRule {
    name: &'static str,
    standard_min: i32,
    dst: Dst,
}

impl ZoneRule {
    fn offset_min_at(&self, utc_ms: i64) -> i32 {
        let year = civil_from_days(utc_ms.div_euclid(MS_PER_DAY)).0;
        let (start, end) = match self.dst {
            Dst::None => return self.standard_min,
            Dst::Us => (
                us_transition_utc(year, 3, 2, self.standard_min),
                us_transition_utc(year, 11, 1, self.standard_min + 60),
            ),
            Dst::Eu => (
                eu_transition_utc(year, 3),
                eu_transition_utc(year, 10),
            ),
        };
        if utc_ms >= start && utc_ms < end {
            self.standard_min + 60
        } else {
            self.standard_min
        }
    }
}

fn us_transition_utc(year: i64, month: u32, nth_sunday: i64, offset_min: i32) -> i64 {
    let first = days_from_civil(year, month, 1);
    let first_sunday = first + (7 - weekday_of_days(first)) % 7;
    let day = first_sunday + (nth_sunday - 1) * 7;
    day * MS_PER_DAY + 2 * 60 * MS_PER_MIN - i64::from(offset_min) * MS_PER_MIN
}

fn eu_transition_utc(year: i64, month: u32) -> i64 {
    let next_month_first = if month == 12 {
        days_from_civil(year + 1, 1, 1)
    } else {
        days_from_civil(year, month + 1, 1)
    };
    let last_day = next_month_first - 1;
    let last_sunday = last_day - weekday_of_days(last_day);
    last_sunday * MS_PER_DAY + 60 * MS_PER_MIN
}

fn weekday_of_days(days: i64) -> i64 {
    (days + 4).rem_euclid(7)
}

const ZONES: &[ZoneRule] = &[
    zone("UTC", 0, Dst::None),
    zone("Etc/UTC", 0, Dst::None),
    zone("GMT", 0, Dst::None),
    zone("America/Sao_Paulo", -180, Dst::None),
    zone("America/Bahia", -180, Dst::None),
    zone("America/Fortaleza", -180, Dst::None),
    zone("America/Recife", -180, Dst::None),
    zone("America/Belem", -180, Dst::None),
    zone("America/Manaus", -240, Dst::None),
    zone("America/Cuiaba", -240, Dst::None),
    zone("America/Rio_Branco", -300, Dst::None),
    zone("America/Argentina/Buenos_Aires", -180, Dst::None),
    zone("America/Bogota", -300, Dst::None),
    zone("America/Lima", -300, Dst::None),
    zone("America/Mexico_City", -360, Dst::None),
    zone("America/Phoenix", -420, Dst::None),
    zone("America/New_York", -300, Dst::Us),
    zone("America/Toronto", -300, Dst::Us),
    zone("America/Chicago", -360, Dst::Us),
    zone("America/Denver", -420, Dst::Us),
    zone("America/Los_Angeles", -480, Dst::Us),
    zone("America/Vancouver", -480, Dst::Us),
    zone("Europe/London", 0, Dst::Eu),
    zone("Europe/Dublin", 0, Dst::Eu),
    zone("Europe/Lisbon", 0, Dst::Eu),
    zone("Europe/Paris", 60, Dst::Eu),
    zone("Europe/Berlin", 60, Dst::Eu),
    zone("Europe/Madrid", 60, Dst::Eu),
    zone("Europe/Rome", 60, Dst::Eu),
    zone("Europe/Amsterdam", 60, Dst::Eu),
    zone("Europe/Brussels", 60, Dst::Eu),
    zone("Europe/Vienna", 60, Dst::Eu),
    zone("Europe/Zurich", 60, Dst::Eu),
    zone("Europe/Stockholm", 60, Dst::Eu),
    zone("Europe/Warsaw", 60, Dst::Eu),
    zone("Europe/Prague", 60, Dst::Eu),
    zone("Europe/Athens", 120, Dst::Eu),
    zone("Europe/Helsinki", 120, Dst::Eu),
    zone("Europe/Kyiv", 120, Dst::Eu),
    zone("Europe/Moscow", 180, Dst::None),
    zone("Europe/Istanbul", 180, Dst::None),
    zone("Africa/Johannesburg", 120, Dst::None),
    zone("Asia/Dubai", 240, Dst::None),
    zone("Asia/Kolkata", 330, Dst::None),
    zone("Asia/Calcutta", 330, Dst::None),
    zone("Asia/Singapore", 480, Dst::None),
    zone("Asia/Shanghai", 480, Dst::None),
    zone("Asia/Hong_Kong", 480, Dst::None),
    zone("Asia/Tokyo", 540, Dst::None),
    zone("Asia/Seoul", 540, Dst::None),
];

const fn zone(name: &'static str, standard_min: i32, dst: Dst) -> ZoneRule {
    ZoneRule {
        name,
        standard_min,
        dst,
    }
}

fn find_zone(name: &str) -> Option<&'static ZoneRule> {
    ZONES.iter().find(|z| z.name.eq_ignore_ascii_case(name))
}

fn resolve_clock(phrase: &ClockPhrase, zone: &Zone, now_ms: i64) -> Option<i64> {
    let time_ms = (i64::from(phrase.hour) * 60 + i64::from(phrase.minute)) * MS_PER_MIN;
    let now_local_days = (now_ms + i64::from(zone.offset_min_at(now_ms)) * MS_PER_MIN)
        .div_euclid(MS_PER_DAY);
    let at_day = |days: i64| zone.local_to_utc(days * MS_PER_DAY + time_ms);

    let Some((month, day)) = phrase.month_day else {
        let today = at_day(now_local_days);
        return Some(if today > now_ms {
            today
        } else {
            at_day(now_local_days + 1)
        });
    };
    if let Some(year) = phrase.year {
        let resolved = at_day(date_days(year, month, day)?);
        return (resolved > now_ms).then_some(resolved);
    }
    let year = civil_from_days(now_local_days).0;
    let this_year = at_day(date_days(year, month, day)?);
    if this_year > now_ms {
        return Some(this_year);
    }
    Some(at_day(date_days(year + 1, month, day)?))
}

pub(super) fn date_days(year: i64, month: u32, day: u32) -> Option<i64> {
    let days = days_from_civil(year, month, day);
    (civil_from_days(days) == (year, month, day)).then_some(days)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc_ms(y: i64, mo: u32, d: u32, h: i64, mi: i64) -> i64 {
        days_from_civil(y, mo, d) * MS_PER_DAY + (h * 60 + mi) * MS_PER_MIN
    }

    fn tail(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    fn run(agent: &str, reason: &str, lines: &[&str], now_ms: i64, offset: i32) -> Classification {
        let tail = tail(lines);
        classify(ClassifyInput {
            agent,
            reason_code: reason,
            tail: &tail,
            now_ms,
            local_offset_min: offset,
        })
    }

    fn quick(agent: &str, lines: &[&str]) -> Classification {
        run(agent, "agent_exited", lines, utc_ms(2026, 11, 22, 12, 0), -180)
    }

    #[test]
    fn date_helpers_match_known_epoch() {
        assert_eq!(utc_ms(2023, 11, 14, 22, 13), 1_699_999_980_000);
        assert_eq!(civil_from_days(days_from_civil(2026, 11, 22)), (2026, 11, 22));
    }

    #[test]
    fn claude_usage_limit_is_account_quota() {
        let got = quick(
            "claude",
            &["You've hit your session limit \u{b7} resets 3am (America/Sao_Paulo)"],
        );
        assert_eq!(got.class, FailureClass::QuotaExhausted);
        assert_eq!(got.scope, Scope::Account);
        assert_eq!(got.confidence, Confidence::Reliable);
        assert_eq!(got.reset_at, Some(utc_ms(2026, 11, 23, 6, 0)));
    }

    #[test]
    fn claude_reached_limit_variant() {
        let got = quick("claude", &["You've reached your weekly limit"]);
        assert_eq!(got.class, FailureClass::QuotaExhausted);
        assert_eq!(got.reset_at, None);
    }

    #[test]
    fn claude_out_of_credits_messages() {
        for line in [
            "You're out of usage credits",
            "You're out of extra usage",
            "Credit balance is too low",
            "Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits.",
            "Usage is paused for your organization. For help, please contact support.",
        ] {
            let got = quick("claude", &[line]);
            assert_eq!(got.class, FailureClass::QuotaExhausted, "{line}");
            assert_eq!(got.confidence, Confidence::Reliable, "{line}");
        }
    }

    #[test]
    fn claude_legacy_banner_is_partial() {
        let got = quick("claude", &["5-hour limit reached \u{2219} resets 3pm"]);
        assert_eq!(got.class, FailureClass::QuotaExhausted);
        assert_eq!(got.confidence, Confidence::Partial);
        assert_eq!(got.reset_at, Some(utc_ms(2026, 11, 22, 18, 0)));
    }

    #[test]
    fn claude_server_throttle_is_rate_limited_not_quota() {
        let got = quick(
            "claude",
            &["Server is temporarily limiting requests (not your usage limit)"],
        );
        assert_eq!(got.class, FailureClass::RateLimited);
    }

    #[test]
    fn claude_high_load_is_model_unavailable() {
        for line in [
            "Opus is experiencing high load, please use /model to switch to Sonnet",
            "Fable is experiencing high load. Switch to Sonnet.",
            "Repeated 529 Overloaded errors",
        ] {
            let got = quick("claude", &[line]);
            assert_eq!(got.class, FailureClass::ModelUnavailable, "{line}");
            assert_eq!(got.scope, Scope::Model, "{line}");
        }
    }

    #[test]
    fn auth_messages_are_never_quota() {
        let cases: [(&str, &str); 14] = [
            ("claude", "Not logged in \u{b7} Please run /login"),
            ("claude", "Invalid API key \u{b7} Fix external API key"),
            ("claude", "Invalid auth token \u{b7} Fix external auth token"),
            ("claude", "OAuth token revoked \u{b7} Please run /login"),
            ("claude", "Login expired \u{b7} Please run /login"),
            ("claude", "Session expired. Please run /login to sign in again."),
            (
                "claude",
                "Failed to authenticate: this login reached its time limit and cannot be renewed. Run `claude auth login` to sign in again",
            ),
            (
                "codex",
                "Your access token could not be refreshed. Please log out and sign in again.",
            ),
            (
                "codex",
                "Not signed in. Please run 'codex login' to sign in with ChatGPT, then re-run 'codex cloud'.",
            ),
            ("pi", "No API key found for anthropic."),
            (
                "pi",
                "Authentication failed for \"openai-codex\". Credentials may have expired or network is unavailable. Run '/login openai-codex' to re-authenticate.",
            ),
            ("kimi", "Provider authentication error"),
            ("agy", "Welcome back! You are currently not signed in."),
            ("claude", "Authentication required \u{b7} Sign in again to continue"),
        ];
        for (agent, line) in cases {
            let got = quick(agent, &[line]);
            assert_eq!(got.class, FailureClass::AuthFailed, "{agent}: {line}");
            assert_eq!(got.scope, Scope::Account, "{agent}: {line}");
            assert_eq!(got.reset_at, None, "{agent}: {line}");
        }
    }

    #[test]
    fn claude_login_time_limit_is_auth_not_quota() {
        let got = quick("claude", &["Login reached its time limit \u{b7} Please run /login"]);
        assert_eq!(got.class, FailureClass::AuthFailed);
    }

    #[test]
    fn codex_usage_limit_variants() {
        let account = [
            "You've hit your usage limit. Upgrade to Plus to continue using Codex (https://chatgpt.com/explore/plus), or try again at 3:15 PM.",
            "You've hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit https://chatgpt.com/codex/settings/usage to purchase more credits or try again at 3:15 PM.",
            "You've hit your usage limit. Visit https://chatgpt.com/codex/settings/usage to purchase more credits or try again at 3:15 PM.",
            "You've hit your usage limit. To get more access now, send a request to your admin or try again at 3:15 PM.",
            "You've hit your usage limit.",
        ];
        for line in account {
            let got = quick("codex", &[line]);
            assert_eq!(got.class, FailureClass::QuotaExhausted, "{line}");
            assert_eq!(got.scope, Scope::Account, "{line}");
            assert_eq!(got.confidence, Confidence::Reliable, "{line}");
        }
        let model = quick(
            "codex",
            &["You've hit your usage limit for GPT-5.5. Switch to another model now, or try again at 3:15 PM."],
        );
        assert_eq!(model.class, FailureClass::QuotaExhausted);
        assert_eq!(model.scope, Scope::Model);
        assert_eq!(model.reset_at, Some(utc_ms(2026, 11, 22, 18, 15)));
    }

    #[test]
    fn codex_usage_limit_with_curly_apostrophe_under_switch_prompt() {
        let got = quick(
            "codex",
            &[
                "⚠ Heads up, you have less than 10% of your 5h limit left. Run /status for a breakdown.",
                "■ You\u{2019}ve hit your usage limit. Try again at 3:15 PM.",
                "",
                "  Approaching rate limits",
                "  Switch to gpt-6-luna for lower credit usage?",
                "› 1. Switch to gpt-6-luna                   Fast and affordable model for easier tasks.",
                "  2. Keep current model",
                "  3. Keep current model (never show again)  Hide future rate limit reminders about switching models",
                "  enter select · esc back",
            ],
        );
        assert_eq!(got.class, FailureClass::QuotaExhausted);
        assert_eq!(got.scope, Scope::Account);
        assert_eq!(got.reset_at, Some(utc_ms(2026, 11, 22, 18, 15)));
    }

    #[test]
    fn codex_workspace_and_billing_messages() {
        for line in [
            "Your workspace is out of credits. Add credits to continue.",
            "Your workspace is out of credits. Ask your workspace owner to refill in order to continue.",
            "You hit your spend cap set in your workspace. Increase your spend cap to continue.",
            "You hit your spend cap set by the owner of your workspace. Ask an owner to increase your spend cap to continue.",
            "Quota exceeded. Check your plan and billing details.",
        ] {
            let got = quick("codex", &[line]);
            assert_eq!(got.class, FailureClass::QuotaExhausted, "{line}");
            assert_eq!(got.reset_at, None, "{line}");
        }
    }

    #[test]
    fn codex_capacity_and_demand() {
        let capacity = quick("codex", &["Selected model is at capacity. Please try a different model."]);
        assert_eq!(capacity.class, FailureClass::ModelUnavailable);
        assert_eq!(capacity.scope, Scope::Model);
        let demand = quick(
            "codex",
            &["We're currently experiencing high demand, which may cause temporary errors."],
        );
        assert_eq!(demand.class, FailureClass::RateLimited);
    }

    #[test]
    fn codex_api_rate_limit_parses_relative_seconds() {
        let now = utc_ms(2026, 11, 22, 12, 0);
        let got = run(
            "codex",
            "agent_exited",
            &["Rate limit reached for gpt-5 in organization org-x on tokens per min (TPM): Limit 30000. Please try again in 1.5s."],
            now,
            -180,
        );
        assert_eq!(got.class, FailureClass::RateLimited);
        assert_eq!(got.confidence, Confidence::Partial);
        assert_eq!(got.reset_at, Some(now + 1500));
    }

    #[test]
    fn codex_wrapped_message_finds_reset_on_next_line() {
        let got = quick(
            "codex",
            &[
                "You've hit your usage limit. Upgrade to Plus to continue using Codex",
                "(https://chatgpt.com/explore/plus), or try again at 3:15 PM.",
            ],
        );
        assert_eq!(got.reset_at, Some(utc_ms(2026, 11, 22, 18, 15)));
    }

    #[test]
    fn pi_rate_limit_and_retry_delay() {
        let now = utc_ms(2026, 11, 22, 12, 0);
        let got = run("pi", "agent_exited", &["rate limit reached; retry in 12s"], now, 0);
        assert_eq!(got.class, FailureClass::RateLimited);
        assert_eq!(got.reset_at, Some(now + 12_000));
    }

    #[test]
    fn kimi_provider_rate_limit_is_partial() {
        let got = quick("kimi", &["Provider rate limit"]);
        assert_eq!(got.class, FailureClass::RateLimited);
        assert_eq!(got.confidence, Confidence::Partial);
    }

    #[test]
    fn agy_quota_messages() {
        let got = quick("agy", &["You have exhausted your quota on this model."]);
        assert_eq!(got.class, FailureClass::QuotaExhausted);
        assert_eq!(got.scope, Scope::Model);
        assert_eq!(got.confidence, Confidence::Reliable);
        let now = utc_ms(2026, 11, 22, 12, 0);
        let capacity = run(
            "antigravity",
            "agent_exited",
            &["You have exhausted your capacity on this model. Your quota will reset after 3h2m1s."],
            now,
            0,
        );
        assert_eq!(capacity.class, FailureClass::QuotaExhausted);
        assert_eq!(capacity.confidence, Confidence::Partial);
        assert_eq!(capacity.reset_at, Some(now + (3 * 3600 + 2 * 60 + 1) * 1000));
    }

    #[test]
    fn decorations_and_error_prefix_are_ignored() {
        let got = quick("claude", &["  \u{23BF}  API Error: Credit balance is too low"]);
        assert_eq!(got.class, FailureClass::QuotaExhausted);
        let ansi = quick("claude", &["\u{1b}[31mNot logged in\u{1b}[0m \u{b7} Please run /login"]);
        assert_eq!(ansi.class, FailureClass::AuthFailed);
    }

    #[test]
    fn absolute_reset_with_zone_and_minutes() {
        let now = utc_ms(2026, 11, 22, 12, 0);
        let got = run(
            "claude",
            "agent_exited",
            &["You've hit your limit \u{b7} resets 3:15pm (America/Sao_Paulo)"],
            now,
            0,
        );
        assert_eq!(got.reset_at, Some(utc_ms(2026, 11, 22, 18, 15)));
    }

    #[test]
    fn absolute_reset_zone_overrides_local_offset() {
        let now = utc_ms(2026, 11, 22, 12, 0);
        let got = run(
            "claude",
            "agent_exited",
            &["You've hit your limit \u{b7} resets 3pm (America/New_York)"],
            now,
            -180,
        );
        assert_eq!(got.reset_at, Some(utc_ms(2026, 11, 22, 20, 0)));
    }

    #[test]
    fn absolute_reset_uses_daylight_saving_offset() {
        let now = utc_ms(2026, 7, 1, 0, 0);
        let got = run(
            "claude",
            "agent_exited",
            &["You've hit your limit \u{b7} resets 3pm (America/New_York)"],
            now,
            0,
        );
        assert_eq!(got.reset_at, Some(utc_ms(2026, 7, 1, 19, 0)));
        let london = run(
            "claude",
            "agent_exited",
            &["You've hit your limit \u{b7} resets 3pm (Europe/London)"],
            now,
            0,
        );
        assert_eq!(london.reset_at, Some(utc_ms(2026, 7, 1, 14, 0)));
    }

    #[test]
    fn absolute_reset_without_zone_rolls_to_next_day() {
        let now = utc_ms(2026, 11, 22, 20, 0);
        let got = run(
            "codex",
            "agent_exited",
            &["You've hit your usage limit. Try again at 3:15 PM."],
            now,
            -180,
        );
        assert_eq!(got.reset_at, Some(utc_ms(2026, 11, 23, 18, 15)));
    }

    #[test]
    fn absolute_reset_without_zone_same_day_when_still_ahead() {
        let now = utc_ms(2026, 11, 22, 12, 0);
        let got = run(
            "codex",
            "agent_exited",
            &["You've hit your usage limit. Try again at 3:15 PM."],
            now,
            -180,
        );
        assert_eq!(got.reset_at, Some(utc_ms(2026, 11, 22, 18, 15)));
    }

    #[test]
    fn absolute_reset_with_date_and_year() {
        let now = utc_ms(2026, 11, 22, 12, 0);
        let got = run(
            "codex",
            "agent_exited",
            &["You've hit your usage limit. Visit https://chatgpt.com/codex/settings/usage to purchase more credits or try again at Nov 25th, 2026 3:15 PM."],
            now,
            -180,
        );
        assert_eq!(got.reset_at, Some(utc_ms(2026, 11, 25, 18, 15)));
    }

    #[test]
    fn absolute_reset_with_date_only_picks_next_occurrence() {
        let now = utc_ms(2026, 11, 22, 12, 0);
        let got = run(
            "claude",
            "agent_exited",
            &["You've hit your limit \u{b7} resets Nov 3 at 9am (UTC)"],
            now,
            0,
        );
        assert_eq!(got.reset_at, Some(utc_ms(2027, 11, 3, 9, 0)));
    }

    #[test]
    fn relative_reset_minutes_and_compound() {
        let now = utc_ms(2026, 11, 22, 12, 0);
        let minutes = run(
            "codex",
            "agent_exited",
            &["You've hit your usage limit. Try again in 5 minutes."],
            now,
            0,
        );
        assert_eq!(minutes.reset_at, Some(now + 5 * MS_PER_MIN));
        let compound = run(
            "claude",
            "agent_exited",
            &["You've hit your limit \u{b7} resets in 2h 10m"],
            now,
            0,
        );
        assert_eq!(compound.reset_at, Some(now + 130 * MS_PER_MIN));
    }

    #[test]
    fn reset_is_not_guessed_when_unstated_or_unparseable() {
        let unstated = quick("claude", &["You've hit your weekly limit"]);
        assert_eq!(unstated.reset_at, None);
        let vague = quick("claude", &["You've hit your limit \u{b7} resets soon"]);
        assert_eq!(vague.reset_at, None);
        let unknown_zone = quick(
            "claude",
            &["You've hit your limit \u{b7} resets 3pm (Mars/Olympus_Mons)"],
        );
        assert_eq!(unknown_zone.class, FailureClass::QuotaExhausted);
        assert_eq!(unknown_zone.reset_at, None);
    }

    #[test]
    fn mentions_inside_code_diffs_and_traces_do_not_match() {
        let lines = [
            "diff --git a/src/limits.rs b/src/limits.rs",
            "+    // You've hit your usage limit. Try again at 3:15 PM.",
            "-        return Err(\"rate limit reached; retry in 12s\");",
            "+        throw new Error(\"You've hit your usage limit\");",
            "+\"You've hit your usage limit.\"",
            "    at handleRateLimit (/app/src/client.ts:42:11) 429",
            "Error: request failed with status 429",
            "  \u{2713} retries after 429 rate limit (12 ms)",
            "test result: FAILED. Quota exceeded handling",
            "const msg = \"Credit balance is too low\";",
            "fn is_not_logged_in() -> bool { // Not logged in",
        ];
        for agent in ["claude", "codex", "pi", "kimi", "agy"] {
            let got = quick(agent, &lines);
            assert_eq!(got.class, FailureClass::ExecutionFailed, "{agent}");
            assert_eq!(got.confidence, Confidence::Partial, "{agent}");
        }
    }

    #[test]
    fn only_last_fifteen_lines_are_inspected() {
        let mut lines = vec!["You've hit your usage limit. Try again at 3:15 PM."];
        lines.extend(std::iter::repeat("working on the task").take(15));
        let got = quick("codex", &lines);
        assert_eq!(got.class, FailureClass::ExecutionFailed);
        let mut recent = vec!["You've hit your usage limit. Try again at 3:15 PM."];
        recent.extend(std::iter::repeat("working on the task").take(14));
        assert_eq!(quick("codex", &recent).class, FailureClass::QuotaExhausted);
    }

    #[test]
    fn agent_patterns_do_not_cross_agents() {
        let got = quick("kimi", &["You've hit your usage limit."]);
        assert_eq!(got.class, FailureClass::ExecutionFailed);
        let unknown = quick("someagent", &["You've hit your usage limit."]);
        assert_eq!(unknown.class, FailureClass::ExecutionFailed);
    }

    #[test]
    fn latest_matching_line_wins() {
        let got = quick(
            "claude",
            &[
                "You've hit your limit \u{b7} resets 3am (UTC)",
                "Not logged in \u{b7} Please run /login",
            ],
        );
        assert_eq!(got.class, FailureClass::AuthFailed);
    }

    #[test]
    fn reason_code_fallbacks() {
        let cases = [
            ("spawn_failed", FailureClass::LaunchFailed),
            ("worktree_failed", FailureClass::LaunchFailed),
            ("brief_failed", FailureClass::LaunchFailed),
            ("session_gone", FailureClass::ExecutionFailed),
            ("agent_exited", FailureClass::ExecutionFailed),
            ("setup_failed", FailureClass::SetupFailed),
            ("worker_failed_report", FailureClass::QualityFailed),
            ("verification_failed", FailureClass::QualityFailed),
            ("something_else", FailureClass::Unknown),
            ("", FailureClass::Unknown),
        ];
        for (code, class) in cases {
            let got = run("claude", code, &["nothing relevant here"], 0, 0);
            assert_eq!(got.class, class, "{code}");
            assert_eq!(got.confidence, Confidence::Partial, "{code}");
            assert_eq!(got.reset_at, None, "{code}");
            assert_eq!(got.source, format!("reason_code:{code}"), "{code}");
        }
    }

    #[test]
    fn partial_support_agents_use_reason_code() {
        let got = run(
            "opencode",
            "spawn_failed",
            &["You've hit your usage limit."],
            0,
            0,
        );
        assert_eq!(got.class, FailureClass::LaunchFailed);
        assert_eq!(got.confidence, Confidence::Partial);
    }

    #[test]
    fn pattern_match_reports_source_and_empty_tail_falls_back() {
        let got = quick("codex", &["Selected model is at capacity. Please try a different model."]);
        assert_eq!(got.source, "classifier:codex:model_capacity");
        let empty = run("codex", "session_gone", &[], 0, 0);
        assert_eq!(empty.class, FailureClass::ExecutionFailed);
    }

    #[test]
    fn failure_class_serializes_snake_case() {
        let json = serde_json::to_string(&FailureClass::QuotaExhausted).unwrap();
        assert_eq!(json, "\"quota_exhausted\"");
        let back: FailureClass = serde_json::from_str("\"model_unavailable\"").unwrap();
        assert_eq!(back, FailureClass::ModelUnavailable);
    }
}
