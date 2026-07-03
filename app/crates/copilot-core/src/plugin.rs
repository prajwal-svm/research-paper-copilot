//! Plugin API (v5): manifests, discovery, compatibility gating.
//!
//! Plugins are directories under the app's plugins dir, each with a
//! `plugin.json` manifest declaring the format major it targets, its
//! capabilities, and the permissions it wants. Discovery only parses
//! manifests — an incompatible plugin is listed with its reason and none
//! of its code ever executes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bundle::FORMAT_MAJOR;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PluginManifest {
    /// Unique name, kebab-case (e.g. `anki-exporter`).
    pub name: String,
    pub version: String,
    /// The `.research` format major this plugin targets. Must equal the
    /// app's format major to load.
    pub format_major: u64,
    /// What the plugin does: "panel" | "exporter" | "importer".
    pub capabilities: Vec<String>,
    /// Permissions the plugin wants: "network" | "filesystem". Everything
    /// else (scoped bundle reads) needs no permission. Each is granted
    /// explicitly by the user and revocable.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Entry point, relative to the plugin dir (e.g. `plugin.wasm`).
    pub entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

pub const KNOWN_CAPABILITIES: [&str; 3] = ["panel", "exporter", "importer"];
pub const KNOWN_PERMISSIONS: [&str; 2] = ["network", "filesystem"];

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PluginStatus {
    Compatible,
    /// Listed but never loaded; `reason` is shown to the user.
    Incompatible {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct DiscoveredPlugin {
    pub manifest: PluginManifest,
    pub dir: PathBuf,
    pub status: PluginStatus,
}

fn compatibility(manifest: &PluginManifest, dir: &Path) -> PluginStatus {
    if manifest.format_major != FORMAT_MAJOR {
        return PluginStatus::Incompatible {
            reason: format!(
                "targets format major {} but this app speaks {} — update the plugin",
                manifest.format_major, FORMAT_MAJOR
            ),
        };
    }
    if let Some(unknown) = manifest
        .capabilities
        .iter()
        .find(|c| !KNOWN_CAPABILITIES.contains(&c.as_str()))
    {
        return PluginStatus::Incompatible {
            reason: format!("unknown capability \"{unknown}\""),
        };
    }
    if let Some(unknown) = manifest
        .permissions
        .iter()
        .find(|p| !KNOWN_PERMISSIONS.contains(&p.as_str()))
    {
        return PluginStatus::Incompatible {
            reason: format!("unknown permission \"{unknown}\""),
        };
    }
    if !dir.join(&manifest.entry).exists() {
        return PluginStatus::Incompatible {
            reason: format!("entry point {} is missing", manifest.entry),
        };
    }
    PluginStatus::Compatible
}

/// Scan the plugins directory. Malformed manifests are surfaced as
/// incompatible entries (with the parse error), never silently dropped —
/// and no plugin code runs during discovery.
pub fn discover(plugins_dir: &Path) -> Vec<DiscoveredPlugin> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(plugins_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let manifest_path = dir.join("plugin.json");
        if !manifest_path.exists() {
            continue;
        }
        match std::fs::read(&manifest_path)
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_slice::<PluginManifest>(&b).map_err(|e| e.to_string()))
        {
            Ok(manifest) => {
                let status = compatibility(&manifest, &dir);
                out.push(DiscoveredPlugin {
                    status,
                    manifest,
                    dir,
                });
            }
            Err(reason) => out.push(DiscoveredPlugin {
                manifest: PluginManifest {
                    name: dir
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unknown".into()),
                    version: "?".into(),
                    format_major: 0,
                    capabilities: Vec::new(),
                    permissions: Vec::new(),
                    entry: String::new(),
                    description: None,
                },
                dir,
                status: PluginStatus::Incompatible {
                    reason: format!("manifest unreadable: {reason}"),
                },
            }),
        }
    }
    out.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_plugin(root: &Path, name: &str, manifest: serde_json::Value, with_entry: bool) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
        if with_entry {
            std::fs::write(dir.join("plugin.wasm"), b"\0asm").unwrap();
        }
    }

    #[test]
    fn discovery_gates_on_format_major_without_executing() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(
            tmp.path(),
            "good",
            serde_json::json!({
                "name": "good", "version": "1.0.0", "format_major": FORMAT_MAJOR,
                "capabilities": ["exporter"], "entry": "plugin.wasm"
            }),
            true,
        );
        write_plugin(
            tmp.path(),
            "from-the-future",
            serde_json::json!({
                "name": "from-the-future", "version": "9.0.0", "format_major": 99,
                "capabilities": ["panel"], "entry": "plugin.wasm"
            }),
            true,
        );
        write_plugin(
            tmp.path(),
            "broken",
            serde_json::json!({"not": "a manifest"}),
            false,
        );

        let found = discover(tmp.path());
        assert_eq!(found.len(), 3);
        let by_name = |n: &str| found.iter().find(|p| p.manifest.name == n).unwrap();
        assert_eq!(by_name("good").status, PluginStatus::Compatible);
        match &by_name("from-the-future").status {
            PluginStatus::Incompatible { reason } => {
                assert!(reason.contains("format major 99"), "{reason}")
            }
            other => panic!("expected incompatible: {other:?}"),
        }
        assert!(matches!(
            by_name("broken").status,
            PluginStatus::Incompatible { .. }
        ));
    }

    #[test]
    fn missing_entry_and_unknown_permission_are_incompatible() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(
            tmp.path(),
            "no-entry",
            serde_json::json!({
                "name": "no-entry", "version": "1.0.0", "format_major": FORMAT_MAJOR,
                "capabilities": ["panel"], "entry": "plugin.wasm"
            }),
            false,
        );
        write_plugin(
            tmp.path(),
            "greedy",
            serde_json::json!({
                "name": "greedy", "version": "1.0.0", "format_major": FORMAT_MAJOR,
                "capabilities": ["panel"], "permissions": ["raw_disk"], "entry": "plugin.wasm"
            }),
            true,
        );
        let found = discover(tmp.path());
        assert!(found
            .iter()
            .all(|p| matches!(p.status, PluginStatus::Incompatible { .. })));
    }
}

