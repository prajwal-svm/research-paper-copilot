use std::sync::Mutex;

use copilot_core::bundle::Paper;
use copilot_core::library::{Library, PaperSummary};
use copilot_core::pipeline::{PipelineOptions, ProgressEvent};
use tauri::{AppHandle, Emitter, Manager, State};

/// Library handle shared across commands.
struct AppState {
    library: Mutex<Library>,
    /// Workspace store (notes/canvases/threads): workspace.db inside the
    /// library root. `Err` keeps the app usable when the store can't open
    /// (e.g. created by a newer app) — commands surface the message.
    workspace: Mutex<Result<copilot_core::workspace::WorkspaceStore, String>>,
    /// Lazily-loaded local embedding model (used by semantic search; the
    /// pipeline loads its own). `None` until first use or if loading fails.
    embedder: Mutex<Option<copilot_core::embeddings::Embedder>>,
    telemetry: copilot_core::telemetry::Telemetry,
    providers: copilot_core::provider_config::ProviderStore,
    /// Request ids with a pending cancellation (cancel-anytime streaming).
    cancelled_requests: Mutex<std::collections::HashSet<String>>,
}

/// Serializable command error: plain-language message for the UI.
fn ui_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
fn core_version() -> String {
    copilot_core::version().to_string()
}

/// Dev-only: open the webview devtools (no-op in release builds).
#[tauri::command]
fn open_devtools(window: tauri::WebviewWindow) {
    #[cfg(debug_assertions)]
    window.open_devtools();
    #[cfg(not(debug_assertions))]
    let _ = window;
}

#[tauri::command]
fn list_papers(state: State<AppState>) -> Result<Vec<PaperSummary>, String> {
    state.library.lock().unwrap().list().map_err(ui_err)
}

/// Import a local PDF file. Returns the new paper id immediately; ingestion
/// runs in the background and emits `ingestion-progress` events.
#[tauri::command(async)]
fn import_pdf_file(app: AppHandle, state: State<AppState>, path: String) -> Result<String, String> {
    let pdf = std::fs::read(&path).map_err(|e| format!("could not read {path}: {e}"))?;
    // Real paper identity, best-effort: the embedded PDF title (confirmed
    // against Crossref) beats the filename; offline just falls back.
    let metadata_title = copilot_core::identify::pdf_metadata_title(&pdf);
    let identified = metadata_title
        .as_deref()
        .and_then(copilot_core::identify::crossref_identify)
        .unwrap_or_default();
    let title = identified
        .title
        .clone()
        .or(metadata_title)
        .or_else(|| {
            std::path::Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "Untitled".to_string());
    let library = state.library.lock().unwrap();
    let id = library.new_bundle_id(&title);
    let bundle_root = library.bundle_path(&id);
    drop(library);

    let mut paper = Paper::new(title);
    paper.authors = identified.authors;
    if let Some(doi) = &identified.doi {
        paper.extra.insert(
            "identifiers".to_string(),
            serde_json::json!({"doi": doi}),
        );
    }
    if let Some(published_at) = &identified.published_at {
        paper.extra.insert(
            "published_at".to_string(),
            serde_json::json!(published_at),
        );
    }
    copilot_core::bundle::Bundle::create(&bundle_root, &pdf, paper, "file").map_err(ui_err)?;
    spawn_ingestion(app, id.clone(), bundle_root);
    Ok(id)
}

/// Import LaTeX source via the shipped importer plugin: source → structured
/// import JSON → bundle with a cover PDF (explicit page-geometry
/// degradation) + the paper outline. Ingestion runs like any import.
#[tauri::command(async)]
fn import_latex(app: AppHandle, state: State<AppState>, path: String) -> Result<String, String> {
    let source =
        std::fs::read_to_string(&path).map_err(|e| format!("could not read {path}: {e}"))?;
    let importer = find_plugin(&app, "latex-importer")?;
    let input = serde_json::json!({ "source": source });
    let report = copilot_core::plugin::run_plugin(
        &importer,
        input.to_string().as_bytes(),
        &Default::default(),
    )
    .map_err(ui_err)?;
    let parsed: serde_json::Value = serde_json::from_slice(&report.output).map_err(ui_err)?;
    if let Some(error) = parsed["error"].as_str() {
        return Err(format!("importer: {error}"));
    }

    let title = parsed["metadata"]["title"]
        .as_str()
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            std::path::Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Imported LaTeX".into())
        });
    let mut paper = Paper::new(title.clone());
    paper.abstract_text = parsed["metadata"]["abstract"].as_str().map(str::to_string);
    if let Some(authors) = parsed["metadata"]["authors"].as_array() {
        paper.authors = authors
            .iter()
            .filter_map(|a| a.as_str().map(str::to_string))
            .collect();
    }

    let library = state.library.lock().unwrap();
    let id = library.new_bundle_id(&title);
    let bundle_root = library.bundle_path(&id);
    drop(library);

    let pdf = copilot_core::plugin::cover_pdf(
        &title,
        "imported from LaTeX source - page geometry unavailable",
    );
    copilot_core::bundle::Bundle::create(&bundle_root, &pdf, paper, "latex").map_err(ui_err)?;
    let mut outline = String::new();
    for section in parsed["sections"].as_array().unwrap_or(&Vec::new()) {
        outline.push_str(&format!(
            "## {}\n\n{}\n\n",
            section["heading"].as_str().unwrap_or(""),
            section["text"].as_str().unwrap_or("")
        ));
    }
    std::fs::create_dir_all(bundle_root.join("research")).map_err(ui_err)?;
    std::fs::write(bundle_root.join("research/imported-outline.md"), outline).map_err(ui_err)?;
    spawn_ingestion(app, id.clone(), bundle_root);
    Ok(id)
}

/// Import from an arXiv URL/id or DOI. Blocking fetch (needs the metadata to
/// name the bundle), then background ingestion like file import. When the
/// import came from a citation card, `source_paper_id` records a suggested
/// backlink citing→cited, visible from both papers' link lists.
#[tauri::command(async)]
fn import_url(
    app: AppHandle,
    state: State<AppState>,
    input: String,
    source_paper_id: Option<String>,
) -> Result<String, String> {
    let fetched = copilot_core::arxiv::fetch(&input).map_err(ui_err)?;
    let library = state.library.lock().unwrap();
    let id = library.new_bundle_id(&fetched.title);
    let bundle_root = library.bundle_path(&id);
    drop(library);

    let mut paper = Paper::new(fetched.title.clone());
    paper.authors = fetched.authors.clone();
    paper.abstract_text = fetched.abstract_text.clone();
    let mut identifiers = serde_json::Map::new();
    if let Some(arxiv_id) = &fetched.arxiv_id {
        identifiers.insert("arxiv_id".to_string(), serde_json::json!(arxiv_id));
    }
    if let Some(doi) = &fetched.doi {
        identifiers.insert("doi".to_string(), serde_json::json!(doi));
    }
    if !identifiers.is_empty() {
        paper.extra.insert(
            "identifiers".to_string(),
            serde_json::Value::Object(identifiers),
        );
    }
    if let Some(published_at) = &fetched.published_at {
        paper.extra.insert(
            "published_at".to_string(),
            serde_json::json!(published_at),
        );
    }

    copilot_core::bundle::Bundle::create(&bundle_root, &fetched.pdf, paper, "arxiv")
        .map_err(ui_err)?;
    if let Some(source) = source_paper_id {
        if let Ok(source_bundle) = state.library.lock().unwrap().get(&source) {
            let _ = copilot_core::backlinks::add_link(
                &source_bundle,
                copilot_core::backlinks::PaperLink {
                    to: copilot_core::backlinks::PaperRef::by_id(&id),
                    kind: "citation".to_string(),
                    note: None,
                    at: copilot_core::bundle::now_rfc3339(),
                },
            );
        }
    }
    spawn_ingestion(app, id.clone(), bundle_root);
    Ok(id)
}

/// Does paragraph text look like chart debris (axis ticks, legend fragments)
/// rather than prose? Mostly-numeric short blocks with no sentence structure.
fn looks_like_chart_debris(text: &str) -> bool {
    if text.len() > 240 {
        return false;
    }
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    let digits = text.chars().filter(|c| c.is_ascii_digit()).count();
    let words = text.split_whitespace().count();
    // "60 50 40 30 20 10 0" / "Uplink Downlink" style fragments: numeric-heavy
    // or a couple of label words with no sentence punctuation.
    (digits > letters && words <= 30) || (words <= 3 && !text.contains('.'))
}

/// Fraction of `a` covered by `b` (same page only).
fn overlap_fraction(a: &copilot_core::layout::BBox, b: &copilot_core::layout::BBox) -> f32 {
    if a.page != b.page || a.width <= 0.0 || a.height <= 0.0 {
        return 0.0;
    }
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    ((x1 - x0) * (y1 - y0)) / (a.width * a.height)
}

/// Assemble the parsed paper as markdown. Figures render either as inline
/// data URLs (`inline_images`) or as stable `figure://<id>` placeholders —
/// the placeholder form is what the AI-refinement pass sees (and what gets
/// cached), so images never bloat prompts or the saved file.
fn assemble_markdown(
    bundle: &copilot_core::bundle::Bundle,
    tree: &copilot_core::objects::SemanticTreeDocument,
    title: &str,
    inline_images: bool,
) -> String {
    use copilot_core::objects::ObjectType;

    // Figure/table footprints: paragraph text inside them is chart lettering
    // (axis ticks, legends), not prose.
    let graphic_regions: Vec<copilot_core::layout::BBox> = tree
        .objects
        .iter()
        .filter(|o| matches!(o.object_type, ObjectType::Figure | ObjectType::Table))
        .flat_map(|o| o.regions.iter().cloned())
        .collect();

    let mut md = format!("# {title}\n");
    for object in &tree.objects {
        let text = object.content.text.trim();
        match object.object_type {
            ObjectType::Section => {
                md.push_str("\n## ");
                md.push_str(text);
                md.push('\n');
            }
            ObjectType::Paragraph => {
                let inside_graphic = object.regions.iter().any(|region| {
                    graphic_regions
                        .iter()
                        .any(|graphic| overlap_fraction(region, graphic) > 0.5)
                });
                if inside_graphic || looks_like_chart_debris(text) {
                    continue;
                }
                md.push('\n');
                md.push_str(text);
                md.push('\n');
            }
            ObjectType::Equation => {
                md.push_str("\n$$\n");
                md.push_str(text);
                md.push_str("\n$$\n");
            }
            ObjectType::Figure => {
                let label = object.semantic_label.as_deref().unwrap_or("figure");
                if inline_images {
                    use base64::Engine;
                    let png = bundle.root().join(format!("figures/{}.png", object.id));
                    if let Ok(bytes) = std::fs::read(&png) {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        md.push_str(&format!("\n![{label}](data:image/png;base64,{b64})\n"));
                    }
                } else {
                    md.push_str(&format!("\n![{label}](figure://{})\n", object.id));
                }
                if !text.is_empty() {
                    md.push_str(&format!("\n*{text}*\n"));
                }
            }
            ObjectType::Table => {
                md.push_str("\n```\n");
                md.push_str(text);
                md.push_str("\n```\n");
            }
            _ => {}
        }
    }
    md
}

/// Replace `figure://<id>` placeholders with inline data URLs for display.
fn substitute_figures(bundle: &copilot_core::bundle::Bundle, md: &str) -> String {
    use base64::Engine;
    let mut out = String::with_capacity(md.len());
    let mut rest = md;
    while let Some(pos) = rest.find("figure://") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + "figure://".len()..];
        let end = after.find(')').unwrap_or(after.len());
        let id = &after[..end];
        let png = bundle.root().join(format!("figures/{id}.png"));
        match std::fs::read(&png) {
            Ok(bytes) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                out.push_str(&format!("data:image/png;base64,{b64}"));
            }
            Err(_) => out.push_str("about:blank"),
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// The parsed paper as one markdown document (raw object-layer assembly,
/// figures inlined). Fast, deterministic, no AI.
#[tauri::command]
fn paper_markdown(state: State<AppState>, paper_id: String) -> Result<String, String> {
    let bundle = state.library.lock().unwrap().get(&paper_id).map_err(ui_err)?;
    let tree: copilot_core::objects::SemanticTreeDocument = bundle
        .read_derived_json("semantic_tree.json")
        .map_err(ui_err)?
        .ok_or("This paper hasn't finished parsing yet — try once \"Extracting objects\" completes.")?;
    let title = bundle.metadata().map_err(ui_err)?.paper.title;
    Ok(assemble_markdown(&bundle, &tree, &title, true))
}

const CLEAN_MD_PATH: &str = "research/paper-clean.md";

/// AI-refined markdown, generated once and cached in the bundle
/// (`research/paper-clean.md`). `generate: false` only reads the cache;
/// `regenerate: true` rebuilds it. Sections are cleaned chunk-by-chunk by
/// the Strong-tier model: prose reflowed, chart debris dropped, tables
/// formatted, figure placeholders preserved, mermaid where a flow is
/// described. Figure placeholders are swapped for data URLs on read.
#[tauri::command(async)]
fn paper_markdown_clean(
    state: State<AppState>,
    paper_id: String,
    generate: bool,
    regenerate: Option<bool>,
) -> Result<Option<String>, String> {
    let bundle = state.library.lock().unwrap().get(&paper_id).map_err(ui_err)?;
    let cache = bundle.root().join(CLEAN_MD_PATH);
    if !regenerate.unwrap_or(false) {
        if let Ok(cached) = std::fs::read_to_string(&cache) {
            return Ok(Some(substitute_figures(&bundle, &cached)));
        }
    }
    if !generate {
        return Ok(None);
    }

    let tree: copilot_core::objects::SemanticTreeDocument = bundle
        .read_derived_json("semantic_tree.json")
        .map_err(ui_err)?
        .ok_or("This paper hasn't finished parsing yet.")?;
    let title = bundle.metadata().map_err(ui_err)?.paper.title;
    let raw = assemble_markdown(&bundle, &tree, &title, false);

    // Chunk on section boundaries to stay well inside context windows.
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for part in raw.split("\n## ") {
        let part = if current.is_empty() && chunks.is_empty() {
            part.to_string()
        } else {
            format!("\n## {part}")
        };
        if !current.is_empty() && current.len() + part.len() > 9_000 {
            chunks.push(std::mem::take(&mut current));
        }
        current.push_str(&part);
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    let (provider, _config) = pick_provider(
        &state.providers,
        copilot_core::ai::ModelClass::Strong,
    )?;
    let total = chunks.len();
    let mut clean = String::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let prompt = format!(
            "You are converting a research paper's raw PDF-extracted text into clean, \
             publication-quality Markdown. This is part {part} of {total} of the paper — \
             convert ONLY this part, do not summarize.\n\
             Rules:\n\
             - Reflow broken lines into proper paragraphs; repair hyphenation and spacing \
             artifacts (e.g. \"s ru 100 o H #\" is extraction garbage — reconstruct or drop it).\n\
             - DELETE stray chart axis ticks, legend fragments, and isolated number runs \
             that leaked from figures.\n\
             - Keep ALL substantive prose and headings (## …). Keep equations as $$ … $$.\n\
             - Lines of the form ![…](figure://…) are image references: keep them EXACTLY \
             as-is, in place, with their *italic* captions.\n\
             - Format tabular data as Markdown tables when the structure is clear.\n\
             - If a passage clearly describes a workflow or architecture, you may add one \
             ```mermaid flowchart for it.\n\
             - Output ONLY the Markdown for this part, no commentary.\n\
             ---\n{chunk}",
            part = i + 1,
        );
        let messages = [copilot_core::ai::ChatMessage {
            images: Vec::new(),
            role: "user".to_string(),
            content: prompt,
        }];
        let piece = provider
            .stream_chat(&messages, &mut |_| {})
            .map_err(|e| format!("AI refinement failed on part {}/{total}: {e}", i + 1))?;
        if !clean.is_empty() {
            clean.push_str("\n\n");
        }
        clean.push_str(piece.trim());
    }

    if let Some(parent) = cache.parent() {
        std::fs::create_dir_all(parent).map_err(ui_err)?;
    }
    std::fs::write(&cache, &clean).map_err(ui_err)?;
    Ok(Some(substitute_figures(&bundle, &clean)))
}

/// Run `f` against the workspace store, surfacing open-failures (e.g. a
/// newer-schema db) as plain command errors instead of crashing startup.
fn with_workspace<T>(
    state: &State<AppState>,
    f: impl FnOnce(
        &copilot_core::workspace::WorkspaceStore,
    ) -> Result<T, copilot_core::workspace::WorkspaceError>,
) -> Result<T, String> {
    let guard = state.workspace.lock().unwrap();
    match guard.as_ref() {
        Ok(store) => f(store).map_err(ui_err),
        Err(e) => Err(format!("workspace store unavailable: {e}")),
    }
}

/// Live workspace items (notes/canvases/threads), newest-updated first.
#[tauri::command]
fn workspace_items_list(
    state: State<AppState>,
    kind: Option<String>,
) -> Result<Vec<copilot_core::workspace::WorkspaceItem>, String> {
    with_workspace(&state, |ws| ws.list_items(kind.as_deref()))
}

#[tauri::command]
fn workspace_item_rename(
    state: State<AppState>,
    id: uuid::Uuid,
    title: String,
) -> Result<(), String> {
    with_workspace(&state, |ws| ws.rename_item(id, &title))
}

/// Soft delete — the tombstone stays for future sync merge.
#[tauri::command]
fn workspace_item_delete(state: State<AppState>, id: uuid::Uuid) -> Result<(), String> {
    with_workspace(&state, |ws| ws.delete_item(id))
}

/// Create a note (registry row + empty document) and return it.
#[tauri::command]
fn workspace_note_create(
    state: State<AppState>,
    title: Option<String>,
) -> Result<copilot_core::workspace::WorkspaceItem, String> {
    with_workspace(&state, |ws| {
        ws.note_create(title.as_deref().unwrap_or("Untitled"))
    })
}

#[tauri::command]
fn workspace_note_get(
    state: State<AppState>,
    id: uuid::Uuid,
) -> Result<Option<copilot_core::workspace::NoteDoc>, String> {
    with_workspace(&state, |ws| ws.note_get(id))
}

/// Autosave: BlockNote JSON + markdown mirror; bumps library recency.
#[tauri::command]
fn workspace_note_save(
    state: State<AppState>,
    id: uuid::Uuid,
    content: String,
    markdown: String,
) -> Result<(), String> {
    with_workspace(&state, |ws| ws.note_save(id, &content, &markdown))
}

/// Reconcile a note's mention backlinks with its current document.
#[tauri::command]
fn workspace_note_refs_sync(
    state: State<AppState>,
    id: uuid::Uuid,
    mentions: Vec<copilot_core::workspace::MentionRef>,
) -> Result<(), String> {
    with_workspace(&state, |ws| ws.note_sync_refs(id, &mentions))
}

/// A figure object's extracted PNG as a data URL, for pinning onto a
/// canvas. `None` when the figure has no rendered image.
#[tauri::command]
fn paper_figure_data_url(
    state: State<AppState>,
    paper_id: String,
    object_id: uuid::Uuid,
) -> Result<Option<String>, String> {
    use base64::Engine;
    let bundle = state.library.lock().unwrap().get(&paper_id).map_err(ui_err)?;
    let path = bundle.root().join(format!("figures/{object_id}.png"));
    Ok(std::fs::read(path).ok().map(|bytes| {
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        )
    }))
}

/// Create a canvas (registry row + empty Excalidraw scene) and return it.
#[tauri::command]
fn workspace_canvas_create(
    state: State<AppState>,
    title: Option<String>,
) -> Result<copilot_core::workspace::WorkspaceItem, String> {
    with_workspace(&state, |ws| {
        ws.canvas_create(title.as_deref().unwrap_or("Untitled canvas"))
    })
}

#[tauri::command]
fn workspace_canvas_get(
    state: State<AppState>,
    id: uuid::Uuid,
) -> Result<Option<copilot_core::workspace::CanvasDoc>, String> {
    with_workspace(&state, |ws| ws.canvas_get(id))
}

/// Autosave: Excalidraw scene JSON + thumbnail PNG data URL; bumps recency.
#[tauri::command]
fn workspace_canvas_save(
    state: State<AppState>,
    id: uuid::Uuid,
    scene: String,
    thumbnail: String,
) -> Result<(), String> {
    with_workspace(&state, |ws| ws.canvas_save(id, &scene, &thumbnail))
}

/// Reconcile a canvas's pinned-content backlinks with its current scene.
#[tauri::command]
fn workspace_canvas_refs_sync(
    state: State<AppState>,
    id: uuid::Uuid,
    pins: Vec<copilot_core::workspace::MentionRef>,
) -> Result<(), String> {
    with_workspace(&state, |ws| ws.canvas_sync_refs(id, &pins))
}

