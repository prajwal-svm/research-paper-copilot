//! Performance budget assertions (perf/budgets.toml → CI release blocker).
//!
//! Run in release mode or the numbers are meaningless:
//!   cargo test --release -p copilot-core --test perf_budgets
//!
//! Debug builds skip measurement (the suite would only produce noise).
//! Every `enforced` budget must have a registered benchmark here; an
//! enforced budget without one fails the suite so the table and the suite
//! cannot drift apart.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use copilot_core::bundle::{sha256_bytes, Bundle, Paper};

#[derive(Debug, serde::Deserialize)]
struct Budgets {
    reference_machine: ReferenceMachine,
    #[serde(rename = "budget")]
    budgets: Vec<BudgetEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct ReferenceMachine {
    #[allow(dead_code)]
    description: String,
    ci_slack_factor: f64,
}

#[derive(Debug, serde::Deserialize)]
struct BudgetEntry {
    id: String,
    description: String,
    budget_ms: f64,
    status: String,
}

/// Median wall time over `runs` executions of `f` (with one warmup).
fn median_ms(runs: usize, mut f: impl FnMut()) -> f64 {
    f(); // warmup
    let mut samples: Vec<f64> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    samples.sort_by(|a, b| a.total_cmp(b));
    samples[samples.len() / 2]
}

fn benchmarks() -> BTreeMap<&'static str, Box<dyn FnMut() -> f64>> {
    let mut map: BTreeMap<&'static str, Box<dyn FnMut() -> f64>> = BTreeMap::new();

