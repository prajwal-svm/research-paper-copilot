//! Ingestion job runner: ordered stages, resumable, per-stage progress.
//!
//! Stages: layout → objects → enrichment (equations, figures/tables,
//! citations) → embeddings. Each stage records completion in
//! `metadata.json.pipeline.stages`; a rerun skips stages whose recorded
//! status is `complete` at the current `pipeline_version`, which is what
//! makes interrupted ingestion resumable — quit during enrichment, relaunch,
//! and only enrichment onward runs again.
//!
//! The runner itself is synchronous; `spawn` runs it on a background thread
//! and streams [`ProgressEvent`]s over a channel (the Tauri shell forwards
//! them to the UI). All PDFium work happens under [`crate::layout::pdfium_lock`].

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use serde::{Deserialize, Serialize};

use crate::bundle::Bundle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Layout,
    Objects,
    Enrichment,
    Embeddings,
    Concepts,
}

impl Stage {
    pub const ALL: [Stage; 5] = [
        Stage::Layout,
        Stage::Objects,
        Stage::Enrichment,
        Stage::Embeddings,
        Stage::Concepts,
    ];

    /// Key in `metadata.json.pipeline.stages`.
    fn metadata_key(self) -> &'static str {
        match self {
            Stage::Layout => "layout",
            Stage::Objects => "objects",
            Stage::Enrichment => "enrichment_parsing",
            Stage::Embeddings => "embeddings",
            Stage::Concepts => "concepts",
        }
    }

    fn current_version(self) -> &'static str {
        match self {
            Stage::Layout => crate::layout::LAYOUT_PIPELINE_VERSION,
            Stage::Objects => crate::objects::OBJECTS_PIPELINE_VERSION,
            Stage::Enrichment => crate::figures_tables::FIGURES_TABLES_PIPELINE_VERSION,
            Stage::Embeddings => crate::embeddings::EMBEDDINGS_PIPELINE_VERSION,
            Stage::Concepts => crate::concepts::CONCEPTS_PIPELINE_VERSION,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProgressEvent {
    StageStarted {
        stage: Stage,
    },
    StageCompleted {
        stage: Stage,
    },
    StageSkipped {
        stage: Stage,
    },
    /// Stage produced usable-but-partial output; ingestion continues.
    StageDegraded {
        stage: Stage,
        reason: String,
    },
    /// Stage failed entirely; ingestion continues (raw view stays available).
    StageFailed {
        stage: Stage,
        reason: String,
    },
    PipelineFinished {
        usable: bool,
    },
}

/// LLM callback for stages that can use one (concept extraction): takes a
/// prompt, returns the model's text, `None` on any failure. Injected by the
/// shell so the core stays provider-agnostic.
pub type StageLlm = std::sync::Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

#[derive(Clone, Default)]
pub struct PipelineOptions {
    /// Skip the embeddings stage (tests/CI without the model; low-resource
    /// machines defer it — search degrades to exact-only until it runs).
    pub skip_embeddings: bool,
    /// LLM for concept extraction; `None` → heuristic graph (degraded).
    pub concepts_llm: Option<StageLlm>,
}

impl PipelineOptions {
    /// v1-compatible constructor used by tests: embeddings on/off, no LLM.
    pub fn local(embeddings: bool) -> Self {
        PipelineOptions {
            skip_embeddings: !embeddings,
            concepts_llm: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
}

/// Is this stage already recorded complete at its current version?
fn stage_is_current(bundle: &Bundle, stage: Stage) -> bool {
    let Ok(metadata) = bundle.metadata() else {
        return false;
    };
    let Some(record) = metadata.pipeline.stages.get(stage.metadata_key()) else {
        return false;
    };
    record["status"] == "complete" && record["pipeline_version"] == stage.current_version()
}

/// Record a stage failure in metadata without aborting the pipeline.
fn record_failure(bundle: &Bundle, stage: Stage, reason: &str) {
    if let Ok(mut metadata) = bundle.metadata() {
        metadata.pipeline.stages.insert(
            stage.metadata_key().to_string(),
            serde_json::json!({
                "pipeline_version": stage.current_version(),
                "status": "failed",
                "completed_at": crate::bundle::now_rfc3339(),
                "failure_reason": reason,
            }),
        );
        let _ = bundle.write_metadata(&metadata);
    }
}

/// Run the pipeline on an existing bundle, emitting progress events.
/// Stage failures degrade (raw view always works); they never abort later
/// stages that don't depend on the failed one, and never return `Err` —
/// `Err` is reserved for the bundle itself being unusable.
pub fn run(
    bundle: &Bundle,
    options: &PipelineOptions,
    progress: &mut dyn FnMut(ProgressEvent),
) -> Result<(), PipelineError> {
    let mut usable = true;

    for stage in Stage::ALL {
        if stage == Stage::Embeddings && options.skip_embeddings {
            progress(ProgressEvent::StageSkipped { stage });
            continue;
        }
        if stage_is_current(bundle, stage) {
            progress(ProgressEvent::StageSkipped { stage });
            continue;
        }
        progress(ProgressEvent::StageStarted { stage });

        let outcome: Result<Option<String>, String> = match stage {
            Stage::Layout => {
                let _lock = crate::layout::pdfium_lock();
                crate::layout::pdfium()
                    .and_then(|pdfium| crate::layout::run_layout_stage(pdfium, bundle))
                    .map(|layout| {
                        let scanned = layout.pages.iter().filter(|p| p.is_scanned).count();
                        (scanned == layout.pages.len() && !layout.pages.is_empty())
                            .then(|| "no text layer found — scanned PDF, raw view only".to_string())
                    })
                    .map_err(|e| e.to_string())
            }
            Stage::Objects => crate::objects::run_objects_stage(bundle)
                .map(|_| None)
                .map_err(|e| e.to_string()),
            Stage::Enrichment => {
                // Three independent slices; a failing slice degrades the
                // stage rather than failing it.
                let mut problems = Vec::new();
                if let Err(e) = crate::equations::run_equations_stage(bundle) {
                    problems.push(format!("equations: {e}"));
                }
                {
                    let _lock = crate::layout::pdfium_lock();
                    if let Err(e) = crate::figures_tables::run_figures_tables_stage(bundle) {
                        problems.push(format!("figures/tables: {e}"));
                    }
                }
                if let Err(e) = crate::citations::run_citations_stage(bundle) {
                    problems.push(format!("citations: {e}"));
                }
                if problems.len() == 3 {
                    Err(problems.join("; "))
                } else if problems.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(problems.join("; ")))
                }
            }
            Stage::Embeddings => crate::embeddings::Embedder::load()
                .and_then(|embedder| crate::embeddings::run_embeddings_stage(bundle, &embedder))
                .map(|_| None)
                .map_err(|e| e.to_string()),
            Stage::Concepts => {
                let llm = options
                    .concepts_llm
                    .as_ref()
                    .map(|f| f.as_ref() as &dyn Fn(&str) -> Option<String>);
                crate::concepts::run_concepts_stage(bundle, llm)
                    .map(|graph| {
                        (graph.extraction == "heuristic").then(|| {
                            "no AI provider — heuristic concept graph (low confidence)".to_string()
                        })
                    })
                    .map_err(|e| e.to_string())
            }
        };

        match outcome {
            Ok(None) => progress(ProgressEvent::StageCompleted { stage }),
            Ok(Some(reason)) => progress(ProgressEvent::StageDegraded { stage, reason }),
            Err(reason) => {
                // Layout failing means nothing downstream can run usefully.
                if stage == Stage::Layout {
                    usable = false;
                }
                record_failure(bundle, stage, &reason);
                progress(ProgressEvent::StageFailed { stage, reason });
            }
        }
    }

    progress(ProgressEvent::PipelineFinished { usable });
    Ok(())
}

/// Run the pipeline on a background thread; events stream over the receiver.
pub fn spawn(
    bundle_root: PathBuf,
    options: PipelineOptions,
) -> (
    std::thread::JoinHandle<Result<(), PipelineError>>,
    mpsc::Receiver<ProgressEvent>,
) {
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let bundle = Bundle::open(&bundle_root)?;
        run(&bundle, &options, &mut |event| {
            let _ = tx.send(event);
        })
    });
    (handle, rx)
}