/// AI-proposed canvas edit: the model returns a JSON array of Excalidraw
/// element skeletons for the instruction, given the scene's structural
/// summary and (optionally) a PNG of the current board. Streamed as
/// `ai-stream`; the editor parses, previews via Confirmation, and merges
/// only on approval — nothing is applied here.
#[tauri::command(async)]
fn canvas_ai_edit(
    app: AppHandle,
    state: State<AppState>,
    request_id: String,
    instruction: String,
    summary: String,
    image: Option<copilot_core::ai::ImageAttachment>,
) -> Result<String, String> {
    let (provider, config) = pick_provider(
        &state.providers,
        copilot_core::ai::ModelClass::Strong,
    )?;
    let prompt = format!(
        "You extend an Excalidraw diagram. The user's instruction: {instruction}\n\n\
         Current canvas (structural summary):\n{summary}\n\n\
         Return ONLY a JSON array of Excalidraw element skeletons to ADD (do not \
         repeat existing elements). Each element is one of:\n\
         {{\"type\":\"rectangle\",\"x\":<n>,\"y\":<n>,\"width\":<n>,\"height\":<n>,\"label\":{{\"text\":\"…\"}}}}\n\
         {{\"type\":\"ellipse\",\"x\":<n>,\"y\":<n>,\"width\":<n>,\"height\":<n>,\"label\":{{\"text\":\"…\"}}}}\n\
         {{\"type\":\"text\",\"x\":<n>,\"y\":<n>,\"text\":\"…\"}}\n\
         {{\"type\":\"arrow\",\"x\":<n>,\"y\":<n>,\"width\":<n>,\"height\":<n>}}\n\
         Lay elements out with sensible non-overlapping coordinates. Output only the JSON array."
    );
    let images = image.into_iter().collect::<Vec<_>>();
    let messages = [copilot_core::ai::ChatMessage {
        images,
        role: "user".to_string(),
        content: prompt,
    }];
    let emit = |event: AiStreamEvent| {
        let _ = app.emit("ai-stream", event);
    };
    emit(AiStreamEvent {
        host: Some(config.host()),
        ..AiStreamEvent::empty(&request_id)
    });
    let is_cancelled = || {
        state
            .cancelled_requests
            .lock()
            .unwrap()
            .contains(&request_id)
    };
    let result = provider.stream_chat_cancellable(
        &messages,
        &mut |token| {
            emit(AiStreamEvent {
                token: Some(token.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
        },
        &is_cancelled,
    );
    state.cancelled_requests.lock().unwrap().remove(&request_id);
    match result {
        Ok(full) => {
            emit(AiStreamEvent {
                done: Some(true),
                ..AiStreamEvent::empty(&request_id)
            });
            Ok(full)
        }
        Err(copilot_core::ai::AiError::Cancelled) => {
            emit(AiStreamEvent {
                cancelled: Some(true),
                ..AiStreamEvent::empty(&request_id)
            });
            Ok(String::new())
        }
        Err(e) => {
            emit(AiStreamEvent {
                error: Some(e.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
            Err(ui_err(e))
        }
    }
}

// ---- Chat threads (chat-threads) ----

/// Fetch a URL's readable text for chat context (cached is a v2 concern;
/// v1 fetches each send).
#[tauri::command(async)]
fn fetch_url_context(url: String) -> Result<String, String> {
    copilot_core::refs_context::fetch_url_text(&url).map_err(ui_err)
}

/// Extract text from a raw PDF (not necessarily in the library) for context.
#[tauri::command(async)]
fn extract_pdf_text(path: String) -> Result<String, String> {
    copilot_core::refs_context::extract_pdf_text(std::path::Path::new(&path)).map_err(ui_err)
}

#[tauri::command]
fn workspace_chat_create(
    state: State<AppState>,
    title: Option<String>,
) -> Result<copilot_core::workspace::WorkspaceItem, String> {
    with_workspace(&state, |ws| {
        ws.chat_create(title.as_deref().unwrap_or("New chat"))
    })
}

#[tauri::command]
fn workspace_chat_messages(
    state: State<AppState>,
    chat_id: uuid::Uuid,
) -> Result<Vec<copilot_core::workspace::ChatMessageRow>, String> {
    with_workspace(&state, |ws| ws.chat_messages(chat_id))
}

#[tauri::command]
fn workspace_chat_edit_message(
    state: State<AppState>,
    message_id: uuid::Uuid,
    content: String,
) -> Result<(), String> {
    with_workspace(&state, |ws| ws.chat_edit_message(message_id, &content))
}

#[tauri::command]
fn workspace_chat_delete_message(
    state: State<AppState>,
    message_id: uuid::Uuid,
) -> Result<(), String> {
    with_workspace(&state, |ws| ws.chat_delete_message(message_id))
}

#[tauri::command]
fn workspace_chat_refs_sync(
    state: State<AppState>,
    id: uuid::Uuid,
    refs: Vec<copilot_core::workspace::MentionRef>,
) -> Result<(), String> {
    with_workspace(&state, |ws| ws.chat_sync_refs(id, &refs))
}

/// A reference the composer attached, resolved to context text server-side.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ChatContextRef {
    Paper { paper_id: String },
    Object { paper_id: String, object_id: String },
    Url { url: String },
    Pdf { path: String },
}

/// Resolve one reference to a fenced context block. Failures degrade to a
/// short note rather than aborting the whole message.
fn resolve_chat_ref(state: &AppState, r: &ChatContextRef) -> String {
    match r {
        ChatContextRef::Paper { paper_id } => {
            let library = state.library.lock().unwrap();
            match library.get(paper_id).ok().and_then(|b| b.metadata().ok()) {
                Some(meta) => format!(
                    "[paper: {}]\n{}",
                    meta.paper.title,
                    meta.paper.abstract_text.unwrap_or_default()
                ),
                None => format!("[paper {paper_id}: unavailable]"),
            }
        }
        ChatContextRef::Object { paper_id, object_id } => {
            let library = state.library.lock().unwrap();
            let text = library.get(paper_id).ok().and_then(|bundle| {
                let tree: Option<copilot_core::objects::SemanticTreeDocument> =
                    bundle.read_derived_json("semantic_tree.json").ok().flatten();
                tree.and_then(|t| {
                    t.objects
                        .into_iter()
                        .find(|o| o.id.to_string() == *object_id)
                        .map(|o| o.content.text)
                })
            });
            match text {
                Some(t) => format!("[object from {paper_id}]\n{t}"),
                None => format!("[object {object_id}: unavailable]"),
            }
        }
        ChatContextRef::Url { url } => match copilot_core::refs_context::fetch_url_text(url) {
            Ok(text) => format!("[url: {url}]\n{text}"),
            Err(e) => format!("[url {url}: {e}]"),
        },
        ChatContextRef::Pdf { path } => {
            match copilot_core::refs_context::extract_pdf_text(std::path::Path::new(path)) {
                Ok(text) => format!("[pdf: {path}]\n{text}"),
                Err(e) => format!("[pdf {path}: {e}]"),
            }
        }
    }
}

/// Stream a chat-thread answer. Assembles context from resolved refs,
/// records the user turn, streams the assistant turn, and persists it (or an
/// `incomplete` partial on failure) — the workspace-store analogue of
/// `ai_stream`. Auto-titles a fresh chat after the first exchange.
#[tauri::command(async)]
fn chat_stream(
    app: AppHandle,
    state: State<AppState>,
    request_id: String,
    chat_id: uuid::Uuid,
    content: String,
    refs: Vec<ChatContextRef>,
    images: Option<Vec<copilot_core::ai::ImageAttachment>>,
) -> Result<String, String> {
    let (provider, config) = pick_provider(
        &state.providers,
        copilot_core::ai::ModelClass::Strong,
    )?;

    // Prior turns (persisted) + this user message become the thread.
    let history = with_workspace(&state, |ws| ws.chat_messages(chat_id))?;
    let is_first = history.is_empty();
    let mut messages: Vec<copilot_core::ai::ChatMessage> = history
        .iter()
        .map(|m| copilot_core::ai::ChatMessage {
            images: Vec::new(),
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect();

    // Assemble referenced context as fenced blocks appended to the message.
    let mut user_content = content.clone();
    for r in &refs {
        let block = resolve_chat_ref(state.inner(), r);
        user_content.push_str(&format!("\n\n---\n{block}\n---"));
    }
    let images = images.unwrap_or_default();
    messages.push(copilot_core::ai::ChatMessage {
        images: images.clone(),
        role: "user".to_string(),
        content: user_content,
    });

    // Record the user turn (the visible text, not the appended context).
    with_workspace(&state, |ws| {
        ws.chat_append_message(chat_id, "user", &content, Some("ask"), false)
    })?;

    let emit = |event: AiStreamEvent| {
        let _ = app.emit("ai-stream", event);
    };
    emit(AiStreamEvent {
        host: Some(config.host()),
        ..AiStreamEvent::empty(&request_id)
    });
    let is_cancelled = || {
        state
            .cancelled_requests
            .lock()
            .unwrap()
            .contains(&request_id)
    };
    let mut accumulated = String::new();
    let result = provider.stream_chat_cancellable(
        &messages,
        &mut |token| {
            accumulated.push_str(token);
            emit(AiStreamEvent {
                token: Some(token.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
        },
        &is_cancelled,
    );
    state.cancelled_requests.lock().unwrap().remove(&request_id);

    match result {
        Ok(full) => {
            with_workspace(&state, |ws| {
                ws.chat_append_message(chat_id, "assistant", &full, None, false)
            })?;
            emit(AiStreamEvent {
                done: Some(true),
                ..AiStreamEvent::empty(&request_id)
            });
            if is_first {
                auto_title_chat(state.inner(), chat_id, &content, &full);
            }
            Ok(full)
        }
        Err(copilot_core::ai::AiError::Cancelled) => {
            if !accumulated.is_empty() {
                let _ = with_workspace(&state, |ws| {
                    ws.chat_append_message(chat_id, "assistant", &accumulated, None, true)
                });
            }
            emit(AiStreamEvent {
                cancelled: Some(true),
                ..AiStreamEvent::empty(&request_id)
            });
            Ok(accumulated)
        }
        Err(e) => {
            if !accumulated.is_empty() {
                let _ = with_workspace(&state, |ws| {
                    ws.chat_append_message(chat_id, "assistant", &accumulated, None, true)
                });
            }
            emit(AiStreamEvent {
                error: Some(e.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
            Err(ui_err(e))
        }
    }
}

/// Best-effort chat title from the first exchange (Light tier). Silent on
/// failure — the truncated first message already serves as a fallback title.
fn auto_title_chat(state: &AppState, chat_id: uuid::Uuid, question: &str, answer: &str) {
    let title = (|| {
        let (provider, _) =
            pick_provider(&state.providers, copilot_core::ai::ModelClass::Light).ok()?;
        let messages = [copilot_core::ai::ChatMessage {
            images: Vec::new(),
            role: "user".to_string(),
            content: format!(
                "Give a 3-6 word title (no quotes, no punctuation at the end) for this chat:\nQ: {}\nA: {}",
                question.chars().take(400).collect::<String>(),
                answer.chars().take(400).collect::<String>(),
            ),
        }];
        provider.stream_chat(&messages, &mut |_| {}).ok()
    })();
    let title = title
        .map(|t| t.trim().trim_matches('"').chars().take(80).collect::<String>())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| question.chars().take(60).collect());
    let guard = state.workspace.lock().unwrap();
    if let Ok(ws) = guard.as_ref() {
        let _ = ws.chat_set_title(chat_id, &title);
    }
}

/// AI assist inside the note editor: one-shot instruction over selected
/// text, streamed as `ai-stream` events (same contract the reader uses).
/// Nothing is persisted — the editor owns accept/discard.
#[tauri::command(async)]
fn note_ai(
    app: AppHandle,
    state: State<AppState>,
    request_id: String,
    action: String,
    text: String,
) -> Result<String, String> {
    let (provider, config) = pick_provider(
        &state.providers,
        copilot_core::ai::ModelClass::Strong,
    )?;
    let instruction = match action.as_str() {
        "improve" => "Rewrite the following note text to be clearer and better written. Keep the meaning, keep markdown formatting. Output only the rewritten text.",
        "summarize" => "Summarize the following note text concisely in markdown. Output only the summary.",
        "expand" => "Expand the following note text with more depth and detail, in the same voice. Output only the expanded text.",
        _ => "Continue writing from the following note text, matching its tone and formatting. Output only the continuation.",
    };
    let messages = [copilot_core::ai::ChatMessage {
        images: Vec::new(),
        role: "user".to_string(),
        content: format!("{instruction}\n\n---\n{text}"),
    }];
    let emit = |event: AiStreamEvent| {
        let _ = app.emit("ai-stream", event);
    };
    emit(AiStreamEvent {
        host: Some(config.host()),
        ..AiStreamEvent::empty(&request_id)
    });
    let is_cancelled = || {
        state
            .cancelled_requests
            .lock()
            .unwrap()
            .contains(&request_id)
    };
    let result = provider.stream_chat_cancellable(
        &messages,
        &mut |token| {
            emit(AiStreamEvent {
                token: Some(token.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
        },
        &is_cancelled,
    );
    state.cancelled_requests.lock().unwrap().remove(&request_id);
    match result {
        Ok(full) => {
            emit(AiStreamEvent {
                done: Some(true),
                ..AiStreamEvent::empty(&request_id)
            });
            Ok(full)
        }
        Err(copilot_core::ai::AiError::Cancelled) => {
            emit(AiStreamEvent {
                cancelled: Some(true),
                ..AiStreamEvent::empty(&request_id)
            });
            Ok(String::new())
        }
        Err(e) => {
            emit(AiStreamEvent {
                error: Some(e.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
            Err(ui_err(e))
        }
    }
}

/// Backlinks: which workspace entities reference this paper.
#[tauri::command]
fn workspace_refs_to_paper(
    state: State<AppState>,
    paper_id: String,
) -> Result<Vec<copilot_core::workspace::WorkspaceRef>, String> {
    with_workspace(&state, |ws| ws.refs_to_paper(&paper_id))
}

/// Export every live workspace entity to a directory; returns kind→count.
#[tauri::command]
fn workspace_export_all(
    state: State<AppState>,
    dir: String,
) -> Result<Vec<(String, usize)>, String> {
    with_workspace(&state, |ws| ws.export_all(std::path::Path::new(&dir)))
}

/// Per-stage pipeline state for a paper, read from bundle metadata — the
/// import-progress UI shows this after view switches and app restarts.
#[tauri::command]
fn pipeline_status(
    state: State<AppState>,
    paper_id: String,
) -> Result<Vec<copilot_core::pipeline::StageStatus>, String> {
    let library = state.library.lock().unwrap();
    let bundle = library.get(&paper_id).map_err(ui_err)?;
    Ok(copilot_core::pipeline::status_snapshot(&bundle))
}

/// Re-run the ingestion pipeline. Stages recorded complete at their current
/// version are skipped, so this is the "re-run the failed stage" action —
/// deterministic and idempotent.
#[tauri::command(async)]
fn retry_ingestion(
    app: AppHandle,
    state: State<AppState>,
    paper_id: String,
) -> Result<(), String> {
    let bundle_root = {
        let library = state.library.lock().unwrap();
        library.bundle_path(&paper_id)
    };
    spawn_ingestion(app, paper_id, bundle_root);
    Ok(())
}

/// A paper's links, both directions ("links out" / "links here").
#[tauri::command]
fn paper_links(state: State<AppState>, paper_id: String) -> Result<serde_json::Value, String> {
    let library = state.library.lock().unwrap();
    let bundle = library.get(&paper_id).map_err(ui_err)?;
    let out = copilot_core::backlinks::links_out(&bundle).map_err(ui_err)?;
    let incoming = copilot_core::backlinks::links_in(&library, &paper_id).map_err(ui_err)?;
    Ok(serde_json::json!({"out": out, "in": incoming}))
}

/// Record a manual paper-to-paper link.
#[tauri::command]
fn paper_link_add(
    state: State<AppState>,
    paper_id: String,
    target_paper_id: String,
    note: Option<String>,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::backlinks::add_link(
        &bundle,
        copilot_core::backlinks::PaperLink {
            to: copilot_core::backlinks::PaperRef::by_id(&target_paper_id),
            kind: "manual".to_string(),
            note,
            at: copilot_core::bundle::now_rfc3339(),
        },
    )
    .map_err(ui_err)?;
    Ok(())
}

#[tauri::command]
fn delete_paper(state: State<AppState>, id: String) -> Result<(), String> {
    let library = state.library.lock().unwrap();
    library.delete(&id).map_err(ui_err)?;
    // Deletion propagates to other devices as a sync tombstone (they trash,
    // never destroy). No-op when sync is off.
    let _ = copilot_core::sync::engine::SyncEngine::record_tombstone(library.root(), &id);
    // Cache-class index: a failure here never blocks the delete.
    if let Ok(mut index) = copilot_core::graph_index::GraphIndex::open(library.root()) {
        let _ = index.remove_paper(&id);
    }
    Ok(())
}

#[tauri::command]
fn paper_toggle_star(state: State<AppState>, id: String) -> Result<bool, String> {
    state
        .library
        .lock()
        .unwrap()
        .toggle_starred(&id)
        .map_err(ui_err)
}

#[tauri::command]
fn paper_set_priority(
    state: State<AppState>,
    id: String,
    priority: Option<String>,
) -> Result<(), String> {
    state
        .library
        .lock()
        .unwrap()
        .set_priority(&id, priority.as_deref())
        .map_err(ui_err)
}

/// The paper's concept graph (with user corrections applied), or `None`
/// while the concepts stage hasn't run yet.
#[tauri::command]
fn graph_get(
    state: State<AppState>,
    paper_id: String,
) -> Result<Option<copilot_core::concepts::KnowledgeGraph>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    bundle
        .read_derived_json("knowledge_graph.json")
        .map_err(ui_err)
}

/// Validate a bundle against the published `.research` JSON Schemas.
/// Returns every violation by file and JSON path (empty = valid).
#[tauri::command]
fn bundle_validate(
    state: State<AppState>,
    paper_id: String,
) -> Result<Vec<copilot_core::schemas::Violation>, String> {
    let path = state.library.lock().unwrap().bundle_path(&paper_id);
    copilot_core::schemas::validate_bundle(&path).map_err(ui_err)
}

/// Record a graph correction (delete edge / rename / delete / merge node),
/// apply it to the stored graph, and refresh the library index. Corrections
/// live in an append-only journal, so they survive re-extraction.
#[tauri::command]
fn graph_override(
    state: State<AppState>,
    paper_id: String,
    event: copilot_core::concepts::GraphOverride,
) -> Result<copilot_core::concepts::KnowledgeGraph, String> {
    let library = state.library.lock().unwrap();
    let bundle = library.get(&paper_id).map_err(ui_err)?;
    copilot_core::concepts::record_override(&bundle, event).map_err(ui_err)?;
    let graph = copilot_core::concepts::reapply_overrides(&bundle).map_err(ui_err)?;
    if let Ok(mut index) = copilot_core::graph_index::GraphIndex::open(library.root()) {
        let _ = index.index_paper(&paper_id, &graph);
    }
    Ok(graph)
}

#[tauri::command]
fn open_paper(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    let library = state.library.lock().unwrap();
    library.touch_opened(&id).map_err(ui_err)?;
    // Re-run the pipeline in the background: stages current at their
    // pipeline_version skip instantly; stages whose parser improved since
    // this paper was ingested re-run, so old bundles pick up fixes.
    spawn_ingestion(app, id.clone(), library.bundle_path(&id));
    Ok(())
}

/// In-paper search: exact always; semantic when embeddings + model exist.
/// Degradation is explicit in the result, never an error.
#[tauri::command(async)]
fn search_paper(
    state: State<AppState>,
    id: String,
    query: String,
) -> Result<copilot_core::search::SearchResults, String> {
    let bundle = state.library.lock().unwrap().get(&id).map_err(ui_err)?;
    let mut embedder_slot = state.embedder.lock().unwrap();
    if embedder_slot.is_none() {
        // First search pays the model load (~130 ms warm cache); failures
        // (e.g. offline before first model download) degrade to exact-only.
        *embedder_slot = copilot_core::embeddings::Embedder::load().ok();
    }
    copilot_core::search::search(&bundle, embedder_slot.as_ref(), &query, 20).map_err(ui_err)
}

/// Raw bytes of the paper's immutable original PDF (feeds pdf.js).
#[tauri::command]
fn read_original_pdf(state: State<AppState>, id: String) -> Result<Vec<u8>, String> {
    let bundle = state.library.lock().unwrap().get(&id).map_err(ui_err)?;
    std::fs::read(bundle.original_pdf_path()).map_err(ui_err)
}

/// A derived JSON artifact from the bundle (layout.json, semantic_tree.json,
/// citations.json …) as a JSON value; null when the stage hasn't run.
#[tauri::command]
fn read_artifact(
    state: State<AppState>,
    id: String,
    artifact: String,
) -> Result<Option<serde_json::Value>, String> {
    // Only bundle-relative JSON artifacts; no path escapes.
    if artifact.contains("..") || artifact.starts_with('/') || !artifact.ends_with(".json") {
        return Err(format!("not a bundle artifact: {artifact}"));
    }
    let bundle = state.library.lock().unwrap().get(&id).map_err(ui_err)?;
    bundle.read_derived_json(&artifact).map_err(ui_err)
}

/// Provider availability snapshot (keychain lookups + local Ollama probe).
#[tauri::command(async)]
fn ai_provider_statuses() -> Vec<copilot_core::ai::ProviderStatus> {
    copilot_core::ai::provider_statuses()
}

/// Validate a key with a test call, then store it in the OS keychain.
#[tauri::command(async)]
fn ai_set_key(kind: copilot_core::ai::ProviderKind, key: String) -> Result<String, String> {
    copilot_core::ai::validate_and_store_key(kind, &key).map_err(ui_err)
}

#[tauri::command(async)]
fn ai_delete_key(kind: copilot_core::ai::ProviderKind) -> Result<(), String> {
    copilot_core::ai::delete_key(kind).map_err(ui_err)
}

/// Pre-generated enrichment for an object (bundled sample paper ships these
/// so first-run works with no key and no network). `None` when absent.
#[tauri::command]
fn read_pregenerated(
    state: State<AppState>,
    paper_id: String,
    object_id: uuid::Uuid,
) -> Result<Option<String>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let path = bundle
        .root()
        .join(format!("glossary/pregenerated/{object_id}.md"));
    if !path.is_file() {
        return Ok(None);
    }
    std::fs::read_to_string(&path).map(Some).map_err(ui_err)
}

#[derive(Clone, serde::Serialize)]
struct AiStreamEvent {
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    done: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Egress indicator: the host paper content is being sent to (first
    /// event of each stream). Always the actual host, never a brand name.
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cancelled: Option<bool>,
}

impl AiStreamEvent {
    fn empty(request_id: &str) -> Self {
        AiStreamEvent {
            request_id: request_id.to_string(),
            token: None,
            done: None,
            error: None,
            host: None,
            cancelled: None,
        }
    }
}

/// First configured provider (store order: first-party kinds, then presets/
/// custom entries) that has a key — or a live Ollama. Returns the runnable
/// provider plus its config (for budget, host, timeout).
fn pick_provider(
    store: &copilot_core::provider_config::ProviderStore,
    class: copilot_core::ai::ModelClass,
) -> Result<
    (
        copilot_core::ai::Provider,
        copilot_core::provider_config::ProviderConfig,
    ),
    String,
> {
    use copilot_core::ai::ProviderKind;
    let configs = store.load();
    // Pass 1: providers the user explicitly configured with a key — a saved
    // key is deliberate intent and outranks a merely-running local Ollama.
    for config in &configs {
        if !config.protocol.needs_key() {
            continue;
        }
        if let Ok(provider) = config.provider(class) {
            return Ok((provider, config.clone()));
        }
    }
    // Pass 2: keyless local providers (Ollama), liveness-checked.
    for config in &configs {
        if config.protocol != ProviderKind::Ollama {
            continue;
        }
        if let Ok(provider) = config.provider(class) {
            if provider.validate().is_ok() {
                return Ok((provider, config.clone()));
            }
        }
    }
    Err(
        "No AI provider configured. Add an API key in Settings, or start Ollama for local models."
            .to_string(),
    )
}

/// Which provider/model answers chat right now (mirrors pick_provider's
/// selection for the strong tier — what "Ask" actually uses).
#[derive(serde::Serialize)]
struct ActiveModelView {
    provider: String,
    model: String,
    host: String,
}

#[tauri::command]
fn active_model(state: State<AppState>) -> Result<ActiveModelView, String> {
    let (_, config) = pick_provider(&state.providers, copilot_core::ai::ModelClass::Strong)?;
    Ok(ActiveModelView {
        provider: config.protocol.id().to_string(),
        model: config.model_for(copilot_core::ai::ModelClass::Strong),
        host: config.host(),
    })
}

/// Read a user-picked file for chat attachment: images return base64 +
/// mime; text files return clamped UTF-8 (context blocks). Binary
/// non-image files are refused with a clear message.
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AttachmentView {
    Image {
        media_type: String,
        data_b64: String,
    },
    Text {
        name: String,
        content: String,
        truncated: bool,
    },
}

#[tauri::command]
fn load_attachment(path: String) -> Result<AttachmentView, String> {
    const TEXT_CAP: usize = 60_000;
    let bytes = std::fs::read(&path).map_err(|e| format!("could not read {path}: {e}"))?;
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    let image_mime = match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    };
    if let Some(media_type) = image_mime {
        use base64::Engine;
        return Ok(AttachmentView::Image {
            media_type: media_type.to_string(),
            data_b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        });
    }
    match String::from_utf8(bytes) {
        Ok(text) => {
            let truncated = text.len() > TEXT_CAP;
            let content = text.chars().take(TEXT_CAP).collect();
            Ok(AttachmentView::Text {
                name,
                content,
                truncated,
            })
        }
        Err(_) => Err(format!(
            "{name} is binary — attach images (png/jpg/gif/webp) or text files"
        )),
    }
}

/// Stream an AI action anchored to an object. Tokens arrive as `ai-stream`
/// events tagged with `request_id`; the full text is also returned.
///
/// The exchange persists in the object's append-only chat journal: the user
/// turn before streaming, the assistant turn after — with `incomplete: true`
/// when the stream failed mid-flight, so partial answers survive honestly
/// and the conversation log is never corrupted.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn ai_stream(
    app: AppHandle,
    state: State<AppState>,
    request_id: String,
    paper_id: String,
    object_id: uuid::Uuid,
    action: copilot_core::context::Action,
    question: Option<String>,
    // Set for ad-hoc selections (text drag / region marquee): the gathered
    // text becomes the anchor instead of an extracted object.
    adhoc_text: Option<String>,
    // Inline image attachments (screenshots, figures) for multimodal models.
    images: Option<Vec<copilot_core::ai::ImageAttachment>>,
) -> Result<String, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let tree: copilot_core::objects::SemanticTreeDocument = bundle
        .read_derived_json("semantic_tree.json")
        .map_err(ui_err)?
        .ok_or("This paper is still being processed — try again shortly.")?;
    let title = bundle.metadata().map_err(ui_err)?.paper.title;

    // Resume the object's conversation from its journal.
    let history = copilot_core::chat::history(&bundle, object_id).map_err(ui_err)?;
    let thread = copilot_core::chat::as_thread(&history);

    // Tables answer from structured data, not the image.
    let table_data = tree
        .objects
        .iter()
        .find(|o| o.id == object_id && o.object_type == copilot_core::objects::ObjectType::Table)
        .and_then(|o| {
            bundle
                .read_derived_json::<serde_json::Value>(&format!("tables/{}.json", o.id))
                .ok()
                .flatten()
                .and_then(|artifact| artifact.get("data").cloned())
        });

    // No provider → designed no-key state, before anything is persisted.
    // Chosen first so the config's context budget (e.g. 1M window) applies.
    let (provider, config) = pick_provider(&state.providers, action.model_class())?;
    let budget = config.context_budget_tokens(action.model_class());

    // Code-understanding (v3): when the code map links this object to
    // repository locations, the prompt learns them — "where is Equation 12
    // in the code?" answers with line-level references, never a repo dump.
    let assembly_question = {
        let code_lines: Vec<String> = copilot_core::codemap::get(&bundle)
            .ok()
            .flatten()
            .map(|map| {
                map.links
                    .iter()
                    .filter(|l| l.object == object_id)
                    .map(|l| {
                        format!(
                            "{}{} lines {}–{}",
                            l.file,
                            l.function
                                .as_deref()
                                .map(|f| format!(" ({f})"))
                                .unwrap_or_default(),
                            l.start_line,
                            l.end_line
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        if code_lines.is_empty() {
            question.clone()
        } else {
            Some(format!(
                "{}\n[Repository locations implementing this object: {}. Cite them by file and line when relevant.]",
                question.as_deref().unwrap_or(""),
                code_lines.join("; ")
            ))
        }
    };
    let mut context = match &adhoc_text {
        Some(text) => copilot_core::context::assemble_adhoc(
            &title,
            text,
            action,
            assembly_question.as_deref(),
            &thread,
            budget,
        ),
        None => {
            // Graph-first (v2): concept neighborhood + learner memory when
            // the paper has a graph covering this anchor; v1 object +
            // relationships otherwise — never worse than v1.
            let graph: Option<copilot_core::concepts::KnowledgeGraph> = bundle
                .read_derived_json("knowledge_graph.json")
                .ok()
                .flatten();
            let graph_context = graph.as_ref().and_then(|graph| {
                let root = state.library.lock().unwrap().root().to_path_buf();
                let model = copilot_core::learning::LearnerModel::open(&root);
                let snapshot = model.snapshot().ok()?;
                let episodes = model.episodes_for(object_id).unwrap_or_default();
                // Global concept ids so mastery earned in other papers counts.
                let node_globals: std::collections::HashMap<uuid::Uuid, uuid::Uuid> =
                    copilot_core::concept_registry::ConceptRegistry::open(&root)
                        .state()
                        .map(|s| {
                            graph
                                .nodes
                                .iter()
                                .filter_map(|n| s.global_for(&paper_id, n.id).map(|g| (n.id, g.id)))
                                .collect()
                        })
                        .unwrap_or_default();
                copilot_core::context::assemble_graph(
                    &tree,
                    &title,
                    object_id,
                    action,
                    assembly_question.as_deref(),
                    &thread,
                    table_data.as_ref(),
                    budget,
                    &copilot_core::context::GraphInputs {
                        graph,
                        snapshot: &snapshot,
                        episodes: &episodes,
                        node_globals: Some(&node_globals),
                    },
                )
            });
            match graph_context {
                Some(context) => context,
                None => copilot_core::context::assemble(
                    &tree,
                    &title,
                    object_id,
                    action,
                    assembly_question.as_deref(),
                    &thread,
                    table_data.as_ref(),
                    budget,
                )
                .ok_or("Object not found in this paper.")?,
            }
        }
    };
    // Local-only instrumentation for the context-efficiency target (task 3.3).
    let _ = state
        .telemetry
        .record_value("prompt_tokens_approx", context.approx_tokens as i64);

    // Persist the user turn (action name + question) before streaming.
    let action_name = serde_json::to_value(action)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "ask".to_string());
    // Repeat-explanation rate (opt-in, content-free): the same action asked
    // again on one object is the clearest "the first answer didn't land".
    if history
        .iter()
        .any(|m| m.role == "user" && m.action.as_deref() == Some(action_name.as_str()))
    {
        let _ = state.telemetry.record("explanation_repeated");
    }
    // Attachments: send with THIS request's user message, and keep copies
    // in the bundle so the conversation stays self-contained.
    let images = images.unwrap_or_default();
    let mut question_text = question.clone().unwrap_or_else(|| action_name.clone());
    if !images.is_empty() {
        let dir = bundle.root().join("chats/attachments");
        std::fs::create_dir_all(&dir).map_err(ui_err)?;
        for image in &images {
            let ext = image.media_type.rsplit('/').next().unwrap_or("png");
            let name = format!("{}.{ext}", uuid::Uuid::new_v4());
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&image.data_b64)
                .map_err(|e| format!("bad attachment encoding: {e}"))?;
            std::fs::write(dir.join(&name), bytes).map_err(ui_err)?;
            question_text.push_str(&format!("\n[attached image: chats/attachments/{name}]"));
        }
        if let Some(last_user) = context.messages.iter_mut().rev().find(|m| m.role == "user") {
            last_user.images = images.clone();
        }
    }
    let user_turn = copilot_core::chat::user_message(&action_name, question_text);
    copilot_core::chat::append(&bundle, object_id, &user_turn).map_err(ui_err)?;

    let emit = |event: AiStreamEvent| {
        let _ = app.emit("ai-stream", event);
    };

    // Egress indicator: name the actual destination host up front.
    emit(AiStreamEvent {
        host: Some(config.host()),
        ..AiStreamEvent::empty(&request_id)
    });

    let is_cancelled = || {
        state
            .cancelled_requests
            .lock()
            .unwrap()
            .contains(&request_id)
    };

    let mut accumulated = String::new();
    let result = provider.stream_chat_cancellable(
        &context.messages,
        &mut |token| {
            accumulated.push_str(token);
            emit(AiStreamEvent {
                token: Some(token.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
        },
        &is_cancelled,
    );
    state.cancelled_requests.lock().unwrap().remove(&request_id);

    match result {
        Ok(full) if full.trim().is_empty() => {
            // A "successful" stream with no visible text is a failure to the
            // reader (reasoning models can exhaust output on thinking, or an
            // endpoint may answer non-streaming). Say so plainly.
            let message = "The model finished without producing any text — it may have spent \
                           its whole output budget reasoning. Try again, or switch the strong \
                           tier to a different model in Settings.";
            emit(AiStreamEvent {
                error: Some(message.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
            Err(message.to_string())
        }
        Ok(full) => {
            let turn = copilot_core::chat::assistant_message(full.clone(), false);
            copilot_core::chat::append(&bundle, object_id, &turn).map_err(ui_err)?;
            emit(AiStreamEvent {
                done: Some(true),
                ..AiStreamEvent::empty(&request_id)
            });
            spawn_episode_summary(&app, paper_id.clone(), object_id);
            Ok(full)
        }
        Err(copilot_core::ai::AiError::Cancelled) => {
            // User-initiated: keep the partial, marked, and report calmly.
            if !accumulated.is_empty() {
                let turn = copilot_core::chat::assistant_message(accumulated.clone(), true);
                let _ = copilot_core::chat::append(&bundle, object_id, &turn);
            }
            emit(AiStreamEvent {
                cancelled: Some(true),
                ..AiStreamEvent::empty(&request_id)
            });
            Ok(accumulated)
        }
        Err(e) => {
            // Honest failure: keep the partial, clearly marked.
            if !accumulated.is_empty() {
                let turn = copilot_core::chat::assistant_message(accumulated, true);
                let _ = copilot_core::chat::append(&bundle, object_id, &turn);
            }
            emit(AiStreamEvent {
                error: Some(e.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
            Err(ui_err(e))
        }
    }
}

/// Refresh episodic memory for an object in the background after a completed
/// exchange. The summarizer is lazy (short/unchanged threads and provider
/// failures all no-op), so calling it after every exchange is cheap.
// ---- Implementation mode (v3) ----

#[derive(serde::Serialize)]
struct ImplementationView {
    implementation: Option<copilot_core::implementations::Implementation>,
    languages_present: Vec<copilot_core::implementations::Language>,
}

#[tauri::command]
fn implementation_get(
    state: State<AppState>,
    paper_id: String,
    object_id: uuid::Uuid,
    language: copilot_core::implementations::Language,
) -> Result<ImplementationView, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let tree = bundle
        .read_derived_json("semantic_tree.json")
        .map_err(ui_err)?
        .ok_or("This paper is still being processed.")?;
    Ok(ImplementationView {
        implementation: copilot_core::implementations::get(&bundle, &tree, object_id, language)
            .map_err(ui_err)?,
        languages_present: copilot_core::implementations::languages_present(&bundle, object_id),
    })
}

/// Generate (or serve cached) — strong tier, cancellable via `ai_cancel`
/// with the same request id. `None` = no provider (designed no-key state).
#[tauri::command(async)]
fn implementation_generate(
    state: State<AppState>,
    request_id: String,
    paper_id: String,
    object_id: uuid::Uuid,
    language: copilot_core::implementations::Language,
    force: bool,
) -> Result<Option<copilot_core::implementations::Implementation>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let tree = bundle
        .read_derived_json("semantic_tree.json")
        .map_err(ui_err)?
        .ok_or("This paper is still being processed.")?;
    let store = state.providers.clone();
    let provenance = pick_provider(&store, copilot_core::ai::ModelClass::Strong)
        .map(|(_, config)| config.host())
        .unwrap_or_else(|_| "none".to_string());
    let llm = |prompt: &str| -> Option<String> {
        let (provider, _) = pick_provider(&store, copilot_core::ai::ModelClass::Strong).ok()?;
        let messages = [copilot_core::ai::ChatMessage {
            images: Vec::new(),
            role: "user".to_string(),
            content: prompt.to_string(),
        }];
        let is_cancelled = || {
            state
                .cancelled_requests
                .lock()
                .unwrap()
                .contains(&request_id)
        };
        provider
            .stream_chat_cancellable(&messages, &mut |_| {}, &is_cancelled)
            .ok()
    };
    let result = copilot_core::implementations::generate(
        &bundle,
        &tree,
        object_id,
        language,
        &llm,
        &provenance,
        force,
    )
    .map_err(ui_err);
    state.cancelled_requests.lock().unwrap().remove(&request_id);
    result
}

#[tauri::command]
fn implementation_save_edit(
    state: State<AppState>,
    paper_id: String,
    object_id: uuid::Uuid,
    language: copilot_core::implementations::Language,
    code: String,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::implementations::save_edit(&bundle, object_id, language, &code).map_err(ui_err)
}

/// Run an implementation (and optionally its checks) in the sandbox.
/// Output streams over `sandbox-progress`; the outcome persists linked to
/// the object; passing checks records a mastery event (source
/// "implementation") — the same single data path the dashboard reads.
#[tauri::command]
fn implementation_run(
    app: AppHandle,
    state: State<AppState>,
    run_id: String,
    paper_id: String,
    object_id: uuid::Uuid,
    language: copilot_core::implementations::Language,
    with_checks: bool,
) -> Result<(), String> {
    use copilot_core::implementations::Language;
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let dir = bundle
        .root()
        .join(copilot_core::implementations::IMPLEMENTATIONS_DIR)
        .join(object_id.to_string());
    if !dir.is_dir() {
        return Err("Nothing to run yet — generate an implementation first.".to_string());
    }
    let key = language.key();
    let ext = language.extension();
    let has_checks = with_checks && dir.join(format!("{key}.checks.{ext}")).is_file();
    let script = match language {
        Language::Rust => {
            let mut s = format!(
                "cp /work/{key}.{ext} /tmp/main.rs && rustc -O /tmp/main.rs -o /tmp/main && /tmp/main"
            );
            if has_checks {
                s.push_str(&format!(
                    " && cp /work/{key}.checks.{ext} /tmp/checks.rs && rustc -O /tmp/checks.rs -o /tmp/checks && /tmp/checks"
                ));
            }
            s
        }
        _ => {
            let mut s = format!("python /work/{key}.{ext}");
            if has_checks {
                s.push_str(&format!(" && python /work/{key}.checks.{ext}"));
            }
            s
        }
    };
    let spec = copilot_core::sandbox::RunSpec {
        image: language.image().to_string(),
        command: vec!["sh".into(), "-c".into(), script],
        mount_ro: Some(dir),
        mount_rw: None,
        timeout: std::time::Duration::from_secs(300),
        ..Default::default()
    };
    let _ = state.telemetry.record("implementation_run");
    let paper = paper_id.clone();
    spawn_sandbox_run(
        &app,
        run_id,
        paper_id,
        copilot_core::sandbox::ConsentScope::Implementations,
        spec,
        move |app, outcome| {
            let state = app.state::<AppState>();
            let Ok(bundle) = state.library.lock().unwrap().get(&paper) else {
                return;
            };
            let passed = matches!(
                outcome.status,
                copilot_core::sandbox::RunStatus::Completed { exit_code: 0 }
            ) && (!has_checks || outcome.stdout.contains("CHECKS PASSED"));
            let check_status = has_checks.then_some(if passed {
                copilot_core::implementations::CheckStatus::Passed
            } else {
                copilot_core::implementations::CheckStatus::Failed
            });
            let output = format!("{}{}", outcome.stdout, outcome.stderr);
            let _ = copilot_core::implementations::record_run(
                &bundle,
                object_id,
                language,
                &output,
                check_status,
            );
            // Checks passing = demonstrated understanding: one mastery event,
            // read by the dashboard and lesson collapsing alike.
            if has_checks && passed {
                let root = state.library.lock().unwrap().root().to_path_buf();
                if let Ok(Some(graph)) = bundle
                    .read_derived_json::<copilot_core::concepts::KnowledgeGraph>(
                        "knowledge_graph.json",
                    )
                {
                    let registry_state =
                        copilot_core::concept_registry::ConceptRegistry::open(&root).state();
                    let model = copilot_core::learning::LearnerModel::open(&root);
                    for node in graph
                        .nodes
                        .iter()
                        .filter(|n| n.object_ids.contains(&object_id))
                    {
                        let concept = registry_state
                            .as_ref()
                            .ok()
                            .and_then(|s| s.global_for(&paper, node.id).map(|g| g.id))
                            .unwrap_or(node.id);
                        let _ = model.record_mastery(&copilot_core::learning::MasteryEvent {
                            concept,
                            object: Some(object_id),
                            quality: 5,
                            source: "implementation".to_string(),
                            at: copilot_core::bundle::now_rfc3339(),
                        });
                    }
                }
            }
        },
    );
    Ok(())
}

/// The user's concept-map canvas (Excalidraw scene), stored as user data
/// (`notes/graph_canvas.json` — journals' sibling; LWW+conflict on sync).
#[tauri::command]
fn canvas_get(
    state: State<AppState>,
    paper_id: String,
) -> Result<Option<serde_json::Value>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let path = bundle.root().join("notes/graph_canvas.json");
    Ok(std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok()))
}

#[tauri::command]
fn canvas_save(
    state: State<AppState>,
    paper_id: String,
    scene: serde_json::Value,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    bundle
        .write_user_json("notes/graph_canvas.json", &scene)
        .map_err(ui_err)
}

// ---- Cloud sync (add-cloud-sync) ----

/// Non-secret sync configuration, stored library-level (cache-class).
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct SyncConfigFile {
    /// "s3" | "folder"
    backend: String,
    #[serde(default)]
    endpoint: String,
    #[serde(default)]
    bucket: String,
    #[serde(default)]
    region: String,
    #[serde(default)]
    folder: String,
}

fn sync_config_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join(copilot_core::sync::engine::SYNC_STATE_DIR)
        .join("config.json")
}

fn load_sync_config(root: &std::path::Path) -> Option<SyncConfigFile> {
    std::fs::read(sync_config_path(root))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

/// Build the configured remote (credentials from the OS keychain).
fn build_remote(
    config: &SyncConfigFile,
) -> Result<Box<dyn copilot_core::sync::remote::Remote>, String> {
    match config.backend.as_str() {
        "folder" => Ok(Box::new(
            copilot_core::sync::remote::FolderRemote::new(std::path::PathBuf::from(&config.folder))
                .map_err(ui_err)?,
        )),
        "s3" => {
            let access = copilot_core::ai::load_key_for("sync-s3-access")
                .ok()
                .flatten()
                .ok_or("S3 access key missing from the keychain — reconfigure sync.")?;
            let secret = copilot_core::ai::load_key_for("sync-s3-secret")
                .ok()
                .flatten()
                .ok_or("S3 secret key missing from the keychain — reconfigure sync.")?;
            let remote =
                copilot_core::sync::remote::S3Remote::new(copilot_core::sync::s3::S3Config {
                    endpoint: config.endpoint.clone(),
                    bucket: config.bucket.clone(),
                    region: if config.region.is_empty() {
                        "us-east-1".into()
                    } else {
                        config.region.clone()
                    },
                    access_key: access,
                    secret_key: secret,
                });
            remote.ensure_bucket().map_err(ui_err)?;
            Ok(Box::new(remote))
        }
        other => Err(format!("unknown sync backend: {other}")),
    }
}

fn sync_passphrase() -> Result<String, String> {
    copilot_core::ai::load_key_for("sync-passphrase")
        .ok()
        .flatten()
        .ok_or_else(|| "Sync passphrase missing from the keychain — reconfigure sync.".to_string())
}

#[derive(serde::Serialize)]
struct SyncStatusView {
    configured: bool,
    backend: Option<String>,
    destination: Option<String>,
    last_generation: u64,
    /// Conflict-copy files awaiting the user's attention.
    conflicts: Vec<String>,
    /// Tombstoned papers in the local trash (grace period).
    trash: Vec<String>,
}

#[tauri::command]
fn sync_status(state: State<AppState>) -> Result<SyncStatusView, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let config = load_sync_config(&root);
    let destination = config.as_ref().map(|c| match c.backend.as_str() {
        "folder" => format!("folder: {}", c.folder),
        _ => format!(
            "s3: {} (bucket {})",
            c.endpoint
                .trim_start_matches("https://")
                .trim_start_matches("http://"),
            c.bucket
        ),
    });
    let last_generation = std::fs::read(
        root.join(copilot_core::sync::engine::SYNC_STATE_DIR)
            .join("state.json"),
    )
    .ok()
    .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
    .and_then(|v| v["last_generation"].as_u64())
    .unwrap_or(0);

    // Conflict copies anywhere in the library (bounded scan).
    fn find_conflicts(dir: &std::path::Path, out: &mut Vec<String>, root: &std::path::Path) {
        if out.len() >= 50 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "repos" || name == "sync_state" {
                continue;
            }
            if path.is_dir() {
                find_conflicts(&path, out, root);
            } else if name.contains(".conflict-") {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().to_string());
                }
            }
        }
    }
    let mut conflicts = Vec::new();
    find_conflicts(&root, &mut conflicts, &root);
    let trash = std::fs::read_dir(root.join(".trash"))
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();

    Ok(SyncStatusView {
        configured: config.is_some(),
        backend: config.map(|c| c.backend),
        destination,
        last_generation,
        conflicts,
        trash,
    })
}

/// Configure (or reconfigure) sync: non-secret config to `sync_state/`,
/// credentials + passphrase to the OS keychain, then a validation round
/// trip (derive key against the remote salt + list) so a wrong endpoint or
/// passphrase fails HERE, not during a background sync.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn sync_configure(
    state: State<AppState>,
    backend: String,
    endpoint: Option<String>,
    bucket: Option<String>,
    region: Option<String>,
    folder: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
    passphrase: String,
) -> Result<String, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    if passphrase.trim().len() < 8 {
        return Err("Choose a passphrase of at least 8 characters — it is the only key to your synced data and cannot be recovered.".into());
    }
    let config = SyncConfigFile {
        backend: backend.clone(),
        endpoint: endpoint.unwrap_or_default(),
        bucket: bucket.unwrap_or_default(),
        region: region.unwrap_or_default(),
        folder: folder.unwrap_or_default(),
    };
    if let (Some(access), Some(secret)) = (access_key, secret_key) {
        if !access.is_empty() {
            copilot_core::ai::store_key_for("sync-s3-access", &access).map_err(ui_err)?;
            copilot_core::ai::store_key_for("sync-s3-secret", &secret).map_err(ui_err)?;
        }
    }
    copilot_core::ai::store_key_for("sync-passphrase", passphrase.trim()).map_err(ui_err)?;

    // Validation round trip before persisting the config.
    let remote = build_remote(&config)?;
    let key = copilot_core::sync::engine::derive_remote_key(remote.as_ref(), passphrase.trim())
        .map_err(ui_err)?;
    // A remote with existing data must decrypt with this passphrase.
    let probe = copilot_core::sync::engine::SyncEngine {
        library_root: &root,
        device_id: device_id(&root),
        key,
        remote: remote.as_ref(),
    };
    let _ = probe; // key derivation + salt bootstrap is the validation
    std::fs::create_dir_all(sync_config_path(&root).parent().unwrap()).map_err(ui_err)?;
    std::fs::write(
        sync_config_path(&root),
        serde_json::to_vec_pretty(&config).expect("serializable"),
    )
    .map_err(ui_err)?;
    Ok(remote.describe())
}

#[tauri::command]
fn sync_disable(state: State<AppState>) -> Result<(), String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let path = sync_config_path(&root);
    if path.is_file() {
        std::fs::remove_file(path).map_err(ui_err)?;
    }
    Ok(())
}

/// Stable per-library device id (created on first use).
fn device_id(root: &std::path::Path) -> String {
    let path = root
        .join(copilot_core::sync::engine::SYNC_STATE_DIR)
        .join("device_id");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        return existing.trim().to_string();
    }
    let id = format!("device-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(&path, &id);
    id
}

#[derive(Clone, serde::Serialize)]
struct SyncEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<copilot_core::sync::engine::SyncOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Run one sync cycle on a worker thread (`sync-progress` events). Never
/// blocks reading; a missing/unreachable remote is a reported state, not a
/// crash.
#[tauri::command]
fn sync_now(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let Some(config) = load_sync_config(&root) else {
        return Err("Sync isn't configured — set a remote in Settings first.".to_string());
    };
    let passphrase = sync_passphrase()?;
    std::thread::spawn(move || {
        let emit = |event: SyncEvent| {
            let _ = app.emit("sync-progress", event);
        };
        let remote = match build_remote(&config) {
            Ok(remote) => remote,
            Err(e) => {
                emit(SyncEvent {
                    line: None,
                    outcome: None,
                    error: Some(e),
                });
                return;
            }
        };
        let key = match copilot_core::sync::engine::derive_remote_key(remote.as_ref(), &passphrase)
        {
            Ok(key) => key,
            Err(e) => {
                emit(SyncEvent {
                    line: None,
                    outcome: None,
                    error: Some(e.to_string()),
                });
                return;
            }
        };
        let engine = copilot_core::sync::engine::SyncEngine {
            library_root: &root,
            device_id: device_id(&root),
            key,
            remote: remote.as_ref(),
        };
        let result = engine.sync(&mut |line| {
            emit(SyncEvent {
                line: Some(line.to_string()),
                outcome: None,
                error: None,
            });
        });
        let state = app.state::<AppState>();
        match result {
            Ok(outcome) => {
                let _ = state.telemetry.record("sync_completed");
                if !outcome.conflict_copies.is_empty() {
                    let _ = state.telemetry.record("sync_conflict_created");
                }
                emit(SyncEvent {
                    line: None,
                    outcome: Some(outcome),
                    error: None,
                });
            }
            Err(e) => emit(SyncEvent {
                line: None,
                outcome: None,
                error: Some(e.to_string()),
            }),
        }
    });
    Ok(())
}

/// Explicit remote garbage collection (never runs on its own).
#[tauri::command(async)]
fn sync_clean_remote(state: State<AppState>) -> Result<usize, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let config = load_sync_config(&root).ok_or("Sync isn't configured.")?;
    let passphrase = sync_passphrase()?;
    let remote = build_remote(&config)?;
    let key = copilot_core::sync::engine::derive_remote_key(remote.as_ref(), &passphrase)
        .map_err(ui_err)?;
    let engine = copilot_core::sync::engine::SyncEngine {
        library_root: &root,
        device_id: device_id(&root),
        key,
        remote: remote.as_ref(),
    };
    engine.clean_remote().map_err(ui_err)
}

/// Capability parity matrix — the UI derives feature availability from
/// this; nothing hard-codes platform checks.
#[tauri::command]
fn capability_matrix() -> Vec<copilot_core::capabilities::Capability> {
    copilot_core::capabilities::capability_matrix()
}

// ---- Plugin API (v5 §4): discovery, consents, execution, panels ----

/// Plugin search path: user-installed plugins in app data, plus the
/// repo-shipped reference plugins in dev builds.
fn plugin_dirs(app: &AppHandle) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(data) = app.path().app_data_dir() {
        dirs.push(data.join("plugins"));
    }
    let dev = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../plugins"));
    if dev.exists() {
        dirs.push(dev);
    }
    dirs
}

fn plugin_consents_journal(app: &AppHandle) -> Result<copilot_core::bundle::Journal, String> {
    let dir = app.path().app_data_dir().map_err(ui_err)?.join("plugins");
    std::fs::create_dir_all(&dir).map_err(ui_err)?;
    Ok(copilot_core::bundle::Journal::at(
        dir.join("consents.jsonl"),
    ))
}

fn find_plugin(
    app: &AppHandle,
    name: &str,
) -> Result<copilot_core::plugin::DiscoveredPlugin, String> {
    plugin_dirs(app)
        .iter()
        .flat_map(|d| copilot_core::plugin::discover(d))
        .find(|p| p.manifest.name == name)
        .ok_or_else(|| format!("plugin {name} not found"))
}

#[derive(serde::Serialize)]
struct PluginView {
    #[serde(flatten)]
    plugin: copilot_core::plugin::DiscoveredPlugin,
    granted: Vec<String>,
}

#[tauri::command]
fn plugin_list(app: AppHandle) -> Result<Vec<PluginView>, String> {
    let grants = copilot_core::plugin::current_grants(&plugin_consents_journal(&app)?);
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for dir in plugin_dirs(&app) {
        for plugin in copilot_core::plugin::discover(&dir) {
            if !seen.insert(plugin.manifest.name.clone()) {
                continue;
            }
            out.push(PluginView {
                granted: grants
                    .get(&plugin.manifest.name)
                    .map(|s| s.iter().cloned().collect())
                    .unwrap_or_default(),
                plugin,
            });
        }
    }
    Ok(out)
}

/// Grant or revoke a permission (recorded append-only, auditable).
#[tauri::command]
fn plugin_set_consent(
    app: AppHandle,
    plugin: String,
    permission: String,
    granted: bool,
) -> Result<(), String> {
    copilot_core::plugin::record_consent(
        &plugin_consents_journal(&app)?,
        &plugin,
        &permission,
        granted,
    )
    .map_err(ui_err)
}

#[derive(serde::Serialize)]
struct PluginRunView {
    /// Plugin output parsed as JSON (exporters/importers/panels all emit JSON).
    output: serde_json::Value,
    /// Permission-gated calls that were blocked (surfaced to the user).
    blocked: Vec<String>,
}

/// Run a plugin over a paper's scoped bundle view.
#[tauri::command(async)]
fn plugin_run(
    app: AppHandle,
    state: State<AppState>,
    plugin: String,
    paper_id: String,
    format: Option<String>,
) -> Result<PluginRunView, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let found = find_plugin(&app, &plugin)?;
    let grants = copilot_core::plugin::current_grants(&plugin_consents_journal(&app)?);
    let granted = grants.get(&plugin).cloned().unwrap_or_default();
    let input = serde_json::json!({
        "format": format,
        "view": copilot_core::plugin::bundle_view(&bundle),
    });
    let report = copilot_core::plugin::run_plugin(&found, input.to_string().as_bytes(), &granted)
        .map_err(ui_err)?;
    let output = serde_json::from_slice(&report.output)
        .unwrap_or_else(|_| serde_json::json!({ "raw": String::from_utf8_lossy(&report.output) }));
    Ok(PluginRunView {
        output,
        blocked: report.blocked,
    })
}

/// Run an exporter and write its `{ files: {path: content} }` output into
/// a user-chosen directory.
#[tauri::command(async)]
fn plugin_export_to_dir(
    app: AppHandle,
    state: State<AppState>,
    plugin: String,
    paper_id: String,
    format: String,
    dest_dir: String,
) -> Result<Vec<String>, String> {
    let run = plugin_run(app, state, plugin, paper_id, Some(format))?;
    let files = run.output["files"]
        .as_object()
        .ok_or("exporter produced no files")?;
    let dest = std::path::PathBuf::from(&dest_dir);
    let mut written = Vec::new();
    for (rel, content) in files {
        if rel.contains("..") {
            return Err(format!("exporter emitted an unsafe path: {rel}"));
        }
        let path = dest.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ui_err)?;
        }
        std::fs::write(&path, content.as_str().unwrap_or_default()).map_err(ui_err)?;
        written.push(rel.clone());
    }
    Ok(written)
}

// ---- Community contributions (v5 §2/§6): propose → review → merge ----

/// Contributor identity: display name in app config, ed25519 signing key
/// in the OS keychain (hex seed). Created lazily on first use.
fn contribution_signing_key() -> Result<ed25519_dalek::SigningKey, String> {
    const ACCOUNT: &str = "contrib-signing-seed";
    if let Ok(Some(hex_seed)) = copilot_core::ai::load_key_for(ACCOUNT) {
        if hex_seed.len() == 64 {
            let mut seed = [0u8; 32];
            for (i, byte) in seed.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&hex_seed[i * 2..i * 2 + 2], 16)
                    .map_err(|_| "corrupt signing seed in keychain")?;
            }
            return Ok(ed25519_dalek::SigningKey::from_bytes(&seed));
        }
    }
    // Seed from OS randomness (two v4 UUIDs) hashed to 32 bytes.
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(uuid::Uuid::new_v4().as_bytes());
    hasher.update(uuid::Uuid::new_v4().as_bytes());
    hasher.update(copilot_core::bundle::now_rfc3339().as_bytes());
    let seed: [u8; 32] = hasher.finalize().into();
    let hex_seed: String = seed.iter().map(|b| format!("{b:02x}")).collect();
    copilot_core::ai::store_key_for(ACCOUNT, &hex_seed).map_err(ui_err)?;
    Ok(ed25519_dalek::SigningKey::from_bytes(&seed))
}

fn contribution_author(root: &std::path::Path) -> copilot_core::contributions::Author {
    let path = root
        .join(copilot_core::sync::engine::SYNC_STATE_DIR)
        .join("contributor.json");
    let name = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["name"].as_str().map(str::to_string));
    copilot_core::contributions::Author {
        id: name.clone().unwrap_or_else(|| "anonymous".into()),
        display_name: name,
    }
}

#[tauri::command]
fn contribution_identity_set(state: State<AppState>, name: String) -> Result<(), String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let dir = root.join(copilot_core::sync::engine::SYNC_STATE_DIR);
    std::fs::create_dir_all(&dir).map_err(ui_err)?;
    std::fs::write(
        dir.join("contributor.json"),
        serde_json::json!({ "name": name }).to_string(),
    )
    .map_err(ui_err)
}

/// Propose the paper's shareable enrichment to the community: selected
/// journals as union-mergeable entries, the canvas as a file add. Queued
/// offline; policy-validated at creation so violations never reach review.
#[tauri::command]
fn contribution_propose(
    state: State<AppState>,
    paper_id: String,
    summary: String,
    include_notes: bool,
    include_canvas: bool,
) -> Result<copilot_core::contributions::Proposal, String> {
    let (bundle, root) = {
        let library = state.library.lock().unwrap();
        (
            library.get(&paper_id).map_err(ui_err)?,
            library.root().to_path_buf(),
        )
    };
    let mut journal_changes = Vec::new();
    if include_notes {
        let entries: Vec<serde_json::Value> = bundle
            .journal("notes/notes.jsonl")
            .read_all()
            .map_err(ui_err)?;
        if !entries.is_empty() {
            journal_changes.push(("notes/notes.jsonl".to_string(), entries));
        }
    }
    let mut file_adds = Vec::new();
    if include_canvas {
        if let Ok(bytes) = std::fs::read(bundle.root().join("notes/graph_canvas.json")) {
            file_adds.push(("notes/graph_canvas.json".to_string(), bytes));
        }
    }
    if journal_changes.is_empty() && file_adds.is_empty() {
        return Err("nothing selected to propose".into());
    }
    let author = contribution_author(&root);
    let proposal = copilot_core::contributions::create_proposal(
        &bundle,
        author.clone(),
        &summary,
        journal_changes,
        file_adds,
    )
    .map_err(ui_err)?;
    copilot_core::contributions::validate_policy(&bundle, &proposal).map_err(ui_err)?;
    let key = contribution_signing_key()?;
    copilot_core::contributions::append_event(
        &bundle,
        &key,
        author,
        copilot_core::contributions::EventKind::Propose {
            proposal_id: proposal.id.clone(),
        },
    )
    .map_err(ui_err)?;
    Ok(proposal)
}

#[derive(serde::Serialize)]
struct ContributionOverview {
    proposals: Vec<copilot_core::contributions::Proposal>,
    revision: String,
    events: Vec<copilot_core::contributions::ProvenanceEvent>,
    reputation: std::collections::BTreeMap<String, copilot_core::contributions::Reputation>,
    my_trust: copilot_core::contributions::TrustLevel,
}

#[tauri::command]
fn contribution_overview(
    state: State<AppState>,
    paper_id: String,
) -> Result<ContributionOverview, String> {
    let (bundle, root) = {
        let library = state.library.lock().unwrap();
        (
            library.get(&paper_id).map_err(ui_err)?,
            library.root().to_path_buf(),
        )
    };
    let events = copilot_core::contributions::read_events(&bundle).map_err(ui_err)?;
    let reputation = copilot_core::contributions::reputation(&events);
    let me = contribution_author(&root);
    let my_trust = reputation
        .get(&me.id)
        .map(copilot_core::contributions::trust_level)
        .unwrap_or(copilot_core::contributions::TrustLevel::New);
    Ok(ContributionOverview {
        proposals: copilot_core::contributions::list_proposals(&bundle).map_err(ui_err)?,
        revision: copilot_core::contributions::current_revision(&bundle).map_err(ui_err)?,
        events,
        reputation,
        my_trust,
    })
}

#[derive(serde::Serialize)]
struct ChangePreview {
    path: String,
    kind: String,
    /// Journal entries or a UTF-8 head of the file payload.
    preview: String,
}

/// Full diff view for a proposal: every changed path with its content.
#[tauri::command]
fn contribution_diff(
    state: State<AppState>,
    paper_id: String,
    proposal_id: String,
) -> Result<Vec<ChangePreview>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let proposal = copilot_core::contributions::list_proposals(&bundle)
        .map_err(ui_err)?
        .into_iter()
        .find(|p| p.id == proposal_id)
        .ok_or("proposal not found")?;
    Ok(proposal
        .changes
        .iter()
        .map(|change| match &change.kind {
            copilot_core::contributions::ChangeKind::JournalAppend { entries } => ChangePreview {
                path: change.path.clone(),
                kind: format!("journal (+{} entries)", entries.len()),
                preview: serde_json::to_string_pretty(entries).unwrap_or_default(),
            },
            copilot_core::contributions::ChangeKind::FileAdd { digest } => {
                let payload =
                    copilot_core::contributions::read_proposal_blob(&bundle, &proposal.id, digest)
                        .unwrap_or_default();
                ChangePreview {
                    path: change.path.clone(),
                    kind: format!("file ({} bytes)", payload.len()),
                    preview: String::from_utf8_lossy(&payload)
                        .chars()
                        .take(4000)
                        .collect(),
                }
            }
        })
        .collect())
}

/// Review: accept merges (surfacing conflicts), reject records the reason.
/// Both land as signed provenance events — authorship always visible.
#[tauri::command]
fn contribution_review(
    state: State<AppState>,
    paper_id: String,
    proposal_id: String,
    accepted: bool,
    reason: Option<String>,
) -> Result<copilot_core::contributions::MergeOutcome, String> {
    let (bundle, root) = {
        let library = state.library.lock().unwrap();
        (
            library.get(&paper_id).map_err(ui_err)?,
            library.root().to_path_buf(),
        )
    };
    let reviewer = contribution_author(&root);
    let key = contribution_signing_key()?;
    copilot_core::contributions::append_event(
        &bundle,
        &key,
        reviewer.clone(),
        copilot_core::contributions::EventKind::Review {
            proposal_id: proposal_id.clone(),
            accepted,
            reason: reason.clone(),
        },
    )
    .map_err(ui_err)?;
    if !accepted {
        let mut proposal = copilot_core::contributions::list_proposals(&bundle)
            .map_err(ui_err)?
            .into_iter()
            .find(|p| p.id == proposal_id)
            .ok_or("proposal not found")?;
        proposal.status = copilot_core::contributions::ProposalStatus::Rejected;
        copilot_core::contributions::write_proposal(&bundle, &proposal).map_err(ui_err)?;
        return Ok(copilot_core::contributions::MergeOutcome {
            merged: false,
            conflicts: Vec::new(),
        });
    }
    copilot_core::contributions::merge_proposal(&bundle, &key, reviewer, &proposal_id)
        .map_err(ui_err)
}

#[tauri::command]
fn contribution_revert(
    state: State<AppState>,
    paper_id: String,
    proposal_id: String,
) -> Result<(), String> {
    let (bundle, root) = {
        let library = state.library.lock().unwrap();
        (
            library.get(&paper_id).map_err(ui_err)?,
            library.root().to_path_buf(),
        )
    };
    let key = contribution_signing_key()?;
    copilot_core::contributions::revert_proposal(
        &bundle,
        &key,
        contribution_author(&root),
        &proposal_id,
    )
    .map_err(ui_err)
}

// ---- Knowledge registry (v5 §3): multi-registry client ----

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
struct RegistriesFile {
    registries: Vec<RegistryEntry>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct RegistryEntry {
    name: String,
    url: String,
    #[serde(default)]
    is_default: bool,
}

fn registries_path(root: &std::path::Path) -> std::path::PathBuf {
    // Lives in sync_state/: per-machine config, excluded from sync.
    root.join(copilot_core::sync::engine::SYNC_STATE_DIR)
        .join("registries.json")
}

fn load_registries(root: &std::path::Path) -> RegistriesFile {
    std::fs::read(registries_path(root))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn registry_token_account(url: &str) -> String {
    format!(
        "registry-{}",
        copilot_core::bundle::sha256_bytes(url.as_bytes())[..15].replace(':', "")
    )
}

fn default_registry_client(
    root: &std::path::Path,
) -> Result<copilot_core::registry::RegistryClient, String> {
    let file = load_registries(root);
    let entry = file
        .registries
        .iter()
        .find(|r| r.is_default)
        .or_else(|| file.registries.first())
        .ok_or("No registry configured — add one in Settings.")?;
    let token = copilot_core::ai::load_key_for(&registry_token_account(&entry.url))
        .ok()
        .flatten();
    Ok(copilot_core::registry::RegistryClient {
        base_url: entry.url.clone(),
        token,
    })
}

#[derive(serde::Serialize)]
struct RegistryEntryView {
    name: String,
    url: String,
    is_default: bool,
    has_token: bool,
}

#[tauri::command]
fn registry_list(state: State<AppState>) -> Result<Vec<RegistryEntryView>, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    Ok(load_registries(&root)
        .registries
        .into_iter()
        .map(|r| RegistryEntryView {
            has_token: copilot_core::ai::load_key_for(&registry_token_account(&r.url))
                .ok()
                .flatten()
                .is_some(),
            name: r.name,
            url: r.url,
            is_default: r.is_default,
        })
        .collect())
}

#[tauri::command]
fn registry_add(
    state: State<AppState>,
    name: String,
    url: String,
    token: Option<String>,
    make_default: bool,
) -> Result<(), String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let mut file = load_registries(&root);
    if let Some(token) = token.filter(|t| !t.is_empty()) {
        copilot_core::ai::store_key_for(&registry_token_account(&url), &token).map_err(ui_err)?;
    }
    file.registries.retain(|r| r.url != url);
    if make_default {
        for r in &mut file.registries {
            r.is_default = false;
        }
    }
    let is_default = make_default || file.registries.is_empty();
    file.registries.push(RegistryEntry {
        name,
        url,
        is_default,
    });
    let path = registries_path(&root);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(ui_err)?;
    std::fs::write(&path, serde_json::to_vec_pretty(&file).map_err(ui_err)?).map_err(ui_err)
}

#[tauri::command]
fn registry_remove(state: State<AppState>, url: String) -> Result<(), String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let mut file = load_registries(&root);
    file.registries.retain(|r| r.url != url);
    if !file.registries.is_empty() && !file.registries.iter().any(|r| r.is_default) {
        file.registries[0].is_default = true;
    }
    std::fs::write(
        registries_path(&root),
        serde_json::to_vec_pretty(&file).map_err(ui_err)?,
    )
    .map_err(ui_err)
}

#[derive(serde::Serialize)]
struct RegistryCheckView {
    eligible: bool,
    canonical_id: Option<String>,
    layers: Vec<copilot_core::registry::LayerManifest>,
}

/// Registry lookup for a paper: canonical identity + available community
/// layers. Unreachable registry → error string; callers fall back to the
/// fully-local flow (v4 behavior).
#[tauri::command(async)]
fn registry_check(state: State<AppState>, paper_id: String) -> Result<RegistryCheckView, String> {
    let (bundle, root) = {
        let library = state.library.lock().unwrap();
        (
            library.get(&paper_id).map_err(ui_err)?,
            library.root().to_path_buf(),
        )
    };
    let reg_state = copilot_core::registry::resolve_state(&bundle).map_err(ui_err)?;
    let Some(id) = &reg_state.canonical_id else {
        return Ok(RegistryCheckView {
            eligible: false,
            canonical_id: None,
            layers: Vec::new(),
        });
    };
    let client = default_registry_client(&root)?;
    let layers = client.layers(&id.key()).map_err(ui_err)?;
    Ok(RegistryCheckView {
        eligible: true,
        canonical_id: Some(id.key()),
        layers,
    })
}

/// Explicit, consent-gated pull of one community layer.
#[tauri::command(async)]
fn registry_pull(
    state: State<AppState>,
    paper_id: String,
    version: u64,
) -> Result<copilot_core::registry::PullReport, String> {
    let (bundle, root) = {
        let library = state.library.lock().unwrap();
        (
            library.get(&paper_id).map_err(ui_err)?,
            library.root().to_path_buf(),
        )
    };
    let reg_state = copilot_core::registry::resolve_state(&bundle).map_err(ui_err)?;
    let id = reg_state
        .canonical_id
        .ok_or("paper is registry-ineligible")?;
    let client = default_registry_client(&root)?;
    let layers = client.layers(&id.key()).map_err(ui_err)?;
    let manifest = layers
        .into_iter()
        .find(|l| l.version == version)
        .ok_or("layer version not found")?;
    let blob = client.blob(&id.key(), version).map_err(ui_err)?;
    copilot_core::registry::pull_layer(&bundle, &manifest, &blob).map_err(ui_err)
}

#[tauri::command]
fn registry_preview(
    state: State<AppState>,
    paper_id: String,
) -> Result<copilot_core::registry::PublishPreview, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::registry::publish_preview(&bundle).map_err(ui_err)
}

/// Publish this paper's enrichment (the preview set) to the default
/// registry. Client-side policy runs first; the server re-validates.
#[tauri::command(async)]
fn registry_publish(state: State<AppState>, paper_id: String) -> Result<u64, String> {
    let (bundle, root) = {
        let library = state.library.lock().unwrap();
        (
            library.get(&paper_id).map_err(ui_err)?,
            library.root().to_path_buf(),
        )
    };
    let reg_state = copilot_core::registry::resolve_state(&bundle).map_err(ui_err)?;
    let id = reg_state
        .canonical_id
        .ok_or("paper is registry-ineligible (no DOI/arXiv id)")?;
    let preview = copilot_core::registry::publish_preview(&bundle).map_err(ui_err)?;
    if preview.included.is_empty() {
        return Err("nothing publishable — the enrichment allowlist matched no files".into());
    }
    copilot_core::registry::validate_publish(&bundle, &preview.included).map_err(ui_err)?;
    let client = default_registry_client(&root)?;
    if client.token.is_none() {
        return Err("publishing needs a registry token — add one in Settings.".into());
    }
    let (manifest, blob) = copilot_core::registry::build_layer(
        &bundle,
        &id.key(),
        0,       // server assigns
        "local", // server stamps the token identity
        &preview.included,
    )
    .map_err(ui_err)?;
    client.publish(&manifest, &blob).map_err(ui_err)
}

// ---- Collaborative workspaces (v4 §7): features over sync primitives ----

/// Per-workspace member identity (this device's user inside the workspace).
#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct WorkspaceMe {
    member_id: uuid::Uuid,
    name: String,
}

fn workspace_dir(root: &std::path::Path, id: uuid::Uuid) -> std::path::PathBuf {
    root.join(copilot_core::collab::WORKSPACES_DIR)
        .join(id.to_string())
}

fn workspace_me(root: &std::path::Path, id: uuid::Uuid) -> Result<WorkspaceMe, String> {
    let path = workspace_dir(root, id).join("me.json");
    std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| "You haven't joined this workspace on this device yet.".to_string())
}

fn write_workspace_me(
    root: &std::path::Path,
    id: uuid::Uuid,
    me: &WorkspaceMe,
) -> Result<(), String> {
    let dir = workspace_dir(root, id);
    std::fs::create_dir_all(&dir).map_err(ui_err)?;
    std::fs::write(
        dir.join("me.json"),
        serde_json::to_vec_pretty(me).map_err(ui_err)?,
    )
    .map_err(ui_err)
}

/// Keychain account names are scoped per workspace.
fn ws_key(id: uuid::Uuid, what: &str) -> String {
    format!("ws-{}-{what}", &id.to_string()[..8])
}

fn workspace_remote(
    root: &std::path::Path,
    id: uuid::Uuid,
) -> Result<(Box<dyn copilot_core::sync::remote::Remote>, String), String> {
    let config_path = workspace_dir(root, id).join("remote.json");
    let config: SyncConfigFile = std::fs::read(&config_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or("This workspace has no shared remote configured yet.")?;
    let remote = match config.backend.as_str() {
        "folder" => Box::new(
            copilot_core::sync::remote::FolderRemote::new(std::path::PathBuf::from(&config.folder))
                .map_err(ui_err)?,
        ) as Box<dyn copilot_core::sync::remote::Remote>,
        "s3" => {
            let access = copilot_core::ai::load_key_for(&ws_key(id, "access"))
                .ok()
                .flatten()
                .ok_or("Workspace S3 access key missing — reconfigure the workspace remote.")?;
            let secret = copilot_core::ai::load_key_for(&ws_key(id, "secret"))
                .ok()
                .flatten()
                .ok_or("Workspace S3 secret key missing — reconfigure the workspace remote.")?;
            let remote =
                copilot_core::sync::remote::S3Remote::new(copilot_core::sync::s3::S3Config {
                    endpoint: config.endpoint.clone(),
                    bucket: config.bucket.clone(),
                    region: if config.region.is_empty() {
                        "us-east-1".into()
                    } else {
                        config.region.clone()
                    },
                    access_key: access,
                    secret_key: secret,
                });
            remote.ensure_bucket().map_err(ui_err)?;
            Box::new(remote)
        }
        other => return Err(format!("unknown workspace backend: {other}")),
    };
    let passphrase = copilot_core::ai::load_key_for(&ws_key(id, "pass"))
        .ok()
        .flatten()
        .ok_or("Workspace passphrase missing — reconfigure the workspace remote.")?;
    Ok((remote, passphrase))
}

/// Create a workspace locally; the creator joins as instructor (reading
/// groups) or member (labs — flat by default).
#[tauri::command]
fn workspace_create(
    state: State<AppState>,
    name: String,
    mode: String,
    member_name: String,
) -> Result<copilot_core::collab::Workspace, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let workspace = copilot_core::collab::create_workspace(&root, &name, &mode).map_err(ui_err)?;
    let me = WorkspaceMe {
        member_id: uuid::Uuid::new_v4(),
        name: member_name.clone(),
    };
    write_workspace_me(&root, workspace.id, &me)?;
    let role = if mode == "reading_group" {
        "instructor"
    } else {
        "member"
    };
    copilot_core::collab::append_member_event(
        &root,
        workspace.id,
        &copilot_core::collab::MemberEvent::Join {
            member_id: me.member_id,
            name: member_name,
            role: role.to_string(),
            at: copilot_core::bundle::now_rfc3339(),
        },
    )
    .map_err(ui_err)?;
    Ok(workspace)
}

#[tauri::command]
fn workspace_list(state: State<AppState>) -> Result<Vec<copilot_core::collab::Workspace>, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    Ok(copilot_core::collab::list_workspaces(&root))
}

