//! Capability parity matrix (v5 platform-parity): the single source of
//! truth for what runs where. UIs derive availability from this — no view
//! hard-codes platform checks, and unavailable features degrade with an
//! explicit explanation, never silently.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// Desktop app only (OS integration the browser can't host).
    Native,
    /// Full parity on web.
    Web,
    /// Works on web when a companion runner is configured (execution
    /// happens on a machine that has the native capability).
    WebViaRunner,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Capability {
    pub id: &'static str,
    pub label: &'static str,
    pub availability: Availability,
    /// Shown verbatim by the web UI when the feature is degraded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_note: Option<&'static str>,
}

pub fn capability_matrix() -> Vec<Capability> {
    use Availability::*;
    let cap = |id, label, availability, web_note| Capability {
        id,
        label,
        availability,
        web_note,
    };
    vec![
        cap("reading", "PDF reading & annotation", Web, None),
        cap("graph", "Concept map canvas", Web, None),
        cap("chat", "Contextual chat", Web, None),
        cap("learning", "Lessons, quizzes, flashcards", Web, None),
        cap("sync", "Cloud sync (E2E encrypted)", Web, None),
        cap("registry", "Knowledge registry publish/pull", Web, None),
        cap("contributions", "Community contributions", Web, None),
        cap("plugins_export", "Exporter/importer plugins", Web, None),
        cap(
            "ingestion",
            "PDF ingestion pipeline",
            Native,
            Some("Import and process papers in the desktop app; processed bundles sync to web."),
        ),
        cap(
            "semantic_search",
            "Semantic search (local embeddings)",
            Native,
            Some("The embedding model runs natively; web falls back to text search."),
        ),
        cap(
            "sandbox",
            "Sandboxed code execution",
            WebViaRunner,
            Some("Execution needs the desktop app or a configured runner; past runs stay readable here."),
        ),
        cap(
            "experiments",
            "Experiment runs",
            WebViaRunner,
            Some("Runs execute on a machine with the sandbox; results and comparisons stay readable here."),
        ),
        cap(
            "reproduction",
            "Reproduction mode",
            WebViaRunner,
            Some("Repo cloning and runs need the desktop app or a runner; the report stays readable here."),
        ),
        cap(
            "plugins_panel",
            "Panel plugins (wasmtime host)",
            Native,
            Some("Third-party panels run in the desktop plugin host for now."),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_well_formed() {
        let matrix = capability_matrix();
        let mut ids = std::collections::BTreeSet::new();
        for capability in &matrix {
            assert!(ids.insert(capability.id), "duplicate id {}", capability.id);
            // Anything not fully available on web must explain itself.
            if capability.availability != Availability::Web {
                assert!(
                    capability.web_note.is_some(),
                    "{} degrades without an explanation",
                    capability.id
                );
            }
        }
        // The execution family is runner-gated, never silently native-only.
        for id in ["sandbox", "experiments", "reproduction"] {
            assert_eq!(
                matrix.iter().find(|c| c.id == id).unwrap().availability,
                Availability::WebViaRunner
            );
        }
    }
}