    map.insert(
        "bundle_open_metadata",
        Box::new(|| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("paper.research");
            Bundle::create(&root, b"%PDF-1.5 bench", Paper::new("Bench"), "file").unwrap();
            median_ms(20, || {
                let bundle = Bundle::open(&root).unwrap();
                let _ = bundle.metadata().unwrap();
            })
        }),
    );

    map.insert(
        "journal_append",
        Box::new(|| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("paper.research");
            let bundle =
                Bundle::create(&root, b"%PDF-1.5 bench", Paper::new("Bench"), "file").unwrap();
            let journal = bundle.journal("chats/bench.jsonl");
            median_ms(50, || {
                journal
                    .append(&serde_json::json!({"role": "user", "text": "benchmark message"}))
                    .unwrap();
            })
        }),
    );

    map.insert(
        "journal_read_1k_entries",
        Box::new(|| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("paper.research");
            let bundle =
                Bundle::create(&root, b"%PDF-1.5 bench", Paper::new("Bench"), "file").unwrap();
            let journal = bundle.journal("chats/bench.jsonl");
            for i in 0..1000 {
                journal
                    .append(&serde_json::json!({"role": "user", "text": format!("message {i}")}))
                    .unwrap();
            }
            median_ms(10, || {
                let entries: Vec<serde_json::Value> = journal.read_all().unwrap();
                assert_eq!(entries.len(), 1000);
            })
        }),
    );

    map.insert(
        "hash_10mb",
        Box::new(|| {
            let bytes = vec![0xabu8; 10 * 1024 * 1024];
            median_ms(10, || {
                let _ = sha256_bytes(&bytes);
            })
        }),
    );

    map.insert(
        "embedding_store_search",
        Box::new(|| {
            use copilot_core::embeddings::{EmbeddingStore, EmbeddingsIndex};
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("paper.research");
            let bundle =
                Bundle::create(&root, b"%PDF-1.5 bench", Paper::new("Bench"), "file").unwrap();

            let dims = 384;
            let rows = 1000;
            let mut bytes = Vec::with_capacity(rows * dims * 4);
            for i in 0..rows * dims {
                bytes.extend_from_slice(&((i % 97) as f32 / 97.0).to_le_bytes());
            }
            std::fs::write(root.join("embeddings.bin"), bytes).unwrap();
            let index = EmbeddingsIndex {
                pipeline_version: "0.1.0".to_string(),
                model: "bench".to_string(),
                dimensions: dims,
                rows: (0..rows).map(|_| uuid::Uuid::new_v4()).collect(),
            };
            bundle
                .write_derived_json(
                    "embeddings_index.json",
                    &index,
                    "embeddings",
                    serde_json::json!({"pipeline_version": "0.1.0", "status": "complete"}),
                )
                .unwrap();

            let store = EmbeddingStore::open(&bundle).unwrap().unwrap();
            let query: Vec<f32> = (0..dims).map(|i| (i % 89) as f32 / 89.0).collect();
            median_ms(20, || {
                let results = store.search(&query, 10);
                assert_eq!(results.len(), 10);
            })
        }),
    );

    map.insert(
        "graph_neighborhood_query",
        Box::new(|| {
            use copilot_core::concepts::{
                concept_id, ConceptEdge, ConceptNode, EdgeKind, KnowledgeGraph,
                CONCEPTS_PIPELINE_VERSION,
            };
            use copilot_core::graph_index::GraphIndex;

            let tmp = tempfile::tempdir().unwrap();
            let mut index = GraphIndex::open(tmp.path()).unwrap();

            // A large library-scale graph: 500 nodes/paper × 4 papers,
            // ~4 edges per node (scale-free-ish fanout).
            for paper in 0..4 {
                let paper_id = format!("paper-{paper}");
                let nodes: Vec<ConceptNode> = (0..500)
                    .map(|i| {
                        let name = format!("concept {i}");
                        ConceptNode {
                            id: concept_id(&paper_id, &name),
                            name,
                            description: None,
                            object_ids: vec![],
                            confidence: 0.8,
                        }
                    })
                    .collect();
                let edges: Vec<ConceptEdge> = (1..500)
                    .flat_map(|i: usize| {
                        [i / 2, i / 3, i.saturating_sub(1), (i * 7) % 500]
                            .into_iter()
                            .filter(move |&j| j != i)
                            .map(|j| ConceptEdge {
                                from: nodes[i].id,
                                to: nodes[j].id,
                                kind: EdgeKind::DependsOn,
                                confidence: 0.8,
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
                let graph = KnowledgeGraph {
                    pipeline_version: CONCEPTS_PIPELINE_VERSION.to_string(),
                    extraction: "llm".to_string(),
                    nodes,
                    edges,
                };
                index.index_paper(&paper_id, &graph).unwrap();
            }

            let query = concept_id("paper-0", "concept 1");
            median_ms(50, || {
                let hood = index.neighborhood(query, 2).unwrap();
                assert!(hood.nodes.len() > 5);
            })
        }),
    );

    map.insert(
        "entity_linking",
        Box::new(|| {
            use copilot_core::concepts::{concept_id, ConceptNode, KnowledgeGraph};
            let nodes: Vec<ConceptNode> = (0..2000)
                .map(|i| {
                    let name = format!("concept variant {i} of scaled dot product attention");
                    ConceptNode {
                        id: concept_id("bench", &name),
                        name,
                        description: None,
                        object_ids: vec![],
                        confidence: 0.8,
                    }
                })
                .collect();
            let graph = KnowledgeGraph {
                pipeline_version: "0.1.0".to_string(),
                extraction: "llm".to_string(),
                nodes,
                edges: vec![],
            };
            let query = "why does concept variant 1500 of scaled dot product attention divide \
                         by the square root of the key dimension before the softmax?";
            median_ms(50, || {
                let linked = copilot_core::context::link_query(&graph, query);
                assert!(!linked.is_empty());
            })
        }),
    );

    map.insert(
        "concept_search_200_papers",
        Box::new(|| {
            use copilot_core::concept_registry::ConceptRegistry;
            use copilot_core::concepts::{concept_id, ConceptNode, KnowledgeGraph};
            let tmp = tempfile::tempdir().unwrap();
            let registry = ConceptRegistry::open(tmp.path());
            // 200 papers × 15 concepts; every 10th paper shares a concept
            // name so the registry has real cross-paper merges to fold.
            for paper in 0..200 {
                let paper_id = format!("paper-{paper}");
                let nodes: Vec<ConceptNode> = (0..15)
                    .map(|i| {
                        let name = if i == 0 && paper % 10 == 0 {
                            "residual connections".to_string()
                        } else {
                            format!("concept {paper}-{i}")
                        };
                        ConceptNode {
                            id: concept_id(&paper_id, &name),
                            name,
                            description: None,
                            object_ids: vec![],
                            confidence: 0.8,
                        }
                    })
                    .collect();
                let graph = KnowledgeGraph {
                    pipeline_version: "0.1.0".to_string(),
                    extraction: "llm".to_string(),
                    nodes,
                    edges: vec![],
                };
                registry.auto_link(&paper_id, &graph, None).unwrap();
            }
            median_ms(10, || {
                let hits = registry.search("residual connections").unwrap();
                assert_eq!(hits[0].members.len(), 20);
            })
        }),
    );

    map.insert(
        "registry_analytics_200_papers",
        Box::new(|| {
            use copilot_core::concept_registry::ConceptRegistry;
            use copilot_core::concepts::{concept_id, ConceptNode, KnowledgeGraph};
            let tmp = tempfile::tempdir().unwrap();
            let registry = ConceptRegistry::open(tmp.path());
            // 200 papers × 15 concepts with heavy sharing (every paper has
            // "attention"; concept i shared by papers where paper % 5 == i % 5).
            for paper in 0..200 {
                let paper_id = format!("paper-{paper}");
                let mut names = vec!["attention".to_string()];
                for i in 0..14 {
                    names.push(if paper % 5 == i % 5 {
                        format!("shared concept {i}")
                    } else {
                        format!("concept {paper}-{i}")
                    });
                }
                let nodes: Vec<ConceptNode> = names
                    .iter()
                    .map(|name| ConceptNode {
                        id: concept_id(&paper_id, name),
                        name: name.clone(),
                        description: None,
                        object_ids: vec![],
                        confidence: 0.8,
                    })
                    .collect();
                registry
                    .auto_link(
                        &paper_id,
                        &KnowledgeGraph {
                            pipeline_version: "0.1.0".to_string(),
                            extraction: "llm".to_string(),
                            nodes,
                            edges: vec![],
                        },
                        None,
                    )
                    .unwrap();
            }
            let state = registry.state().unwrap();
            let attention = state
                .concepts
                .iter()
                .find(|c| c.name == "attention")
                .unwrap()
                .id;
            let scope: Vec<uuid::Uuid> = state.concepts.iter().map(|c| c.id).take(60).collect();
            let dates: std::collections::HashMap<String, Option<String>> = (0..200)
                .map(|i| (format!("paper-{i}"), Some(format!("20{:02}-01-01", i % 25))))
                .collect();
            median_ms(10, || {
                let lineage = state.lineage(attention, &dates);
                assert_eq!(lineage.len(), 200);
                let matrix = state.co_occurrence(&scope);
                assert!(!matrix.is_empty());
            })
        }),
    );

    map.insert(
        "gap_computation_200_papers",
        Box::new(|| {
            use copilot_core::concept_registry::ConceptRegistry;
            use copilot_core::concepts::{concept_id, ConceptNode, KnowledgeGraph};
            let tmp = tempfile::tempdir().unwrap();
            let registry = ConceptRegistry::open(tmp.path());
            for paper in 0..200 {
                let paper_id = format!("paper-{paper}");
                let mut names = Vec::new();
                for i in 0..15 {
                    names.push(if (paper + i) % 4 == 0 {
                        format!("shared concept {}", i % 30)
                    } else {
                        format!("concept {paper}-{i}")
                    });
                }
                let nodes: Vec<ConceptNode> = names
                    .iter()
                    .map(|name| ConceptNode {
                        id: concept_id(&paper_id, name),
                        name: name.clone(),
                        description: None,
                        object_ids: vec![],
                        confidence: 0.8,
                    })
                    .collect();
                registry
                    .auto_link(
                        &paper_id,
                        &KnowledgeGraph {
                            pipeline_version: "0.1.0".to_string(),
                            extraction: "llm".to_string(),
                            nodes,
                            edges: vec![],
                        },
                        None,
                    )
                    .unwrap();
            }
            let state = registry.state().unwrap();
            let dates: std::collections::HashMap<String, Option<String>> = (0..200)
                .map(|i| (format!("paper-{i}"), Some(format!("20{:02}-06-01", i % 26))))
                .collect();
            median_ms(5, || {
                let report = copilot_core::gaps::compute_gaps(&state, &[], &dates);
                assert!(matches!(
                    report,
                    copilot_core::gaps::GapReport::Report { .. }
                ));
            })
        }),
    );

    map.insert(
        "sync_manifest_build_50_papers",
        Box::new(|| {
            let tmp = tempfile::tempdir().unwrap();
            for i in 0..50 {
                let dir = tmp.path().join(format!("paper-{i}.research"));
                let bundle = Bundle::create(
                    &dir,
                    format!("%PDF-1.5 {i}").as_bytes(),
                    Paper::new("B"),
                    "file",
                )
                .unwrap();
                bundle
                    .journal("notes/notes.jsonl")
                    .append(&serde_json::json!({"at": "2026-07-01T00:00:00Z", "note": i}))
                    .unwrap();
            }
            median_ms(10, || {
                let entries = copilot_core::sync::manifest::build_entries(tmp.path()).unwrap();
                assert!(entries.len() >= 100);
            })
        }),
    );

    map.insert(
        "sync_no_change_cycle",
        Box::new(|| {
            use copilot_core::sync::engine::{derive_remote_key, SyncEngine};
            use copilot_core::sync::remote::MemoryRemote;
            let tmp = tempfile::tempdir().unwrap();
            for i in 0..20 {
                Bundle::create(
                    &tmp.path().join(format!("paper-{i}.research")),
                    format!("%PDF-1.5 {i}").as_bytes(),
                    Paper::new("B"),
                    "file",
                )
                .unwrap();
            }
            let remote = MemoryRemote::default();
            let key = derive_remote_key(&remote, "bench-passphrase").unwrap();
            let engine = SyncEngine {
                library_root: tmp.path(),
                device_id: "bench".into(),
                key,
                remote: &remote,
            };
            engine.sync(&mut |_| {}).unwrap(); // initial full push
            median_ms(5, || {
                let outcome = engine.sync(&mut |_| {}).unwrap();
                assert_eq!(outcome.pushed_blobs, 0, "no-change cycle uploads nothing");
            })
        }),
    );

    map.insert(
        "implementation_get_cached",
        Box::new(|| {
            use copilot_core::implementations::{save_generated, Language};
            use copilot_core::layout::BBox;
            use copilot_core::objects::{Content, Object, ObjectType, SemanticTreeDocument};
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("paper.research");
            let bundle =
                Bundle::create(&root, b"%PDF-1.5 bench", Paper::new("Bench"), "file").unwrap();
            let object_id = uuid::Uuid::new_v4();
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
                        text: "eq".to_string(),
                        latex: None,
                        caption: None,
                    },
                    semantic_label: None,
                    relationships: vec![],
                    embedding: None,
                    content_hash: sha256_bytes(b"eq"),
                    confidence: 0.9,
                }],
                tree: vec![],
            };
            let code = "def f():\n    pass\n".repeat(200);
            save_generated(
                &bundle,
                &tree,
                object_id,
                Language::Python,
                &code,
                Some("assert True"),
                vec!["line 1: stub".into()],
                "bench",
                false,
            )
            .unwrap();
            copilot_core::implementations::record_run(
                &bundle,
                object_id,
                Language::Python,
                &"output\n".repeat(500),
                None,
            )
            .unwrap();
            median_ms(50, || {
                let implementation =
                    copilot_core::implementations::get(&bundle, &tree, object_id, Language::Python)
                        .unwrap()
                        .unwrap();
                assert!(!implementation.code.is_empty());
            })
        }),
    );

    map.insert(
        "ingestion_10_page_stages_1_3",
        Box::new(|| {
            use copilot_core::pipeline::{import_pdf, PipelineOptions};
            use pdfium_render::prelude::*;

            // Synthetic 10-page paper: headings, dense paragraphs, an
            // equation with a marker, and captions — exercises every stage.
            // The lock is scoped to PDF creation only: import_pdf takes the
            // same (non-reentrant) pdfium lock internally.
            let lock = copilot_core::layout::pdfium_lock();
            let pdfium = copilot_core::layout::pdfium().expect("pdfium missing");
            let bytes = {
                let mut document = pdfium.create_new_pdf().unwrap();
                let font = document.fonts_mut().helvetica();
                for page_index in 0..10 {
                    let mut page = document
                        .pages_mut()
                        .create_page_at_end(PdfPagePaperSize::a4())
                        .unwrap();
                    let h = page.height().value;
                    let add = |page: &mut PdfPage, text: &str, x: f32, y: f32, size: f32| {
                        page.objects_mut()
                            .create_text_object(
                                PdfPoints::new(x),
                                PdfPoints::new(h - y),
                                text,
                                font,
                                PdfPoints::new(size),
                            )
                            .unwrap();
                    };
                    add(
                        &mut page,
                        &format!("{} Section Heading {page_index}", page_index + 1),
                        72.0,
                        80.0,
                        14.0,
                    );
                    for line in 0..30 {
                        add(
                            &mut page,
                            "The dominant sequence transduction models are based on complex recurrent networks.",
                            72.0,
                            120.0 + line as f32 * 18.0,
                            10.0,
                        );
                    }
                    add(&mut page, "y = mx + b", 200.0, 680.0, 10.0);
                    add(&mut page, "(1)", 500.0, 680.0, 10.0);
                }
                document.save_to_bytes().unwrap()
            };
            drop(lock);

            let tmp = tempfile::tempdir().unwrap();
            let start = Instant::now();
            import_pdf(
                &bytes,
                &tmp.path().join("bench.research"),
                Paper::new("Bench"),
                "file",
                &PipelineOptions::local(false),
                &mut |_| {},
            )
            .unwrap();
            start.elapsed().as_secs_f64() * 1000.0
        }),
    );

    map.insert(
        "registry_pull_layer",
        Box::new(|| {
            use copilot_core::bundle::{Bundle, Paper};
            use copilot_core::registry::{build_layer, pull_layer};
            // Community layer of realistic size: 200 journal entries + 20
            // enrichment files, pulled into a fresh bundle (verify + union
            // merge + provenance tag) — the pull-on-import hot path.
            let src_tmp = tempfile::tempdir().unwrap();
            let src = Bundle::create(
                &src_tmp.path().join("s.research"),
                b"%PDF-1.5 x",
                Paper::new("Perf"),
                "file",
            )
            .unwrap();
            let notes = src.journal("notes/notes.jsonl");
            for i in 0..200 {
                notes
                    .append(&serde_json::json!({
                        "at": format!("2026-01-01T00:{:02}:{:02}Z", i / 60, i % 60),
                        "text": format!("community note {i}"),
                    }))
                    .unwrap();
            }
            let mut paths = vec!["notes/notes.jsonl".to_string()];
            for i in 0..20 {
                let rel = format!("glossary/term_{i}.json");
                std::fs::write(
                    src.root().join(&rel),
                    serde_json::json!({ "term": i, "explanation": "x".repeat(2000) }).to_string(),
                )
                .unwrap();
                paths.push(rel);
            }
            let (manifest, blob) = build_layer(&src, "arxiv:perf", 1, "bench", &paths).unwrap();

            let dst_tmp = tempfile::tempdir().unwrap();
            let dst = Bundle::create(
                &dst_tmp.path().join("d.research"),
                b"%PDF-1.5 y",
                Paper::new("Perf"),
                "file",
            )
            .unwrap();
            let start = Instant::now();
            pull_layer(&dst, &manifest, &blob).unwrap();
            start.elapsed().as_secs_f64() * 1000.0
        }),
    );

    map.insert(
        "web_bundle_open",
        Box::new(|| {
            // Web bundle-open: the wasm32-wasip1 core instantiates and does
            // a full bundle round trip against preopened storage (module
            // compilation excluded — browsers cache compiled modules).
            use wasmtime::{Engine, Linker, Module, Store};
            use wasmtime_wasi::p1::WasiP1Ctx;
            use wasmtime_wasi::{DirPerms, FilePerms, WasiCtxBuilder};
            let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
            let status = std::process::Command::new("cargo")
                .args([
                    "build",
                    "--bin",
                    "wasi_roundtrip",
                    "-p",
                    "copilot-core",
                    "--no-default-features",
                    "--target",
                    "wasm32-wasip1",
                    "--release",
                ])
                .current_dir(manifest_dir)
                .status()
                .expect("cargo runs");
            assert!(status.success());
            let wasm = manifest_dir.join("../../target/wasm32-wasip1/release/wasi_roundtrip.wasm");
            let engine = Engine::default();
            let module = Module::from_file(&engine, &wasm).unwrap();
            let data = tempfile::tempdir().unwrap();

            let start = Instant::now();
            let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
            wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t| t).unwrap();
            let wasi = WasiCtxBuilder::new()
                .preopened_dir(data.path(), "/data", DirPerms::all(), FilePerms::all())
                .unwrap()
                .build_p1();
            let mut store = Store::new(&engine, wasi);
            let instance = linker.instantiate(&mut store, &module).unwrap();
            instance
                .get_typed_func::<(), ()>(&mut store, "_start")
                .unwrap()
                .call(&mut store, ())
                .unwrap();
            start.elapsed().as_secs_f64() * 1000.0
        }),
    );

    map
}