/// Configure the shared remote for a workspace (create side or join side).
/// Secrets go to the OS keychain; validation derives the key against the
/// remote so a wrong endpoint/passphrase fails here.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn workspace_configure(
    state: State<AppState>,
    workspace_id: uuid::Uuid,
    backend: String,
    endpoint: String,
    bucket: String,
    region: String,
    folder: String,
    access_key: String,
    secret_key: String,
    passphrase: String,
) -> Result<(), String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let dir = workspace_dir(&root, workspace_id);
    std::fs::create_dir_all(&dir).map_err(ui_err)?;
    if backend == "s3" {
        copilot_core::ai::store_key_for(&ws_key(workspace_id, "access"), &access_key)
            .map_err(ui_err)?;
        copilot_core::ai::store_key_for(&ws_key(workspace_id, "secret"), &secret_key)
            .map_err(ui_err)?;
    }
    copilot_core::ai::store_key_for(&ws_key(workspace_id, "pass"), &passphrase).map_err(ui_err)?;
    let config = SyncConfigFile {
        backend,
        endpoint,
        bucket,
        region,
        folder,
    };
    std::fs::write(
        dir.join("remote.json"),
        serde_json::to_vec_pretty(&config).map_err(ui_err)?,
    )
    .map_err(ui_err)?;
    // Validation round trip.
    let (remote, passphrase) = workspace_remote(&root, workspace_id)?;
    copilot_core::sync::engine::derive_remote_key(remote.as_ref(), &passphrase)
        .map_err(|e| format!("remote validation failed: {e}"))?;
    Ok(())
}

