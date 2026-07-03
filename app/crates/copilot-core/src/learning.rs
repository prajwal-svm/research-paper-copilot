//! Learner memory (v2): three event-sourced stores under the library-level
//! `learning_state/` directory —
//!
//!   mastery.jsonl      per-concept quiz/tutor outcomes (SM-2-family curve)
//!   preferences.jsonl  learning-style signals (style, verbosity)
//!   episodes.jsonl     per-object confusion/insight summaries
//!
//! All three are append-only JSONL journals (same crash-safety and
//! sync-mergeability as notes/chats), folded into `snapshot.json` at read.
//! Spaced-repetition decay is computed at fold time from timestamps — no
//! background scheduler. The whole directory is the privacy boundary:
//! local-only, deletable per store or wholesale.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::bundle::Journal;

pub const LEARNING_STATE_DIR: &str = "learning_state";

/// Below this many recorded signals a concept's mastery is an estimate —
/// surfaced as such, never used to gate content.
pub const ESTIMATE_THRESHOLD: u32 = 3;

// ---------------------------------------------------------------------------
// Events (journal entries)
// ---------------------------------------------------------------------------

/// One learning interaction outcome for a concept. `quality` follows the
/// SM-2 scale: 0–2 = failed recall, 3–5 = successful (5 = effortless).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasteryEvent {
    pub concept: Uuid,
    /// Paper object the interaction was anchored to, when there was one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<Uuid>,
    pub quality: u8,
    /// "quiz" | "flashcard" | "tutor"
    pub source: String,
    pub at: String,
}

/// A learning-style signal, latest-wins per key (e.g. key "style" →
/// "visual" | "code" | "formal"; key "verbosity" → "terse" | "detailed").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceEvent {
    pub key: String,
    pub value: String,
    pub at: String,
}

/// A summarized confusion or insight tied to an object/concept — episodic
/// memory feeding future context assembly ("last time you confused X and Y").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeEvent {
    pub paper_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concept: Option<Uuid>,
    /// "confusion" | "insight"
    pub kind: String,
    pub summary: String,
    /// Chat turns covered when this summary was generated — the lazy
    /// summarizer's cache key (no re-summarizing an unchanged thread).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub covered_turns: Option<u32>,
    pub at: String,
}

