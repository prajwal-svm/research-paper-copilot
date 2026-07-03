//! Opt-in, content-free, local-only instrumentation (tasks 8.3/8.4).
//!
//! Everything stays on the user's machine in the app data directory; nothing
//! is transmitted anywhere in v1. Events are bare kind+timestamp — never
//! paper content, notes, queries, or identifiers. Disabled by default.
//!
//! Crash-free sessions: `session_start` is recorded at launch and
//! `session_end` at clean shutdown; a start without a matching end counts as
//! a crashed session in the summary.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Allowed event kinds — a closed set so content can't leak by construction.
pub const EVENT_KINDS: [&str; 21] = [
    "session_start",
    "session_end",
    "first_launch",
    "first_object_interaction", // time-to-first-wow = this minus first_launch
    "object_interaction",
    "ai_answer_completed",
    "answer_thumbs_up",
    "answer_thumbs_down",
    "quiz_answered",          // v2: quiz participation
    "explanation_repeated",   // v2: same action re-asked on one object (confusion signal)
    "implementation_run",     // v3: PRD success metric — runs/month
    "experiment_run",         // v3: PRD success metric — runs/month
    "reproduction_attempted", // v3: PRD success metric — attempts
    "reproduction_completed", // v3: PRD success metric — report produced
    "review_generated",       // v4: PRD quality proxy — generated → edited
    "review_edited",          //   → exported, ≥50% edited-not-regenerated
    "review_exported",        // v4
    "gap_report_generated",   // v4
    "draft_exported",         // v4
    "sync_completed",         // sync: runs (content-free count)
    "sync_conflict_created",  // sync: conflict copies encountered
];

/// Allowed numeric-measurement kinds (closed set; values are numbers only,
/// so these stay content-free by construction like [`EVENT_KINDS`]).
pub const VALUE_KINDS: [&str; 1] = [
    "prompt_tokens_approx", // context-efficiency check (v2 ≥60% reduction target)
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Event {
    kind: String,
    at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Settings {
    enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySummary {
    pub enabled: bool,
    pub sessions: usize,
    pub clean_sessions: usize,
    /// Seconds from first launch to first object interaction, when both seen.
    pub time_to_first_wow_secs: Option<i64>,
    pub object_interactions: usize,
    pub answers: usize,
    pub thumbs_up: usize,
    pub thumbs_down: usize,
}

pub struct Telemetry {
    dir: PathBuf,
}

impl Telemetry {
    pub fn open(dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        Ok(Telemetry {
            dir: dir.to_path_buf(),
        })
    }

    fn settings_path(&self) -> PathBuf {
        self.dir.join("telemetry.json")
    }

    fn events_path(&self) -> PathBuf {
        self.dir.join("events.jsonl")
    }

    pub fn enabled(&self) -> bool {
        std::fs::read(self.settings_path())
            .ok()
            .and_then(|b| serde_json::from_slice::<Settings>(&b).ok())
            .map(|s| s.enabled)
            .unwrap_or(false) // opt-IN: off until the user turns it on
    }

    pub fn set_enabled(&self, enabled: bool) -> std::io::Result<()> {
        std::fs::write(
            self.settings_path(),
            serde_json::to_vec_pretty(&Settings { enabled }).unwrap(),
        )
    }

    /// Record an event. Unknown kinds are rejected (content-free by
    /// construction); disabled telemetry is a silent no-op.
    pub fn record(&self, kind: &str) -> std::io::Result<()> {
        if !EVENT_KINDS.contains(&kind) || !self.enabled() {
            return Ok(());
        }
        // First-launch and first-wow are once-ever events.
        if kind.starts_with("first_") && self.has_event(kind) {
            return Ok(());
        }
        self.write_event(Event {
            kind: kind.to_string(),
            at: crate::bundle::now_rfc3339(),
            value: None,
        })
    }

    /// Record a numeric measurement (e.g. approximate prompt tokens).
    /// Same rules: closed kind set, opt-in, local-only.
    pub fn record_value(&self, kind: &str, value: i64) -> std::io::Result<()> {
        if !VALUE_KINDS.contains(&kind) || !self.enabled() {
            return Ok(());
        }
        self.write_event(Event {
            kind: kind.to_string(),
            at: crate::bundle::now_rfc3339(),
            value: Some(value),
        })
    }

    fn write_event(&self, event: Event) -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.events_path())?;
        writeln!(file, "{}", serde_json::to_string(&event).unwrap())
    }

    fn events(&self) -> Vec<Event> {
        std::fs::read_to_string(self.events_path())
            .map(|s| {
                s.lines()
                    .filter_map(|l| serde_json::from_str(l).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn has_event(&self, kind: &str) -> bool {
        self.events().iter().any(|e| e.kind == kind)
    }

    pub fn summary(&self) -> TelemetrySummary {
        let events = self.events();
        let count = |kind: &str| events.iter().filter(|e| e.kind == kind).count();
        let first_at = |kind: &str| -> Option<time::OffsetDateTime> {
            events.iter().find(|e| e.kind == kind).and_then(|e| {
                time::OffsetDateTime::parse(&e.at, &time::format_description::well_known::Rfc3339)
                    .ok()
            })
        };
        let time_to_first_wow_secs = match (
            first_at("first_launch"),
            first_at("first_object_interaction"),
        ) {
            (Some(launch), Some(wow)) => Some((wow - launch).whole_seconds()),
            _ => None,
        };
        TelemetrySummary {
            enabled: self.enabled(),
            sessions: count("session_start"),
            clean_sessions: count("session_end"),
            time_to_first_wow_secs,
            object_interactions: count("object_interaction"),
            answers: count("ai_answer_completed"),
            thumbs_up: count("answer_thumbs_up"),
            thumbs_down: count("answer_thumbs_down"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_by_default_and_records_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::open(tmp.path()).unwrap();
        assert!(!telemetry.enabled());
        telemetry.record("object_interaction").unwrap();
        assert_eq!(telemetry.summary().object_interactions, 0);
    }

    #[test]
    fn records_only_known_kinds_when_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::open(tmp.path()).unwrap();
        telemetry.set_enabled(true).unwrap();

        telemetry.record("object_interaction").unwrap();
        telemetry.record("object_interaction").unwrap();
        telemetry.record("paper content leak!!").unwrap(); // rejected
        telemetry.record("answer_thumbs_up").unwrap();

        let summary = telemetry.summary();
        assert_eq!(summary.object_interactions, 2);
        assert_eq!(summary.thumbs_up, 1);
        // Raw file holds nothing but known kinds + timestamps.
        let raw = std::fs::read_to_string(tmp.path().join("events.jsonl")).unwrap();
        assert!(!raw.contains("leak"));
    }

    #[test]
    fn crash_free_sessions_and_first_wow() {
        let tmp = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::open(tmp.path()).unwrap();
        telemetry.set_enabled(true).unwrap();

        telemetry.record("first_launch").unwrap();
        telemetry.record("session_start").unwrap();
        telemetry.record("first_object_interaction").unwrap();
        telemetry.record("first_object_interaction").unwrap(); // once-ever
        telemetry.record("session_end").unwrap();
        telemetry.record("session_start").unwrap(); // crashed (no end)

        let summary = telemetry.summary();
        assert_eq!(summary.sessions, 2);
        assert_eq!(summary.clean_sessions, 1);
        assert!(summary.time_to_first_wow_secs.is_some());
        assert!(summary.time_to_first_wow_secs.unwrap() >= 0);
    }
}