/// Join a workspace someone shared with you: configure the remote first
/// (workspace_configure), then join pulls the workspace and records your
/// membership.
#[tauri::command(async)]
fn workspace_join(
    state: State<AppState>,
    workspace_id: uuid::Uuid,
    member_name: String,
) -> Result<copilot_core::sync::engine::SyncOutcome, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let (remote, passphrase) = workspace_remote(&root, workspace_id)?;
    let key = copilot_core::sync::engine::derive_remote_key(remote.as_ref(), &passphrase)
        .map_err(ui_err)?;
    let outcome = copilot_core::collab::sync_workspace(
        &root,
        workspace_id,
        &device_id(&root),
        key,
        remote.as_ref(),
    )
    .map_err(ui_err)?;
    let me = WorkspaceMe {
        member_id: uuid::Uuid::new_v4(),
        name: member_name.clone(),
    };
    write_workspace_me(&root, workspace_id, &me)?;
    copilot_core::collab::append_member_event(
        &root,
        workspace_id,
        &copilot_core::collab::MemberEvent::Join {
            member_id: me.member_id,
            name: member_name,
            role: "member".to_string(),
            at: copilot_core::bundle::now_rfc3339(),
        },
    )
    .map_err(ui_err)?;
    Ok(outcome)
}

/// Sync a workspace with its shared remote (presence heartbeat included).
#[tauri::command(async)]
fn workspace_sync(
    state: State<AppState>,
    workspace_id: uuid::Uuid,
) -> Result<copilot_core::sync::engine::SyncOutcome, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    if let Ok(me) = workspace_me(&root, workspace_id) {
        let _ = copilot_core::collab::record_presence(&root, workspace_id, me.member_id, &me.name);
    }
    let (remote, passphrase) = workspace_remote(&root, workspace_id)?;
    let key = copilot_core::sync::engine::derive_remote_key(remote.as_ref(), &passphrase)
        .map_err(ui_err)?;
    copilot_core::collab::sync_workspace(
        &root,
        workspace_id,
        &device_id(&root),
        key,
        remote.as_ref(),
    )
    .map_err(ui_err)
}

