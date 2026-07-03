//! Sandboxed execution (v3) — the single choke point for ALL code execution.
//!
//! Every run path (implementation kernels, experiment runs, reproduction
//! build/run) goes through [`run`]. There is deliberately no other way to
//! execute anything: `run` requires a [`ConsentGrant`], and the only way to
//! obtain one is [`check_grant`] against the bundle's append-only consent
//! journal — the compiler enforces the no-consent-no-execution rule.
//!
//! Isolation posture (verified in the task 1.1 spike, asserted from inside
//! a container): network disabled by default, memory/CPU/pids/time limits,
//! read-only rootfs + tmpfs scratch, capabilities dropped, only the intended
//! bundle subdirectory mounted (read-only source, read-write output only).
//! Containers never inherit host environment. Kill-anytime (<100 ms).

use std::io::BufRead;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::bundle::Bundle;

const CONSENTS_JOURNAL: &str = "consents.jsonl";

// ---------------------------------------------------------------------------
// Runtime detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInfo {
    /// Executable to invoke ("docker" | "podman", or an absolute path).
    pub program: String,
    pub version: String,
}

/// Detect a supported container runtime. Absence is a designed state, never
/// an error: callers surface install guidance and keep non-execution
/// features working.
pub fn detect_runtime() -> Option<RuntimeInfo> {
    detect_runtime_from(&["docker", "podman"])
}