// ---------------------------------------------------------------------------
// Host runtime (task 4.2): wasmtime, scoped reads, permission-gated imports
// ---------------------------------------------------------------------------

/// ABI (documented for plugin authors, stable within a format major):
/// - plugin exports `alloc(size: i32) -> i32` and
///   `run(ptr: i32, len: i32) -> i64` (packed: high 32 bits ptr, low 32 len)
/// - input is a JSON "bundle view" the host assembles (scoped read API —
///   the plugin never touches the filesystem)
/// - output bytes are read back from the returned (ptr, len)
/// - optional host imports, linked ONLY when the permission is granted:
///   `host.http_fetch(ptr, len) -> i32` (0 = ok). Without the grant the
///   import is a stub returning -1 and the access is surfaced, never a
///   crash and never a silent grant.
#[derive(Debug, thiserror::Error)]
pub enum PluginRunError {
    #[error("plugin {0} is incompatible and was not executed")]
    Incompatible(String),
    #[error("plugin runtime: {0}")]
    Runtime(String),
}

#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
pub struct PluginRunReport {
    pub output: Vec<u8>,
    /// Permission-gated calls that were blocked (undeclared or ungranted),
    /// surfaced to the user.
    pub blocked: Vec<String>,
}

/// Scoped bundle view for exporters/panels: only enrichment kinds, no
/// filesystem handles cross the boundary.
pub fn bundle_view(bundle: &crate::bundle::Bundle) -> serde_json::Value {
    let read_journal = |path: &str| -> Vec<serde_json::Value> {
        bundle.journal(path).read_all().unwrap_or_default()
    };
    serde_json::json!({
        "metadata": bundle.metadata().ok(),
        "knowledge_graph": bundle
            .read_derived_json::<serde_json::Value>("knowledge_graph.json")
            .ok()
            .flatten(),
        "notes": read_journal("notes/notes.jsonl"),
        "flashcards": bundle
            .read_derived_json::<serde_json::Value>("flashcards/deck.json")
            .ok()
            .flatten(),
        "glossary": bundle
            .read_derived_json::<serde_json::Value>("glossary/terms.json")
            .ok()
            .flatten(),
    })
}