#[tauri::command]
fn workspace_share_paper(
    state: State<AppState>,
    workspace_id: uuid::Uuid,
    paper_id: String,
) -> Result<(), String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    copilot_core::collab::share_paper(&root, workspace_id, &paper_id)
        .map(|_| ())
        .map_err(ui_err)
}

#[derive(serde::Serialize)]
struct WorkspaceMemberView {
    member_id: uuid::Uuid,
    name: String,
    role: String,
    present: bool,
}

#[tauri::command]
fn workspace_members(
    state: State<AppState>,
    workspace_id: uuid::Uuid,
) -> Result<Vec<WorkspaceMemberView>, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let now = copilot_core::bundle::now_rfc3339();
    let active: std::collections::HashSet<uuid::Uuid> =
        copilot_core::collab::active_members(&root, workspace_id, &now)
            .map_err(ui_err)?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
    Ok(copilot_core::collab::members(&root, workspace_id)
        .map_err(ui_err)?
        .into_iter()
        .map(|(member_id, name, role)| WorkspaceMemberView {
            present: active.contains(&member_id),
            member_id,
            name,
            role,
        })
        .collect())
}

#[tauri::command]
fn workspace_thread(
    state: State<AppState>,
    workspace_id: uuid::Uuid,
    anchor: uuid::Uuid,
) -> Result<Vec<copilot_core::collab::ThreadMessage>, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    copilot_core::collab::thread(&root, workspace_id, anchor).map_err(ui_err)
}

#[tauri::command]
fn workspace_thread_post(
    state: State<AppState>,
    workspace_id: uuid::Uuid,
    anchor: uuid::Uuid,
    content: String,
) -> Result<copilot_core::collab::ThreadMessage, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let me = workspace_me(&root, workspace_id)?;
    let message = copilot_core::collab::ThreadMessage {
        id: uuid::Uuid::new_v4(),
        author_id: me.member_id,
        author_name: me.name,
        content,
        at: copilot_core::bundle::now_rfc3339(),
    };
    copilot_core::collab::append_thread_message(&root, workspace_id, anchor, &message)
        .map_err(ui_err)?;
    Ok(message)
}

#[tauri::command]
fn workspace_assign(
    state: State<AppState>,
    workspace_id: uuid::Uuid,
    paper_ref: String,
    quiz_node: Option<uuid::Uuid>,
) -> Result<(), String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let me = workspace_me(&root, workspace_id)?;
    copilot_core::collab::append_assignment(
        &root,
        workspace_id,
        &copilot_core::collab::Assignment {
            id: uuid::Uuid::new_v4(),
            paper_ref,
            quiz_node,
            assigned_by: me.member_id,
            at: copilot_core::bundle::now_rfc3339(),
        },
    )
    .map_err(ui_err)
}

#[tauri::command]
fn workspace_assignments(
    state: State<AppState>,
    workspace_id: uuid::Uuid,
) -> Result<Vec<copilot_core::collab::Assignment>, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    copilot_core::collab::assignments(&root, workspace_id).map_err(ui_err)
}

/// Opt in/out of progress sharing, or record a completion. The event states
/// exactly what is shared — consent is auditable in the data.
#[tauri::command]
fn workspace_progress(
    state: State<AppState>,
    workspace_id: uuid::Uuid,
    event: copilot_core::collab::ProgressEvent,
) -> Result<(), String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    copilot_core::collab::append_progress(&root, workspace_id, &event).map_err(ui_err)
}

/// This device's member identity in a workspace.
#[tauri::command]
fn workspace_whoami(
    state: State<AppState>,
    workspace_id: uuid::Uuid,
) -> Result<WorkspaceMe, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    workspace_me(&root, workspace_id)
}

#[tauri::command]
fn workspace_cohort(
    state: State<AppState>,
    workspace_id: uuid::Uuid,
) -> Result<Vec<copilot_core::collab::CohortRow>, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    copilot_core::collab::cohort_progress(&root, workspace_id).map_err(ui_err)
}

// ---- Literature reviews & gap detection (v4, library-level) ----

/// (paper_id → (title, published_at)) for the whole library.
fn paper_dates(
    state: &AppState,
) -> (
    Vec<copilot_core::library::PaperSummary>,
    std::collections::HashMap<String, Option<String>>,
) {
    let papers = state.library.lock().unwrap().list().unwrap_or_default();
    let dates = papers
        .iter()
        .map(|p| (p.id.clone(), p.published_at.clone()))
        .collect();
    (papers, dates)
}

#[tauri::command]
fn review_list(state: State<AppState>) -> Vec<copilot_core::reviews::Review> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    copilot_core::reviews::list(&root)
}

/// Create a review scoped to a concept query: papers = all library papers
/// whose registry concepts match the query (empty query = whole library).
#[tauri::command]
fn review_create(
    state: State<AppState>,
    name: String,
    query: String,
) -> Result<copilot_core::reviews::Review, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let registry = copilot_core::concept_registry::ConceptRegistry::open(&root);
    let (papers, _) = paper_dates(&state);
    let (concepts, scoped): (Vec<uuid::Uuid>, Vec<String>) = if query.trim().is_empty() {
        (vec![], papers.iter().map(|p| p.id.clone()).collect())
    } else {
        let hits = registry.search(&query).map_err(ui_err)?;
        let mut paper_ids: Vec<String> = hits
            .iter()
            .flat_map(|c| c.members.iter().map(|(p, _)| p.clone()))
            .collect();
        paper_ids.sort();
        paper_ids.dedup();
        (hits.iter().map(|c| c.id).collect(), paper_ids)
    };
    if scoped.is_empty() {
        return Err("No papers in scope — add papers or broaden the query.".to_string());
    }
    copilot_core::reviews::create(&root, &name, concepts, scoped).map_err(ui_err)
}

#[tauri::command]
fn review_get(state: State<AppState>, id: uuid::Uuid) -> Result<serde_json::Value, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let review = copilot_core::reviews::get(&root, id).map_err(ui_err)?;
    Ok(serde_json::json!({
        "review": review,
        "generated": copilot_core::reviews::generated(&root, id),
        "document": copilot_core::reviews::document(&root, id),
    }))
}

#[tauri::command]
fn review_save_document(
    state: State<AppState>,
    id: uuid::Uuid,
    content: String,
) -> Result<(), String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let _ = state.telemetry.record("review_edited");
    copilot_core::reviews::write_document(&root, id, &content).map_err(ui_err)
}

/// Regenerate the machine synthesis (generated.md only — the user's
/// document is untouched). Structure comes from the registry: shared
/// concepts, chronological builds-on relations from lineage order.
#[tauri::command(async)]
fn review_regenerate(
    state: State<AppState>,
    request_id: String,
    id: uuid::Uuid,
) -> Result<Option<copilot_core::reviews::RefreshSummary>, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let review = copilot_core::reviews::get(&root, id).map_err(ui_err)?;
    let registry_state = copilot_core::concept_registry::ConceptRegistry::open(&root)
        .state()
        .map_err(ui_err)?;
    let (papers, dates) = paper_dates(&state);

    let scoped: Vec<(String, String, Option<String>)> = papers
        .iter()
        .filter(|p| review.papers.contains(&p.id))
        .map(|p| (p.id.clone(), p.title.clone(), p.published_at.clone()))
        .collect();
    let mut shared: Vec<(String, Vec<String>)> = registry_state
        .concepts
        .iter()
        .filter_map(|concept| {
            let members: Vec<String> = concept
                .members
                .iter()
                .map(|(p, _)| p.clone())
                .filter(|p| review.papers.contains(p))
                .collect();
            (members.len() >= 2).then(|| (concept.name.clone(), members))
        })
        .collect();
    shared.sort_by_key(|s| std::cmp::Reverse(s.1.len()));
    shared.truncate(12);
    // Builds-on relations: chronological order within each shared concept.
    let mut relations: Vec<(String, String, String)> = Vec::new();
    for concept in &registry_state.concepts {
        let lineage = registry_state.lineage(concept.id, &dates);
        let in_scope: Vec<&(String, uuid::Uuid, Option<String>)> = lineage
            .iter()
            .filter(|(p, _, _)| review.papers.contains(p))
            .collect();
        for window in in_scope.windows(2) {
            relations.push((
                window[1].0.clone(),
                window[0].0.clone(),
                format!("builds on (via \"{}\")", concept.name),
            ));
        }
    }
    relations.sort();
    relations.dedup();
    relations.truncate(20);

    let _ = state.telemetry.record("review_generated");
    let llm = strong_llm_cancellable(&state, &request_id);
    let result = copilot_core::reviews::regenerate(
        &root,
        &review,
        &copilot_core::reviews::SynthesisInputs {
            papers: &scoped,
            shared_concepts: &shared,
            relations: &relations,
        },
        &llm,
    )
    .map_err(ui_err);
    state.cancelled_requests.lock().unwrap().remove(&request_id);
    result
}

/// Generate a gap report: structural computation first (deterministic),
/// light-tier narration second (prose only), persisted in `gaps/`.
#[tauri::command(async)]
fn gaps_generate(state: State<AppState>) -> Result<copilot_core::gaps::GapReport, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let registry_state = copilot_core::concept_registry::ConceptRegistry::open(&root)
        .state()
        .map_err(ui_err)?;
    let (_papers, dates) = paper_dates(&state);
    // Library-wide contradicts edges, node ids resolved to global concepts.
    let edges: Vec<copilot_core::gaps::GlobalEdge> =
        copilot_core::graph_index::GraphIndex::open(&root)
            .and_then(|index| index.edges_of_kind("contradicts"))
            .map(|edges| {
                edges
                    .into_iter()
                    .filter_map(|e| {
                        let from = registry_state.global_for(&e.paper_id, e.from)?.id;
                        let to = registry_state.global_for(&e.paper_id, e.to)?.id;
                        Some((e.paper_id, from, to, e.kind))
                    })
                    .collect()
            })
            .unwrap_or_default();

    let mut report = copilot_core::gaps::compute_gaps(&registry_state, &edges, &dates);
    let store = state.providers.clone();
    let narrator = |prompt: &str| -> Option<String> {
        let (provider, _) = pick_provider(&store, copilot_core::ai::ModelClass::Light).ok()?;
        provider
            .stream_chat(
                &[copilot_core::ai::ChatMessage {
                    images: Vec::new(),
                    role: "user".into(),
                    content: prompt.into(),
                }],
                &mut |_| {},
            )
            .ok()
    };
    copilot_core::gaps::narrate(&mut report, &narrator);
    copilot_core::gaps::save_report(&root, &report).map_err(ui_err)?;
    let _ = state.telemetry.record("gap_report_generated");
    Ok(report)
}

#[tauri::command]
fn gaps_latest(state: State<AppState>) -> Option<copilot_core::gaps::GapReport> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    copilot_core::gaps::latest_report(&root)
}

// ---- Extension mode (v4) ----

#[derive(serde::Serialize)]
struct ExtensionView {
    weaknesses: Option<copilot_core::extension::WeaknessDoc>,
    cards: Vec<copilot_core::extension::HypothesisCard>,
    outline: Option<String>,
    draft: Option<String>,
}

#[tauri::command]
fn extension_state(state: State<AppState>, paper_id: String) -> Result<ExtensionView, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    Ok(ExtensionView {
        weaknesses: copilot_core::extension::weaknesses(&bundle).map_err(ui_err)?,
        cards: copilot_core::extension::cards(&bundle).map_err(ui_err)?,
        outline: copilot_core::extension::read_document(&bundle, "outline.md"),
        draft: copilot_core::extension::read_document(&bundle, "draft.md"),
    })
}

/// Cancellable strong-tier completion bound to a request id.
fn strong_llm_cancellable<'a>(
    state: &'a AppState,
    request_id: &'a str,
) -> impl Fn(&str) -> Option<String> + 'a {
    move |prompt: &str| {
        let (provider, _) =
            pick_provider(&state.providers, copilot_core::ai::ModelClass::Strong).ok()?;
        let messages = [copilot_core::ai::ChatMessage {
            images: Vec::new(),
            role: "user".to_string(),
            content: prompt.to_string(),
        }];
        let is_cancelled = || {
            state
                .cancelled_requests
                .lock()
                .unwrap()
                .contains(request_id)
        };
        provider
            .stream_chat_cancellable(&messages, &mut |_| {}, &is_cancelled)
            .ok()
    }
}

/// Run/re-run the weaknesses stage (object-grounded, citation-validated).
#[tauri::command(async)]
fn extension_weaknesses(
    state: State<AppState>,
    request_id: String,
    paper_id: String,
) -> Result<Option<copilot_core::extension::WeaknessDoc>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let tree = bundle
        .read_derived_json("semantic_tree.json")
        .map_err(ui_err)?
        .ok_or("This paper is still being processed.")?;
    let title = bundle.metadata().map_err(ui_err)?.paper.title;
    let llm = strong_llm_cancellable(&state, &request_id);
    let result =
        copilot_core::extension::run_weaknesses(&bundle, &tree, &title, &llm).map_err(ui_err);
    state.cancelled_requests.lock().unwrap().remove(&request_id);
    result
}

/// Generate candidate hypothesis cards (added alongside, never replacing,
/// existing cards). Returns the refreshed live set.
#[tauri::command(async)]
fn extension_generate_cards(
    state: State<AppState>,
    request_id: String,
    paper_id: String,
) -> Result<Vec<copilot_core::extension::HypothesisCard>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let doc = copilot_core::extension::weaknesses(&bundle)
        .map_err(ui_err)?
        .ok_or("Run weakness finding first.")?;
    let title = bundle.metadata().map_err(ui_err)?.paper.title;
    let llm = strong_llm_cancellable(&state, &request_id);
    if let Some(raw) = llm(&copilot_core::extension::cards_prompt(&doc, &title)) {
        for card in copilot_core::extension::parse_cards(&doc, &raw) {
            copilot_core::extension::create_card(&bundle, card).map_err(ui_err)?;
        }
    }
    state.cancelled_requests.lock().unwrap().remove(&request_id);
    copilot_core::extension::cards(&bundle).map_err(ui_err)
}

#[tauri::command]
fn extension_card_edit(
    state: State<AppState>,
    paper_id: String,
    card_id: uuid::Uuid,
    claim: String,
    rationale: String,
    required_experiment: String,
    expected_evidence: String,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::extension::edit_card(
        &bundle,
        card_id,
        claim,
        rationale,
        required_experiment,
        expected_evidence,
    )
    .map_err(ui_err)
}

#[tauri::command]
fn extension_card_archive(
    state: State<AppState>,
    paper_id: String,
    card_id: uuid::Uuid,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::extension::archive_card(&bundle, card_id).map_err(ui_err)
}

/// Novelty check for one card: queries arXiv + Semantic Scholar (explicit
/// action; only the claim-derived query is sent — hosts shown in the UI),
/// ranks with local embeddings when loaded, records the evidence-backed
/// verdict on the card.
#[tauri::command(async)]
fn extension_novelty(
    state: State<AppState>,
    paper_id: String,
    card_id: uuid::Uuid,
) -> Result<copilot_core::novelty::NoveltyResult, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let card = copilot_core::extension::cards(&bundle)
        .map_err(ui_err)?
        .into_iter()
        .find(|c| c.id == card_id)
        .ok_or("Card not found.")?;

    let s2_key = copilot_core::ai::load_key_for("semantic-scholar").unwrap_or(None);
    let mut works = Vec::new();
    if let Ok(mut found) =
        copilot_core::novelty::search_semantic_scholar(&card.claim, s2_key.as_deref(), 20)
    {
        works.append(&mut found);
    }
    if let Ok(mut found) = copilot_core::novelty::search_arxiv(&card.claim, 20) {
        works.append(&mut found);
    }

    let embedder_guard = state.embedder.lock().unwrap();
    let embed = embedder_guard.as_ref().map(|embedder| {
        move |text: &str| {
            embedder
                .embed(&[text])
                .ok()
                .and_then(|mut v| (!v.is_empty()).then(|| v.remove(0)))
        }
    });
    let result = copilot_core::novelty::score_and_judge(
        &card.claim,
        works,
        embed
            .as_ref()
            .map(|f| f as &dyn Fn(&str) -> Option<Vec<f32>>),
    );
    copilot_core::extension::set_card_novelty(&bundle, card_id, result.clone()).map_err(ui_err)?;
    Ok(result)
}

