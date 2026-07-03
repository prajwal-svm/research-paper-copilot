//! Reproduction mode (v3): clone → env → explain → map → run → verify →
//! report, as a resumable step pipeline mirroring ingestion.
//!
//! Bundle artifacts (portable): `reproduction/state.json` (step records),
//! `repo.json` (remote + commit reference), `env.json` (detected plan),
//! `architecture.md`, `code_map.json` (owned by code-understanding),
//! `run_log.txt`, `report.md`. The clone itself lives in a library-level
//! cache (`repos/<hash>/`) so bundles stay small and portable — deleting
//! the cache loses nothing that can't be re-cloned.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::bundle::Bundle;

pub const REPRODUCTION_DIR: &str = "reproduction";
pub const REPOS_CACHE_DIR: &str = "repos";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Clone,
    Env,
    Explain,
    Map,
    Run,
    Verify,
    Report,
}

impl Step {
    pub const ALL: [Step; 7] = [
        Step::Clone,
        Step::Env,
        Step::Explain,
        Step::Map,
        Step::Run,
        Step::Verify,
        Step::Report,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Step::Clone => "clone",
            Step::Env => "env",
            Step::Explain => "explain",
            Step::Map => "map",
            Step::Run => "run",
            Step::Verify => "verify",
            Step::Report => "report",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    /// "completed" | "failed" | "skipped"
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReproState {
    pub steps: BTreeMap<String, StepRecord>,
}

impl ReproState {
    /// First step that hasn't completed — where a resumed session picks up.
    pub fn next_step(&self) -> Option<Step> {
        Step::ALL.into_iter().find(|step| {
            self.steps
                .get(step.key())
                .map(|r| r.status != "completed" && r.status != "skipped")
                .unwrap_or(true)
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRef {
    pub remote: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Whether the remote is on the curated, gate-tested corpus.
    #[serde(default)]
    pub curated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ReproError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error("reproduction: {0}")]
    Io(#[from] std::io::Error),
    #[error("git failed: {0}")]
    Git(String),
    #[error("no repository linked — start with the repo URL")]
    NoRepo,
}

// ---------------------------------------------------------------------------
// State persistence (resumable; step records survive interrupts)
// ---------------------------------------------------------------------------

fn dir(bundle: &Bundle) -> PathBuf {
    bundle.root().join(REPRODUCTION_DIR)
}

pub fn state(bundle: &Bundle) -> ReproState {
    std::fs::read(dir(bundle).join("state.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn record_step(
    bundle: &Bundle,
    step: Step,
    status: &str,
    detail: Option<String>,
) -> Result<ReproState, ReproError> {
    let mut current = state(bundle);
    current.steps.insert(
        step.key().to_string(),
        StepRecord {
            status: status.to_string(),
            detail,
            at: crate::bundle::now_rfc3339(),
        },
    );
    std::fs::create_dir_all(dir(bundle))?;
    let path = dir(bundle).join("state.json");
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(&current).expect("serializable"),
    )?;
    std::fs::rename(&tmp, &path)?;
    Ok(current)
}

pub fn repo_ref(bundle: &Bundle) -> Option<RepoRef> {
    std::fs::read(dir(bundle).join("repo.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

pub fn set_repo_ref(bundle: &Bundle, repo: &RepoRef) -> Result<(), ReproError> {
    std::fs::create_dir_all(dir(bundle))?;
    std::fs::write(
        dir(bundle).join("repo.json"),
        serde_json::to_vec_pretty(repo).expect("serializable"),
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Clone (library-level cache, keyed by remote)
// ---------------------------------------------------------------------------

/// Cache directory for a remote (stable, filesystem-safe key).
pub fn cache_dir(library_root: &Path, remote: &str) -> PathBuf {
    let digest = crate::bundle::sha256_bytes(remote.trim().to_lowercase().as_bytes());
    let short = digest
        .trim_start_matches("sha256:")
        .chars()
        .take(16)
        .collect::<String>();
    library_root.join(REPOS_CACHE_DIR).join(short)
}

/// Clone (or reuse) the repo into the library cache; returns (path, HEAD
/// commit). Progress lines stream via `on_log`. Uses the host `git` —
/// cloning downloads data, it never executes repo code.
pub fn clone_repo(
    library_root: &Path,
    remote: &str,
    on_log: &mut dyn FnMut(&str),
) -> Result<(PathBuf, String), ReproError> {
    let target = cache_dir(library_root, remote);
    if !target.join(".git").is_dir() {
        std::fs::create_dir_all(target.parent().expect("cache parent"))?;
        on_log(&format!("$ git clone --depth 50 {remote}"));
        let output = Command::new("git")
            .args(["clone", "--depth", "50", remote])
            .arg(&target)
            .stdin(Stdio::null())
            .output()?;
        for line in String::from_utf8_lossy(&output.stderr).lines() {
            on_log(line);
        }
        if !output.status.success() {
            // Leave no half-clone behind to poison resumes.
            let _ = std::fs::remove_dir_all(&target);
            return Err(ReproError::Git(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
    } else {
        on_log("clone cached — reusing");
    }
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&target)
        .output()?;
    let commit = String::from_utf8_lossy(&head.stdout).trim().to_string();
    Ok((target, commit))
}

// ---------------------------------------------------------------------------
// Environment detection (uv > conda > docker > pip; exact commands shown)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvPlan {
    /// "uv" | "conda" | "docker" | "pip" | "none"
    pub kind: String,
    /// Exact commands, shown to the user before anything runs. Setup runs
    /// in the sandbox and needs the network consent (dependency downloads).
    pub setup_commands: Vec<String>,
    /// Why this plan was chosen (which files were found).
    pub evidence: Vec<String>,
}

/// Deterministic-first detection: uv (fastest, lockfile-exact) > conda >
/// docker > plain pip. The user can override; this is the default.
pub fn detect_env(repo: &Path) -> EnvPlan {
    let has = |name: &str| repo.join(name).is_file();
    if has("uv.lock") {
        return EnvPlan {
            kind: "uv".into(),
            setup_commands: vec!["uv sync --frozen".into()],
            evidence: vec!["uv.lock".into()],
        };
    }
    if has("environment.yml") || has("environment.yaml") {
        let file = if has("environment.yml") {
            "environment.yml"
        } else {
            "environment.yaml"
        };
        return EnvPlan {
            kind: "conda".into(),
            setup_commands: vec![format!("conda env create -f {file}")],
            evidence: vec![file.into()],
        };
    }
    if has("Dockerfile") {
        return EnvPlan {
            kind: "docker".into(),
            setup_commands: vec!["docker build -t rpc-repro .".into()],
            evidence: vec!["Dockerfile".into()],
        };
    }
    if has("pyproject.toml") {
        return EnvPlan {
            kind: "uv".into(),
            setup_commands: vec!["uv sync".into()],
            evidence: vec!["pyproject.toml".into()],
        };
    }
    if has("requirements.txt") {
        return EnvPlan {
            kind: "pip".into(),
            setup_commands: vec!["pip install -r requirements.txt".into()],
            evidence: vec!["requirements.txt".into()],
        };
    }
    EnvPlan {
        kind: "none".into(),
        setup_commands: vec![],
        evidence: vec![],
    }
}

pub fn save_env_plan(bundle: &Bundle, plan: &EnvPlan) -> Result<(), ReproError> {
    std::fs::create_dir_all(dir(bundle))?;
    std::fs::write(
        dir(bundle).join("env.json"),
        serde_json::to_vec_pretty(plan).expect("serializable"),
    )?;
    Ok(())
}

pub fn env_plan(bundle: &Bundle) -> Option<EnvPlan> {
    std::fs::read(dir(bundle).join("env.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

// ---------------------------------------------------------------------------
// Verification & report
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricComparison {
    pub metric: String,
    pub reported: f64,
    pub produced: f64,
    pub delta: f64,
    /// |delta| within 1% of the reported value.
    pub matched: bool,
}

/// Compare produced metrics against the paper's reported numbers. Honest by
/// construction: only metrics present on BOTH sides compare; nothing is
/// rounded away.
pub fn verify(
    reported: &BTreeMap<String, f64>,
    produced: &BTreeMap<String, f64>,
) -> Vec<MetricComparison> {
    reported
        .iter()
        .filter_map(|(metric, &reported_value)| {
            let &produced_value = produced.get(metric)?;
            let delta = produced_value - reported_value;
            Some(MetricComparison {
                metric: metric.clone(),
                reported: reported_value,
                produced: produced_value,
                delta,
                matched: delta.abs() <= (reported_value.abs() * 0.01).max(f64::EPSILON),
            })
        })
        .collect()
}

/// Write `report.md`: what matched, what diverged and by how much, what was
/// actually run (always labeled verification-scale), full provenance.
pub fn write_report(
    bundle: &Bundle,
    repo: &RepoRef,
    plan: Option<&EnvPlan>,
    comparisons: &[MetricComparison],
    run_notes: &str,
) -> Result<String, ReproError> {
    let title = bundle.metadata()?.paper.title;
    let mut report = format!(
        "# Reproduction report — {title}\n\n\
         **Scope: verification run** (small-scale local check — not a full-scale reproduction).\n\n\
         ## Provenance\n\n\
         - Repository: {remote}\n\
         - Commit: {commit}\n\
         - Environment: {env}\n\
         - Generated: {at}\n\n",
        remote = repo.remote,
        commit = repo.commit.as_deref().unwrap_or("unknown"),
        env = plan
            .map(|p| format!("{} ({})", p.kind, p.setup_commands.join("; ")))
            .unwrap_or_else(|| "not set up".to_string()),
        at = crate::bundle::now_rfc3339(),
    );
    if comparisons.is_empty() {
        report.push_str("## Metrics\n\nNo comparable metrics were captured.\n\n");
    } else {
        report.push_str(
            "## Metrics\n\n| metric | reported | produced | Δ | verdict |\n|---|---|---|---|---|\n",
        );
        for c in comparisons {
            report.push_str(&format!(
                "| {} | {} | {} | {:+.4} | {} |\n",
                c.metric,
                c.reported,
                c.produced,
                c.delta,
                if c.matched { "matched" } else { "diverged" }
            ));
        }
        report.push('\n');
    }
    if !run_notes.trim().is_empty() {
        report.push_str(&format!("## Run notes\n\n{run_notes}\n"));
    }
    std::fs::create_dir_all(dir(bundle))?;
    std::fs::write(dir(bundle).join("report.md"), &report)?;
    Ok(report)
}

pub fn report(bundle: &Bundle) -> Option<String> {
    std::fs::read_to_string(dir(bundle).join("report.md")).ok()
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;

    fn setup() -> (tempfile::TempDir, Bundle) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("T"), "file").unwrap();
        (tmp, bundle)
    }

    #[test]
    fn pipeline_state_is_resumable_and_ordered() {
        let (_tmp, bundle) = setup();
        assert_eq!(state(&bundle).next_step(), Some(Step::Clone));

        record_step(&bundle, Step::Clone, "completed", None).unwrap();
        record_step(&bundle, Step::Env, "completed", Some("uv".into())).unwrap();
        // Interrupt mid-explain (failed): resume points at Explain again.
        record_step(&bundle, Step::Explain, "failed", Some("cancelled".into())).unwrap();
        assert_eq!(state(&bundle).next_step(), Some(Step::Explain));

        // Skips advance the pipeline without lying about completion.
        record_step(
            &bundle,
            Step::Explain,
            "skipped",
            Some("no provider".into()),
        )
        .unwrap();
        assert_eq!(state(&bundle).next_step(), Some(Step::Map));

        // Prior records survive (nothing corrupted by later writes).
        let s = state(&bundle);
        assert_eq!(s.steps["clone"].status, "completed");
        assert_eq!(s.steps["env"].detail.as_deref(), Some("uv"));
    }

    #[test]
    fn env_detection_prefers_deterministic_options() {
        let repo = tempfile::tempdir().unwrap();
        assert_eq!(detect_env(repo.path()).kind, "none");

        std::fs::write(repo.path().join("requirements.txt"), "torch\n").unwrap();
        assert_eq!(detect_env(repo.path()).kind, "pip");

        std::fs::write(repo.path().join("Dockerfile"), "FROM python\n").unwrap();
        assert_eq!(detect_env(repo.path()).kind, "docker");

        std::fs::write(repo.path().join("environment.yml"), "name: x\n").unwrap();
        assert_eq!(detect_env(repo.path()).kind, "conda");

        std::fs::write(repo.path().join("uv.lock"), "").unwrap();
        let plan = detect_env(repo.path());
        assert_eq!(plan.kind, "uv", "uv wins when present");
        assert_eq!(plan.setup_commands, vec!["uv sync --frozen".to_string()]);
        assert_eq!(plan.evidence, vec!["uv.lock".to_string()]);
    }

    #[test]
    fn bundle_stores_repo_reference_not_the_clone() {
        let (_tmp, bundle) = setup();
        let library = tempfile::tempdir().unwrap();
        let repo = RepoRef {
            remote: "https://github.com/x/y".into(),
            commit: Some("abc123".into()),
            curated: false,
        };
        set_repo_ref(&bundle, &repo).unwrap();
        assert_eq!(repo_ref(&bundle).unwrap().remote, "https://github.com/x/y");

        // Portability: the bundle contains only small JSON references — the
        // clone target lives under the LIBRARY cache, outside the bundle.
        let cache = cache_dir(library.path(), &repo.remote);
        assert!(cache.starts_with(library.path().join(REPOS_CACHE_DIR)));
        assert!(!cache.starts_with(bundle.root()));
        let bundle_repro: Vec<_> = std::fs::read_dir(bundle.root().join(REPRODUCTION_DIR))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(bundle_repro, vec!["repo.json"]);
        // Same remote (case/space-insensitive) → same cache key.
        assert_eq!(
            cache_dir(library.path(), " https://github.com/x/y "),
            cache_dir(
                library.path(),
                "HTTPS://GITHUB.COM/x/y".to_lowercase().as_str()
            )
        );
    }

    #[test]
    fn verification_is_honest_about_divergence() {
        let reported = BTreeMap::from([("bleu".to_string(), 28.4), ("ppl".to_string(), 4.33)]);
        let produced = BTreeMap::from([("bleu".to_string(), 27.9), ("loss".to_string(), 0.4)]);
        let comparisons = verify(&reported, &produced);
        assert_eq!(comparisons.len(), 1, "only shared metrics compare");
        let bleu = &comparisons[0];
        assert!((bleu.delta - (-0.5)).abs() < 1e-9);
        assert!(!bleu.matched, "0.5 off 28.4 is a divergence, not noise");

        let close = verify(
            &BTreeMap::from([("bleu".to_string(), 28.4)]),
            &BTreeMap::from([("bleu".to_string(), 28.3)]),
        );
        assert!(close[0].matched, "within 1% counts as matched");
    }

    #[test]
    fn report_labels_scope_and_shows_deltas() {
        let (_tmp, bundle) = setup();
        let repo = RepoRef {
            remote: "https://github.com/x/y".into(),
            commit: Some("abc123".into()),
            curated: true,
        };
        let comparisons = verify(
            &BTreeMap::from([("bleu".to_string(), 28.4)]),
            &BTreeMap::from([("bleu".to_string(), 27.9)]),
        );
        let report_text = write_report(&bundle, &repo, None, &comparisons, "seed=42").unwrap();
        assert!(report_text.contains("verification run"), "scope labeled");
        assert!(report_text.contains("-0.5"), "delta not rounded away");
        assert!(report_text.contains("diverged"));
        assert!(report_text.contains("abc123"), "provenance present");
        assert_eq!(report(&bundle).unwrap(), report_text, "persists in-bundle");
    }
}
