use std::collections::BTreeSet;

pub const MINUTE_MS: u64 = 60_000;
pub const HOUR_MS: u64 = 60 * MINUTE_MS;
pub const DAY_MS: u64 = 24 * HOUR_MS;
pub const MIN_SPACING_MS: u64 = 5 * MINUTE_MS;
pub const EVERY_MAX_MS: u64 = 7 * DAY_MS;
const SEARCH_DAYS: u64 = 366 * 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cron {
    pub minutes: BTreeSet<u32>,
    pub hours: BTreeSet<u32>,
    pub doms: BTreeSet<u32>,
    pub months: BTreeSet<u32>,
    pub dows: BTreeSet<u32>,
    pub dom_any: bool,
    pub dow_any: bool,
}

const DAY_NAMES: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
const MONTH_NAMES: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

fn value(raw: &str, min: u32, max: u32, names: &[&str], name_base: u32) -> Result<u32, String> {
    if let Some(i) = names.iter().position(|n| raw.eq_ignore_ascii_case(n)) {
        return Ok(i as u32 + name_base);
    }
    let v: u32 = raw.parse().map_err(|_| format!("bad value {raw}"))?;
    if v < min || v > max {
        return Err(format!("{v} is outside {min}-{max}"));
    }
    Ok(v)
}

fn field(raw: &str, min: u32, max: u32, names: &[&str], name_base: u32) -> Result<(BTreeSet<u32>, bool), String> {
    let mut set = BTreeSet::new();
    let any = raw == "*";
    for part in raw.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>().ok().filter(|s| *s > 0).ok_or_else(|| format!("bad step in {part}"))?),
            None => (part, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (value(a, min, max, names, name_base)?, value(b, min, max, names, name_base)?)
        } else {
            let v = value(range, min, max, names, name_base)?;
            (v, if part.contains('/') { max } else { v })
        };
        if lo > hi {
            return Err(format!("bad range {range}"));
        }
        set.extend((lo..=hi).step_by(step as usize));
    }
    if set.is_empty() {
        return Err(format!("empty field {raw}"));
    }
    Ok((set, any))
}

pub fn parse_cron(expr: &str) -> Result<Cron, String> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return Err("cron needs 5 fields: minute hour day month weekday".into());
    }
    let (minutes, _) = field(parts[0], 0, 59, &[], 0)?;
    let (hours, _) = field(parts[1], 0, 23, &[], 0)?;
    let (doms, dom_any) = field(parts[2], 1, 31, &[], 0)?;
    let (months, _) = field(parts[3], 1, 12, &MONTH_NAMES, 1)?;
    let (mut dows, dow_any) = field(parts[4], 0, 7, &DAY_NAMES, 0)?;
    if dows.remove(&7) {
        dows.insert(0);
    }
    let cron = Cron { minutes, hours, doms, months, dows, dom_any, dow_any };
    if !cron.spaced() {
        return Err("a schedule may run at most once every 5 minutes".into());
    }
    Ok(cron)
}

impl Cron {
    fn spaced(&self) -> bool {
        let mins: Vec<u32> = self.minutes.iter().copied().collect();
        if mins.windows(2).any(|w| w[1] - w[0] < 5) {
            return false;
        }
        let consecutive_hours = self.hours.iter().any(|h| self.hours.contains(&((h + 1) % 24)));
        let wrap = 60 - mins[mins.len() - 1] + mins[0];
        !(consecutive_hours && wrap < 5)
    }

    fn day_matches(&self, month: u32, dom: u32, dow: u32) -> bool {
        if !self.months.contains(&month) {
            return false;
        }
        let dom_ok = self.doms.contains(&dom);
        let dow_ok = self.dows.contains(&dow);
        match (self.dom_any, self.dow_any) {
            (true, true) => true,
            (false, true) => dom_ok,
            (true, false) => dow_ok,
            (false, false) => dom_ok || dow_ok,
        }
    }

    pub fn next_after(&self, wall_ms: u64) -> Option<u64> {
        let start_min = wall_ms / MINUTE_MS + 1;
        let mut day = start_min / (24 * 60);
        let mut first_minute_of_day = (start_min % (24 * 60)) as u32;
        for _ in 0..SEARCH_DAYS {
            let (_, month, dom) = civil_from_days(day as i64);
            let dow = ((day + 4) % 7) as u32;
            if self.day_matches(month, dom, dow) {
                for &h in self.hours.iter().filter(|h| **h * 60 + 59 >= first_minute_of_day) {
                    for &m in self.minutes.iter() {
                        if h * 60 + m >= first_minute_of_day {
                            return Some((day * 24 * 60 + u64::from(h * 60 + m)) * MINUTE_MS);
                        }
                    }
                }
            }
            day += 1;
            first_minute_of_day = 0;
        }
        None
    }
}

pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

pub fn days_to_cron(days: &str) -> Result<String, String> {
    match days {
        "" | "daily" => Ok("*".into()),
        "weekdays" => Ok("1-5".into()),
        "weekends" => Ok("0,6".into()),
        _ => {
            let mut out = Vec::new();
            for part in days.split(',') {
                let piece = match part.split_once('-') {
                    Some((a, b)) => format!("{}-{}", day_index(a)?, day_index(b)?),
                    None => day_index(part)?.to_string(),
                };
                out.push(piece);
            }
            Ok(out.join(","))
        }
    }
}

fn day_index(name: &str) -> Result<u32, String> {
    DAY_NAMES.iter().position(|d| name.eq_ignore_ascii_case(d)).map(|i| i as u32)
        .ok_or_else(|| format!("unknown day {name}: use mon..sun, daily or weekdays"))
}

pub fn at_to_cron(at: &str, days: &str) -> Result<String, String> {
    let (h, m) = at.split_once(':').ok_or("--at takes HH:MM")?;
    let h: u32 = h.parse().ok().filter(|h| *h < 24).ok_or("--at hour must be 0-23")?;
    let m: u32 = m.parse().ok().filter(|m| *m < 60).ok_or("--at minute must be 0-59")?;
    Ok(format!("{m} {h} * * {}", days_to_cron(days)?))
}

pub fn parse_every(raw: &str) -> Result<u64, String> {
    let ms = crate::agents::waits::parse_duration(raw).ok_or("--every takes a duration like 30m, 2h or 1d")?;
    if !(MIN_SPACING_MS..=EVERY_MAX_MS).contains(&ms) {
        return Err("--every must be between 5m and 7d".into());
    }
    Ok(ms)
}

pub fn next_every(anchor: u64, every: u64, now: u64) -> u64 {
    if now < anchor {
        return anchor;
    }
    anchor + ((now - anchor) / every + 1) * every
}

fn list_human(set: &BTreeSet<u32>, names: &[&str]) -> String {
    set.iter().map(|v| names.get(*v as usize).map_or(v.to_string(), |n| n.to_string())).collect::<Vec<_>>().join(", ")
}

pub fn cron_human(cron: &Cron) -> String {
    let single_time = cron.minutes.len() == 1 && cron.hours.len() == 1;
    let at = if single_time {
        format!("at {:02}:{:02}", cron.hours.first().unwrap_or(&0), cron.minutes.first().unwrap_or(&0))
    } else {
        format!("at minutes {} of hours {}", list_human(&cron.minutes, &[]), list_human(&cron.hours, &[]))
    };
    let days = if cron.dow_any && cron.dom_any {
        "Every day".to_string()
    } else if cron.dom_any && cron.dows == (1..=5).collect() {
        "Weekdays".to_string()
    } else if cron.dom_any {
        format!("On {}", list_human(&cron.dows, &DAY_NAMES))
    } else {
        format!("On day {}", list_human(&cron.doms, &[]))
    };
    let months = if cron.months.len() == 12 { String::new() } else { format!(" in {}", list_human(&cron.months, &["", "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"])) };
    format!("{days} {at}{months}")
}

pub fn every_human(ms: u64) -> String {
    if ms % DAY_MS == 0 {
        format!("Every {} d", ms / DAY_MS)
    } else if ms % HOUR_MS == 0 {
        format!("Every {} h", ms / HOUR_MS)
    } else {
        format!("Every {} min", ms / MINUTE_MS)
    }
}