/// "Design this experiment": creates a v3 experiment pre-filled from the
/// card and links the two both ways.
#[tauri::command]
fn extension_card_experiment(
    state: State<AppState>,
    paper_id: String,
    card_id: uuid::Uuid,
    object_id: uuid::Uuid,
    language: copilot_core::implementations::Language,
) -> Result<copilot_core::experiments::Experiment, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let card = copilot_core::extension::cards(&bundle)
        .map_err(ui_err)?
        .into_iter()
        .find(|c| c.id == card_id)
        .ok_or("Card not found.")?;
    let name = format!("H: {}", card.claim.chars().take(60).collect::<String>());
    let experiment = copilot_core::experiments::create(&bundle, &name, object_id, language, vec![])
        .map_err(ui_err)?;
    copilot_core::extension::link_card_experiment(&bundle, card_id, experiment.id)
        .map_err(ui_err)?;
    Ok(experiment)
}

/// Generate the outline or draft, constrained to the fixed bibliography;
/// unknown citation keys are stripped and the removed count returned.
#[tauri::command(async)]
fn extension_draft(
    state: State<AppState>,
    request_id: String,
    paper_id: String,
    stage: String, // "outline" | "draft"
) -> Result<serde_json::Value, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let metadata = bundle.metadata().map_err(ui_err)?;
    let bibliography = copilot_core::extension::assemble_bibliography(
        &bundle,
        &metadata.paper.title,
        &metadata.paper.authors,
    )
    .map_err(ui_err)?;
    let cards = copilot_core::extension::cards(&bundle).map_err(ui_err)?;
    if cards.is_empty() {
        return Err("Create at least one hypothesis card first.".to_string());
    }
    let keys: Vec<String> = bibliography
        .iter()
        .map(|b| {
            format!(
                "{} = \"{}\" ({})",
                b.key,
                b.title,
                b.year.map(|y| y.to_string()).unwrap_or_default()
            )
        })
        .collect();
    let card_block: String = cards
        .iter()
        .map(|c| {
            format!(
                "- Claim: {}\n  Rationale: {}\n  Experiment: {}\n  Novelty: {}\n",
                c.claim,
                c.rationale,
                c.required_experiment,
                c.novelty
                    .as_ref()
                    .map(|n| format!("{:?} ({} evidence items)", n.verdict, n.evidence.len()))
                    .unwrap_or_else(|| "unchecked".to_string()),
            )
        })
        .collect();
    let outline = copilot_core::extension::read_document(&bundle, "outline.md");
    let prompt = match stage.as_str() {
        "draft" => format!(
            "Write a LaTeX-body draft (sections, no preamble) of a paper extending \"{title}\" \
             based on the hypotheses and outline below. Cite ONLY with \\cite{{key}} using keys \
             from this closed list — any other key will be removed:\n{keys}\n\n\
             Hypotheses:\n{card_block}\nOutline:\n{outline}\n\
             Be precise; claims that need support you don't have should say so rather than cite loosely.",
            title = metadata.paper.title,
            keys = keys.join("\n"),
            outline = outline.as_deref().unwrap_or("(none — structure it yourself)"),
        ),
        _ => format!(
            "Write a section outline (markdown, one line of intent per section) for a paper \
             extending \"{title}\" based on these hypotheses. Where a section will rely on prior \
             work, name the citation key from this closed list:\n{keys}\n\nHypotheses:\n{card_block}",
            title = metadata.paper.title,
            keys = keys.join("\n"),
        ),
    };
    let llm = strong_llm_cancellable(&state, &request_id);
    let Some(raw) = llm(&prompt) else {
        state.cancelled_requests.lock().unwrap().remove(&request_id);
        return Ok(serde_json::json!({"content": null, "removed_citations": 0}));
    };
    state.cancelled_requests.lock().unwrap().remove(&request_id);
    let (cleaned, removed) = copilot_core::extension::strip_unknown_citations(&raw, &bibliography);
    let file = if stage == "draft" {
        "draft.md"
    } else {
        "outline.md"
    };
    copilot_core::extension::write_document(&bundle, file, &cleaned).map_err(ui_err)?;
    Ok(serde_json::json!({"content": cleaned, "removed_citations": removed}))
}

/// Save a user edit to the outline or draft.
#[tauri::command]
fn extension_save_document(
    state: State<AppState>,
    paper_id: String,
    name: String,
    content: String,
) -> Result<(), String> {
    if name != "outline.md" && name != "draft.md" {
        return Err("unknown document".to_string());
    }
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::extension::write_document(&bundle, &name, &content).map_err(ui_err)
}

/// Export main.tex + references.bib (resolved metadata only, provenance
/// marked, draft-labeled) to a user-chosen directory.
#[tauri::command]
fn extension_export(
    state: State<AppState>,
    paper_id: String,
    dest_dir: String,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let metadata = bundle.metadata().map_err(ui_err)?;
    let draft = copilot_core::extension::read_document(&bundle, "draft.md")
        .ok_or("No draft to export yet.")?;
    let bibliography = copilot_core::extension::assemble_bibliography(
        &bundle,
        &metadata.paper.title,
        &metadata.paper.authors,
    )
    .map_err(ui_err)?;
    let (main, bib) =
        copilot_core::extension::export_latex(&draft, &metadata.paper.title, &bibliography);
    let dest = std::path::Path::new(&dest_dir);
    std::fs::create_dir_all(dest).map_err(ui_err)?;
    std::fs::write(dest.join("main.tex"), main).map_err(ui_err)?;
    std::fs::write(dest.join("references.bib"), bib).map_err(ui_err)?;
    let _ = state.telemetry.record("draft_exported");
    Ok(())
}

// ---- Reproduction mode (v3) ----

#[derive(serde::Serialize)]
struct ReproView {
    state: copilot_core::reproduction::ReproState,
    repo: Option<copilot_core::reproduction::RepoRef>,
    plan: Option<copilot_core::reproduction::EnvPlan>,
    report: Option<String>,
    next_step: Option<copilot_core::reproduction::Step>,
}

#[tauri::command]
fn repro_state(state: State<AppState>, paper_id: String) -> Result<ReproView, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let repro = copilot_core::reproduction::state(&bundle);
    Ok(ReproView {
        next_step: repro.next_step(),
        state: repro,
        repo: copilot_core::reproduction::repo_ref(&bundle),
        plan: copilot_core::reproduction::env_plan(&bundle),
        report: copilot_core::reproduction::report(&bundle),
    })
}

/// Link the paper to its repository (resets nothing; clone reuses cache).
#[tauri::command]
fn repro_set_repo(state: State<AppState>, paper_id: String, remote: String) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let curated = CURATED_REPOS
        .iter()
        .any(|r| remote.trim().eq_ignore_ascii_case(r));
    copilot_core::reproduction::set_repo_ref(
        &bundle,
        &copilot_core::reproduction::RepoRef {
            remote: remote.trim().to_string(),
            commit: None,
            curated,
        },
    )
    .map_err(ui_err)
}

/// Curated, gate-tested repos (PRD: environment-hell mitigation). Everything
/// else still works with an explicit "unverified repo" notice.
const CURATED_REPOS: [&str; 3] = [
    "https://github.com/karpathy/micrograd",
    "https://github.com/karpathy/minGPT",
    "https://github.com/karpathy/nanoGPT",
];

/// Verification-run configuration: the command to run and the paper's
/// reported metrics to compare against (user- or corpus-provided).
#[tauri::command]
fn repro_configure_run(
    state: State<AppState>,
    paper_id: String,
    run_command: String,
    reported: std::collections::BTreeMap<String, f64>,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let dir = bundle
        .root()
        .join(copilot_core::reproduction::REPRODUCTION_DIR);
    std::fs::create_dir_all(&dir).map_err(ui_err)?;
    std::fs::write(
        dir.join("run.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "run_command": run_command,
            "reported": reported,
        }))
        .expect("serializable"),
    )
    .map_err(ui_err)
}

#[derive(Clone, serde::Serialize)]
struct ReproEvent {
    paper_id: String,
    step: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    done: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Advance the reproduction pipeline by one step on a worker thread.
/// Observable (`repro-progress` events), interruptible (sandbox kill for
/// the Run step; each step records its outcome so resumes are exact).
#[tauri::command]
fn repro_advance(
    app: AppHandle,
    state: State<AppState>,
    paper_id: String,
    run_id: String,
) -> Result<(), String> {
    use copilot_core::reproduction::{self as repro, Step};
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let Some(step) = repro::state(&bundle).next_step() else {
        return Err("Reproduction already complete — see the report.".to_string());
    };
    let repo = repro::repo_ref(&bundle).ok_or("Link the paper's GitHub repo first.")?;
    // The whole pipeline runs under the repo-scope consent (clone included:
    // it's the pipeline's first observable action).
    let scope = copilot_core::sandbox::ConsentScope::Repo(repo.remote.clone());
    if copilot_core::sandbox::check_grant(&bundle, &scope)
        .map_err(ui_err)?
        .is_none()
    {
        return Err("consent_required".to_string());
    }
    if matches!(step, Step::Clone) {
        let _ = state.telemetry.record("reproduction_attempted");
    }
    let library_root = state.library.lock().unwrap().root().to_path_buf();
    let app2 = app.clone();
    let store = state.providers.clone();

    std::thread::spawn(move || {
        let emit = |line: Option<String>, done: Option<bool>, error: Option<String>| {
            let _ = app2.emit(
                "repro-progress",
                ReproEvent {
                    paper_id: paper_id.clone(),
                    step: step.key().to_string(),
                    line,
                    done,
                    error,
                },
            );
        };
        let state = app2.state::<AppState>();
        let Ok(bundle) = state.library.lock().unwrap().get(&paper_id) else {
            emit(None, None, Some("paper not found".into()));
            return;
        };
        let strong = |prompt: &str| -> Option<String> {
            let (provider, _) = pick_provider(&store, copilot_core::ai::ModelClass::Strong).ok()?;
            provider
                .stream_chat(
                    &[copilot_core::ai::ChatMessage {
                        images: Vec::new(),
                        role: "user".into(),
                        content: prompt.into(),
                    }],
                    &mut |_| {},
                )
                .ok()
        };

        let outcome: Result<(String, Option<String>), String> = (|| {
            match step {
                Step::Clone => {
                    let (path, commit) = repro::clone_repo(&library_root, &repo.remote, &mut |l| {
                        emit(Some(l.to_string()), None, None)
                    })
                    .map_err(|e| e.to_string())?;
                    let _ = repro::set_repo_ref(
                        &bundle,
                        &copilot_core::reproduction::RepoRef {
                            commit: Some(commit.clone()),
                            ..repo.clone()
                        },
                    );
                    emit(
                        Some(format!("HEAD {commit} at {}", path.display())),
                        None,
                        None,
                    );
                    Ok(("completed".into(), Some(commit)))
                }
                Step::Env => {
                    let repo_dir = repro::cache_dir(&library_root, &repo.remote);
                    let plan = repro::detect_env(&repo_dir);
                    for command in &plan.setup_commands {
                        emit(Some(format!("$ {command}")), None, None);
                    }
                    repro::save_env_plan(&bundle, &plan).map_err(|e| e.to_string())?;
                    Ok(("completed".into(), Some(plan.kind)))
                }
                Step::Explain => {
                    let repo_dir = repro::cache_dir(&library_root, &repo.remote);
                    let tree: copilot_core::objects::SemanticTreeDocument = bundle
                        .read_derived_json("semantic_tree.json")
                        .ok()
                        .flatten()
                        .ok_or("paper not processed")?;
                    let prompt = copilot_core::codemap::mapping_prompt(&tree, &repo_dir, 6);
                    let prompt = format!(
                        "Explain this repository's architecture to someone who just read the paper: \
                         main components, data flow, where training/evaluation live. Markdown, concise.\n\
                         Use the same repository context below (ignore the JSON instructions in it).\n\n{prompt}"
                    );
                    match strong(&prompt) {
                        Some(text) => {
                            let dir = bundle.root().join(repro::REPRODUCTION_DIR);
                            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                            std::fs::write(dir.join("architecture.md"), &text)
                                .map_err(|e| e.to_string())?;
                            Ok(("completed".into(), None))
                        }
                        None => Ok(("skipped".into(), Some("no AI provider".into()))),
                    }
                }
                Step::Map => {
                    let repo_dir = repro::cache_dir(&library_root, &repo.remote);
                    let tree: copilot_core::objects::SemanticTreeDocument = bundle
                        .read_derived_json("semantic_tree.json")
                        .ok()
                        .flatten()
                        .ok_or("paper not processed")?;
                    let map =
                        copilot_core::codemap::run_mapping(&bundle, &tree, &repo_dir, &strong)
                            .map_err(|e| e.to_string())?;
                    match map {
                        Some(map) => Ok((
                            "completed".into(),
                            Some(format!("{} links", map.links.len())),
                        )),
                        None => Ok(("skipped".into(), Some("no AI provider".into()))),
                    }
                }
                Step::Run => {
                    let config: serde_json::Value =
                        std::fs::read(bundle.root().join(repro::REPRODUCTION_DIR).join("run.json"))
                            .ok()
                            .and_then(|b| serde_json::from_slice(&b).ok())
                            .ok_or("Configure the verification run command first.")?;
                    let run_command = config["run_command"].as_str().unwrap_or("").to_string();
                    if run_command.is_empty() {
                        return Err("Configure the verification run command first.".into());
                    }
                    let plan = repro::env_plan(&bundle);
                    let needs_network = plan
                        .as_ref()
                        .map(|p| !p.setup_commands.is_empty())
                        .unwrap_or(false);
                    let grant = copilot_core::sandbox::check_grant(&bundle, &scope)
                        .map_err(|e| e.to_string())?
                        .ok_or("consent_required")?;
                    if needs_network && !grant.network() {
                        return Err("network_consent_required".into());
                    }
                    let runtime = copilot_core::sandbox::detect_runtime()
                        .ok_or(copilot_core::sandbox::SandboxError::NoRuntime.to_string())?;
                    let setup = plan
                        .map(|p| {
                            p.setup_commands
                                .iter()
                                .map(|c| {
                                    // pip installs go to tmpfs (rootfs is RO).
                                    c.replace("pip install", "pip install --target /tmp/deps")
                                })
                                .collect::<Vec<_>>()
                                .join(" && ")
                        })
                        .unwrap_or_default();
                    let script = if setup.is_empty() {
                        run_command.clone()
                    } else {
                        format!("{setup} && PYTHONPATH=/tmp/deps {run_command}")
                    };
                    let repo_dir = repro::cache_dir(&library_root, &repo.remote);
                    let spec = copilot_core::sandbox::RunSpec {
                        image: "python:3.12-slim".into(),
                        command: vec!["sh".into(), "-c".into(), script],
                        mount_ro: Some(repo_dir),
                        network: needs_network,
                        memory_mb: 2048,
                        cpus: 2.0,
                        pids: 256,
                        timeout: std::time::Duration::from_secs(1800),
                        ..Default::default()
                    };
                    let is_cancelled =
                        || state.cancelled_requests.lock().unwrap().contains(&run_id);
                    let outcome = copilot_core::sandbox::run(
                        &runtime,
                        &spec,
                        &grant,
                        &mut |line| emit(Some(line.to_string()), None, None),
                        &is_cancelled,
                    )
                    .map_err(|e| e.to_string())?;
                    let dir = bundle.root().join(repro::REPRODUCTION_DIR);
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    std::fs::write(
                        dir.join("run_log.txt"),
                        format!("{}\n{}", outcome.stdout, outcome.stderr),
                    )
                    .map_err(|e| e.to_string())?;
                    match outcome.status {
                        copilot_core::sandbox::RunStatus::Completed { exit_code: 0 } => {
                            Ok(("completed".into(), None))
                        }
                        other => Err(format!("run did not complete cleanly: {other:?}")),
                    }
                }
                Step::Verify => {
                    let log = std::fs::read_to_string(
                        bundle
                            .root()
                            .join(repro::REPRODUCTION_DIR)
                            .join("run_log.txt"),
                    )
                    .map_err(|_| "no run log — run the verification first")?;
                    let produced_f64: std::collections::BTreeMap<String, f64> =
                        copilot_core::experiments::parse_metrics(&log);
                    let config: serde_json::Value =
                        std::fs::read(bundle.root().join(repro::REPRODUCTION_DIR).join("run.json"))
                            .ok()
                            .and_then(|b| serde_json::from_slice(&b).ok())
                            .unwrap_or_default();
                    let reported: std::collections::BTreeMap<String, f64> =
                        serde_json::from_value(config["reported"].clone()).unwrap_or_default();
                    let comparisons = copilot_core::reproduction::verify(&reported, &produced_f64);
                    let detail = serde_json::to_string(&comparisons).ok();
                    std::fs::write(
                        bundle
                            .root()
                            .join(repro::REPRODUCTION_DIR)
                            .join("verify.json"),
                        serde_json::to_vec_pretty(&comparisons).expect("serializable"),
                    )
                    .map_err(|e| e.to_string())?;
                    Ok(("completed".into(), detail))
                }
                Step::Report => {
                    let comparisons: Vec<copilot_core::reproduction::MetricComparison> =
                        std::fs::read(
                            bundle
                                .root()
                                .join(repro::REPRODUCTION_DIR)
                                .join("verify.json"),
                        )
                        .ok()
                        .and_then(|b| serde_json::from_slice(&b).ok())
                        .unwrap_or_default();
                    let plan = repro::env_plan(&bundle);
                    let current = repro::repo_ref(&bundle).unwrap_or(repo.clone());
                    let notes = if current.curated {
                        String::new()
                    } else {
                        "Unverified repo — outside the curated corpus; steps may have needed manual help.".to_string()
                    };
                    repro::write_report(&bundle, &current, plan.as_ref(), &comparisons, &notes)
                        .map_err(|e| e.to_string())?;
                    Ok(("completed".into(), None))
                }
            }
        })();

        match outcome {
            Ok((status, detail)) => {
                let _ = repro::record_step(&bundle, step, &status, detail);
                if matches!(step, Step::Report) && status == "completed" {
                    let _ = state.telemetry.record("reproduction_completed");
                }
                emit(None, Some(true), None);
            }
            Err(message) => {
                let _ = repro::record_step(&bundle, step, "failed", Some(message.clone()));
                emit(None, None, Some(message));
            }
        }
        state.cancelled_requests.lock().unwrap().remove(&run_id);
    });
    Ok(())
}

/// The architecture explanation and code map for the repo browser.
#[tauri::command]
fn repro_artifacts(state: State<AppState>, paper_id: String) -> Result<serde_json::Value, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let architecture = std::fs::read_to_string(
        bundle
            .root()
            .join(copilot_core::reproduction::REPRODUCTION_DIR)
            .join("architecture.md"),
    )
    .ok();
    let map = copilot_core::codemap::get(&bundle).map_err(ui_err)?;
    Ok(serde_json::json!({"architecture": architecture, "code_map": map}))
}

/// Repo browser: file tree + file contents from the library cache (offline
/// once cloned; no container runtime involved).
#[tauri::command]
fn repro_list_files(state: State<AppState>, paper_id: String) -> Result<Vec<String>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let repo = copilot_core::reproduction::repo_ref(&bundle).ok_or("no repo linked")?;
    let root = state.library.lock().unwrap().root().to_path_buf();
    let dir = copilot_core::reproduction::cache_dir(&root, &repo.remote);
    let mut files = Vec::new();
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" || name == "__pycache__" {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, out);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().to_string());
            }
        }
    }
    walk(&dir, &dir, &mut files);
    files.sort();
    Ok(files)
}

#[tauri::command]
fn repro_read_file(
    state: State<AppState>,
    paper_id: String,
    file: String,
) -> Result<String, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let repo = copilot_core::reproduction::repo_ref(&bundle).ok_or("no repo linked")?;
    let root = state.library.lock().unwrap().root().to_path_buf();
    let dir = copilot_core::reproduction::cache_dir(&root, &repo.remote);
    // Path containment: never read outside the cached clone.
    let requested = dir.join(&file);
    let canonical = requested.canonicalize().map_err(ui_err)?;
    if !canonical.starts_with(dir.canonicalize().map_err(ui_err)?) {
        return Err("invalid path".to_string());
    }
    std::fs::read_to_string(canonical).map_err(ui_err)
}

/// Repo-cache disk usage for the settings hygiene surface.
#[tauri::command]
fn repos_cache_usage(state: State<AppState>) -> Result<serde_json::Value, String> {
    let root = state
        .library
        .lock()
        .unwrap()
        .root()
        .join(copilot_core::reproduction::REPOS_CACHE_DIR);
    fn dir_size(path: &std::path::Path) -> u64 {
        let Ok(entries) = std::fs::read_dir(path) else {
            return 0;
        };
        entries
            .flatten()
            .map(|e| {
                let p = e.path();
                if p.is_dir() {
                    dir_size(&p)
                } else {
                    e.metadata().map(|m| m.len()).unwrap_or(0)
                }
            })
            .sum()
    }
    let repos = std::fs::read_dir(&root).map(|d| d.count()).unwrap_or(0);
    Ok(serde_json::json!({
        "path": root.to_string_lossy(),
        "bytes": dir_size(&root),
        "repos": repos,
    }))
}

/// Clear the repo cache (explicitly confirmed in the UI). Bundles keep
/// their references and derived artifacts; clones re-download on demand.
#[tauri::command]
fn repos_cache_clear(state: State<AppState>) -> Result<(), String> {
    let root = state
        .library
        .lock()
        .unwrap()
        .root()
        .join(copilot_core::reproduction::REPOS_CACHE_DIR);
    if root.is_dir() {
        std::fs::remove_dir_all(&root).map_err(ui_err)?;
    }
    Ok(())
}

// ---- Experiment mode (v3) ----

#[tauri::command]
fn experiment_create(
    state: State<AppState>,
    paper_id: String,
    name: String,
    object_id: uuid::Uuid,
    language: copilot_core::implementations::Language,
    parameters: Vec<copilot_core::experiments::ParameterSpec>,
) -> Result<copilot_core::experiments::Experiment, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::experiments::create(&bundle, &name, object_id, language, parameters)
        .map_err(ui_err)
}

#[tauri::command]
fn experiment_list(
    state: State<AppState>,
    paper_id: String,
) -> Result<Vec<copilot_core::experiments::Experiment>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::experiments::list(&bundle).map_err(ui_err)
}

#[tauri::command]
fn experiment_runs(
    state: State<AppState>,
    paper_id: String,
    experiment_id: uuid::Uuid,
) -> Result<Vec<copilot_core::experiments::ExperimentRun>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::experiments::runs(&bundle, experiment_id).map_err(ui_err)
}