fn detect_runtime_from(candidates: &[&str]) -> Option<RuntimeInfo> {
    for program in candidates {
        // `version --format` talks to the daemon — proves it's actually
        // usable, not just installed.
        let output = Command::new(program)
            .args(["version", "--format", "{{.Server.Version}}"])
            .stdin(Stdio::null())
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !version.is_empty() {
                    return Some(RuntimeInfo {
                        program: program.to_string(),
                        version,
                    });
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Consent (append-only, per-scope, revocable; network is a separate grant)
// ---------------------------------------------------------------------------

/// What the user consented to run. Serialized as a stable string key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "key")]
pub enum ConsentScope {
    /// All generated implementations of this paper.
    Implementations,
    /// One experiment (by experiment uuid).
    Experiment(uuid::Uuid),
    /// One cloned repository (by normalized remote URL).
    Repo(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum ConsentEvent {
    Grant {
        scope: ConsentScope,
        /// Network for THIS grant is never implied; per-run network grants
        /// are recorded separately with their reason.
        at: String,
    },
    GrantNetwork {
        scope: ConsentScope,
        reason: String,
        at: String,
    },
    Revoke {
        scope: ConsentScope,
        at: String,
    },
}

/// Proof of consent for one scope. Deliberately impossible to construct
/// outside this module — [`run`] demanding one makes "execute without
/// consent" a compile error, not a code-review hope.
#[derive(Debug)]
pub struct ConsentGrant {
    scope: ConsentScope,
    network: bool,
    _private: (),
}

impl ConsentGrant {
    pub fn scope(&self) -> &ConsentScope {
        &self.scope
    }
    pub fn network(&self) -> bool {
        self.network
    }
}

/// Record the user's approval for a scope (UI calls this only after the
/// explicit dialog showing mounts + no-network policy).
pub fn record_grant(bundle: &Bundle, scope: ConsentScope) -> Result<(), SandboxError> {
    bundle
        .journal(CONSENTS_JOURNAL)
        .append(&ConsentEvent::Grant {
            scope,
            at: crate::bundle::now_rfc3339(),
        })?;
    Ok(())
}

/// Record a per-run network grant with its stated reason.
pub fn record_network_grant(
    bundle: &Bundle,
    scope: ConsentScope,
    reason: &str,
) -> Result<(), SandboxError> {
    bundle
        .journal(CONSENTS_JOURNAL)
        .append(&ConsentEvent::GrantNetwork {
            scope,
            reason: reason.to_string(),
            at: crate::bundle::now_rfc3339(),
        })?;
    Ok(())
}

pub fn revoke_grant(bundle: &Bundle, scope: ConsentScope) -> Result<(), SandboxError> {
    bundle
        .journal(CONSENTS_JOURNAL)
        .append(&ConsentEvent::Revoke {
            scope,
            at: crate::bundle::now_rfc3339(),
        })?;
    Ok(())
}

/// Standing grants for display in settings (scope, network?, granted_at).
pub fn list_grants(bundle: &Bundle) -> Result<Vec<(ConsentScope, bool, String)>, SandboxError> {
    let events: Vec<ConsentEvent> = bundle.journal(CONSENTS_JOURNAL).read_all()?;
    let mut grants: Vec<(ConsentScope, bool, String)> = Vec::new();
    for event in events {
        match event {
            ConsentEvent::Grant { scope, at } => {
                grants.retain(|(s, _, _)| *s != scope);
                grants.push((scope, false, at));
            }
            ConsentEvent::GrantNetwork { scope, .. } => {
                if let Some(g) = grants.iter_mut().find(|(s, _, _)| *s == scope) {
                    g.1 = true;
                }
            }
            ConsentEvent::Revoke { scope, .. } => {
                grants.retain(|(s, _, _)| *s != scope);
            }
        }
    }
    Ok(grants)
}

/// The ONLY source of [`ConsentGrant`]s: folds the journal; a Revoke after
/// the last Grant means no token. `network` is true only when a network
/// grant exists for the scope and hasn't been revoked with it.
pub fn check_grant(
    bundle: &Bundle,
    scope: &ConsentScope,
) -> Result<Option<ConsentGrant>, SandboxError> {
    let grants = list_grants(bundle)?;
    Ok(grants
        .into_iter()
        .find(|(s, _, _)| s == scope)
        .map(|(scope, network, _)| ConsentGrant {
            scope,
            network,
            _private: (),
        }))
}

// ---------------------------------------------------------------------------
// Run specification & outcome
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RunSpec {
    pub image: String,
    pub command: Vec<String>,
    /// Source directory, mounted read-only at /work.
    pub mount_ro: Option<PathBuf>,
    /// Output directory, mounted read-write at /work/out.
    pub mount_rw: Option<PathBuf>,
    /// Requires a network grant on the consent token; default false.
    pub network: bool,
    pub memory_mb: u32,
    pub cpus: f32,
    pub pids: u32,
    pub timeout: Duration,
    /// Explicit environment for the container (parameters, seeds). This is
    /// the ONLY env the code sees — host env is never inherited.
    pub env: Vec<(String, String)>,
}

impl Default for RunSpec {
    fn default() -> Self {
        RunSpec {
            image: String::new(),
            command: Vec::new(),
            mount_ro: None,
            mount_rw: None,
            network: false,
            memory_mb: 512,
            cpus: 1.0,
            pids: 64,
            timeout: Duration::from_secs(120),
            env: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RunStatus {
    Completed {
        exit_code: i32,
    },
    /// Resource limit hit (OOM kill / time limit) — partials preserved.
    LimitKilled {
        reason: String,
    },
    /// User-initiated kill — partials preserved.
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunOutcome {
    pub status: RunStatus,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("No container runtime found. Install Docker or Podman to run code — everything else keeps working without one.")]
    NoRuntime,
    #[error("This run needs network access, which hasn't been granted for this scope.")]
    NetworkNotGranted,
    #[error("could not start the container runtime: {0}")]
    Spawn(#[from] std::io::Error),
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
}

/// Build the exact `docker run` argument vector (pure, unit-testable — this
/// IS the isolation policy; changing it should break a test).
fn build_args(name: &str, spec: &RunSpec, network: bool) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "--name".into(),
        name.into(),
        // Isolation posture (spike-verified):
        format!("--network={}", if network { "bridge" } else { "none" }),
        format!("--memory={}m", spec.memory_mb),
        format!("--cpus={}", spec.cpus),
        format!("--pids-limit={}", spec.pids),
        "--read-only".into(),
        "--tmpfs".into(),
        "/tmp:size=64m".into(),
        "--security-opt".into(),
        "no-new-privileges".into(),
        "--cap-drop=ALL".into(),
        "--workdir".into(),
        "/work".into(),
    ];
    for (key, value) in &spec.env {
        args.push("-e".into());
        args.push(format!("{key}={value}"));
    }
    if let Some(ro) = &spec.mount_ro {
        args.push("-v".into());
        args.push(format!("{}:/work:ro", ro.display()));
    }
    if let Some(rw) = &spec.mount_rw {
        args.push("-v".into());
        args.push(format!("{}:/work/out:rw", rw.display()));
    }
    args.push(spec.image.clone());
    args.extend(spec.command.iter().cloned());
    args
}

/// Execute a spec in the sandbox. Requires a [`ConsentGrant`] (compile-time
/// choke point); a network-needing spec additionally requires the grant to
/// carry network. Log lines stream to `on_log` as they arrive; the run is
/// killable at any time via `is_cancelled` (polled) and self-terminates at
/// the spec's time limit. Never blocks the caller's UI thread — callers run
/// this on a worker thread and stream events.
pub fn run(
    runtime: &RuntimeInfo,
    spec: &RunSpec,
    consent: &ConsentGrant,
    on_log: &mut dyn FnMut(&str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<RunOutcome, SandboxError> {
    if spec.network && !consent.network {
        return Err(SandboxError::NetworkNotGranted);
    }
    // The rw mount nests inside the ro mount (/work/out). With a read-only
    // rootfs the runtime cannot create that mountpoint itself, so the `out/`
    // directory must exist in the ro source before the container starts.
    if let (Some(ro), Some(_)) = (&spec.mount_ro, &spec.mount_rw) {
        let _ = std::fs::create_dir_all(ro.join("out"));
    }
    let name = format!("rpc-run-{}", uuid::Uuid::new_v4());
    let args = build_args(&name, spec, spec.network && consent.network);

    let started = Instant::now();
    let mut child = Command::new(&runtime.program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Reader threads: stream lines out as they arrive, collect for the
    // persisted outcome.
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let (tx, rx) = std::sync::mpsc::channel::<(bool, String)>();
    let tx_err = tx.clone();
    let out_handle = std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            if tx.send((false, line)).is_err() {
                break;
            }
        }
    });
    let err_handle = std::thread::spawn(move || {
        for line in std::io::BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
        {
            if tx_err.send((true, line)).is_err() {
                break;
            }
        }
    });

    let mut collected_out = String::new();
    let mut collected_err = String::new();
    let mut cancelled = false;
    let mut timed_out = false;
    let status = loop {
        // Drain available log lines.
        while let Ok((is_err, line)) = rx.try_recv() {
            on_log(&line);
            let sink = if is_err {
                &mut collected_err
            } else {
                &mut collected_out
            };
            sink.push_str(&line);
            sink.push('\n');
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if is_cancelled() && !cancelled {
            cancelled = true;
            kill_container(runtime, &name, &mut child);
        }
        if started.elapsed() > spec.timeout && !timed_out && !cancelled {
            timed_out = true;
            kill_container(runtime, &name, &mut child);
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    // Final drain with a deadline: a killed process can leave grandchildren
    // holding the pipe open, so never join the readers unbounded — take what
    // arrives quickly, then detach them (they exit when the pipe closes).
    drop(child);
    let drain_deadline = Instant::now() + Duration::from_millis(300);
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((is_err, line)) => {
                on_log(&line);
                let sink = if is_err {
                    &mut collected_err
                } else {
                    &mut collected_out
                };
                sink.push_str(&line);
                sink.push('\n');
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if Instant::now() > drain_deadline {
                    break;
                }
            }
        }
    }
    drop(out_handle);
    drop(err_handle);

    let exit_code = status.code().unwrap_or(-1);
    let run_status = if cancelled {
        RunStatus::Cancelled
    } else if timed_out {
        RunStatus::LimitKilled {
            reason: format!("time limit ({}s) exceeded", spec.timeout.as_secs()),
        }
    } else if exit_code == 137 {
        // SIGKILL from the runtime: OOM killer / hard stop.
        RunStatus::LimitKilled {
            reason: "memory limit exceeded (killed by the runtime)".to_string(),
        }
    } else {
        RunStatus::Completed { exit_code }
    };
    Ok(RunOutcome {
        status: run_status,
        stdout: collected_out,
        stderr: collected_err,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

fn kill_container(runtime: &RuntimeInfo, name: &str, child: &mut std::process::Child) {
    // Ask the runtime to kill the container (fast, <100 ms in the spike),
    // then make sure the client process is gone too.
    let _ = Command::new(&runtime.program)
        .args(["kill", name])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
}

// ---------------------------------------------------------------------------

// The suite drives a fake runtime built from `#!/bin/sh` scripts (unix
// only by construction); production Windows uses the real docker CLI.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::bundle::Paper;

    fn test_bundle() -> (tempfile::TempDir, Bundle) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("S"), "file").unwrap();
        (tmp, bundle)
    }

    /// A fake runtime: a shell script standing in for docker. Lets tests
    /// exercise the full lifecycle without a container daemon.
    fn fake_runtime(dir: &std::path::Path, script_body: &str) -> RuntimeInfo {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-docker");
        std::fs::write(&path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        RuntimeInfo {
            program: path.to_string_lossy().to_string(),
            version: "fake".to_string(),
        }
    }

    fn grant_for(bundle: &Bundle, scope: ConsentScope) -> ConsentGrant {
        record_grant(bundle, scope.clone()).unwrap();
        check_grant(bundle, &scope).unwrap().expect("granted")
    }

    #[test]
    fn isolation_flags_are_always_present() {
        let spec = RunSpec {
            image: "python:3.12-slim".into(),
            command: vec!["python".into(), "x.py".into()],
            mount_ro: Some(PathBuf::from("/b/implementations")),
            mount_rw: Some(PathBuf::from("/b/implementations/out")),
            ..Default::default()
        };
        let args = build_args("rpc-run-x", &spec, false);
        for required in [
            "--network=none",
            "--memory=512m",
            "--pids-limit=64",
            "--read-only",
            "--cap-drop=ALL",
            "no-new-privileges",
        ] {
            assert!(
                args.iter().any(|a| a == required),
                "missing {required} in {args:?}"
            );
        }
        assert!(args.contains(&"/b/implementations:/work:ro".to_string()));
        assert!(args.contains(&"/b/implementations/out:/work/out:rw".to_string()));
        // Network on requires both spec AND grant — args say bridge only then.
        let net_args = build_args("rpc-run-x", &spec, true);
        assert!(net_args.iter().any(|a| a == "--network=bridge"));
    }

    #[test]
    fn run_streams_logs_and_completes() {
        let tmp = tempfile::tempdir().unwrap();
        let (_btmp, bundle) = test_bundle();
        let runtime = fake_runtime(tmp.path(), "echo line1; echo line2 >&2; echo line3; exit 0");
        let grant = grant_for(&bundle, ConsentScope::Implementations);

        let mut seen = Vec::new();
        let outcome = run(
            &runtime,
            &RunSpec {
                image: "img".into(),
                command: vec!["cmd".into()],
                ..Default::default()
            },
            &grant,
            &mut |line| seen.push(line.to_string()),
            &|| false,
        )
        .unwrap();
        assert_eq!(outcome.status, RunStatus::Completed { exit_code: 0 });
        assert!(outcome.stdout.contains("line1") && outcome.stdout.contains("line3"));
        assert!(outcome.stderr.contains("line2"));
        assert!(seen.len() >= 3, "logs streamed: {seen:?}");
    }

    #[test]
    fn timeout_kills_and_preserves_partials() {
        let tmp = tempfile::tempdir().unwrap();
        let (_btmp, bundle) = test_bundle();
        let runtime = fake_runtime(
            tmp.path(),
            r#"case "$1" in kill) exit 0;; esac; echo partial; sleep 30"#,
        );
        let grant = grant_for(&bundle, ConsentScope::Implementations);

        let outcome = run(
            &runtime,
            &RunSpec {
                image: "img".into(),
                command: vec!["cmd".into()],
                timeout: Duration::from_millis(300),
                ..Default::default()
            },
            &grant,
            &mut |_| {},
            &|| false,
        )
        .unwrap();
        assert!(matches!(outcome.status, RunStatus::LimitKilled { .. }));
        assert!(outcome.stdout.contains("partial"), "partial output kept");
        assert!(outcome.duration_ms < 5_000, "killed promptly");
    }

    #[test]
    fn cancel_kills_immediately() {
        let tmp = tempfile::tempdir().unwrap();
        let (_btmp, bundle) = test_bundle();
        let runtime = fake_runtime(
            tmp.path(),
            r#"case "$1" in kill) exit 0;; esac; echo started; sleep 30"#,
        );
        let grant = grant_for(&bundle, ConsentScope::Implementations);
        let started = Instant::now();
        let outcome = run(
            &runtime,
            &RunSpec {
                image: "img".into(),
                command: vec!["cmd".into()],
                ..Default::default()
            },
            &grant,
            &mut |_| {},
            &|| started.elapsed() > Duration::from_millis(200),
        )
        .unwrap();
        assert_eq!(outcome.status, RunStatus::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn oom_exit_code_maps_to_limit_killed() {
        let tmp = tempfile::tempdir().unwrap();
        let (_btmp, bundle) = test_bundle();
        let runtime = fake_runtime(tmp.path(), "exit 137");
        let grant = grant_for(&bundle, ConsentScope::Implementations);
        let outcome = run(
            &runtime,
            &RunSpec {
                image: "img".into(),
                command: vec!["cmd".into()],
                ..Default::default()
            },
            &grant,
            &mut |_| {},
            &|| false,
        )
        .unwrap();
        assert!(
            matches!(outcome.status, RunStatus::LimitKilled { ref reason } if reason.contains("memory")),
            "{:?}",
            outcome.status
        );
    }

    #[test]
    fn consent_lifecycle_grant_revoke_network() {
        let (_tmp, bundle) = test_bundle();
        let scope = ConsentScope::Repo("github.com/x/y".into());

        // No grant → no token (and therefore no way to call run()).
        assert!(check_grant(&bundle, &scope).unwrap().is_none());

        record_grant(&bundle, scope.clone()).unwrap();
        let grant = check_grant(&bundle, &scope).unwrap().expect("granted");
        assert!(!grant.network(), "network never implied");

        // Network needs its own recorded grant with a reason.
        record_network_grant(&bundle, scope.clone(), "pip install torch").unwrap();
        let grant = check_grant(&bundle, &scope).unwrap().unwrap();
        assert!(grant.network());

        // Revocation blocks the next run.
        revoke_grant(&bundle, scope.clone()).unwrap();
        assert!(check_grant(&bundle, &scope).unwrap().is_none());

        // Scopes are independent.
        assert!(check_grant(&bundle, &ConsentScope::Implementations)
            .unwrap()
            .is_none());
    }

    /// End-to-end against a real daemon (needs docker/podman + the image):
    ///   cargo test -p copilot-core --lib sandbox::tests::real_runtime_smoke -- --ignored
    #[test]
    #[ignore = "needs a running container runtime"]
    fn real_runtime_smoke() {
        let Some(runtime) = detect_runtime() else {
            panic!("no runtime — start Docker/Podman to run this test");
        };
        let (_tmp, bundle) = test_bundle();
        let grant = grant_for(&bundle, ConsentScope::Implementations);
        // Security posture asserted from INSIDE the container: network off,
        // source mount read-only, only the output mount writable.
        let source = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("marker.txt"), "src").unwrap();
        let probe = "import socket, os\n\
                     try:\n socket.create_connection(('1.1.1.1',53),timeout=2); print('NET-OPEN')\n\
                     except OSError:\n print('net-blocked')\n\
                     try:\n open('/work/violation.txt','w').write('x'); print('SRC-WRITABLE')\n\
                     except OSError:\n print('src-readonly')\n\
                     open('/work/out/result.txt','w').write('ok'); print('out-writable')";
        let outcome = run(
            &runtime,
            &RunSpec {
                image: "python:3.12-slim".into(),
                command: vec!["python".into(), "-c".into(), probe.into()],
                mount_ro: Some(source.path().to_path_buf()),
                mount_rw: Some(out.path().to_path_buf()),
                timeout: Duration::from_secs(60),
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
        assert!(
            outcome.stdout.contains("net-blocked"),
            "network off by default"
        );
        assert!(
            outcome.stdout.contains("src-readonly"),
            "source mount is read-only"
        );
        assert!(
            outcome.stdout.contains("out-writable"),
            "output mount works"
        );
        assert!(
            !source.path().join("violation.txt").exists(),
            "no write escaped into the source mount"
        );
        assert_eq!(
            std::fs::read_to_string(out.path().join("result.txt")).unwrap(),
            "ok"
        );
    }

    #[test]
    fn network_spec_without_network_grant_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let (_btmp, bundle) = test_bundle();
        let runtime = fake_runtime(tmp.path(), "echo should-not-run > \"$0.executed\"; exit 0");
        let grant = grant_for(&bundle, ConsentScope::Implementations);
        let result = run(
            &runtime,
            &RunSpec {
                image: "img".into(),
                command: vec!["cmd".into()],
                network: true, // spec wants network, grant doesn't carry it
                ..Default::default()
            },
            &grant,
            &mut |_| {},
            &|| false,
        );
        assert!(matches!(result, Err(SandboxError::NetworkNotGranted)));
        // The choke point refused BEFORE any process spawned.
        assert!(!std::path::Path::new(&format!("{}.executed", runtime.program)).exists());
    }
}
