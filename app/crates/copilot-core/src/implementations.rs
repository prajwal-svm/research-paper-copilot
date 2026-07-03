//! Implementation mode (v3): generated, editable, runnable implementations
//! of equations/algorithms, stored in the bundle's `implementations/`
//! directory keyed by object UUID + language.
//!
//! Layout per object:
//!   implementations/<object-uuid>/<language>.<ext>   the code (user-editable)
//!   implementations/<object-uuid>/<language>.checks.<ext>  generated checks
//!   implementations/<object-uuid>/<language>.meta.json     anchor + provenance
//!
//! Rules (same spirit as graph overrides): user edits are never overwritten
//! by silent regeneration; a changed anchor content-hash flags the
//! implementation for review instead of discarding it; check status is
//! honest — "generated, not yet verified" until checks actually pass.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Bundle;
use crate::objects::SemanticTreeDocument;

pub const IMPLEMENTATIONS_DIR: &str = "implementations";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Python,
    Pytorch,
    Tensorflow,
    Jax,
    Rust,
}

impl Language {
    pub const ALL: [Language; 5] = [
        Language::Python,
        Language::Pytorch,
        Language::Tensorflow,
        Language::Jax,
        Language::Rust,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::Pytorch => "pytorch",
            Language::Tensorflow => "tensorflow",
            Language::Jax => "jax",
            Language::Rust => "rust",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Language::Rust => "rs",
            _ => "py",
        }
    }

    /// Container image for running this language (task 1.1 spike decision:
    /// official per-language images, pulled on first consented use).
    pub fn image(self) -> &'static str {
        match self {
            Language::Python => "python:3.12-slim",
            Language::Pytorch => "pytorch/pytorch:latest",
            Language::Tensorflow => "tensorflow/tensorflow:latest",
            // JAX: CPU wheels on slim python (installed via consented pip).
            Language::Jax => "python:3.12-slim",
            Language::Rust => "rust:1-slim",
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            Language::Python => "Python",
            Language::Pytorch => "PyTorch",
            Language::Tensorflow => "TensorFlow",
            Language::Jax => "JAX",
            Language::Rust => "Rust",
        }
    }
}

/// Check verification state — deliberately three-valued: absence of a run
/// is not failure, and neither counts as verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// Generated, checks never run — "not yet verified" in the UI.
    Unverified,
    Passed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplementationMeta {
    pub object_id: Uuid,
    pub language: Language,
    /// Content hash of the anchor object at generation time.
    pub anchor_hash: String,
    /// Model/provider provenance string (e.g. "glm-5.2 via zai-glm").
    pub provenance: String,
    pub generated_at: String,
    /// True once the user saved an edit — regeneration must not overwrite.
    #[serde(default)]
    pub user_edited: bool,
    pub check_status: CheckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,
    /// Line-anchored guidance ("line 12 implements the QKᵀ term") and
    /// common-pitfall notes, generated alongside the code.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guidance: Vec<String>,
    /// Computed at read: anchor content changed since generation.
    #[serde(default)]
    pub stale: bool,
}

/// Everything the UI needs for one implementation.
#[derive(Debug, Clone, Serialize)]
pub struct Implementation {
    pub meta: ImplementationMeta,
    pub code: String,
    pub checks: Option<String>,
    /// Latest persisted run output, if any.
    pub last_output: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImplementationsError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error("implementations: {0}")]
    Io(#[from] std::io::Error),
    #[error("object not found in this paper")]
    ObjectMissing,
}

fn dir_for(bundle: &Bundle, object_id: Uuid) -> PathBuf {
    bundle
        .root()
        .join(IMPLEMENTATIONS_DIR)
        .join(object_id.to_string())
}

fn code_path(bundle: &Bundle, object_id: Uuid, language: Language) -> PathBuf {
    dir_for(bundle, object_id).join(format!("{}.{}", language.key(), language.extension()))
}