/// Run an experiment: the anchored implementation executes in the sandbox
/// with parameters as env vars (`EXP_<NAME>`); metrics parse from the
/// documented stdout convention; the run record appends when the container
/// finishes — including limit-killed/cancelled outcomes (honest statuses).
/// A pre-run `prediction` rides on the record (predict–observe–explain) and
/// feeds learner memory.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn experiment_run(
    app: AppHandle,
    state: State<AppState>,
    run_id: String,
    paper_id: String,
    experiment_id: uuid::Uuid,
    params: std::collections::BTreeMap<String, String>,
    prediction: Option<String>,
    run_by: Option<String>,
) -> Result<(), String> {
    use copilot_core::implementations::Language;
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let experiment = copilot_core::experiments::get(&bundle, experiment_id).map_err(ui_err)?;
    let dir = bundle
        .root()
        .join(copilot_core::implementations::IMPLEMENTATIONS_DIR)
        .join(experiment.object_id.to_string());
    let key = experiment.language.key();
    let ext = experiment.language.extension();
    if !dir.join(format!("{key}.{ext}")).is_file() {
        return Err("Generate an implementation for this object first.".to_string());
    }
    let script = match experiment.language {
        Language::Rust => format!(
            "cp /work/{key}.{ext} /tmp/main.rs && rustc -O /tmp/main.rs -o /tmp/main && /tmp/main"
        ),
        _ => format!("python /work/{key}.{ext}"),
    };
    let env: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| {
            (
                format!(
                    "EXP_{}",
                    k.to_uppercase()
                        .replace(|c: char| !c.is_alphanumeric(), "_")
                ),
                v.clone(),
            )
        })
        .collect();
    let spec = copilot_core::sandbox::RunSpec {
        image: experiment.language.image().to_string(),
        command: vec!["sh".into(), "-c".into(), script],
        mount_ro: Some(dir),
        env,
        timeout: std::time::Duration::from_secs(600),
        ..Default::default()
    };
    let _ = state.telemetry.record("experiment_run");
    let paper = paper_id.clone();
    spawn_sandbox_run(
        &app,
        run_id,
        paper_id,
        copilot_core::sandbox::ConsentScope::Experiment(experiment_id),
        spec,
        move |app, outcome| {
            use copilot_core::sandbox::RunStatus;
            let state = app.state::<AppState>();
            let Ok(bundle) = state.library.lock().unwrap().get(&paper) else {
                return;
            };
            let status = match &outcome.status {
                RunStatus::Completed { exit_code: 0 } => "completed",
                RunStatus::Completed { .. } => "failed",
                RunStatus::LimitKilled { .. } => "limit_killed",
                RunStatus::Cancelled => "cancelled",
            };
            let metrics = copilot_core::experiments::parse_metrics(&outcome.stdout);
            let tail: String = outcome
                .stdout
                .lines()
                .rev()
                .take(30)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            let run = copilot_core::experiments::ExperimentRun {
                run_id: uuid::Uuid::new_v4(),
                params: params.clone(),
                metrics: metrics.clone(),
                stdout_tail: tail,
                duration_ms: outcome.duration_ms,
                status: status.to_string(),
                prediction: prediction.clone(),
                run_by: run_by.clone(),
                at: copilot_core::bundle::now_rfc3339(),
            };
            let _ = copilot_core::experiments::record_run(&bundle, experiment_id, &run);
            // Predict–observe–explain feeds learner memory: the prediction
            // moment is the learning signal, recorded honestly either way.
            if let Some(prediction) = &prediction {
                if status == "completed" {
                    let root = state.library.lock().unwrap().root().to_path_buf();
                    let model = copilot_core::learning::LearnerModel::open(&root);
                    let observed: Vec<String> =
                        metrics.iter().map(|(k, v)| format!("{k}={v}")).collect();
                    let _ = model.record_episode(&copilot_core::learning::EpisodeEvent {
                        paper_id: paper.clone(),
                        object: Some(experiment.object_id),
                        concept: None,
                        kind: "insight".to_string(),
                        summary: format!(
                            "Experiment \"{}\": predicted \"{}\", observed {}",
                            experiment.name,
                            prediction,
                            if observed.is_empty() {
                                "no metrics".to_string()
                            } else {
                                observed.join(", ")
                            }
                        ),
                        covered_turns: None,
                        at: copilot_core::bundle::now_rfc3339(),
                    });
                }
            }
        },
    );
    Ok(())
}

/// Streamed AI discussion of experiment results (same `ai-stream` envelope);
/// context = definition + recorded runs, persisted to the experiment's
/// discussion journal with v1 honesty rules.
#[tauri::command(async)]
fn experiment_stream(
    app: AppHandle,
    state: State<AppState>,
    request_id: String,
    paper_id: String,
    experiment_id: uuid::Uuid,
    question: String,
) -> Result<String, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let experiment = copilot_core::experiments::get(&bundle, experiment_id).map_err(ui_err)?;
    let runs = copilot_core::experiments::runs(&bundle, experiment_id).map_err(ui_err)?;
    let title = bundle.metadata().map_err(ui_err)?.paper.title;
    let history = copilot_core::chat::history(&bundle, experiment_id).map_err(ui_err)?;
    let thread = copilot_core::chat::as_thread(&history);

    let (provider, config) = pick_provider(&state.providers, copilot_core::ai::ModelClass::Strong)?;
    let budget = config.context_budget_tokens(copilot_core::ai::ModelClass::Strong);
    let context = copilot_core::context::assemble_experiment(
        &title,
        &experiment,
        &runs,
        Some(&question),
        &thread,
        budget,
    );
    copilot_core::chat::append(
        &bundle,
        experiment_id,
        &copilot_core::chat::user_message("experiment", question),
    )
    .map_err(ui_err)?;

    let emit = |event: AiStreamEvent| {
        let _ = app.emit("ai-stream", event);
    };
    emit(AiStreamEvent {
        host: Some(config.host()),
        ..AiStreamEvent::empty(&request_id)
    });
    let is_cancelled = || {
        state
            .cancelled_requests
            .lock()
            .unwrap()
            .contains(&request_id)
    };
    let mut accumulated = String::new();
    let result = provider.stream_chat_cancellable(
        &context.messages,
        &mut |token| {
            accumulated.push_str(token);
            emit(AiStreamEvent {
                token: Some(token.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
        },
        &is_cancelled,
    );
    state.cancelled_requests.lock().unwrap().remove(&request_id);
    match result {
        Ok(full) if !full.trim().is_empty() => {
            let turn = copilot_core::chat::assistant_message(full.clone(), false);
            copilot_core::chat::append(&bundle, experiment_id, &turn).map_err(ui_err)?;
            emit(AiStreamEvent {
                done: Some(true),
                ..AiStreamEvent::empty(&request_id)
            });
            Ok(full)
        }
        Ok(_) => {
            let message = "The model produced no text — try again.";
            emit(AiStreamEvent {
                error: Some(message.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
            Err(message.to_string())
        }
        Err(e) => {
            if !accumulated.is_empty() {
                let turn = copilot_core::chat::assistant_message(accumulated, true);
                let _ = copilot_core::chat::append(&bundle, experiment_id, &turn);
            }
            let cancelled = matches!(e, copilot_core::ai::AiError::Cancelled);
            emit(AiStreamEvent {
                cancelled: cancelled.then_some(true),
                error: (!cancelled).then(|| e.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
            if cancelled {
                Ok(String::new())
            } else {
                Err(ui_err(e))
            }
        }
    }
}

// ---- Sandboxed execution (v3): runtime status, consent, run plumbing ----

/// Detected container runtime, or `None` → the designed "install Docker or
/// Podman" state. Detection is a subprocess probe; run it async.
#[tauri::command(async)]
fn sandbox_runtime_status() -> Option<copilot_core::sandbox::RuntimeInfo> {
    copilot_core::sandbox::detect_runtime()
}

/// Standing consents for a paper: (scope, network?, granted_at).
#[tauri::command]
fn sandbox_consents(
    state: State<AppState>,
    paper_id: String,
) -> Result<Vec<(copilot_core::sandbox::ConsentScope, bool, String)>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::sandbox::list_grants(&bundle).map_err(ui_err)
}

/// Record the user's explicit approval for a scope. The UI calls this only
/// from the consent dialog that showed mounts + the no-network policy.
#[tauri::command]
fn sandbox_grant(
    state: State<AppState>,
    paper_id: String,
    scope: copilot_core::sandbox::ConsentScope,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::sandbox::record_grant(&bundle, scope).map_err(ui_err)
}

/// Per-run network opt-in with its stated reason.
#[tauri::command]
fn sandbox_grant_network(
    state: State<AppState>,
    paper_id: String,
    scope: copilot_core::sandbox::ConsentScope,
    reason: String,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::sandbox::record_network_grant(&bundle, scope, &reason).map_err(ui_err)
}

#[tauri::command]
fn sandbox_revoke(
    state: State<AppState>,
    paper_id: String,
    scope: copilot_core::sandbox::ConsentScope,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::sandbox::revoke_grant(&bundle, scope).map_err(ui_err)
}

/// Kill a running sandbox job (kill-anytime; partials are preserved).
#[tauri::command]
fn sandbox_kill(state: State<AppState>, run_id: String) {
    state.cancelled_requests.lock().unwrap().insert(run_id);
}

#[derive(Clone, serde::Serialize)]
struct SandboxEvent {
    run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<copilot_core::sandbox::RunOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Run a spec on a worker thread, streaming `sandbox-progress` events
/// (log lines, then one outcome or error). The consent token is checked
/// here — inside the paper's bundle — right before the run; a revocation
/// that landed after the UI opened still blocks. Every v3 feature's run
/// path funnels through this helper.
fn spawn_sandbox_run(
    app: &AppHandle,
    run_id: String,
    paper_id: String,
    scope: copilot_core::sandbox::ConsentScope,
    spec: copilot_core::sandbox::RunSpec,
    on_done: impl FnOnce(&AppHandle, &copilot_core::sandbox::RunOutcome) + Send + 'static,
) {
    let app = app.clone();
    std::thread::spawn(move || {
        let emit = |event: SandboxEvent| {
            let _ = app.emit("sandbox-progress", event);
        };
        let empty = |run_id: &str| SandboxEvent {
            run_id: run_id.to_string(),
            line: None,
            outcome: None,
            error: None,
        };
        let state = app.state::<AppState>();
        let bundle = match state.library.lock().unwrap().get(&paper_id) {
            Ok(bundle) => bundle,
            Err(e) => {
                emit(SandboxEvent {
                    error: Some(e.to_string()),
                    ..empty(&run_id)
                });
                return;
            }
        };
        let Some(runtime) = copilot_core::sandbox::detect_runtime() else {
            emit(SandboxEvent {
                error: Some(copilot_core::sandbox::SandboxError::NoRuntime.to_string()),
                ..empty(&run_id)
            });
            return;
        };
        let grant = match copilot_core::sandbox::check_grant(&bundle, &scope) {
            Ok(Some(grant)) => grant,
            Ok(None) => {
                emit(SandboxEvent {
                    error: Some("consent_required".to_string()),
                    ..empty(&run_id)
                });
                return;
            }
            Err(e) => {
                emit(SandboxEvent {
                    error: Some(e.to_string()),
                    ..empty(&run_id)
                });
                return;
            }
        };
        let is_cancelled = || state.cancelled_requests.lock().unwrap().contains(&run_id);
        let result = copilot_core::sandbox::run(
            &runtime,
            &spec,
            &grant,
            &mut |line| {
                emit(SandboxEvent {
                    line: Some(line.to_string()),
                    ..empty(&run_id)
                });
            },
            &is_cancelled,
        );
        state.cancelled_requests.lock().unwrap().remove(&run_id);
        match result {
            Ok(outcome) => {
                on_done(&app, &outcome);
                emit(SandboxEvent {
                    outcome: Some(outcome),
                    ..empty(&run_id)
                });
            }
            Err(e) => emit(SandboxEvent {
                error: Some(e.to_string()),
                ..empty(&run_id)
            }),
        }
    });
}

fn spawn_episode_summary(app: &AppHandle, paper_id: String, object_id: uuid::Uuid) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let (bundle, root) = {
            let library = state.library.lock().unwrap();
            let Ok(bundle) = library.get(&paper_id) else {
                return;
            };
            (bundle, library.root().to_path_buf())
        };
        let model = copilot_core::learning::LearnerModel::open(&root);
        let store = state.providers.clone();
        let llm = move |prompt: &str| {
            let (provider, _) = pick_provider(&store, copilot_core::ai::ModelClass::Light).ok()?;
            let messages = [copilot_core::ai::ChatMessage {
                images: Vec::new(),
                role: "user".to_string(),
                content: prompt.to_string(),
            }];
            provider.stream_chat(&messages, &mut |_| {}).ok()
        };
        let _ =
            copilot_core::learning::summarize_episode(&bundle, &model, &paper_id, object_id, &llm);
    });
}

/// Library-wide concept search: "where did I learn X" → global concepts with
/// the papers/nodes where they appear. Offline, <150 ms budget.
#[tauri::command]
fn concept_search(
    state: State<AppState>,
    query: String,
) -> Result<Vec<copilot_core::concept_registry::GlobalConcept>, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    copilot_core::concept_registry::ConceptRegistry::open(&root)
        .search(&query)
        .map_err(ui_err)
}

/// Other papers where this node's concept appears ("seen in paper X").
#[tauri::command]
fn concept_occurrences(
    state: State<AppState>,
    paper_id: String,
    node: uuid::Uuid,
) -> Result<Vec<(String, uuid::Uuid)>, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let registry = copilot_core::concept_registry::ConceptRegistry::open(&root);
    Ok(registry
        .state()
        .map_err(ui_err)?
        .occurrences_elsewhere(&paper_id, node))
}

#[derive(serde::Serialize)]
struct SeenElsewhere {
    concept: String,
    paper_id: String,
    paper_title: String,
    /// The concept's node in the other paper; its introducing object is the
    /// navigation target.
    node: uuid::Uuid,
    object: Option<uuid::Uuid>,
}

/// "Seen in paper X": for an object, every concept anchored to it that also
/// appears in other papers — with the other paper's introducing object for
/// one-click cross-paper navigation.
#[tauri::command]
fn object_seen_elsewhere(
    state: State<AppState>,
    paper_id: String,
    object_id: uuid::Uuid,
) -> Result<Vec<SeenElsewhere>, String> {
    let (root, papers) = {
        let library = state.library.lock().unwrap();
        (
            library.root().to_path_buf(),
            library.list().map_err(ui_err)?,
        )
    };
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let Some(graph) = bundle
        .read_derived_json::<copilot_core::concepts::KnowledgeGraph>("knowledge_graph.json")
        .map_err(ui_err)?
    else {
        return Ok(Vec::new());
    };
    let registry_state = copilot_core::concept_registry::ConceptRegistry::open(&root)
        .state()
        .map_err(ui_err)?;

    let mut seen = Vec::new();
    for node in graph
        .nodes
        .iter()
        .filter(|n| n.object_ids.contains(&object_id))
    {
        for (other_paper, other_node) in registry_state.occurrences_elsewhere(&paper_id, node.id) {
            let Some(summary) = papers.iter().find(|p| p.id == other_paper) else {
                continue;
            };
            // The other paper's introducing object for this concept.
            let object = state
                .library
                .lock()
                .unwrap()
                .get(&other_paper)
                .ok()
                .and_then(|b| {
                    b.read_derived_json::<copilot_core::concepts::KnowledgeGraph>(
                        "knowledge_graph.json",
                    )
                    .ok()
                    .flatten()
                })
                .and_then(|g| {
                    g.nodes
                        .iter()
                        .find(|n| n.id == other_node)
                        .and_then(|n| n.object_ids.first().copied())
                });
            seen.push(SeenElsewhere {
                concept: node.name.clone(),
                paper_id: other_paper,
                paper_title: summary.title.clone(),
                node: other_node,
                object,
            });
        }
    }
    Ok(seen)
}

/// User-confirmed registry correction: merge two global concepts or split a
/// node out of one (splits are respected by future auto-matching).
#[tauri::command]
fn concept_registry_record(
    state: State<AppState>,
    event: copilot_core::concept_registry::RegistryEvent,
) -> Result<(), String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    copilot_core::concept_registry::ConceptRegistry::open(&root)
        .record(event)
        .map_err(ui_err)
}

// ---- Reading mode: lessons, quizzes, flashcards, tutor (v2) ----

/// Strong-tier blocking completion for lesson/quiz/flashcard generation.
/// `None` when no provider is configured — callers show the no-key state.
fn strong_llm(state: &AppState) -> impl Fn(&str) -> Option<String> {
    let store = state.providers.clone();
    move |prompt: &str| {
        let (provider, _) = pick_provider(&store, copilot_core::ai::ModelClass::Strong).ok()?;
        let messages = [copilot_core::ai::ChatMessage {
            images: Vec::new(),
            role: "user".to_string(),
            content: prompt.to_string(),
        }];
        provider.stream_chat(&messages, &mut |_| {}).ok()
    }
}

fn paper_graph(
    state: &AppState,
    paper_id: &str,
) -> Result<
    (
        copilot_core::bundle::Bundle,
        copilot_core::concepts::KnowledgeGraph,
        copilot_core::objects::SemanticTreeDocument,
    ),
    String,
> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(paper_id)
        .map_err(ui_err)?;
    let graph = bundle
        .read_derived_json("knowledge_graph.json")
        .map_err(ui_err)?
        .ok_or("This paper's concept graph isn't ready yet.")?;
    let tree = bundle
        .read_derived_json("semantic_tree.json")
        .map_err(ui_err)?
        .ok_or("This paper is still being processed.")?;
    Ok((bundle, graph, tree))
}

/// The paper's course outline: topological lesson order with mastery flags
/// (mastered lessons collapse in the UI — they are never locked).
#[tauri::command]
fn lessons_sequence(
    state: State<AppState>,
    paper_id: String,
) -> Result<Vec<copilot_core::lessons::LessonEntry>, String> {
    let (_bundle, graph, _tree) = paper_graph(&state, &paper_id)?;
    let root = state.library.lock().unwrap().root().to_path_buf();
    let snapshot = copilot_core::learning::LearnerModel::open(&root)
        .snapshot()
        .map_err(ui_err)?;
    let node_globals: std::collections::HashMap<uuid::Uuid, uuid::Uuid> =
        copilot_core::concept_registry::ConceptRegistry::open(&root)
            .state()
            .map(|s| {
                graph
                    .nodes
                    .iter()
                    .filter_map(|n| s.global_for(&paper_id, n.id).map(|g| (n.id, g.id)))
                    .collect()
            })
            .unwrap_or_default();
    let mastered = |id: uuid::Uuid| {
        let by_global = node_globals.get(&id).and_then(|g| snapshot.mastery_of(*g));
        by_global
            .or_else(|| snapshot.mastery_of(id))
            .map(|m| !m.estimated && m.score >= copilot_core::learning::MASTERED_SCORE)
            .unwrap_or(false)
    };
    Ok(copilot_core::lessons::lesson_sequence(&graph, &mastered))
}

/// Cached lesson if present; otherwise generate via the strong tier (blocking
/// async command — the UI shows a skeleton meanwhile). `None` = no provider.
#[tauri::command(async)]
fn lesson_get_or_generate(
    state: State<AppState>,
    paper_id: String,
    node: uuid::Uuid,
) -> Result<Option<copilot_core::lessons::Lesson>, String> {
    let (bundle, graph, tree) = paper_graph(&state, &paper_id)?;
    if let Some(cached) = copilot_core::lessons::lesson_get(&bundle, node).map_err(ui_err)? {
        return Ok(Some(cached));
    }
    let llm = strong_llm(&state);
    copilot_core::lessons::lesson_generate(&bundle, &graph, &tree, node, &llm).map_err(ui_err)
}

#[tauri::command(async)]
fn quiz_get_or_generate(
    state: State<AppState>,
    paper_id: String,
    node: uuid::Uuid,
) -> Result<Option<copilot_core::lessons::Quiz>, String> {
    let (bundle, graph, tree) = paper_graph(&state, &paper_id)?;
    let llm = strong_llm(&state);
    copilot_core::lessons::quiz_generate(&bundle, &graph, &tree, node, &llm).map_err(ui_err)
}

#[tauri::command(async)]
fn flashcards_get_or_generate(
    state: State<AppState>,
    paper_id: String,
    node: uuid::Uuid,
) -> Result<Option<copilot_core::lessons::FlashcardDeck>, String> {
    let (bundle, graph, tree) = paper_graph(&state, &paper_id)?;
    let llm = strong_llm(&state);
    copilot_core::lessons::deck_generate(&bundle, &graph, &tree, node, &llm).map_err(ui_err)
}

/// Record a learning outcome (quiz answer, flashcard review, tutor attempt)
/// into mastery memory. `quality` is the SM-2 scale 0–5. The concept is
/// recorded under its *global* id when the registry maps it, so mastery is
/// shared across papers. One data path: dashboard, lesson collapsing, and
/// due queues all read the same events.
#[tauri::command]
fn learning_record(
    state: State<AppState>,
    paper_id: String,
    node: uuid::Uuid,
    quality: u8,
    source: String,
    object: Option<uuid::Uuid>,
) -> Result<(), String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let concept = copilot_core::concept_registry::ConceptRegistry::open(&root)
        .state()
        .ok()
        .and_then(|s| s.global_for(&paper_id, node).map(|g| g.id))
        .unwrap_or(node);
    if source == "quiz" {
        let _ = state.telemetry.record("quiz_answered");
    }
    copilot_core::learning::LearnerModel::open(&root)
        .record_mastery(&copilot_core::learning::MasteryEvent {
            concept,
            object,
            quality: quality.min(5),
            source,
            at: copilot_core::bundle::now_rfc3339(),
        })
        .map_err(ui_err)
}

/// Due-for-review queue: lesson nodes whose concept mastery interval has
/// elapsed. Per paper; the library-wide queue comes from `concept_search`
/// plus each paper's queue (all reading the same mastery events).
#[tauri::command]
fn review_due(
    state: State<AppState>,
    paper_id: String,
) -> Result<Vec<copilot_core::lessons::LessonEntry>, String> {
    let (_bundle, graph, _tree) = paper_graph(&state, &paper_id)?;
    let root = state.library.lock().unwrap().root().to_path_buf();
    let snapshot = copilot_core::learning::LearnerModel::open(&root)
        .snapshot()
        .map_err(ui_err)?;
    let node_globals: std::collections::HashMap<uuid::Uuid, uuid::Uuid> =
        copilot_core::concept_registry::ConceptRegistry::open(&root)
            .state()
            .map(|s| {
                graph
                    .nodes
                    .iter()
                    .filter_map(|n| s.global_for(&paper_id, n.id).map(|g| (n.id, g.id)))
                    .collect()
            })
            .unwrap_or_default();
    let due = |id: uuid::Uuid| {
        let by_global = node_globals.get(&id).and_then(|g| snapshot.mastery_of(*g));
        by_global
            .or_else(|| snapshot.mastery_of(id))
            .map(|m| m.due)
            .unwrap_or(false)
    };
    Ok(copilot_core::lessons::lesson_sequence(&graph, &|_| false)
        .into_iter()
        .filter(|entry| due(entry.node))
        .collect())
}

