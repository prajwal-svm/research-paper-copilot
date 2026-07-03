//! Curated-corpus reproduction gate (v3 task 4.5): the clone → env → run →
//! verify path must work end-to-end against pinned, well-behaved ML repos.
//!
//! Ignored by default (network for the clone + a running container runtime):
//!   cargo test -p copilot-core --test repro_corpus -- --ignored --nocapture

use std::collections::BTreeMap;
use std::time::Duration;

use copilot_core::bundle::{Bundle, Paper};
use copilot_core::reproduction as repro;
use copilot_core::sandbox::{self, ConsentScope, RunSpec, RunStatus};

/// Pinned corpus: tiny, dependency-light, deterministic.
const MICROGRAD: &str = "https://github.com/karpathy/micrograd";

#[test]
#[ignore = "clones from GitHub and needs a container runtime"]
fn micrograd_clone_env_run_verify_end_to_end() {
    let Some(runtime) = sandbox::detect_runtime() else {
        panic!("no container runtime — start Docker/Podman for this gate");
    };
    let tmp = tempfile::tempdir().unwrap();
    let library_root = tmp.path();
    let bundle_root = library_root.join("paper.research");
    let bundle = Bundle::create(
        &bundle_root,
        b"%PDF-1.5 fake",
        Paper::new("micrograd"),
        "file",
    )
    .unwrap();

    // Clone (host git, library cache) — observable.
    let mut log = Vec::new();
    let (repo_dir, commit) =
        repro::clone_repo(library_root, MICROGRAD, &mut |l| log.push(l.to_string())).unwrap();
    assert!(repo_dir.join(".git").is_dir());
    assert_eq!(commit.len(), 40, "full HEAD commit recorded");
    repro::record_step(
        &bundle,
        repro::Step::Clone,
        "completed",
        Some(commit.clone()),
    )
    .unwrap();

    // Env detection: micrograd is dependency-free at import time.
    let plan = repro::detect_env(&repo_dir);
    repro::save_env_plan(&bundle, &plan).unwrap();
    repro::record_step(
        &bundle,
        repro::Step::Env,
        "completed",
        Some(plan.kind.clone()),
    )
    .unwrap();

    // Verification run in the sandbox: a known gradient check from the
    // repo's own engine. No network, repo mounted read-only.
    sandbox::record_grant(&bundle, ConsentScope::Repo(MICROGRAD.into())).unwrap();
    let grant = sandbox::check_grant(&bundle, &ConsentScope::Repo(MICROGRAD.into()))
        .unwrap()
        .expect("granted");
    let program = "import json, sys\n\
                   sys.path.insert(0, '/work')\n\
                   from micrograd.engine import Value\n\
                   a = Value(-4.0); b = Value(2.0)\n\
                   c = a + b; d = a * b + b**3\n\
                   c += c + 1; c += 1 + c + (-a)\n\
                   d += d * 2 + (b + a).relu()\n\
                   d += 3 * d + (b - a).relu()\n\
                   e = c - d; f = e**2; g = f / 2.0; g += 10.0 / f\n\
                   g.backward()\n\
                   print(json.dumps({'g': g.data, 'dg_da': a.grad, 'dg_db': b.grad}))";
    let outcome = sandbox::run(
        &runtime,
        &RunSpec {
            image: "python:3.12-slim".into(),
            command: vec!["python".into(), "-c".into(), program.into()],
            mount_ro: Some(repo_dir),
            timeout: Duration::from_secs(120),
            ..Default::default()
        },
        &grant,
        &mut |_| {},
        &|| false,
    )
    .unwrap();
    assert_eq!(
        outcome.status,
        RunStatus::Completed { exit_code: 0 },
        "{}",
        outcome.stderr
    );
    repro::record_step(&bundle, repro::Step::Run, "completed", None).unwrap();

    // Verify against the repo README's own reported values.
    let produced = copilot_core::experiments::parse_metrics(&outcome.stdout);
    let reported = BTreeMap::from([
        ("g".to_string(), 24.7041),
        ("dg_da".to_string(), 138.8338),
        ("dg_db".to_string(), 645.5773),
    ]);
    let comparisons = repro::verify(&reported, &produced);
    assert_eq!(comparisons.len(), 3);
    assert!(
        comparisons.iter().all(|c| c.matched),
        "micrograd gradients must reproduce: {comparisons:?}"
    );

    // Report writes and labels scope honestly.
    let repo_ref = repro::RepoRef {
        remote: MICROGRAD.into(),
        commit: Some(commit),
        curated: true,
    };
    let report = repro::write_report(&bundle, &repo_ref, Some(&plan), &comparisons, "").unwrap();
    assert!(report.contains("verification run"));
    assert!(report.contains("matched"));
    eprintln!("corpus gate OK:\n{report}");
}
