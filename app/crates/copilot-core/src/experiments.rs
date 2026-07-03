//! Experiment mode (v3): parameterized runs over an implementation,
//! persisted in the bundle's `experiments/` directory.
//!
//! Layout per experiment:
//!   experiments/<experiment-uuid>/experiment.json   definition (name, anchor, params)
//!   experiments/<experiment-uuid>/runs.jsonl        append-only run records
//!   chats/<experiment-uuid>.jsonl                   discussion (chat journal semantics)
//!
//! Runs are append-only journal entries — a crash mid-run never corrupts
//! committed runs, and side-by-side comparison is just reading the list.
//! Metrics come from a documented stdout convention: any line that parses
//! as a flat JSON object of numbers, e.g. `{"loss": 0.42, "step": 100}`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Bundle;

pub const EXPERIMENTS_DIR: &str = "experiments";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterSpec {
    pub name: String,
    /// "number" | "text" — kept simple; values are passed as env vars.
    pub kind: String,
    pub default: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiment {
    pub id: Uuid,
    pub name: String,
    /// The implementation this experiment drives.
    pub object_id: Uuid,
    pub language: crate::implementations::Language,
    pub parameters: Vec<ParameterSpec>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentRun {
    pub run_id: Uuid,
    /// Parameter values used (name → value).
    pub params: BTreeMap<String, String>,
    /// Metrics parsed from stdout (`{"metric": value}` lines; last wins).
    pub metrics: BTreeMap<String, f64>,
    pub stdout_tail: String,
    pub duration_ms: u64,
    /// "completed" | "failed" | "limit_killed" | "cancelled" | "incomplete"
    pub status: String,
    /// The user's prediction, recorded BEFORE the run (predict–observe–explain).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prediction: Option<String>,
    /// Lab-mode attribution: which workspace member ran this. Absent for
    /// solo runs; always visible in shared comparisons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_by: Option<String>,
    pub at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ExperimentsError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error("experiments: {0}")]
    Io(#[from] std::io::Error),
    #[error("experiment not found")]
    NotFound,
}

fn dir_for(bundle: &Bundle, id: Uuid) -> PathBuf {
    bundle.root().join(EXPERIMENTS_DIR).join(id.to_string())
}

fn runs_journal(bundle: &Bundle, id: Uuid) -> crate::bundle::Journal {
    crate::bundle::Journal::at(dir_for(bundle, id).join("runs.jsonl"))
}

pub fn create(
    bundle: &Bundle,
    name: &str,
    object_id: Uuid,
    language: crate::implementations::Language,
    parameters: Vec<ParameterSpec>,
) -> Result<Experiment, ExperimentsError> {
    let experiment = Experiment {
        id: Uuid::new_v4(),
        name: name.to_string(),
        object_id,
        language,
        parameters,
        created_at: crate::bundle::now_rfc3339(),
    };
    let dir = dir_for(bundle, experiment.id);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("experiment.json"),
        serde_json::to_vec_pretty(&experiment).expect("serializable"),
    )?;
    Ok(experiment)
}

pub fn list(bundle: &Bundle) -> Result<Vec<Experiment>, ExperimentsError> {
    let root = bundle.root().join(EXPERIMENTS_DIR);
    let mut experiments = Vec::new();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Ok(experiments);
    };
    for entry in entries.flatten() {
        let file = entry.path().join("experiment.json");
        if let Ok(bytes) = std::fs::read(&file) {
            if let Ok(experiment) = serde_json::from_slice::<Experiment>(&bytes) {
                experiments.push(experiment);
            }
        }
    }
    experiments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(experiments)
}

pub fn get(bundle: &Bundle, id: Uuid) -> Result<Experiment, ExperimentsError> {
    let file = dir_for(bundle, id).join("experiment.json");
    let bytes = std::fs::read(&file).map_err(|_| ExperimentsError::NotFound)?;
    serde_json::from_slice(&bytes).map_err(|e| {
        ExperimentsError::Bundle(crate::bundle::BundleError::Json {
            path: file,
            source: e,
        })
    })
}

/// Committed runs, oldest first (torn writes skipped by journal semantics).
pub fn runs(bundle: &Bundle, id: Uuid) -> Result<Vec<ExperimentRun>, ExperimentsError> {
    Ok(runs_journal(bundle, id).read_all()?)
}

