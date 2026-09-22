use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveDateTime, TimeZone};
use clap::Parser;
use flate2::read::GzDecoder;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const CONSENT_EVENT: &str = "CONSENT_GUARDRAIL";

#[derive(Debug, Default, PartialEq, Eq)]
struct GateTotals {
    fired: u64,
    approved: u64,
    denied: u64,
}

/// Read-only report of actual structured consent events from the local daily
/// logs. A cached boundary reuse is deliberately excluded: it is evidence of
/// a prior approval, not another prompt firing.
pub(crate) fn last_seven_days_report() -> Result<String> {
    let now = Local::now();
    report_from_dir(&crate::storage::logs_dir()?, now)
}

pub(crate) fn print_last_seven_days_report() -> Result<()> {
    print!("{}", last_seven_days_report()?);
    Ok(())
}

pub(crate) fn run_if_requested() -> Result<bool> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments
        .iter()
        .any(|argument| matches!(argument.to_str(), Some("--help" | "-h")))
    {
        return Ok(false);
    }
    if !super::args::Args::try_parse_from(
        std::iter::once(std::ffi::OsString::from("jcode")).chain(arguments),
    )
    .is_ok_and(|args| matches!(args.command, Some(super::args::Command::GateTelemetry)))
    {
        return Ok(false);
    }
    print_last_seven_days_report()?;
    Ok(true)
}

fn report_from_dir(log_dir: &Path, now: DateTime<Local>) -> Result<String> {
    let start = now - Duration::days(7);
    let mut totals = BTreeMap::<String, GateTotals>::new();
    let entries = match fs::read_dir(log_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(empty_report(start, now));
        }
        Err(error) => return Err(error.into()),
    };

    let mut logs = BTreeMap::<String, (bool, PathBuf)>::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Some((logical_name, compressed)) = jcode_log_name(&entry.file_name()) else {
            continue;
        };
        let path = entry.path();
        if logs
            .get(&logical_name)
            .is_none_or(|(selected_compressed, _)| *selected_compressed && !compressed)
        {
            logs.insert(logical_name, (compressed, path));
        }
    }

    for (_, (compressed, path)) in logs {
        if compressed {
            let reader = BufReader::new(GzDecoder::new(fs::File::open(path)?));
            read_log_lines(reader, start, now, &mut totals)?;
        } else {
            let reader = BufReader::new(fs::File::open(path)?);
            read_log_lines(reader, start, now, &mut totals)?;
        }
    }

    if totals.values().all(|total| total == &GateTotals::default()) {
        return Ok(empty_report(start, now));
    }

    let mut report = format!(
        "Consent gate telemetry, actual logs from {} through {}\n\nclass                 fired  approved  denied\n",
        start.to_rfc3339(),
        now.to_rfc3339(),
    );
    for (class, total) in totals {
        report.push_str(&format!(
            "{class:<21} {:>5}  {:>8}  {:>6}\n",
            total.fired, total.approved, total.denied
        ));
    }
    Ok(report)
}

fn read_log_lines(
    reader: impl BufRead,
    start: DateTime<Local>,
    now: DateTime<Local>,
    totals: &mut BTreeMap<String, GateTotals>,
) -> Result<()> {
    for line in reader.lines() {
        let line = line?;
        let Some(at) = timestamp_from_line(&line) else {
            continue;
        };
        if at < start || at > now {
            continue;
        }
        let Some((class, decision)) = consent_event_fields(&line) else {
            continue;
        };
        let class_totals = totals.entry(class).or_default();
        match decision.as_str() {
            "ask" => class_totals.fired += 1,
            "approved" | "allow" => class_totals.approved += 1,
            "denied" | "deny" => class_totals.denied += 1,
            // `reuse`, `block`, timeout and disconnect are not a fired
            // prompt or an affirmative/negative user decision.
            _ => {}
        }
    }
    Ok(())
}

fn empty_report(start: DateTime<Local>, now: DateTime<Local>) -> String {
    format!(
        "Consent gate telemetry, actual logs from {} through {}\n\nNo consent gate prompts or approval decisions were recorded in this window.\n",
        start.to_rfc3339(),
        now.to_rfc3339(),
    )
}

fn jcode_log_name(name: &std::ffi::OsStr) -> Option<(String, bool)> {
    let name = name.to_str()?;
    let (logical_name, compressed) = name
        .strip_suffix(".log.gz")
        .map(|name| (name, true))
        .or_else(|| name.strip_suffix(".log").map(|name| (name, false)))?;
    logical_name
        .starts_with("jcode-")
        .then(|| (logical_name.to_string(), compressed))
}