fn checks_path(bundle: &Bundle, object_id: Uuid, language: Language) -> PathBuf {
    dir_for(bundle, object_id).join(format!(
        "{}.checks.{}",
        language.key(),
        language.extension()
    ))
}

fn meta_path(bundle: &Bundle, object_id: Uuid, language: Language) -> PathBuf {
    dir_for(bundle, object_id).join(format!("{}.meta.json", language.key()))
}

fn output_path(bundle: &Bundle, object_id: Uuid, language: Language) -> PathBuf {
    dir_for(bundle, object_id).join(format!("{}.output.txt", language.key()))
}

fn anchor_hash(tree: &SemanticTreeDocument, object_id: Uuid) -> Option<String> {
    tree.objects
        .iter()
        .find(|o| o.id == object_id)
        .map(|o| o.content_hash.clone())
}

/// Load one implementation with staleness computed against the current tree.
pub fn get(
    bundle: &Bundle,
    tree: &SemanticTreeDocument,
    object_id: Uuid,
    language: Language,
) -> Result<Option<Implementation>, ImplementationsError> {
    let meta_file = meta_path(bundle, object_id, language);
    if !meta_file.is_file() {
        return Ok(None);
    }
    let mut meta: ImplementationMeta = serde_json::from_slice(&std::fs::read(&meta_file)?)
        .map_err(|e| {
            ImplementationsError::Bundle(crate::bundle::BundleError::Json {
                path: meta_file.clone(),
                source: e,
            })
        })?;
    meta.stale = anchor_hash(tree, object_id).as_deref() != Some(meta.anchor_hash.as_str());
    let code = std::fs::read_to_string(code_path(bundle, object_id, language))?;
    let checks = std::fs::read_to_string(checks_path(bundle, object_id, language)).ok();
    let last_output = std::fs::read_to_string(output_path(bundle, object_id, language)).ok();
    Ok(Some(Implementation {
        meta,
        code,
        checks,
        last_output,
    }))
}

/// Which languages exist for an object (for the tab strip).
pub fn languages_present(bundle: &Bundle, object_id: Uuid) -> Vec<Language> {
    Language::ALL
        .into_iter()
        .filter(|l| meta_path(bundle, object_id, *l).is_file())
        .collect()
}

/// Persist a freshly generated implementation. Refuses to overwrite a
/// user-edited one unless `force` (the explicit "regenerate anyway" action).
#[allow(clippy::too_many_arguments)]
pub fn save_generated(
    bundle: &Bundle,
    tree: &SemanticTreeDocument,
    object_id: Uuid,
    language: Language,
    code: &str,
    checks: Option<&str>,
    guidance: Vec<String>,
    provenance: &str,
    force: bool,
) -> Result<Implementation, ImplementationsError> {
    if let Some(existing) = get(bundle, tree, object_id, language)? {
        if existing.meta.user_edited && !force {
            // Never silently overwrite user work.
            return Ok(existing);
        }
    }
    let hash = anchor_hash(tree, object_id).ok_or(ImplementationsError::ObjectMissing)?;
    std::fs::create_dir_all(dir_for(bundle, object_id))?;
    std::fs::write(code_path(bundle, object_id, language), code)?;
    if let Some(checks) = checks {
        std::fs::write(checks_path(bundle, object_id, language), checks)?;
    }
    let meta = ImplementationMeta {
        object_id,
        language,
        anchor_hash: hash,
        provenance: provenance.to_string(),
        generated_at: crate::bundle::now_rfc3339(),
        user_edited: false,
        check_status: CheckStatus::Unverified,
        last_run_at: None,
        guidance,
        stale: false,
    };
    write_meta(bundle, object_id, language, &meta)?;
    get(bundle, tree, object_id, language)?.ok_or(ImplementationsError::ObjectMissing)
}