/// Append one run record (called after the sandbox run finishes — or with
/// status "incomplete" when it was interrupted before an outcome).
pub fn record_run(bundle: &Bundle, id: Uuid, run: &ExperimentRun) -> Result<(), ExperimentsError> {
    std::fs::create_dir_all(dir_for(bundle, id))?;
    runs_journal(bundle, id).append(run)?;
    Ok(())
}

/// Parse metrics from stdout: every line that is a flat JSON object with
/// numeric values contributes; later lines win per key. This is the whole
/// convention — documented, boring, language-agnostic.
pub fn parse_metrics(stdout: &str) -> BTreeMap<String, f64> {
    let mut metrics = BTreeMap::new();
    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with('{') || !line.ends_with('}') {
            continue;
        }
        if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(line) {
            for (key, value) in map {
                if let Some(number) = value.as_f64() {
                    metrics.insert(key, number);
                }
            }
        }
    }
    metrics
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;
    use crate::implementations::Language;

    fn setup() -> (tempfile::TempDir, Bundle) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("T"), "file").unwrap();
        (tmp, bundle)
    }

    fn run_with(lr: &str, loss: f64, prediction: Option<&str>) -> ExperimentRun {
        ExperimentRun {
            run_id: Uuid::new_v4(),
            params: BTreeMap::from([("learning_rate".to_string(), lr.to_string())]),
            metrics: BTreeMap::from([("loss".to_string(), loss)]),
            stdout_tail: String::new(),
            duration_ms: 10,
            status: "completed".to_string(),
            prediction: prediction.map(|p| p.to_string()),
            run_by: None,
            at: crate::bundle::now_rfc3339(),
        }
    }

    #[test]
    fn lifecycle_create_run_compare() {
        let (_tmp, bundle) = setup();
        let experiment = create(
            &bundle,
            "LR sweep",
            Uuid::new_v4(),
            Language::Python,
            vec![ParameterSpec {
                name: "learning_rate".into(),
                kind: "number".into(),
                default: "0.01".into(),
            }],
        )
        .unwrap();

        record_run(
            &bundle,
            experiment.id,
            &run_with("0.1", 2.31, Some("will diverge")),
        )
        .unwrap();
        record_run(&bundle, experiment.id, &run_with("0.01", 0.42, None)).unwrap();
        record_run(&bundle, experiment.id, &run_with("0.001", 0.61, None)).unwrap();

        let all = runs(&bundle, experiment.id).unwrap();
        assert_eq!(all.len(), 3, "three comparable runs persisted");
        assert_eq!(all[0].prediction.as_deref(), Some("will diverge"));
        assert_eq!(all[1].metrics["loss"], 0.42);
        assert_eq!(list(&bundle).unwrap().len(), 1);
        assert_eq!(get(&bundle, experiment.id).unwrap().name, "LR sweep");
    }

    #[test]
    fn torn_run_write_never_corrupts_committed_runs() {
        let (_tmp, bundle) = setup();
        let experiment = create(&bundle, "X", Uuid::new_v4(), Language::Python, vec![]).unwrap();
        record_run(&bundle, experiment.id, &run_with("0.1", 1.0, None)).unwrap();
        // Simulate a crash mid-append: torn trailing bytes.
        let path = dir_for(&bundle, experiment.id).join("runs.jsonl");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(br#"{"run_id":"tor"#);
        std::fs::write(&path, bytes).unwrap();

        let all = runs(&bundle, experiment.id).unwrap();
        assert_eq!(all.len(), 1, "committed run intact, torn write skipped");
        // And the journal heals on the next append.
        record_run(&bundle, experiment.id, &run_with("0.01", 0.5, None)).unwrap();
        assert_eq!(runs(&bundle, experiment.id).unwrap().len(), 2);
    }

    #[test]
    fn stdout_metric_convention() {
        let parsed = parse_metrics(
            "starting up\n{\"loss\": 2.5, \"step\": 1}\nnoise {not json}\n{\"loss\": 0.42}\n{\"acc\": 0.91, \"note\": \"text ignored\"}\ndone\n",
        );
        assert_eq!(parsed["loss"], 0.42, "last value wins");
        assert_eq!(parsed["step"], 1.0);
        assert_eq!(parsed["acc"], 0.91);
        assert!(!parsed.contains_key("note"), "non-numeric values ignored");
        assert!(parse_metrics("no metrics here").is_empty());
    }
}