pub fn wall_human(wall_ms: u64) -> String {
    let (y, mo, d) = civil_from_days((wall_ms / DAY_MS) as i64);
    let mins = (wall_ms % DAY_MS) / MINUTE_MS;
    format!("{y:04}-{mo:02}-{d:02} {:02}:{:02}", mins / 60, mins % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(y: i64, mo: u32, d: u32, h: u64, mi: u64) -> u64 {
        let days = (0..).map(|n: i64| n).find(|n| civil_from_days(*n) == (y, mo, d)).unwrap() as u64;
        days * DAY_MS + h * HOUR_MS + mi * MINUTE_MS
    }

    #[test]
    fn cron_fields_parse_and_refuse_what_is_not_supported() {
        let c = parse_cron("0,30 8-10 * * mon-fri").unwrap();
        assert_eq!(c.minutes, [0, 30].into());
        assert_eq!(c.hours, [8, 9, 10].into());
        assert_eq!(c.dows, (1..=5).collect());
        assert_eq!(parse_cron("*/15 * * jan,dec 7").unwrap().dows, [0].into());
        assert_eq!(parse_cron("0 */6 * * *").unwrap().hours, [0, 6, 12, 18].into());
        for bad in ["* * * *", "0 0 * * * *", "0 0 L * *", "60 0 * * *", "0 24 * * *", "0 0 0 * *", "0 0 * 13 *", "0 0 * * 8",
            "*/2 * * * *", "0,3 9 * * *", "0,58 8,9 * * *", "0 0 * * funday", "0 0 1#2 * *", "5-1 0 * * *"] {
            assert!(parse_cron(bad).is_err(), "{bad}");
        }
        assert!(parse_cron("0,58 8,10 * * *").is_ok(), "hours that are not consecutive do not wrap");
        assert!(parse_cron("*/5 * * * *").is_ok());
    }

    #[test]
    fn at_and_days_are_sugar_for_cron() {
        assert_eq!(at_to_cron("08:30", "mon-fri").unwrap(), "30 8 * * 1-5");
        assert_eq!(parse_cron(&at_to_cron("08:30", "mon-fri").unwrap()), parse_cron("30 8 * * 1-5"));
        assert_eq!(at_to_cron("7:05", "").unwrap(), "5 7 * * *");
        assert_eq!(at_to_cron("09:00", "mon,thu").unwrap(), "0 9 * * 1,4");
        assert_eq!(at_to_cron("09:00", "weekdays").unwrap(), "0 9 * * 1-5");
        for bad in [("25:00", ""), ("9", ""), ("09:60", ""), ("09:00", "someday")] {
            assert!(at_to_cron(bad.0, bad.1).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn next_wall_time_crosses_month_end_leap_day_and_weekends() {
        let c = parse_cron("30 8 * * *").unwrap();
        assert_eq!(c.next_after(wall(2026, 1, 31, 9, 0)), Some(wall(2026, 2, 1, 8, 30)));
        assert_eq!(c.next_after(wall(2026, 1, 31, 8, 29)), Some(wall(2026, 1, 31, 8, 30)));
        assert_eq!(c.next_after(wall(2026, 1, 31, 8, 30)), Some(wall(2026, 2, 1, 8, 30)), "strictly after");
        let leap = parse_cron("0 12 29 2 *").unwrap();
        assert_eq!(leap.next_after(wall(2026, 3, 1, 0, 0)), Some(wall(2028, 2, 29, 12, 0)));
        let weekdays = parse_cron("30 8 * * 1-5").unwrap();
        assert_eq!(weekdays.next_after(wall(2026, 11, 27, 9, 0)), Some(wall(2026, 11, 30, 8, 30)), "friday after 9 -> monday");
        let either = parse_cron("0 9 1 * mon").unwrap();
        assert_eq!(either.next_after(wall(2026, 11, 27, 10, 0)), Some(wall(2026, 11, 30, 9, 0)), "day-of-month or weekday");
    }

    #[test]
    fn intervals_stay_on_their_anchor() {
        let anchor = 1_000 * MINUTE_MS;
        let every = 30 * MINUTE_MS;
        assert_eq!(next_every(anchor, every, anchor + 7 * MINUTE_MS), anchor + every, "a 7 minute run does not shift the next due");
        assert_eq!(next_every(anchor, every, anchor + every), anchor + 2 * every);
        assert_eq!(next_every(anchor, every, anchor - 1), anchor);
        assert!(parse_every("4m").is_err() && parse_every("8d").is_err());
        assert_eq!(parse_every("30m"), Ok(30 * MINUTE_MS));
    }

    #[test]
    fn human_text_is_generated() {
        assert_eq!(cron_human(&parse_cron("30 8 * * 1-5").unwrap()), "Weekdays at 08:30");
        assert_eq!(cron_human(&parse_cron("0 9 * * *").unwrap()), "Every day at 09:00");
        assert_eq!(cron_human(&parse_cron("0 9 * * 1,4").unwrap()), "On mon, thu at 09:00");
        assert_eq!(every_human(30 * MINUTE_MS), "Every 30 min");
        assert_eq!(every_human(2 * HOUR_MS), "Every 2 h");
        assert_eq!(wall_human(wall(2026, 11, 26, 7, 5)), "2026-11-26 07:05");
    }
}