/// Save a user edit: code updated, `user_edited` set, verification reset —
/// edited code is honest-unverified until its checks pass again.
pub fn save_edit(
    bundle: &Bundle,
    object_id: Uuid,
    language: Language,
    code: &str,
) -> Result<(), ImplementationsError> {
    let meta_file = meta_path(bundle, object_id, language);
    let mut meta: ImplementationMeta = serde_json::from_slice(&std::fs::read(&meta_file)?)
        .map_err(|e| {
            ImplementationsError::Bundle(crate::bundle::BundleError::Json {
                path: meta_file.clone(),
                source: e,
            })
        })?;
    std::fs::write(code_path(bundle, object_id, language), code)?;
    meta.user_edited = true;
    meta.check_status = CheckStatus::Unverified;
    write_meta(bundle, object_id, language, &meta)
}

/// Record a run's captured output and (when checks ran) their verdict.
pub fn record_run(
    bundle: &Bundle,
    object_id: Uuid,
    language: Language,
    output: &str,
    check_status: Option<CheckStatus>,
) -> Result<(), ImplementationsError> {
    let meta_file = meta_path(bundle, object_id, language);
    let mut meta: ImplementationMeta = serde_json::from_slice(&std::fs::read(&meta_file)?)
        .map_err(|e| {
            ImplementationsError::Bundle(crate::bundle::BundleError::Json {
                path: meta_file.clone(),
                source: e,
            })
        })?;
    std::fs::write(output_path(bundle, object_id, language), output)?;
    meta.last_run_at = Some(crate::bundle::now_rfc3339());
    if let Some(status) = check_status {
        meta.check_status = status;
    }
    write_meta(bundle, object_id, language, &meta)
}

fn write_meta(
    bundle: &Bundle,
    object_id: Uuid,
    language: Language,
    meta: &ImplementationMeta,
) -> Result<(), ImplementationsError> {
    let path = meta_path(bundle, object_id, language);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(meta).expect("serializable"))?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Generation (one prompt template, parameterized by language)
// ---------------------------------------------------------------------------

/// Prompt for generating an implementation of one object. One canonical
/// template for all five languages — the parameterization is the language
/// name, idioms line, and check style.
pub fn generation_prompt(
    tree: &SemanticTreeDocument,
    object_id: Uuid,
    language: Language,
) -> Option<String> {
    let object = tree.objects.iter().find(|o| o.id == object_id)?;
    let latex = object
        .content
        .latex
        .as_deref()
        .map(|l| format!("LaTeX: {l}\n"))
        .unwrap_or_default();
    let (idioms, check_style) = match language {
        Language::Python => ("plain Python + numpy only", "plain assert statements"),
        Language::Pytorch => (
            "idiomatic PyTorch (torch only)",
            "plain assert statements with torch.allclose",
        ),
        Language::Tensorflow => (
            "idiomatic TensorFlow 2",
            "plain assert statements with tf.debugging",
        ),
        Language::Jax => (
            "idiomatic JAX (jax.numpy)",
            "plain assert statements with jnp.allclose",
        ),
        Language::Rust => (
            "dependency-free Rust (std only), a `main` that demos it",
            "assert!/assert_eq! in main",
        ),
    };
    Some(format!(
        "Implement the following from a research paper in {name} ({idioms}).\n\
         Respond in EXACTLY this structure:\n\
         ## Code\n```\n<the implementation, self-contained and runnable>\n```\n\
         ## Checks\n```\n<a separate runnable file verifying correctness on small examples, {check_style}; \
         it may reimplement expected values by hand. Print 'CHECKS PASSED' as its last line on success.>\n```\n\
         ## Guidance\n\
         - line <n>: <which paper term that line implements>\n\
         - pitfall: <a common mistake implementing this and how this code avoids it>\n\n\
         Source ({label}):\n{text}\n{latex}",
        name = language.display(),
        label = object.semantic_label.as_deref().unwrap_or("object"),
        text = object.content.text.chars().take(3000).collect::<String>(),
    ))
}