fn timestamp_from_line(line: &str) -> Option<DateTime<Local>> {
    let timestamp = line.strip_prefix('[')?.split_once(']')?.0;
    let timestamp = NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S%.f").ok()?;
    Local.from_local_datetime(&timestamp).earliest()
}

fn consent_event_fields(line: &str) -> Option<(String, String)> {
    if let Some(json) = line.split_once("EVENT_JSON ").map(|(_, json)| json) {
        let value: Value = serde_json::from_str(json).ok()?;
        if value.get("event")?.as_str()? != CONSENT_EVENT {
            return None;
        }
        return Some((
            value
                .get("class")
                .or_else(|| value.get("family"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            value.get("decision")?.as_str()?.to_string(),
        ));
    }

    let event = text_field(line, "event")?;
    if event != CONSENT_EVENT {
        return None;
    }
    Some((
        text_field(line, "class")
            .or_else(|| text_field(line, "family"))
            .unwrap_or_else(|| "unknown".to_string()),
        text_field(line, "decision")?,
    ))
}

fn text_field(line: &str, key: &str) -> Option<String> {
    line.split_whitespace()
        .find_map(|field| field.strip_prefix(&format!("{key}=")).map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_now() -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 9, 6, 14, 0, 0)
            .earliest()
            .expect("test local time")
    }

    #[test]
    fn read_only_startup_selector_accepts_only_the_exact_command() {
        let matches = |arguments: &[&str]| {
            !arguments
                .iter()
                .any(|argument| matches!(*argument, "--help" | "-h"))
                && super::super::args::Args::try_parse_from(
                    std::iter::once(std::ffi::OsString::from("jcode"))
                        .chain(arguments.iter().map(std::ffi::OsString::from)),
                )
                .is_ok_and(|args| {
                    matches!(
                        args.command,
                        Some(super::super::args::Command::GateTelemetry)
                    )
                })
        };
        assert!(matches(&["--quiet", "gate-telemetry"]));
        assert!(!matches(&["gate-telemetry", "--help"]));
        assert!(!matches(&["run", "gate-telemetry"]));
    }

    #[test]
    fn seven_day_report_counts_actual_prompts_and_decisions_but_not_reuse() {
        let logs = tempfile::tempdir().expect("temporary logs");
        fs::write(
            logs.path().join("jcode-2026-09-06.log"),
            concat!(
                "[2026-09-06 12:00:00.000] [INFO] EVENT event=CONSENT_GUARDRAIL class=browser decision=ask\n",
                "[2026-09-06 12:00:01.000] [INFO] EVENT event=CONSENT_GUARDRAIL class=browser decision=approved\n",
                "[2026-09-06 12:00:02.000] [INFO] EVENT event=CONSENT_GUARDRAIL class=browser decision=reuse\n",
                "[2026-09-06 12:01:00.000] [INFO] EVENT event=CONSENT_GUARDRAIL class=applescript decision=ask\n",
                "[2026-09-06 12:01:01.000] [INFO] EVENT event=CONSENT_GUARDRAIL class=applescript decision=denied\n",
                "[2026-08-30 13:59:59.000] [INFO] EVENT event=CONSENT_GUARDRAIL class=browser decision=ask\n",
            ),
        )
        .expect("fixture log");

        let report = report_from_dir(logs.path(), fixture_now()).expect("report");
        assert!(report.contains("applescript"));
        assert!(report.contains("browser"));
        assert!(report.contains("browser                   1         1       0"));
        assert!(report.contains("applescript               1         0       1"));
    }

    #[test]
    fn seven_day_report_supports_json_events_and_empty_windows() {
        let logs = tempfile::tempdir().expect("temporary logs");
        fs::write(
            logs.path().join("jcode-2026-09-06.log"),
            "[2026-09-06 12:00:00.000] [INFO] EVENT_JSON {\"event\":\"CONSENT_GUARDRAIL\",\"family\":\"legacy\",\"decision\":\"allow\"}\n",
        )
        .expect("json fixture log");
        let report = report_from_dir(logs.path(), fixture_now()).expect("json report");
        assert!(report.contains("legacy                    0         1       0"));

        let empty = tempfile::tempdir().expect("empty logs");
        assert!(
            report_from_dir(empty.path(), fixture_now())
                .expect("empty report")
                .contains("No consent gate prompts or approval decisions")
        );
    }
}