/// Import a PDF into a new bundle directory and run the pipeline.
pub fn import_pdf(
    pdf_bytes: &[u8],
    bundle_root: &Path,
    paper: crate::bundle::Paper,
    imported_from: &str,
    options: &PipelineOptions,
    progress: &mut dyn FnMut(ProgressEvent),
) -> Result<Bundle, PipelineError> {
    let bundle = Bundle::create(bundle_root, pdf_bytes, paper, imported_from)?;
    run(&bundle, options, progress)?;
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;
    use pdfium_render::prelude::*;

    fn sample_pdf_bytes() -> Vec<u8> {
        let pdfium = crate::layout::pdfium().expect("pdfium missing");
        let mut document = pdfium.create_new_pdf().unwrap();
        let font = document.fonts_mut().helvetica();
        let mut page = document
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::a4())
            .unwrap();
        let page_height = page.height().value;
        let add = |page: &mut PdfPage, text: &str, x: f32, y_top: f32, size: f32| {
            page.objects_mut()
                .create_text_object(
                    PdfPoints::new(x),
                    PdfPoints::new(page_height - y_top),
                    text,
                    font,
                    PdfPoints::new(size),
                )
                .unwrap();
        };
        add(&mut page, "A Tiny Paper", 200.0, 80.0, 18.0);
        add(&mut page, "1 Introduction", 72.0, 140.0, 12.0);
        add(
            &mut page,
            "This paper is small. It exists to test the pipeline.",
            72.0,
            170.0,
            10.0,
        );
        document.save_to_bytes().unwrap()
    }

    fn no_embeddings() -> PipelineOptions {
        PipelineOptions::local(false)
    }

    #[test]
    fn full_run_emits_ordered_events_and_completes_stages() {
        let _lock = crate::layout::pdfium_lock();
        let bytes = sample_pdf_bytes();
        drop(_lock);

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let mut events = Vec::new();
        let bundle = import_pdf(
            &bytes,
            &root,
            Paper::new("A Tiny Paper"),
            "file",
            &no_embeddings(),
            &mut |e| events.push(e),
        )
        .unwrap();

        // layout, objects, enrichment complete; embeddings skipped; finished.
        let kinds: Vec<String> = events
            .iter()
            .map(|e| {
                serde_json::to_value(e).unwrap()["kind"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert!(
            kinds.ends_with(&["pipeline_finished".to_string()]),
            "{kinds:?}"
        );
        assert_eq!(
            kinds.iter().filter(|k| *k == "stage_completed").count(),
            3,
            "{events:#?}"
        );

        let metadata = bundle.metadata().unwrap();
        for key in ["layout", "objects", "enrichment_parsing"] {
            assert_eq!(
                metadata.pipeline.stages[key]["status"], "complete",
                "stage {key}"
            );
        }
    }

    #[test]
    fn rerun_skips_completed_stages() {
        let _lock = crate::layout::pdfium_lock();
        let bytes = sample_pdf_bytes();
        drop(_lock);

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = import_pdf(
            &bytes,
            &root,
            Paper::new("A Tiny Paper"),
            "file",
            &no_embeddings(),
            &mut |_| {},
        )
        .unwrap();

        let mut events = Vec::new();
        run(&bundle, &no_embeddings(), &mut |e| events.push(e)).unwrap();
        let skipped = events
            .iter()
            .filter(|e| matches!(e, ProgressEvent::StageSkipped { .. }))
            .count();
        assert_eq!(skipped, 4, "{events:#?}"); // 3 complete + embeddings opt-out
    }

    #[test]
    fn interrupted_ingestion_resumes_from_incomplete_stage() {
        let _lock = crate::layout::pdfium_lock();
        let bytes = sample_pdf_bytes();
        drop(_lock);

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = import_pdf(
            &bytes,
            &root,
            Paper::new("A Tiny Paper"),
            "file",
            &no_embeddings(),
            &mut |_| {},
        )
        .unwrap();

        // Simulate a crash mid-enrichment: stage record says running.
        let mut metadata = bundle.metadata().unwrap();
        metadata.pipeline.stages.insert(
            "enrichment_parsing".to_string(),
            serde_json::json!({
                "pipeline_version": crate::figures_tables::FIGURES_TABLES_PIPELINE_VERSION,
                "status": "running",
            }),
        );
        bundle.write_metadata(&metadata).unwrap();

        let mut events = Vec::new();
        run(&bundle, &no_embeddings(), &mut |e| events.push(e)).unwrap();

        // Layout and objects skipped; enrichment re-ran to completion.
        let describe: Vec<(String, Option<Stage>)> = events
            .iter()
            .map(|e| match e {
                ProgressEvent::StageSkipped { stage } => ("skip".to_string(), Some(*stage)),
                ProgressEvent::StageStarted { stage } => ("start".to_string(), Some(*stage)),
                ProgressEvent::StageCompleted { stage } => ("done".to_string(), Some(*stage)),
                _ => ("other".to_string(), None),
            })
            .collect();
        assert!(
            describe.contains(&("skip".to_string(), Some(Stage::Layout))),
            "{describe:?}"
        );
        assert!(
            describe.contains(&("done".to_string(), Some(Stage::Enrichment))),
            "{describe:?}"
        );
        let metadata = bundle.metadata().unwrap();
        assert_eq!(
            metadata.pipeline.stages["enrichment_parsing"]["status"],
            "complete"
        );
    }

    #[test]
    fn background_spawn_streams_events() {
        let _lock = crate::layout::pdfium_lock();
        let bytes = sample_pdf_bytes();
        drop(_lock);

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        crate::bundle::Bundle::create(&root, &bytes, Paper::new("A Tiny Paper"), "file").unwrap();

        let (handle, rx) = spawn(root, no_embeddings());
        let events: Vec<ProgressEvent> = rx.iter().collect();
        handle.join().unwrap().unwrap();
        assert!(matches!(
            events.last(),
            Some(ProgressEvent::PipelineFinished { usable: true })
        ));
    }

    #[test]
    fn hostile_input_degrades_never_panics() {
        // Not a PDF at all.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let mut events = Vec::new();
        let result = import_pdf(
            b"this is not a pdf",
            &root,
            Paper::new("Garbage"),
            "file",
            &no_embeddings(),
            &mut |e| events.push(e),
        );
        // Bundle is created; layout fails; pipeline reports not-usable.
        let bundle = result.unwrap();
        assert!(matches!(
            events.last(),
            Some(ProgressEvent::PipelineFinished { usable: false })
        ));
        let metadata = bundle.metadata().unwrap();
        assert_eq!(metadata.pipeline.stages["layout"]["status"], "failed");
        assert!(metadata.pipeline.stages["layout"]["failure_reason"]
            .as_str()
            .is_some());
    }
}