/// Execute a plugin's `run` over an input payload. `granted` holds the
/// permissions the user has granted AND the manifest declared; anything
/// else is linked as a blocking stub.
#[cfg(feature = "native")]
pub fn run_plugin(
    plugin: &DiscoveredPlugin,
    input: &[u8],
    granted: &std::collections::BTreeSet<String>,
) -> Result<PluginRunReport, PluginRunError> {
    if let PluginStatus::Incompatible { .. } = plugin.status {
        return Err(PluginRunError::Incompatible(plugin.manifest.name.clone()));
    }
    let wasm = std::fs::read(plugin.dir.join(&plugin.manifest.entry))
        .map_err(|e| PluginRunError::Runtime(e.to_string()))?;

    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, wasm).map_err(|e| PluginRunError::Runtime(e.to_string()))?;

    struct HostState {
        blocked: Vec<String>,
    }
    let mut store = wasmtime::Store::new(
        &engine,
        HostState {
            blocked: Vec::new(),
        },
    );
    let mut linker = wasmtime::Linker::new(&engine);

    // network permission: real fetch only when declared AND granted.
    let network_allowed =
        plugin.manifest.permissions.iter().any(|p| p == "network") && granted.contains("network");
    let plugin_name = plugin.manifest.name.clone();
    if network_allowed {
        linker
            .func_wrap(
                "host",
                "http_fetch",
                |_caller: wasmtime::Caller<'_, HostState>, _ptr: i32, _len: i32| -> i32 {
                    // Reference host: reachable only under an explicit grant.
                    // (Actual fetch wiring lands with the first networked
                    // plugin; the permission boundary is what's contractual.)
                    0
                },
            )
            .map_err(|e| PluginRunError::Runtime(e.to_string()))?;
    } else {
        linker
            .func_wrap(
                "host",
                "http_fetch",
                move |mut caller: wasmtime::Caller<'_, HostState>, _ptr: i32, _len: i32| -> i32 {
                    caller
                        .data_mut()
                        .blocked
                        .push(format!("{plugin_name}: http_fetch (network not granted)"));
                    -1
                },
            )
            .map_err(|e| PluginRunError::Runtime(e.to_string()))?;
    }

    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| PluginRunError::Runtime(e.to_string()))?;
    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or_else(|| PluginRunError::Runtime("plugin exports no memory".into()))?;
    let alloc = instance
        .get_typed_func::<i32, i32>(&mut store, "alloc")
        .map_err(|e| PluginRunError::Runtime(format!("missing alloc: {e}")))?;
    let run = instance
        .get_typed_func::<(i32, i32), i64>(&mut store, "run")
        .map_err(|e| PluginRunError::Runtime(format!("missing run: {e}")))?;

    let ptr = alloc
        .call(&mut store, input.len() as i32)
        .map_err(|e| PluginRunError::Runtime(e.to_string()))?;
    memory
        .write(&mut store, ptr as usize, input)
        .map_err(|e| PluginRunError::Runtime(e.to_string()))?;
    let packed = run
        .call(&mut store, (ptr, input.len() as i32))
        .map_err(|e| PluginRunError::Runtime(e.to_string()))?;
    let (out_ptr, out_len) = ((packed >> 32) as u32 as usize, packed as u32 as usize);
    let mut output = vec![0u8; out_len];
    memory
        .read(&store, out_ptr, &mut output)
        .map_err(|e| PluginRunError::Runtime(e.to_string()))?;

    Ok(PluginRunReport {
        output,
        blocked: store.into_data().blocked,
    })
}

#[cfg(test)]
mod host_tests {
    use super::*;

    /// Minimal well-behaved plugin: copies input to a fixed buffer, calls
    /// host.http_fetch once, returns the input back (echo).
    const ECHO_WAT: &str = r#"
    (module
      (import "host" "http_fetch" (func $fetch (param i32 i32) (result i32)))
      (memory (export "memory") 2)
      (global $bump (mut i32) (i32.const 1024))
      (func (export "alloc") (param $size i32) (result i32)
        (local $ptr i32)
        (local.set $ptr (global.get $bump))
        (global.set $bump (i32.add (global.get $bump) (local.get $size)))
        (local.get $ptr))
      (func (export "run") (param $ptr i32) (param $len i32) (result i64)
        (drop (call $fetch (local.get $ptr) (local.get $len)))
        (i64.or
          (i64.shl (i64.extend_i32_u (local.get $ptr)) (i64.const 32))
          (i64.extend_i32_u (local.get $len))))
    )"#;

