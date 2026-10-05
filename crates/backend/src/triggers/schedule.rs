//! Schedule config parsing and next-run calculation.
//!
//! Supported config shapes:
//! ```json
//! { "cron": "*/5 * * * *", "timezone": "UTC" }
//! { "schedule": "*/5 * * * *", "timezone": "UTC" }
//! { "every": { "seconds": 10 }, "timezone": "UTC" }
//! { "every": { "minutes": 5 }, "timezone": "UTC" }
//! { "every": { "hours": 1 }, "timezone": "UTC" }
//! { "daily_at": "09:00", "timezone": "Europe/Moscow" }
//! { "after_completion": { "seconds": 30 }, "on_overlap": "skip" }
//! ```

use chrono::{DateTime, Duration, NaiveTime, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use croner::Cron;
use serde_json::Value;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapPolicy {
    Skip,
    Queue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSchedule {
    pub cron: String,
    pub timezone: String,
}

pub fn parse_overlap(config: &Value) -> OverlapPolicy {
    match config.get("on_overlap").and_then(Value::as_str) {
        Some("queue") => OverlapPolicy::Queue,
        _ => OverlapPolicy::Skip,
    }
}

/// Delay in seconds after a successful execution, if configured.
pub fn parse_after_completion_secs(config: &Value) -> Option<u64> {
    let ac = config.get("after_completion")?;
    if let Some(s) = ac.as_u64() {
        return Some(s.max(1));
    }
    if let Some(s) = ac.as_str().and_then(|v| v.parse::<u64>().ok()) {
        return Some(s.max(1));
    }
    if let Some(s) = ac.get("seconds").and_then(Value::as_u64) {
        return Some(s.max(1));
    }
    if let Some(m) = ac.get("minutes").and_then(Value::as_u64) {
        return Some(m.max(1).saturating_mul(60));
    }
    None
}

fn has_interval_fields(config: &Value) -> bool {
    config.get("cron").is_some()
        || config.get("schedule").is_some()
        || config.get("every").is_some()
        || config.get("daily_at").is_some()
}

pub fn parse_schedule_config(config: &Value) -> Result<ParsedSchedule, String> {
    let timezone = config
        .get("timezone")
        .and_then(Value::as_str)
        .unwrap_or("UTC")
        .to_string();

    if parse_after_completion_secs(config).is_some() && !has_interval_fields(config) {
        return Ok(ParsedSchedule {
            cron: "@after_completion".into(),
            timezone,
        });
    }

    if let Some(cron) = config
        .get("cron")
        .or_else(|| config.get("schedule"))
        .and_then(Value::as_str)
    {
        validate_cron(cron)?;
        return Ok(ParsedSchedule {
            cron: cron.to_string(),
            timezone,
        });
    }

    if let Some(every) = config.get("every") {
        let cron = if let Some(secs) = every.get("seconds").and_then(Value::as_u64) {
            if secs == 0 || secs > 3600 {
                return Err("`every.seconds` must be 1..=3600".into());
            }
            format!("@every {secs}s")
        } else if let Some(mins) = every.get("minutes").and_then(Value::as_u64) {
            if mins == 0 || mins > 59 {
                return Err("`every.minutes` must be 1..=59".into());
            }
            format!("*/{mins} * * * *")
        } else if let Some(hours) = every.get("hours").and_then(Value::as_u64) {
            if hours == 0 || hours > 23 {
                return Err("`every.hours` must be 1..=23".into());
            }
            if hours == 1 {
                "0 * * * *".to_string()
            } else {
                format!("0 */{hours} * * *")
            }
        } else {
            return Err("`every` requires `seconds`, `minutes` or `hours`".into());
        };
        if !cron.starts_with("@every ") {
            validate_cron(&cron)?;
        }
        return Ok(ParsedSchedule { cron, timezone });
    }

    if let Some(daily) = config.get("daily_at").and_then(Value::as_str) {
        let time = NaiveTime::parse_from_str(daily, "%H:%M")
            .or_else(|_| NaiveTime::parse_from_str(daily, "%H:%M:%S"))
            .map_err(|_| format!("invalid daily_at `{daily}`, expected HH:MM"))?;
        let cron = format!("{} {} * * *", time.minute(), time.hour());
        validate_cron(&cron)?;
        return Ok(ParsedSchedule { cron, timezone });
    }

    Err("trigger.schedule: provide `cron`/`schedule`, `every`, or `daily_at`".into())
}

fn validate_cron(expr: &str) -> Result<(), String> {
    Cron::from_str(expr).map_err(|e| format!("invalid cron `{expr}`: {e}"))?;
    Ok(())
}

/// Compute the next run after `from` (exclusive) in UTC.
pub fn next_run_after(
    cron_expr: &str,
    timezone: &str,
    from: DateTime<Utc>,
) -> Result<DateTime<Utc>, String> {
    if cron_expr == "@after_completion" {
        return Err("after_completion is owned by the runtime supervisor".into());
    }
    if let Some(spec) = cron_expr.strip_prefix("@every ") {
        let dur = parse_every_spec(spec)?;
        return Ok(from + dur);
    }
    let cron = Cron::from_str(cron_expr).map_err(|e| format!("invalid cron: {e}"))?;

    if timezone.eq_ignore_ascii_case("UTC") || timezone == "Etc/UTC" {
        let next = cron
            .find_next_occurrence(&from, false)
            .map_err(|e| format!("next occurrence: {e}"))?;
        return Ok(next.with_timezone(&Utc));
    }

    let tz: Tz = timezone
        .parse()
        .map_err(|_| format!("unknown timezone `{timezone}`"))?;
    let local = from.with_timezone(&tz);
    let next_local = cron
        .find_next_occurrence(&local, false)
        .map_err(|e| format!("next occurrence: {e}"))?;
    Ok(next_local.with_timezone(&Utc))
}

/// Extract schedule nodes from a workflow definition.
pub fn schedules_from_definition(
    definition: &boarddo_shared::WorkflowDefinition,
) -> Result<Vec<(String, ParsedSchedule)>, Vec<String>> {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for node in &definition.nodes {
        if node.type_id != boarddo_shared::type_ids::TRIGGER_SCHEDULE {
            continue;
        }
        match parse_schedule_config(&node.config) {
            Ok(parsed) if parsed.cron == "@after_completion" => {}
            Ok(parsed) => out.push((node.id.clone(), parsed)),
            Err(e) => errors.push(format!("node `{}`: {e}", node.id)),
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_cron() {
        let p = parse_schedule_config(&json!({"cron": "*/5 * * * *"})).unwrap();
        assert_eq!(p.cron, "*/5 * * * *");
    }

    #[test]
    fn parse_every_minutes() {
        let p = parse_schedule_config(&json!({"every": {"minutes": 5}})).unwrap();
        assert_eq!(p.cron, "*/5 * * * *");
    }

    #[test]
    fn parse_daily() {
        let p = parse_schedule_config(&json!({"daily_at": "09:30", "timezone": "UTC"})).unwrap();
        assert_eq!(p.cron, "30 9 * * *");
    }

    #[test]
    fn next_run_future() {
        let from = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let next = next_run_after("*/5 * * * *", "UTC", from).unwrap();
        assert!(next > from);
        assert_eq!(next.minute() % 5, 0);
    }

    #[test]
    fn parse_every_seconds() {
        let p = parse_schedule_config(&json!({"every": {"seconds": 10}})).unwrap();
        assert_eq!(p.cron, "@every 10s");
        let from = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let next = next_run_after(&p.cron, "UTC", from).unwrap();
        assert_eq!(next, from + Duration::seconds(10));
    }

    #[test]
    fn parse_after_completion() {
        let cfg = json!({"after_completion": {"seconds": 30}, "on_overlap": "skip"});
        assert_eq!(parse_after_completion_secs(&cfg), Some(30));
        assert_eq!(parse_overlap(&cfg), OverlapPolicy::Skip);
        let p = parse_schedule_config(&cfg).unwrap();
        assert_eq!(p.cron, "@after_completion");
    }
}

fn parse_every_spec(spec: &str) -> Result<Duration, String> {
    let spec = spec.trim();
    let (num, unit) = spec.split_at(spec.len().saturating_sub(1));
    let n: i64 = num
        .trim()
        .parse()
        .map_err(|_| format!("invalid @every `{spec}`"))?;
    if n <= 0 {
        return Err("@every duration must be > 0".into());
    }
    match unit {
        "s" => Ok(Duration::seconds(n)),
        "m" => Ok(Duration::minutes(n)),
        "h" => Ok(Duration::hours(n)),
        _ => Err(format!("invalid @every unit in `{spec}`")),
    }
}