#[test]
fn budgets_hold() {
    if cfg!(debug_assertions) {
        eprintln!("perf_budgets: skipped in debug build — run with --release");
        return;
    }

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let budgets_path = manifest.join("../../perf/budgets.toml");
    let budgets: Budgets =
        toml::from_str(&std::fs::read_to_string(&budgets_path).unwrap()).unwrap();

    let slack: f64 = std::env::var("PERF_CI_SLACK")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(budgets.reference_machine.ci_slack_factor);

    let mut benches = benchmarks();
    let mut failures = Vec::new();

    for entry in &budgets.budgets {
        match (entry.status.as_str(), benches.remove(entry.id.as_str())) {
            ("enforced", Some(mut bench)) => {
                let measured = bench();
                let limit = entry.budget_ms * slack;
                let verdict = if measured <= limit { "ok  " } else { "FAIL" };
                eprintln!(
                    "{verdict} {id}: {measured:.2} ms (budget {budget} ms × slack {slack} = {limit:.0} ms) — {desc}",
                    id = entry.id,
                    budget = entry.budget_ms,
                    desc = entry.description,
                );
                if measured > limit {
                    failures.push(format!("{}: {measured:.2} ms > {limit:.0} ms", entry.id));
                }
            }
            ("enforced", None) => failures.push(format!(
                "{}: enforced in budgets.toml but no benchmark registered",
                entry.id
            )),
            ("pending", _) => {
                eprintln!(
                    "skip {id}: pending — {desc}",
                    id = entry.id,
                    desc = entry.description
                );
            }
            (other, _) => failures.push(format!("{}: unknown status {other:?}", entry.id)),
        }
    }

    assert!(
        failures.is_empty(),
        "performance budget violations:\n  {}",
        failures.join("\n  ")
    );
}