    fn plugin_dir(wat: &str) -> (tempfile::TempDir, DiscoveredPlugin) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("echo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plugin.wasm"), wat::parse_str(wat).unwrap()).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            serde_json::json!({
                "name": "echo", "version": "1.0.0",
                "format_major": crate::bundle::FORMAT_MAJOR,
                "capabilities": ["exporter"], "permissions": ["network"],
                "entry": "plugin.wasm"
            })
            .to_string(),
        )
        .unwrap();
        let discovered = discover(tmp.path()).remove(0);
        assert_eq!(discovered.status, PluginStatus::Compatible);
        (tmp, discovered)
    }

    #[test]
    fn ungranted_network_is_blocked_surfaced_and_nonfatal() {
        let (_tmp, plugin) = plugin_dir(ECHO_WAT);
        let report = run_plugin(&plugin, b"hello view", &Default::default()).unwrap();
        assert_eq!(report.output, b"hello view", "plugin kept running");
        assert_eq!(report.blocked.len(), 1);
        assert!(
            report.blocked[0].contains("network not granted"),
            "{:?}",
            report.blocked
        );
    }

    #[test]
    fn granted_network_is_not_blocked() {
        let (_tmp, plugin) = plugin_dir(ECHO_WAT);
        let granted = std::collections::BTreeSet::from(["network".to_string()]);
        let report = run_plugin(&plugin, b"x", &granted).unwrap();
        assert!(report.blocked.is_empty());
    }

    #[test]
    fn incompatible_plugin_never_executes() {
        let (_tmp, mut plugin) = plugin_dir(ECHO_WAT);
        plugin.status = PluginStatus::Incompatible {
            reason: "test".into(),
        };
        assert!(matches!(
            run_plugin(&plugin, b"x", &Default::default()),
            Err(PluginRunError::Incompatible(_))
        ));
    }
}

// ---------------------------------------------------------------------------
// Consents (task 4.2): recorded, revocable, folded like sandbox consents
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConsentEvent {
    pub plugin: String,
    pub permission: String,
    pub granted: bool,
    pub at: String,
}

/// Record a grant or revocation (append-only journal, auditable history).
pub fn record_consent(
    journal: &crate::bundle::Journal,
    plugin: &str,
    permission: &str,
    granted: bool,
) -> Result<(), crate::bundle::BundleError> {
    journal.append(&ConsentEvent {
        plugin: plugin.to_string(),
        permission: permission.to_string(),
        granted,
        at: crate::bundle::now_rfc3339(),
    })
}

/// Current grants per plugin: latest event per (plugin, permission) wins.
pub fn current_grants(
    journal: &crate::bundle::Journal,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    let mut latest: std::collections::BTreeMap<(String, String), bool> = Default::default();
    for event in journal.read_all::<ConsentEvent>().unwrap_or_default() {
        latest.insert((event.plugin, event.permission), event.granted);
    }
    let mut grants: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        Default::default();
    for ((plugin, permission), granted) in latest {
        if granted {
            grants.entry(plugin).or_default().insert(permission);
        }
    }
    grants
}

#[cfg(test)]
mod consent_tests {
    use super::*;

    #[test]
    fn grants_are_revocable_and_fold_latest_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let journal = crate::bundle::Journal::at(tmp.path().join("consents.jsonl"));
        record_consent(&journal, "echo", "network", true).unwrap();
        assert!(current_grants(&journal)["echo"].contains("network"));
        record_consent(&journal, "echo", "network", false).unwrap();
        assert!(current_grants(&journal).get("echo").is_none(), "revoked");
        // History remains auditable — both events are in the journal.
        assert_eq!(journal.read_all::<ConsentEvent>().unwrap().len(), 2);
    }
}

// ---------------------------------------------------------------------------
// Importer support (task 4.5): cover PDF for sources with no typeset pages
// ---------------------------------------------------------------------------

/// A minimal, valid one-page PDF used as `original.pdf` for imports that
/// have no publisher PDF (LaTeX source, HTML papers). The cover states the
/// degradation explicitly — page-geometry features are absent by nature
/// for such imports, never silently broken.
pub fn cover_pdf(title: &str, note: &str) -> Vec<u8> {
    let text = |s: &str| s.replace('\\', "").replace('(', "[").replace(')', "]");
    let stream = format!(
        "BT /F1 18 Tf 72 720 Td ({}) Tj ET\nBT /F1 10 Tf 72 690 Td ({}) Tj ET",
        text(title),
        text(note)
    );
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_string(),
        format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.drain(..).enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref_at = pdf.len();
    let mut xref = format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
    for offset in &offsets {
        xref.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.extend_from_slice(xref.as_bytes());
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

#[cfg(test)]
mod cover_tests {
    #[test]
    fn cover_pdf_is_a_valid_bundle_source() {
        let pdf = super::cover_pdf("Imported: My Paper", "from LaTeX source — no page geometry");
        assert!(pdf.starts_with(b"%PDF-1.4"));
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("p.research");
        crate::bundle::Bundle::create(&root, &pdf, crate::bundle::Paper::new("My Paper"), "latex")
            .unwrap();
        assert!(crate::schemas::validate_bundle(&root).unwrap().is_empty());
    }
}