/// Parse the generation response. Tolerates missing sections — code is the
/// only hard requirement; `None` means the response was unusable.
pub fn parse_generated(raw: &str) -> Option<(String, Option<String>, Vec<String>)> {
    let code = fenced_after(raw, "## Code")?;
    let checks = fenced_after(raw, "## Checks");
    let guidance: Vec<String> = raw
        .split("## Guidance")
        .nth(1)
        .map(|section| {
            section
                .lines()
                .filter_map(|l| l.trim().strip_prefix("- ").map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Some((code, checks, guidance))
}

fn fenced_after(raw: &str, heading: &str) -> Option<String> {
    let section = raw.split(heading).nth(1)?;
    let start = section.find("```")?;
    let after_fence = &section[start + 3..];
    // Skip an optional language tag on the fence line.
    let body_start = after_fence.find('\n')? + 1;
    let body = &after_fence[body_start..];
    let end = body.find("```")?;
    let code = body[..end].trim_end().to_string();
    (!code.trim().is_empty()).then_some(code)
}

/// Generate (or return existing) via the injected LLM. `None` from the
/// closure (no key / failure) → `Ok(None)`, the designed no-key state;
/// cached implementations remain fully usable without a provider.
#[allow(clippy::too_many_arguments)]
pub fn generate(
    bundle: &Bundle,
    tree: &SemanticTreeDocument,
    object_id: Uuid,
    language: Language,
    llm: &dyn Fn(&str) -> Option<String>,
    provenance: &str,
    force: bool,
) -> Result<Option<Implementation>, ImplementationsError> {
    if !force {
        if let Some(existing) = get(bundle, tree, object_id, language)? {
            return Ok(Some(existing));
        }
    }
    let Some(prompt) = generation_prompt(tree, object_id, language) else {
        return Err(ImplementationsError::ObjectMissing);
    };
    let Some(raw) = llm(&prompt) else {
        return Ok(None);
    };
    let Some((code, checks, guidance)) = parse_generated(&raw) else {
        return Ok(None);
    };
    let implementation = save_generated(
        bundle,
        tree,
        object_id,
        language,
        &code,
        checks.as_deref(),
        guidance,
        provenance,
        force,
    )?;
    Ok(Some(implementation))
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;
    use crate::layout::BBox;
    use crate::objects::{Content, Object, ObjectType};

    fn setup() -> (tempfile::TempDir, Bundle, SemanticTreeDocument, Uuid) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("T"), "file").unwrap();
        let object_id = Uuid::new_v4();
        let tree = SemanticTreeDocument {
            pipeline_version: "0.1.0".to_string(),
            objects: vec![Object {
                id: object_id,
                object_type: ObjectType::Equation,
                regions: vec![BBox {
                    page: 0,
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                }],
                content: Content {
                    text: "Attention(Q,K,V) = softmax(QK^T/sqrt(dk))V".to_string(),
                    latex: None,
                    caption: None,
                },
                semantic_label: Some("Equation 1".to_string()),
                relationships: vec![],
                embedding: None,
                content_hash: crate::bundle::sha256_bytes(b"eq v1"),
                confidence: 0.9,
            }],
            tree: vec![],
        };
        (tmp, bundle, tree, object_id)
    }

    #[test]
    fn generation_parses_structured_response_and_no_key_skips() {
        let (_tmp, bundle, tree, object) = setup();
        let raw = "Some preamble\n## Code\n```python\nimport numpy as np\ndef attention(q, k, v):\n    return v\n```\n## Checks\n```python\nassert True\nprint('CHECKS PASSED')\n```\n## Guidance\n- line 2: implements softmax(QK^T)\n- pitfall: forgetting the 1/sqrt(dk) scale\n";
        let generated = generate(
            &bundle,
            &tree,
            object,
            Language::Python,
            &|_| Some(raw.to_string()),
            "glm-5.2",
            false,
        )
        .unwrap()
        .expect("generated");
        assert!(generated.code.contains("def attention"));
        assert!(generated
            .checks
            .as_deref()
            .unwrap()
            .contains("CHECKS PASSED"));
        assert_eq!(generated.meta.guidance.len(), 2);
        assert_eq!(generated.meta.check_status, CheckStatus::Unverified);

        // No provider → None; nothing persisted for a different language.
        assert!(
            generate(&bundle, &tree, object, Language::Jax, &|_| None, "m", false)
                .unwrap()
                .is_none()
        );
        assert!(get(&bundle, &tree, object, Language::Jax)
            .unwrap()
            .is_none());

        // Second call serves the cache without invoking the LLM.
        let calls = std::cell::Cell::new(0u32);
        let cached = generate(
            &bundle,
            &tree,
            object,
            Language::Python,
            &|_| {
                calls.set(calls.get() + 1);
                Some(raw.to_string())
            },
            "m",
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(calls.get(), 0);
        assert!(cached.code.contains("def attention"));
    }

    #[test]
    fn generate_edit_regenerate_respects_user_work() {
        let (_tmp, bundle, tree, object) = setup();
        save_generated(
            &bundle,
            &tree,
            object,
            Language::Python,
            "def attention(): pass",
            Some("assert attention() is None"),
            vec!["line 1: stub".into()],
            "test-model",
            false,
        )
        .unwrap();

        // User edits → flagged, verification reset.
        save_edit(
            &bundle,
            object,
            Language::Python,
            "def attention(): return 1",
        )
        .unwrap();
        let after_edit = get(&bundle, &tree, object, Language::Python)
            .unwrap()
            .unwrap();
        assert!(after_edit.meta.user_edited);
        assert_eq!(after_edit.meta.check_status, CheckStatus::Unverified);
        assert_eq!(after_edit.code, "def attention(): return 1");

        // Regeneration without force keeps the user's code.
        let kept = save_generated(
            &bundle,
            &tree,
            object,
            Language::Python,
            "REGENERATED",
            None,
            vec![],
            "m",
            false,
        )
        .unwrap();
        assert_eq!(kept.code, "def attention(): return 1", "edit preserved");

        // Explicit force replaces it.
        let replaced = save_generated(
            &bundle,
            &tree,
            object,
            Language::Python,
            "REGENERATED",
            None,
            vec![],
            "m",
            true,
        )
        .unwrap();
        assert_eq!(replaced.code, "REGENERATED");
        assert!(!replaced.meta.user_edited);
    }

    #[test]
    fn stale_anchor_flagged_never_discarded() {
        let (_tmp, bundle, mut tree, object) = setup();
        save_generated(
            &bundle,
            &tree,
            object,
            Language::Rust,
            "fn main() {}",
            None,
            vec![],
            "m",
            false,
        )
        .unwrap();

        // Re-parse changes the anchor's hash.
        tree.objects[0].content_hash = crate::bundle::sha256_bytes(b"eq v2");
        let implementation = get(&bundle, &tree, object, Language::Rust)
            .unwrap()
            .unwrap();
        assert!(implementation.meta.stale, "flagged for review");
        assert_eq!(implementation.code, "fn main() {}", "still fully present");
    }

    #[test]
    fn run_recording_and_check_status_honesty() {
        let (_tmp, bundle, tree, object) = setup();
        save_generated(
            &bundle,
            &tree,
            object,
            Language::Python,
            "print(1)",
            Some("assert True"),
            vec![],
            "m",
            false,
        )
        .unwrap();
        let fresh = get(&bundle, &tree, object, Language::Python)
            .unwrap()
            .unwrap();
        assert_eq!(fresh.meta.check_status, CheckStatus::Unverified);
        assert!(fresh.last_output.is_none());

        record_run(
            &bundle,
            object,
            Language::Python,
            "1\n",
            Some(CheckStatus::Passed),
        )
        .unwrap();
        let ran = get(&bundle, &tree, object, Language::Python)
            .unwrap()
            .unwrap();
        assert_eq!(ran.meta.check_status, CheckStatus::Passed);
        assert_eq!(ran.last_output.as_deref(), Some("1\n"));
        assert!(ran.meta.last_run_at.is_some());

        assert_eq!(languages_present(&bundle, object), vec![Language::Python]);
    }
}