// ---------------------------------------------------------------------------
// Folded snapshot
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptMastery {
    pub concept: Uuid,
    /// Retention estimate in [0, 1] at fold time (decayed since last review).
    pub score: f32,
    /// Recorded signals; below [`ESTIMATE_THRESHOLD`] the score is an estimate.
    pub signals: u32,
    pub estimated: bool,
    /// SM-2 state, carried for the next update.
    pub ease: f32,
    pub interval_days: f32,
    pub repetitions: u32,
    pub last_review: String,
    /// Review is due when the interval has fully elapsed.
    pub due: bool,
    /// Consecutive failed attempts (signals "change explanatory approach").
    pub consecutive_failures: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LearnerSnapshot {
    pub mastery: Vec<ConceptMastery>,
    /// Latest value per preference key.
    pub preferences: HashMap<String, String>,
    /// Episode count (episodes themselves are read per-object, not folded).
    pub episodes: u32,
    pub folded_at: String,
}

impl LearnerSnapshot {
    pub fn mastery_of(&self, concept: Uuid) -> Option<&ConceptMastery> {
        self.mastery.iter().find(|m| m.concept == concept)
    }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum LearningError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error("learning state: {0}")]
    Io(#[from] std::io::Error),
}

/// Handle on the library's `learning_state/` directory.
pub struct LearnerModel {
    root: PathBuf,
}

impl LearnerModel {
    pub fn open(library_root: &Path) -> LearnerModel {
        LearnerModel {
            root: library_root.join(LEARNING_STATE_DIR),
        }
    }

    fn journal(&self, file: &str) -> Journal {
        Journal::at(self.root.join(file))
    }

    pub fn record_mastery(&self, event: &MasteryEvent) -> Result<(), LearningError> {
        self.journal("mastery.jsonl").append(event)?;
        Ok(())
    }

    pub fn record_preference(&self, event: &PreferenceEvent) -> Result<(), LearningError> {
        self.journal("preferences.jsonl").append(event)?;
        Ok(())
    }

    pub fn record_episode(&self, event: &EpisodeEvent) -> Result<(), LearningError> {
        self.journal("episodes.jsonl").append(event)?;
        Ok(())
    }

    /// Episodes for one object, oldest first (context assembly reads these).
    pub fn episodes_for(&self, object: Uuid) -> Result<Vec<EpisodeEvent>, LearningError> {
        let all: Vec<EpisodeEvent> = self.journal("episodes.jsonl").read_all()?;
        Ok(all
            .into_iter()
            .filter(|e| e.object == Some(object))
            .collect())
    }

    /// Fold all journals into a snapshot (decay computed against `now`) and
    /// persist it to `snapshot.json` for cheap dashboard reads. The snapshot
    /// is a derived cache: journals stay the source of truth.
    pub fn snapshot(&self) -> Result<LearnerSnapshot, LearningError> {
        self.snapshot_at(OffsetDateTime::now_utc())
    }

    pub fn snapshot_at(&self, now: OffsetDateTime) -> Result<LearnerSnapshot, LearningError> {
        let mastery_events: Vec<MasteryEvent> = self.journal("mastery.jsonl").read_all()?;
        let preference_events: Vec<PreferenceEvent> =
            self.journal("preferences.jsonl").read_all()?;
        let episode_events: Vec<EpisodeEvent> = self.journal("episodes.jsonl").read_all()?;

        let mut states: HashMap<Uuid, Sm2State> = HashMap::new();
        for event in &mastery_events {
            states.entry(event.concept).or_default().update(event);
        }
        let mut mastery: Vec<ConceptMastery> = states
            .into_iter()
            .map(|(concept, state)| state.into_mastery(concept, now))
            .collect();
        mastery.sort_by(|a, b| a.concept.cmp(&b.concept));

        let mut preferences = HashMap::new();
        for event in preference_events {
            preferences.insert(event.key, event.value); // journal order → latest wins
        }

        let snapshot = LearnerSnapshot {
            mastery,
            preferences,
            episodes: episode_events.len() as u32,
            folded_at: now
                .format(&Rfc3339)
                .unwrap_or_else(|_| crate::bundle::now_rfc3339()),
        };

        // Atomic cache write; a failure here never loses journal data.
        if let Ok(json) = serde_json::to_vec_pretty(&snapshot) {
            let tmp = self.root.join("snapshot.json.tmp");
            std::fs::create_dir_all(&self.root)?;
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, self.root.join("snapshot.json"));
            }
        }
        Ok(snapshot)
    }

    /// Delete one store's journal (snapshot refolds without it).
    pub fn reset_store(&self, store: &str) -> Result<(), LearningError> {
        let file = match store {
            "mastery" => "mastery.jsonl",
            "preferences" => "preferences.jsonl",
            "episodes" => "episodes.jsonl",
            other => {
                return Err(LearningError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("unknown learning store: {other}"),
                )))
            }
        };
        let path = self.root.join(file);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        let _ = std::fs::remove_file(self.root.join("snapshot.json"));
        Ok(())
    }

    /// Wholesale reset: remove `learning_state/` entirely. Touches nothing
    /// else (papers, notes, chats live elsewhere).
    pub fn reset_all(&self) -> Result<(), LearningError> {
        if self.root.exists() {
            std::fs::remove_dir_all(&self.root)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Learner-profile block (v2 task 2.3): compact ids/levels/style for prompts
// ---------------------------------------------------------------------------

/// Mastery score above which a concept counts as mastered for prompting.
pub const MASTERED_SCORE: f32 = 0.6;
/// Consecutive failures at which prompts should change explanatory approach.
pub const STRUGGLE_FAILURES: u32 = 3;

/// Compact learner-profile block for prompt context: concept levels and
/// style preferences only — ids resolve to names via `names` (from the
/// paper's graph) so the block never carries transcripts or quiz content.
/// `None` when there's nothing worth telling the model (cold start).
pub fn profile_block(snapshot: &LearnerSnapshot, names: &HashMap<Uuid, String>) -> Option<String> {
    let label = |id: Uuid| names.get(&id).cloned().unwrap_or_else(|| id.to_string());

    let mastered: Vec<String> = snapshot
        .mastery
        .iter()
        .filter(|m| !m.estimated && m.score >= MASTERED_SCORE)
        .map(|m| label(m.concept))
        .collect();
    let struggling: Vec<String> = snapshot
        .mastery
        .iter()
        .filter(|m| m.consecutive_failures >= STRUGGLE_FAILURES)
        .map(|m| label(m.concept))
        .collect();

    let mut lines = Vec::new();
    if !snapshot.preferences.is_empty() {
        let mut prefs: Vec<String> = snapshot
            .preferences
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        prefs.sort();
        lines.push(format!("Preferences: {}.", prefs.join(", ")));
    }
    if !mastered.is_empty() {
        lines.push(format!(
            "Already mastered (reference briefly, do not re-teach): {}.",
            mastered.join(", ")
        ));
    }
    if !struggling.is_empty() {
        lines.push(format!(
            "Struggling repeatedly with (change approach: new analogy, smaller steps): {}.",
            struggling.join(", ")
        ));
    }
    if lines.is_empty() {
        return None;
    }
    Some(format!("Learner profile:\n{}", lines.join("\n")))
}

// ---------------------------------------------------------------------------
// Episodic summarizer (v2 task 2.2): lazy, cached, light-tier, no-key skip
// ---------------------------------------------------------------------------

/// Don't summarize threads shorter than this — nothing worth remembering.
const EPISODE_MIN_TURNS: u32 = 4;

/// Summarize an object's chat thread into one episodic memory event.
/// Lazy and cached: returns `Ok(None)` without an LLM call when the thread
/// is short, unchanged since the last summary, or the model returns nothing
/// (no-key skip — episodic memory is an enhancement, never a blocker).
pub fn summarize_episode(
    bundle: &crate::bundle::Bundle,
    model: &LearnerModel,
    paper_id: &str,
    object: Uuid,
    llm: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<EpisodeEvent>, LearningError> {
    let history = crate::chat::history(bundle, object)?;
    let turns = history.len() as u32;
    if turns < EPISODE_MIN_TURNS {
        return Ok(None);
    }
    let already_covered = model
        .episodes_for(object)?
        .iter()
        .filter_map(|e| e.covered_turns)
        .max()
        .unwrap_or(0);
    if turns <= already_covered {
        return Ok(None); // cached — thread unchanged since last summary
    }

    let transcript: String = history
        .iter()
        .map(|m| format!("{}: {}\n", m.role, m.content))
        .collect();
    let prompt = format!(
        "Below is a learner's conversation about one part of a research paper.\n\
         Summarize, in one or two sentences addressed to a future tutor, what the \
         learner struggled with or came to understand. Respond with JSON only:\n\
         {{\"kind\": \"confusion\" | \"insight\", \"summary\": \"...\"}}\n\n{transcript}"
    );
    let Some(raw) = llm(&prompt) else {
        return Ok(None); // no key / provider failure → skip silently
    };
    // Tolerate code fences and prose around the JSON object.
    let json = raw
        .find('{')
        .and_then(|start| raw.rfind('}').map(|end| &raw[start..=end]));
    let Some(parsed) = json.and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok()) else {
        return Ok(None);
    };
    let summary = parsed["summary"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_string();
    if summary.is_empty() {
        return Ok(None);
    }
    let kind = match parsed["kind"].as_str() {
        Some("insight") => "insight",
        _ => "confusion",
    };
    let event = EpisodeEvent {
        paper_id: paper_id.to_string(),
        object: Some(object),
        concept: None,
        kind: kind.to_string(),
        summary,
        covered_turns: Some(turns),
        at: crate::bundle::now_rfc3339(),
    };
    model.record_episode(&event)?;
    Ok(Some(event))
}

// ---------------------------------------------------------------------------
// SM-2-family scoring
// ---------------------------------------------------------------------------

struct Sm2State {
    ease: f32,
    interval_days: f32,
    repetitions: u32,
    signals: u32,
    consecutive_failures: u32,
    last_review: String,
}

impl Default for Sm2State {
    fn default() -> Self {
        Sm2State {
            ease: 2.5,
            interval_days: 0.0,
            repetitions: 0,
            signals: 0,
            consecutive_failures: 0,
            last_review: String::new(),
        }
    }
}

impl Sm2State {
    fn update(&mut self, event: &MasteryEvent) {
        let quality = event.quality.min(5) as f32;
        self.signals += 1;
        self.last_review = event.at.clone();
        if quality >= 3.0 {
            self.repetitions += 1;
            self.consecutive_failures = 0;
            self.interval_days = match self.repetitions {
                1 => 1.0,
                2 => 6.0,
                _ => self.interval_days * self.ease,
            };
            // SM-2 ease adjustment, floored at 1.3.
            self.ease =
                (self.ease + 0.1 - (5.0 - quality) * (0.08 + (5.0 - quality) * 0.02)).max(1.3);
        } else {
            self.repetitions = 0;
            self.consecutive_failures += 1;
            self.interval_days = 1.0;
        }
    }

    fn into_mastery(self, concept: Uuid, now: OffsetDateTime) -> ConceptMastery {
        let elapsed_days = OffsetDateTime::parse(&self.last_review, &Rfc3339)
            .map(|reviewed| ((now - reviewed).whole_seconds() as f32 / 86_400.0).max(0.0))
            .unwrap_or(0.0);
        // Exponential forgetting: retention halves roughly every interval.
        // Interval 0 (only failures so far) → score 0.
        let retention = if self.interval_days > 0.0 {
            (-elapsed_days * std::f32::consts::LN_2 / self.interval_days).exp()
        } else {
            0.0
        };
        // Confidence in the retention estimate grows with successful reps.
        let strength = (self.repetitions as f32 / 3.0).min(1.0);
        ConceptMastery {
            concept,
            score: retention * strength,
            signals: self.signals,
            estimated: self.signals < ESTIMATE_THRESHOLD,
            ease: self.ease,
            interval_days: self.interval_days,
            repetitions: self.repetitions,
            due: self.interval_days > 0.0 && elapsed_days >= self.interval_days,
            last_review: self.last_review,
            consecutive_failures: self.consecutive_failures,
        }
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn at(days_ago: f32) -> String {
        (OffsetDateTime::now_utc() - time::Duration::seconds_f32(days_ago * 86_400.0))
            .format(&Rfc3339)
            .unwrap()
    }

    fn mastery_event(concept: Uuid, quality: u8, when: String) -> MasteryEvent {
        MasteryEvent {
            concept,
            object: None,
            quality,
            source: "quiz".to_string(),
            at: when,
        }
    }

    #[test]
    fn fail_fail_pass_updates_score_and_interval() {
        let tmp = tempfile::tempdir().unwrap();
        let model = LearnerModel::open(tmp.path());
        let concept = Uuid::new_v4();
        // LayerNorm scenario from the spec: wrong twice, then right.
        model
            .record_mastery(&mastery_event(concept, 1, at(2.0)))
            .unwrap();
        model
            .record_mastery(&mastery_event(concept, 2, at(1.0)))
            .unwrap();
        model
            .record_mastery(&mastery_event(concept, 4, at(0.0)))
            .unwrap();

        let snapshot = model.snapshot().unwrap();
        let m = snapshot.mastery_of(concept).expect("concept folded");
        assert_eq!(m.signals, 3);
        assert!(!m.estimated, "3 signals reaches the estimate threshold");
        assert_eq!(m.repetitions, 1);
        assert!((m.interval_days - 1.0).abs() < f32::EPSILON);
        assert!(m.score > 0.0 && m.score < 1.0);
        assert_eq!(m.consecutive_failures, 0);
        // Snapshot persisted for cheap reads.
        assert!(tmp.path().join("learning_state/snapshot.json").is_file());
    }

    #[test]
    fn score_decays_over_elapsed_time() {
        let tmp = tempfile::tempdir().unwrap();
        let model = LearnerModel::open(tmp.path());
        let concept = Uuid::new_v4();
        model
            .record_mastery(&mastery_event(concept, 5, at(0.0)))
            .unwrap();

        let now = OffsetDateTime::now_utc();
        let fresh = model.snapshot_at(now).unwrap();
        let later = model.snapshot_at(now + time::Duration::days(10)).unwrap();
        let fresh_score = fresh.mastery_of(concept).unwrap().score;
        let later_score = later.mastery_of(concept).unwrap().score;
        assert!(later_score < fresh_score, "decay computed at read");
        assert!(
            later.mastery_of(concept).unwrap().due,
            "interval elapsed → due"
        );
        assert!(
            fresh.mastery_of(concept).unwrap().estimated,
            "1 signal is an estimate"
        );
    }

    #[test]
    fn repeated_failures_tracked_for_changed_approach() {
        let tmp = tempfile::tempdir().unwrap();
        let model = LearnerModel::open(tmp.path());
        let concept = Uuid::new_v4();
        for _ in 0..3 {
            model
                .record_mastery(&mastery_event(concept, 1, at(0.0)))
                .unwrap();
        }
        let snapshot = model.snapshot().unwrap();
        assert_eq!(
            snapshot.mastery_of(concept).unwrap().consecutive_failures,
            3
        );
        assert_eq!(snapshot.mastery_of(concept).unwrap().score, 0.0);
    }

    #[test]
    fn torn_write_discarded_committed_events_load() {
        let tmp = tempfile::tempdir().unwrap();
        let model = LearnerModel::open(tmp.path());
        let concept = Uuid::new_v4();
        model
            .record_mastery(&mastery_event(concept, 4, at(0.0)))
            .unwrap();
        // Simulate a crash mid-append: torn, unparseable trailing line.
        let path = tmp.path().join("learning_state/mastery.jsonl");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(br#"{"concept":"tor"#);
        std::fs::write(&path, bytes).unwrap();

        let snapshot = model.snapshot().unwrap();
        assert_eq!(snapshot.mastery_of(concept).unwrap().signals, 1);
    }

    #[test]
    fn preferences_latest_wins_and_episodes_filter_by_object() {
        let tmp = tempfile::tempdir().unwrap();
        let model = LearnerModel::open(tmp.path());
        for value in ["visual", "code"] {
            model
                .record_preference(&PreferenceEvent {
                    key: "style".to_string(),
                    value: value.to_string(),
                    at: at(0.0),
                })
                .unwrap();
        }
        let object = Uuid::new_v4();
        model
            .record_episode(&EpisodeEvent {
                paper_id: "p1".to_string(),
                object: Some(object),
                concept: None,
                kind: "confusion".to_string(),
                summary: "confused Q/K/V projections with head splitting".to_string(),
                covered_turns: None,
                at: at(0.0),
            })
            .unwrap();

        let snapshot = model.snapshot().unwrap();
        assert_eq!(snapshot.preferences["style"], "code");
        assert_eq!(snapshot.episodes, 1);
        assert_eq!(model.episodes_for(object).unwrap().len(), 1);
        assert!(model.episodes_for(Uuid::new_v4()).unwrap().is_empty());
    }

    #[test]
    fn profile_block_is_compact_and_cold_start_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let model = LearnerModel::open(tmp.path());
        let names: HashMap<Uuid, String> = HashMap::new();

        // Cold start → no block at all.
        assert!(profile_block(&model.snapshot().unwrap(), &names).is_none());

        let mastered = Uuid::new_v4();
        let struggling = Uuid::new_v4();
        for _ in 0..3 {
            model
                .record_mastery(&mastery_event(mastered, 5, at(0.0)))
                .unwrap();
            model
                .record_mastery(&mastery_event(struggling, 1, at(0.0)))
                .unwrap();
        }
        model
            .record_preference(&PreferenceEvent {
                key: "style".to_string(),
                value: "code".to_string(),
                at: at(0.0),
            })
            .unwrap();

        let names = HashMap::from([(mastered, "Softmax".to_string())]);
        let block = profile_block(&model.snapshot().unwrap(), &names).expect("block");
        assert!(block.contains("style=code"));
        assert!(
            block.contains("do not re-teach"),
            "mastered guidance present"
        );
        assert!(block.contains("Softmax"), "id resolved to name");
        assert!(
            block.contains("change approach"),
            "struggle guidance present"
        );
        assert!(
            block.contains(&struggling.to_string()),
            "unresolved id falls back to the id"
        );
        assert!(
            !block.to_lowercase().contains("quiz"),
            "no transcripts/quiz content"
        );
    }

    #[test]
    fn episodic_summarizer_is_lazy_cached_and_key_optional() {
        let tmp = tempfile::tempdir().unwrap();
        let model = LearnerModel::open(tmp.path());
        let root = tmp.path().join("paper.research");
        let bundle = crate::bundle::Bundle::create(
            &root,
            b"%PDF-1.5 fake",
            crate::bundle::Paper::new("T"),
            "file",
        )
        .unwrap();
        let object = Uuid::new_v4();

        let calls = std::cell::Cell::new(0u32);
        let llm = |_: &str| {
            calls.set(calls.get() + 1);
            Some(r#"{"kind": "confusion", "summary": "mixed up Q and K"}"#.to_string())
        };

        // Short thread → no call at all (lazy).
        for i in 0..3 {
            crate::chat::append(
                &bundle,
                object,
                &crate::chat::user_message("ask", format!("q{i}")),
            )
            .unwrap();
        }
        let result = summarize_episode(&bundle, &model, "p1", object, &llm).unwrap();
        assert!(result.is_none());
        assert_eq!(calls.get(), 0);

        // Long enough → one summary, recorded as an episode.
        crate::chat::append(
            &bundle,
            object,
            &crate::chat::user_message("ask", "q4".into()),
        )
        .unwrap();
        let event = summarize_episode(&bundle, &model, "p1", object, &llm)
            .unwrap()
            .expect("summarized");
        assert_eq!(event.kind, "confusion");
        assert_eq!(event.covered_turns, Some(4));
        assert_eq!(calls.get(), 1);

        // Unchanged thread → cached, no second call.
        let again = summarize_episode(&bundle, &model, "p1", object, &llm).unwrap();
        assert!(again.is_none());
        assert_eq!(calls.get(), 1);

        // Provider failure (no key) → silent skip, nothing recorded.
        crate::chat::append(
            &bundle,
            object,
            &crate::chat::user_message("ask", "q5".into()),
        )
        .unwrap();
        let skipped = summarize_episode(&bundle, &model, "p1", object, &|_| None).unwrap();
        assert!(skipped.is_none());
        assert_eq!(model.episodes_for(object).unwrap().len(), 1);
    }

    #[test]
    fn reset_store_and_wholesale_reset() {
        let tmp = tempfile::tempdir().unwrap();
        let model = LearnerModel::open(tmp.path());
        let concept = Uuid::new_v4();
        model
            .record_mastery(&mastery_event(concept, 4, at(0.0)))
            .unwrap();
        model
            .record_preference(&PreferenceEvent {
                key: "style".to_string(),
                value: "visual".to_string(),
                at: at(0.0),
            })
            .unwrap();

        model.reset_store("mastery").unwrap();
        let snapshot = model.snapshot().unwrap();
        assert!(snapshot.mastery.is_empty(), "mastery cleared");
        assert_eq!(
            snapshot.preferences["style"], "visual",
            "other stores intact"
        );

        // A sibling file stands in for "everything else" (papers, notes).
        std::fs::write(tmp.path().join("other.txt"), b"untouched").unwrap();
        model.reset_all().unwrap();
        assert!(!tmp.path().join("learning_state").exists());
        assert!(
            tmp.path().join("other.txt").exists(),
            "only learning_state removed"
        );
    }
}