/// One Socratic tutor turn, streamed over `ai-stream` events (same envelope
/// as `ai_stream`: token/done/error/cancelled + egress host). The client
/// state machine picks `phase`; the model is prompted per phase and never
/// free-runs the loop. Turns persist to the node's chat journal with v1
/// honesty rules (partials kept, marked incomplete).
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn tutor_stream(
    app: AppHandle,
    state: State<AppState>,
    request_id: String,
    paper_id: String,
    node: uuid::Uuid,
    // phase: "ask" | "hint" | "correct";
    // attempt: the user's latest answer (None for the opening question).
    phase: String,
    attempt: Option<String>,
    hints_used: Option<u8>,
) -> Result<String, String> {
    let (bundle, graph, tree) = paper_graph(&state, &paper_id)?;
    let Some(concept) = graph.nodes.iter().find(|n| n.id == node) else {
        return Err("Concept not found in this paper.".to_string());
    };
    let excerpts: String = concept
        .object_ids
        .iter()
        .filter_map(|oid| tree.objects.iter().find(|o| o.id == *oid))
        .map(|o| {
            let text: String = o.content.text.chars().take(1200).collect();
            format!("[[object:{}]] {}\n", o.id, text)
        })
        .collect();

    let phase_contract = match phase.as_str() {
        "hint" => format!(
            "Judge the learner's latest attempt. If it is fully correct: confirm briefly, \
             reinforce the key idea, and end your reply with the exact token [CORRECT]. \
             If it is wrong or partial: give hint #{n} — narrow the gap WITHOUT revealing \
             the answer, address only what's missing, then re-ask concisely (no [CORRECT] token).",
            n = hints_used.unwrap_or(0) + 1
        ),
        "correct" => "Hints are exhausted or the learner asked for the answer. Give the \
                      correction now: the answer plus a concise explanation grounded in the \
                      excerpts. Never scold; continue supportively."
            .to_string(),
        _ => "Pose exactly ONE question testing understanding of this concept, grounded in \
              the excerpts (cite as [[object:ID]]). Then STOP and wait — do not answer it, \
              do not add hints."
            .to_string(),
    };
    let system = format!(
        "You are a Socratic tutor inside a research-paper reader, teaching \"{name}\". \
         Follow the phase contract exactly — the application controls the loop, you never \
         run ahead of it.\nPhase contract: {phase_contract}\n\nPaper excerpts:\n{excerpts}",
        name = concept.name,
    );

    // Resume the node's persisted conversation; append the attempt first so
    // a crash never loses the learner's answer.
    let history = copilot_core::chat::history(&bundle, node).map_err(ui_err)?;
    let mut messages = vec![copilot_core::ai::ChatMessage {
        images: Vec::new(),
        role: "system".to_string(),
        content: system,
    }];
    messages.extend(copilot_core::chat::as_thread(&history));
    if let Some(attempt) = &attempt {
        if !attempt.trim().is_empty() {
            copilot_core::chat::append(
                &bundle,
                node,
                &copilot_core::chat::user_message("tutor", attempt.clone()),
            )
            .map_err(ui_err)?;
            messages.push(copilot_core::ai::ChatMessage {
                images: Vec::new(),
                role: "user".to_string(),
                content: attempt.clone(),
            });
        }
    }
    if messages.last().map(|m| m.role.as_str()) != Some("user") {
        messages.push(copilot_core::ai::ChatMessage {
            images: Vec::new(),
            role: "user".to_string(),
            content: "(begin)".to_string(),
        });
    }

    let (provider, config) = pick_provider(&state.providers, copilot_core::ai::ModelClass::Strong)?;
    let emit = |event: AiStreamEvent| {
        let _ = app.emit("ai-stream", event);
    };
    emit(AiStreamEvent {
        host: Some(config.host()),
        ..AiStreamEvent::empty(&request_id)
    });
    let is_cancelled = || {
        state
            .cancelled_requests
            .lock()
            .unwrap()
            .contains(&request_id)
    };
    let mut accumulated = String::new();
    let result = provider.stream_chat_cancellable(
        &messages,
        &mut |token| {
            accumulated.push_str(token);
            emit(AiStreamEvent {
                token: Some(token.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
        },
        &is_cancelled,
    );
    state.cancelled_requests.lock().unwrap().remove(&request_id);
    match result {
        Ok(full) if !full.trim().is_empty() => {
            let turn = copilot_core::chat::assistant_message(full.clone(), false);
            copilot_core::chat::append(&bundle, node, &turn).map_err(ui_err)?;
            emit(AiStreamEvent {
                done: Some(true),
                ..AiStreamEvent::empty(&request_id)
            });
            Ok(full)
        }
        Ok(_) => {
            let message = "The model produced no text — try again.";
            emit(AiStreamEvent {
                error: Some(message.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
            Err(message.to_string())
        }
        Err(copilot_core::ai::AiError::Cancelled) => {
            if !accumulated.is_empty() {
                let turn = copilot_core::chat::assistant_message(accumulated.clone(), true);
                let _ = copilot_core::chat::append(&bundle, node, &turn);
            }
            emit(AiStreamEvent {
                cancelled: Some(true),
                ..AiStreamEvent::empty(&request_id)
            });
            Ok(accumulated)
        }
        Err(e) => {
            if !accumulated.is_empty() {
                let turn = copilot_core::chat::assistant_message(accumulated, true);
                let _ = copilot_core::chat::append(&bundle, node, &turn);
            }
            emit(AiStreamEvent {
                error: Some(e.to_string()),
                ..AiStreamEvent::empty(&request_id)
            });
            Err(ui_err(e))
        }
    }
}

/// Folded learner-model snapshot (mastery, preferences, episode count) —
/// the settings inspection surface and dashboard source.
#[tauri::command]
fn learning_snapshot(
    state: State<AppState>,
) -> Result<copilot_core::learning::LearnerSnapshot, String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    copilot_core::learning::LearnerModel::open(&root)
        .snapshot()
        .map_err(ui_err)
}

/// Delete learning data: one store ("mastery" | "preferences" | "episodes")
/// or everything when `store` is `None`. Touches nothing outside
/// `learning_state/` — papers, notes, and chats are unaffected.
#[tauri::command]
fn learning_reset(state: State<AppState>, store: Option<String>) -> Result<(), String> {
    let root = state.library.lock().unwrap().root().to_path_buf();
    let model = copilot_core::learning::LearnerModel::open(&root);
    match store.as_deref() {
        Some(name) => model.reset_store(name).map_err(ui_err),
        None => model.reset_all().map_err(ui_err),
    }
}

/// Cancel a running AI stream (cancel-anytime UX for slow reasoning models).
#[tauri::command]
fn ai_cancel(state: State<AppState>, request_id: String) {
    state.cancelled_requests.lock().unwrap().insert(request_id);
}

/// Full persisted conversation for an object (resume-on-open).
#[tauri::command]
fn chat_history(
    state: State<AppState>,
    paper_id: String,
    object_id: uuid::Uuid,
) -> Result<Vec<copilot_core::chat::StoredChatMessage>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::chat::history(&bundle, object_id).map_err(ui_err)
}

/// Edit any chat message (user or assistant) — append-only correction.
#[tauri::command]
fn chat_edit(
    state: State<AppState>,
    paper_id: String,
    object_id: uuid::Uuid,
    message_id: uuid::Uuid,
    content: String,
) -> Result<Vec<copilot_core::chat::StoredChatMessage>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::chat::edit_message(&bundle, object_id, message_id, content).map_err(ui_err)?;
    copilot_core::chat::history(&bundle, object_id).map_err(ui_err)
}

/// Remove a chat message from the conversation (append-only tombstone).
#[tauri::command]
fn chat_delete(
    state: State<AppState>,
    paper_id: String,
    object_id: uuid::Uuid,
    message_id: uuid::Uuid,
) -> Result<Vec<copilot_core::chat::StoredChatMessage>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::chat::delete_message(&bundle, object_id, message_id).map_err(ui_err)?;
    copilot_core::chat::history(&bundle, object_id).map_err(ui_err)
}

// ---- Provider configuration (presets, custom endpoints, tier mappings) ----

#[derive(serde::Serialize)]
struct ProviderConfigView {
    #[serde(flatten)]
    config: copilot_core::provider_config::ProviderConfig,
    has_key: bool,
    host: String,
    is_custom_url: bool,
    /// Preset defaults for revert-to-preset in the mapping editor.
    #[serde(skip_serializing_if = "Option::is_none")]
    preset_defaults: Option<copilot_core::provider_config::ProviderPreset>,
}

#[tauri::command]
fn provider_configs(state: State<AppState>) -> Vec<ProviderConfigView> {
    state
        .providers
        .load()
        .into_iter()
        .map(|config| ProviderConfigView {
            has_key: copilot_core::ai::load_key_for(&config.id)
                .ok()
                .flatten()
                .is_some(),
            host: config.host(),
            is_custom_url: config.is_custom_url(),
            preset_defaults: config
                .preset_id
                .as_deref()
                .and_then(copilot_core::provider_config::preset),
            config,
        })
        .collect()
}

#[tauri::command]
fn provider_presets() -> Vec<copilot_core::provider_config::ProviderPreset> {
    copilot_core::provider_config::presets()
}

/// Save a provider configuration. When a key is supplied it is validated
/// against the configured endpoint first — on failure nothing is saved
/// (no partial configuration). Returns a human-readable success summary.
#[tauri::command(async)]
fn save_provider_config(
    state: State<AppState>,
    config: copilot_core::provider_config::ProviderConfig,
    key: Option<String>,
) -> Result<String, String> {
    use copilot_core::ai::ModelClass;
    let summary = if let Some(key) = &key {
        let probe = copilot_core::ai::Provider::with_base_url(
            config.protocol,
            &config.model_for(ModelClass::Light),
            &config.base_url,
            Some(key.clone()),
        )
        .with_timeout(std::time::Duration::from_secs(20));
        let summary = probe.validate().map_err(ui_err)?;
        copilot_core::ai::store_key_for(&config.id, key).map_err(ui_err)?;
        summary
    } else {
        format!("saved — requests go to {}", config.host())
    };
    state.providers.upsert(config).map_err(ui_err)?;
    Ok(summary)
}

/// Remove a configured provider (preset/custom) and its keychain entry.
#[tauri::command]
fn remove_provider_config(state: State<AppState>, id: String) -> Result<(), String> {
    copilot_core::ai::delete_key_for(&id).map_err(ui_err)?;
    state.providers.remove(&id).map_err(ui_err)?;
    Ok(())
}

// ---- Telemetry: opt-in, content-free, local-only ----

#[tauri::command]
fn telemetry_record(state: State<AppState>, kind: String) {
    let _ = state.telemetry.record(&kind);
}

#[tauri::command]
fn telemetry_set_enabled(state: State<AppState>, enabled: bool) -> Result<(), String> {
    state.telemetry.set_enabled(enabled).map_err(ui_err)
}

#[tauri::command]
fn telemetry_summary(state: State<AppState>) -> copilot_core::telemetry::TelemetrySummary {
    state.telemetry.summary()
}

// ---- Annotations: notes, bookmarks, export ----

#[tauri::command]
fn notes_list(
    state: State<AppState>,
    paper_id: String,
) -> Result<Vec<copilot_core::annotations::Note>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::annotations::notes(&bundle).map_err(ui_err)
}

#[tauri::command]
fn note_save(
    state: State<AppState>,
    paper_id: String,
    note_id: uuid::Uuid,
    object_id: uuid::Uuid,
    anchor_hash: String,
    markdown: String,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    // Auto-link the note to the anchor object's graph concepts, so it
    // surfaces in graph/lesson views. No graph yet → unlinked, still saved.
    let concepts: Vec<uuid::Uuid> = bundle
        .read_derived_json::<copilot_core::concepts::KnowledgeGraph>("knowledge_graph.json")
        .ok()
        .flatten()
        .map(|g| {
            g.nodes
                .iter()
                .filter(|n| n.object_ids.contains(&object_id))
                .map(|n| n.id)
                .collect()
        })
        .unwrap_or_default();
    copilot_core::annotations::save_note(
        &bundle,
        note_id,
        object_id,
        &anchor_hash,
        &markdown,
        concepts,
    )
    .map_err(ui_err)
}

#[tauri::command]
fn note_delete(
    state: State<AppState>,
    paper_id: String,
    note_id: uuid::Uuid,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::annotations::delete_note(&bundle, note_id).map_err(ui_err)
}

#[tauri::command]
fn bookmarks_list(
    state: State<AppState>,
    paper_id: String,
) -> Result<Vec<copilot_core::annotations::Bookmark>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::annotations::bookmarks(&bundle).map_err(ui_err)
}

#[tauri::command]
fn bookmark_toggle(
    state: State<AppState>,
    paper_id: String,
    object_id: uuid::Uuid,
    anchor_hash: String,
) -> Result<bool, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::annotations::toggle_bookmark(&bundle, object_id, &anchor_hash).map_err(ui_err)
}

#[tauri::command]
fn ink_list(
    state: State<AppState>,
    paper_id: String,
) -> Result<Vec<copilot_core::annotations::InkStroke>, String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::annotations::ink_strokes(&bundle).map_err(ui_err)
}

#[tauri::command]
fn ink_add(
    state: State<AppState>,
    paper_id: String,
    stroke: copilot_core::annotations::InkStroke,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::annotations::ink_add(&bundle, stroke).map_err(ui_err)
}

#[tauri::command]
fn ink_delete(
    state: State<AppState>,
    paper_id: String,
    stroke_id: uuid::Uuid,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    copilot_core::annotations::ink_delete(&bundle, stroke_id).map_err(ui_err)
}

/// Export notes + bookmarks as Markdown; written to the chosen path.
#[tauri::command]
fn export_annotations(
    state: State<AppState>,
    paper_id: String,
    dest_path: String,
) -> Result<(), String> {
    let bundle = state
        .library
        .lock()
        .unwrap()
        .get(&paper_id)
        .map_err(ui_err)?;
    let markdown = copilot_core::annotations::export_markdown(&bundle).map_err(ui_err)?;
    std::fs::write(&dest_path, markdown).map_err(ui_err)
}

/// Persist per-paper reading state (position, panels) into the bundle.
#[tauri::command]
fn save_reading_state(
    state: State<AppState>,
    id: String,
    reading_state: serde_json::Value,
) -> Result<(), String> {
    let bundle = state.library.lock().unwrap().get(&id).map_err(ui_err)?;
    bundle
        .write_user_json("reading_state.json", &reading_state)
        .map_err(ui_err)
}

/// Reveal the bundle directory in the OS file manager.
#[tauri::command]
fn reveal_paper(state: State<AppState>, id: String) -> Result<(), String> {
    let path = state.library.lock().unwrap().bundle_path(&id);
    if !path.is_dir() {
        return Err(format!("paper not found: {id}"));
    }
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(&path).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer").arg(&path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(&path).spawn();
    result.map(|_| ()).map_err(ui_err)
}

#[derive(Clone, serde::Serialize)]
struct IngestionProgress {
    paper_id: String,
    event: ProgressEvent,
}

/// Pipeline options with concept extraction backed by the light-tier
/// provider when one is configured; picked lazily at stage time so a key
/// added mid-session applies without restart. No provider → `None` inside
/// the closure → the stage degrades to the heuristic graph.
fn pipeline_options(state: &AppState) -> PipelineOptions {
    let store = state.providers.clone();
    PipelineOptions {
        skip_embeddings: false,
        concepts_llm: Some(std::sync::Arc::new(move |prompt: &str| {
            let (provider, _config) =
                pick_provider(&store, copilot_core::ai::ModelClass::Light).ok()?;
            let messages = [copilot_core::ai::ChatMessage {
                images: Vec::new(),
                role: "user".to_string(),
                content: prompt.to_string(),
            }];
            provider.stream_chat(&messages, &mut |_| {}).ok()
        })),
    }
}

fn spawn_ingestion(app: AppHandle, paper_id: String, bundle_root: std::path::PathBuf) {
    let options = pipeline_options(&app.state::<AppState>());
    let (_handle, rx) = copilot_core::pipeline::spawn(bundle_root.clone(), options);
    std::thread::spawn(move || {
        for event in rx {
            let finished = matches!(event, ProgressEvent::PipelineFinished { .. });
            let _ = app.emit(
                "ingestion-progress",
                IngestionProgress {
                    paper_id: paper_id.clone(),
                    event,
                },
            );
            if finished {
                update_graph_index(&app, &paper_id, &bundle_root);
            }
        }
    });
}

/// Mirror the bundle's concept graph into the library-level SQLite index
/// (`graph.db`). Cache-class: any failure is silently skipped — the index is
/// rebuildable and never a source of truth.
fn update_graph_index(app: &AppHandle, paper_id: &str, bundle_root: &std::path::Path) {
    let root = {
        let state = app.state::<AppState>();
        let library = state.library.lock().unwrap();
        library.root().to_path_buf()
    };
    let Ok(bundle) = copilot_core::bundle::Bundle::open(bundle_root) else {
        return;
    };
    let Ok(Some(graph)) =
        bundle.read_derived_json::<copilot_core::concepts::KnowledgeGraph>("knowledge_graph.json")
    else {
        return;
    };
    if let Ok(mut index) = copilot_core::graph_index::GraphIndex::open(&root) {
        let _ = index.index_paper(paper_id, &graph);
    }
    // Cross-paper identity: conservatively link this paper's concepts into
    // the global registry (name match; embedding-tightened when the local
    // model is already loaded — never load it just for this).
    let registry = copilot_core::concept_registry::ConceptRegistry::open(&root);
    let state = app.state::<AppState>();
    let embedder_guard = state.embedder.lock().unwrap();
    let embed = embedder_guard.as_ref().map(|embedder| {
        move |name: &str| {
            embedder
                .embed(&[name])
                .ok()
                .and_then(|mut vectors| (!vectors.is_empty()).then(|| vectors.remove(0)))
        }
    });
    let _ = registry.auto_link(
        paper_id,
        &graph,
        embed
            .as_ref()
            .map(|f| f as &dyn Fn(&str) -> Option<Vec<f32>>),
    );
}

/// First-run experience: an empty library gets the bundled, pre-enriched
/// sample paper so a new user reaches a working object interaction with no
/// key and no network. Never overwrites anything.
fn install_sample_paper(app: &AppHandle, library: &Library) {
    let is_empty = library.list().map(|l| l.is_empty()).unwrap_or(false);
    if !is_empty {
        return;
    }
    // Bundled resource in production; repo path during development.
    let candidates = [
        app.path()
            .resolve("resources/sample", tauri::path::BaseDirectory::Resource)
            .ok(),
        Some(std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/sample"
        ))),
    ];
    let Some(sample_dir) = candidates.into_iter().flatten().find(|p| p.is_dir()) else {
        return; // no sample shipped in this build — not an error
    };
    let Ok(entries) = std::fs::read_dir(&sample_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let src = entry.path();
        if src.extension().and_then(|e| e.to_str()) != Some("research") {
            continue;
        }
        let dest = library.root().join(entry.file_name());
        if dest.exists() {
            continue;
        }
        if let Err(e) = copy_dir(&src, &dest) {
            eprintln!("sample install failed: {e}");
            let _ = std::fs::remove_dir_all(&dest);
        }
    }
}

fn copy_dir(src: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Release bundles ship PDFium in resources/pdfium (the release
            // workflow copies it in); point the core there unless the
            // developer already set PDFIUM_LIB_DIR. Dev builds keep the
            // vendor/ and system fallbacks.
            if std::env::var("PDFIUM_LIB_DIR").is_err() {
                if let Ok(dir) = app
                    .path()
                    .resolve("resources/pdfium", tauri::path::BaseDirectory::Resource)
                {
                    if dir.exists() {
                        std::env::set_var("PDFIUM_LIB_DIR", dir.to_string_lossy().to_string());
                    }
                }
            }
            let library_root = app
                .path()
                .app_data_dir()
                .expect("no app data dir")
                .join("library");
            let library = Library::open(&library_root)?;
            install_sample_paper(app.handle(), &library);
            let telemetry = copilot_core::telemetry::Telemetry::open(
                &app.path().app_data_dir().expect("no app data dir"),
            )?;
            let _ = telemetry.record("first_launch");
            let _ = telemetry.record("session_start");
            let providers = copilot_core::provider_config::ProviderStore::new(
                &app.path().app_config_dir().expect("no app config dir"),
            )?;
            let workspace = copilot_core::workspace::WorkspaceStore::open(&library_root)
                .map_err(|e| e.to_string());
            app.manage(AppState {
                library: Mutex::new(library),
                workspace: Mutex::new(workspace),
                embedder: Mutex::new(None),
                telemetry,
                providers,
                cancelled_requests: Mutex::new(std::collections::HashSet::new()),
            });
            // Background sync on app open (no-op when sync isn't configured;
            // never blocks startup — it's a worker thread behind sync_now).
            if load_sync_config(&library_root).is_some() {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    let state = handle.state::<AppState>();
                    let _ = sync_now(handle.clone(), state);
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                let state: State<AppState> = window.state();
                let _ = state.telemetry.record("session_end");
            }
        })
        .invoke_handler(tauri::generate_handler![
            core_version,
            open_devtools,
            list_papers,
            search_paper,
            ai_provider_statuses,
            ai_set_key,
            ai_delete_key,
            ai_stream,
            active_model,
            load_attachment,
            ai_cancel,
            provider_configs,
            provider_presets,
            save_provider_config,
            remove_provider_config,
            chat_history,
            read_pregenerated,
            notes_list,
            note_save,
            note_delete,
            bookmarks_list,
            bookmark_toggle,
            ink_list,
            ink_add,
            ink_delete,
            export_annotations,
            telemetry_record,
            telemetry_set_enabled,
            telemetry_summary,
            save_reading_state,
            read_original_pdf,
            read_artifact,
            import_pdf_file,
            import_url,
            paper_markdown,
            paper_markdown_clean,
            pipeline_status,
            retry_ingestion,
            workspace_items_list,
            workspace_item_rename,
            workspace_item_delete,
            workspace_refs_to_paper,
            workspace_export_all,
            workspace_note_create,
            workspace_note_get,
            workspace_note_save,
            workspace_note_refs_sync,
            note_ai,
            paper_figure_data_url,
            workspace_canvas_create,
            workspace_canvas_get,
            workspace_canvas_save,
            workspace_canvas_refs_sync,
            canvas_ai_edit,
            fetch_url_context,
            extract_pdf_text,
            workspace_chat_create,
            workspace_chat_messages,
            workspace_chat_edit_message,
            workspace_chat_delete_message,
            workspace_chat_refs_sync,
            chat_stream,
            delete_paper,
            paper_toggle_star,
            paper_set_priority,
            canvas_get,
            canvas_save,
            graph_get,
            graph_override,
            bundle_validate,
            import_latex,
            capability_matrix,
            plugin_list,
            plugin_set_consent,
            plugin_run,
            plugin_export_to_dir,
            contribution_identity_set,
            contribution_propose,
            contribution_overview,
            contribution_diff,
            contribution_review,
            contribution_revert,
            registry_list,
            registry_add,
            registry_remove,
            registry_check,
            registry_pull,
            registry_preview,
            registry_publish,
            workspace_create,
            workspace_list,
            workspace_configure,
            workspace_join,
            workspace_sync,
            workspace_share_paper,
            workspace_members,
            workspace_thread,
            workspace_thread_post,
            workspace_assign,
            workspace_assignments,
            workspace_progress,
            workspace_cohort,
            workspace_whoami,
            learning_snapshot,
            learning_reset,
            concept_search,
            concept_occurrences,
            object_seen_elsewhere,
            lessons_sequence,
            lesson_get_or_generate,
            quiz_get_or_generate,
            flashcards_get_or_generate,
            learning_record,
            review_due,
            tutor_stream,
            chat_edit,
            chat_delete,
            sync_status,
            sync_configure,
            sync_disable,
            sync_now,
            sync_clean_remote,
            review_list,
            review_create,
            review_get,
            review_save_document,
            review_regenerate,
            gaps_generate,
            gaps_latest,
            extension_state,
            extension_weaknesses,
            extension_generate_cards,
            extension_card_edit,
            extension_card_archive,
            extension_novelty,
            extension_card_experiment,
            extension_draft,
            extension_save_document,
            extension_export,
            repos_cache_usage,
            repos_cache_clear,
            repro_state,
            repro_set_repo,
            repro_configure_run,
            repro_advance,
            repro_artifacts,
            repro_list_files,
            repro_read_file,
            experiment_create,
            experiment_list,
            experiment_runs,
            experiment_run,
            experiment_stream,
            implementation_get,
            implementation_generate,
            implementation_save_edit,
            implementation_run,
            sandbox_runtime_status,
            sandbox_consents,
            sandbox_grant,
            sandbox_grant_network,
            sandbox_revoke,
            sandbox_kill,
            concept_registry_record,
            paper_links,
            paper_link_add,
            open_paper,
            reveal_paper
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
