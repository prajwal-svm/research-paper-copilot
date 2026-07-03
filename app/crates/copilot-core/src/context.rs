//! Per-action model routing and token-budgeted context assembly.
//!
//! Prompts are built from the anchored object's extracted content, its
//! relationships (the equation a figure depends on, the section it belongs
//! to), and the object's own conversation history — never naive
//! whole-document chunk retrieval. Assembly enforces a fixed token budget:
//! relationship context is trimmed first, then older thread messages.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ai::{ChatMessage, ModelClass};
use crate::objects::{Object, ObjectType, SemanticTreeDocument};

/// AI actions the app offers; each routes to a cost/latency class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Explain,
    Ask,
    // Equation
    VariableBreakdown,
    StepByStep,
    Intuition,
    // Equation/figure deep-dive tabs (v2)
    Derivation,
    Assumptions,
    Prerequisites,
    CommonMistakes,
    // Figure
    FigureDescribe,
    FigureInterpret,
    // Table
    TableSummarize,
    TableQuery,
    // Lightweight surfaces
    CitationCard,
    HoverSummary,
}

impl Action {
    /// Routing: cheap surfaces go to the light class, everything that
    /// explains or derives goes to the strong class.
    pub fn model_class(self) -> ModelClass {
        match self {
            Action::CitationCard | Action::HoverSummary | Action::TableSummarize => {
                ModelClass::Light
            }
            _ => ModelClass::Strong,
        }
    }

    fn instruction(self) -> &'static str {
        match self {
            Action::Explain => "Explain this to the reader clearly and concretely, in the context of this paper.",
            Action::Ask => "Answer the reader's question about this object, grounded in the provided context.",
            Action::VariableBreakdown => "List every symbol/variable in this equation. For each: its name and its meaning in this paper's context.",
            Action::StepByStep => "Walk through this equation step by step: what each operation does and why, in order.",
            Action::Intuition => "Give the plain-language intuition for this equation: what it means and why it makes sense. Avoid heavy notation.",
            Action::Derivation => "Derive this result from first principles, step by step: where does it come from, and what justifies each step? Keep every step small.",
            Action::Assumptions => "List the assumptions this relies on — explicit and implicit. For each: what breaks if it doesn't hold?",
            Action::Prerequisites => "List the concepts the reader must already understand for this to make sense, ordered from most to least fundamental, each with a one-sentence refresher.",
            Action::CommonMistakes => "List the common mistakes and misconceptions people have about this, and for each: why it's wrong and the correct mental model.",
            Action::FigureDescribe => "Describe this figure visually: what each axis, element, and panel shows.",
            Action::FigureInterpret => "Interpret this figure: what should the reader conclude from it, and what supports that conclusion?",
            Action::TableSummarize => "Summarize what this table shows and the key takeaway.",
            Action::TableQuery => "Answer the reader's question strictly from the table data provided. Cite the specific rows/cells you used. If the data cannot answer it, say so.",
            Action::CitationCard => "In 2-3 sentences: what is the cited work about, and why is it cited at this location in this paper?",
            Action::HoverSummary => "Summarize this in one short sentence.",
        }
    }
}

/// Approximate tokens (chars/4 heuristic — deliberately conservative).
pub fn approx_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssembledContext {
    pub messages: Vec<ChatMessage>,
    pub model_class: ModelClass,
    /// Approximate prompt tokens after budgeting.
    pub approx_tokens: usize,
    /// True when relationship/thread content was dropped to fit the budget.
    pub trimmed: bool,
}

const SYSTEM_PREAMBLE: &str = "You are a research-paper reading companion. Ground every answer in the provided paper context. \
When you reference a part of the paper that is listed in the context with an id, cite it inline as [[object:ID]] so the app can link it. \
Be precise; if the context is insufficient, say what is missing instead of guessing.";

/// Assemble messages for an action anchored to an ad-hoc selection (text
/// drag or region marquee) rather than an extracted object. The selection's
/// gathered text is the anchor; thread budgeting matches [`assemble`].
pub fn assemble_adhoc(
    paper_title: &str,
    selection_text: &str,
    action: Action,
    question: Option<&str>,
    thread: &[ChatMessage],
    budget_tokens: usize,
) -> AssembledContext {
    let mut trimmed = false;
    let anchor = format!(
        "Paper: {paper_title}\n\nReader-selected passage/region (ad-hoc selection):\n{content}\n",
        content = clip(selection_text, 6000),
    );

    let system_and_anchor = approx_tokens(SYSTEM_PREAMBLE) + approx_tokens(&anchor) + 200;
    let mut remaining = budget_tokens.saturating_sub(system_and_anchor);
    let mut kept_thread: Vec<ChatMessage> = Vec::new();
    for message in thread.iter().rev() {
        let cost = approx_tokens(&message.content) + 8;
        if cost > remaining {
            trimmed = true;
            break;
        }
        remaining -= cost;
        kept_thread.push(message.clone());
    }
    kept_thread.reverse();

    let mut user_content = anchor;
    user_content.push_str(&format!("\nTask: {}\n", action.instruction()));
    if let Some(question) = question {
        user_content.push_str(&format!("Reader's question: {question}\n"));
    }

    let mut messages = vec![ChatMessage {
        role: "system".to_string(),
        content: SYSTEM_PREAMBLE.to_string(),
    }];
    messages.extend(kept_thread);
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_content,
    });
    let total = messages.iter().map(|m| approx_tokens(&m.content)).sum();
    AssembledContext {
        messages,
        model_class: action.model_class(),
        approx_tokens: total,
        trimmed,
    }
}

/// Assemble messages for an action anchored to an object.
///
/// `paper_title`: for the system frame. `thread`: prior conversation of this
/// object (oldest first). `question`: the user's free-form input for
/// Ask/TableQuery actions. `budget_tokens`: hard cap on the assembled prompt.
#[allow(clippy::too_many_arguments)]
pub fn assemble(
    tree: &SemanticTreeDocument,
    paper_title: &str,
    object_id: Uuid,
    action: Action,
    question: Option<&str>,
    thread: &[ChatMessage],
    table_data: Option<&serde_json::Value>,
    budget_tokens: usize,
) -> Option<AssembledContext> {
    let object = tree.objects.iter().find(|o| o.id == object_id)?;
    let mut trimmed = false;

    // --- Anchor block (never trimmed) ---
    let mut anchor = format!(
        "Paper: {paper_title}\n\nAnchored object [[object:{id}]] ({kind:?}{label}):\n{content}\n",
        id = object.id,
        kind = object.object_type,
        label = object
            .semantic_label
            .as_deref()
            .map(|l| format!(", {l}"))
            .unwrap_or_default(),
        content = clip(&object.content.text, 4000),
    );
    if let Some(latex) = &object.content.latex {
        anchor.push_str(&format!("LaTeX: {latex}\n"));
    }
    if let Some(data) = table_data {
        anchor.push_str(&format!(
            "Structured table data (authoritative):\n{}\n",
            clip(&data.to_string(), 6000)
        ));
    }

    // --- Relationship context, nearest first, budget-permitting ---
    let system_and_anchor = approx_tokens(SYSTEM_PREAMBLE) + approx_tokens(&anchor) + 200;
    let mut remaining = budget_tokens.saturating_sub(system_and_anchor);

    let mut related_block = String::new();
    for relationship in object.relationships.iter() {
        let Some(target) = tree.objects.iter().find(|o| o.id == relationship.target) else {
            continue;
        };
        let entry = format!(
            "- {rel:?} [[object:{id}]] ({kind:?}{label}): {text}\n",
            rel = relationship.relationship_type,
            id = target.id,
            kind = target.object_type,
            label = target
                .semantic_label
                .as_deref()
                .map(|l| format!(", {l}"))
                .unwrap_or_default(),
            text = clip(&target.content.text, 600),
        );
        let cost = approx_tokens(&entry);
        if cost > remaining {
            trimmed = true;
            continue; // a smaller later entry may still fit
        }
        remaining -= cost;
        related_block.push_str(&entry);
    }

    // --- Thread history, newest kept, oldest dropped ---
    let mut kept_thread: Vec<ChatMessage> = Vec::new();
    for message in thread.iter().rev() {
        let cost = approx_tokens(&message.content) + 8;
        if cost > remaining {
            trimmed = true;
            break;
        }
        remaining -= cost;
        kept_thread.push(message.clone());
    }
    kept_thread.reverse();

    // --- Compose ---
    let mut user_content = anchor;
    if !related_block.is_empty() {
        user_content.push_str("\nRelated objects:\n");
        user_content.push_str(&related_block);
    }
    user_content.push_str(&format!("\nTask: {}\n", action.instruction()));
    if let Some(question) = question {
        user_content.push_str(&format!("Reader's question: {question}\n"));
    }

    let mut messages = vec![ChatMessage {
        role: "system".to_string(),
        content: SYSTEM_PREAMBLE.to_string(),
    }];
    messages.extend(kept_thread);
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_content,
    });

    let total = messages.iter().map(|m| approx_tokens(&m.content)).sum();
    Some(AssembledContext {
        messages,
        model_class: action.model_class(),
        approx_tokens: total,
        trimmed,
    })
}

// ---------------------------------------------------------------------------
// v2: entity linking + graph-first assembly (contextual-chat delta)
// ---------------------------------------------------------------------------

/// Link a free-form query to graph nodes by normalized name match, ranked
/// longest-name first (most specific concept wins). Budget: <50 ms locally
/// (perf suite) — pure string work over an in-memory graph. Embedding-based
/// linking arrives with the cross-paper registry; name match already covers
/// the anchored-chat path because anchors resolve via object ids first.
pub fn link_query(graph: &crate::concepts::KnowledgeGraph, query: &str) -> Vec<Uuid> {
    let normalized_query = normalize(query);
    let mut matches: Vec<(usize, Uuid)> = graph
        .nodes
        .iter()
        .filter_map(|node| {
            let name = normalize(&node.name);
            (!name.is_empty() && normalized_query.contains(&name)).then_some((name.len(), node.id))
        })
        .collect();
    matches.sort_by_key(|m| std::cmp::Reverse(m.0));
    matches.into_iter().map(|(_, id)| id).collect()
}

fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Graph-side inputs for [`assemble_graph`]; gathered by the shell.
pub struct GraphInputs<'a> {
    pub graph: &'a crate::concepts::KnowledgeGraph,
    pub snapshot: &'a crate::learning::LearnerSnapshot,
    /// Episodic memories for the anchored object (oldest first).
    pub episodes: &'a [crate::learning::EpisodeEvent],
    /// Per-paper node id → library-global concept id (from the registry).
    /// Mastery is shared globally: mastering a concept in one paper counts
    /// in every paper where it appears.
    pub node_globals: Option<&'a std::collections::HashMap<Uuid, Uuid>>,
}

/// Graph-first context assembly: resolve the anchor to concept nodes, expand
/// a bounded neighborhood in priority order — unmastered prerequisites, then
/// definitions, then dependents — and attach episodic summaries plus the
/// compact learner profile. Mastered concepts enter as reference lines, not
/// re-explained content. Same budget mechanism and trimming order as v1
/// (graph block trims before thread history; anchor never trims).
///
/// Returns `None` when the anchor links to no graph node — the caller falls
/// back to [`assemble`] (v1 path), never a worse experience than v1.
#[allow(clippy::too_many_arguments)]
pub fn assemble_graph(
    tree: &SemanticTreeDocument,
    paper_title: &str,
    object_id: Uuid,
    action: Action,
    question: Option<&str>,
    thread: &[ChatMessage],
    table_data: Option<&serde_json::Value>,
    budget_tokens: usize,
    inputs: &GraphInputs,
) -> Option<AssembledContext> {
    use crate::concepts::EdgeKind;
    use crate::learning::MASTERED_SCORE;

    let object = tree.objects.iter().find(|o| o.id == object_id)?;
    let graph = inputs.graph;

    // --- Resolve anchor → nodes: object link first, query terms second ---
    let mut anchor_nodes: Vec<Uuid> = graph
        .nodes
        .iter()
        .filter(|n| n.object_ids.contains(&object_id))
        .map(|n| n.id)
        .collect();
    if anchor_nodes.is_empty() {
        if let Some(question) = question {
            anchor_nodes = link_query(graph, question);
        }
    }
    if anchor_nodes.is_empty() {
        anchor_nodes = link_query(graph, &object.content.text);
    }
    if anchor_nodes.is_empty() {
        return None; // no graph coverage for this anchor → v1 fallback
    }
    anchor_nodes.truncate(3);

    let node = |id: Uuid| graph.nodes.iter().find(|n| n.id == id);
    // Mastery lookup: global concept id first (shared across papers via the
    // registry), per-paper node id as fallback.
    let mastered = |id: Uuid| {
        let global = inputs.node_globals.and_then(|m| m.get(&id)).copied();
        global
            .and_then(|g| inputs.snapshot.mastery_of(g))
            .or_else(|| inputs.snapshot.mastery_of(id))
            .map(|m| !m.estimated && m.score >= MASTERED_SCORE)
            .unwrap_or(false)
    };

    // --- Expansion candidates in priority order ---
    // 1. unmastered prerequisites (X prerequisite_of anchor | anchor depends_on X)
    // 2. definitions (anchor defined_in X)
    // 3. dependents (anchor prerequisite_of Y | Y depends_on anchor)
    let mut ordered: Vec<Uuid> = Vec::new();
    let push = |id: Uuid, ordered: &mut Vec<Uuid>| {
        if !ordered.contains(&id) && !anchor_nodes.contains(&id) {
            ordered.push(id);
        }
    };
    for &anchor_id in &anchor_nodes {
        for edge in &graph.edges {
            let prerequisite = (edge.kind == EdgeKind::PrerequisiteOf && edge.to == anchor_id)
                .then_some(edge.from)
                .or_else(|| {
                    (edge.kind == EdgeKind::DependsOn && edge.from == anchor_id).then_some(edge.to)
                });
            if let Some(id) = prerequisite {
                if !mastered(id) {
                    push(id, &mut ordered);
                }
            }
        }
    }
    let unmastered_count = ordered.len();
    for &anchor_id in &anchor_nodes {
        for edge in &graph.edges {
            if edge.kind == EdgeKind::DefinedIn && edge.from == anchor_id {
                push(edge.to, &mut ordered);
            }
        }
    }
    for &anchor_id in &anchor_nodes {
        for edge in &graph.edges {
            let dependent = (edge.kind == EdgeKind::PrerequisiteOf && edge.from == anchor_id)
                .then_some(edge.to)
                .or_else(|| {
                    (edge.kind == EdgeKind::DependsOn && edge.to == anchor_id).then_some(edge.from)
                });
            if let Some(id) = dependent {
                push(id, &mut ordered);
            }
        }
    }
    // Bounded neighborhood: the priority ordering already put the most
    // valuable nodes first; beyond this the graph block stops adding signal.
    const MAX_EXPANDED_NODES: usize = 8;
    let unmastered_count = unmastered_count.min(MAX_EXPANDED_NODES);
    ordered.truncate(MAX_EXPANDED_NODES);

    // Mastered prerequisites still appear — as reference ids only.
    let mut mastered_refs: Vec<String> = Vec::new();
    for &anchor_id in &anchor_nodes {
        for edge in &graph.edges {
            let prerequisite = (edge.kind == EdgeKind::PrerequisiteOf && edge.to == anchor_id)
                .then_some(edge.from)
                .or_else(|| {
                    (edge.kind == EdgeKind::DependsOn && edge.from == anchor_id).then_some(edge.to)
                });
            if let Some(id) = prerequisite {
                if mastered(id) {
                    if let Some(n) = node(id) {
                        let line = format!("{} [[concept:{}]]", n.name, n.id);
                        if !mastered_refs.contains(&line) {
                            mastered_refs.push(line);
                        }
                    }
                }
            }
        }
    }

    // --- Anchor block (never trimmed) ---
    let anchor_names: Vec<String> = anchor_nodes
        .iter()
        .filter_map(|&id| node(id).map(|n| n.name.clone()))
        .collect();
    let mut anchor = format!(
        "Paper: {paper_title}\n\nAnchored object [[object:{id}]] ({kind:?}{label}; concept: {concepts}):\n{content}\n",
        id = object.id,
        kind = object.object_type,
        label = object
            .semantic_label
            .as_deref()
            .map(|l| format!(", {l}"))
            .unwrap_or_default(),
        concepts = anchor_names.join(", "),
        content = clip(&object.content.text, 4000),
    );
    if let Some(latex) = &object.content.latex {
        anchor.push_str(&format!("LaTeX: {latex}\n"));
    }
    if let Some(data) = table_data {
        anchor.push_str(&format!(
            "Structured table data (authoritative):\n{}\n",
            clip(&data.to_string(), 6000)
        ));
    }

    let system_and_anchor = approx_tokens(SYSTEM_PREAMBLE) + approx_tokens(&anchor) + 200;
    let mut remaining = budget_tokens.saturating_sub(system_and_anchor);
    let mut trimmed = false;

    // --- Graph block, priority order, budget-permitting ---
    let mut graph_block = String::new();
    for (i, id) in ordered.iter().enumerate() {
        let Some(n) = node(*id) else { continue };
        let is_prerequisite = i < unmastered_count;
        let role = if is_prerequisite {
            "prerequisite (unmastered)"
        } else {
            "related"
        };
        // Only unmastered prerequisites get a grounding excerpt from their
        // introducing object — the reader may need them re-taught. Dependents
        // and definitions are oriented by name + description alone.
        let excerpt = if is_prerequisite {
            n.object_ids
                .iter()
                .filter_map(|oid| tree.objects.iter().find(|o| o.id == *oid))
                .map(|o| clip(&o.content.text, 300))
                .next()
                .unwrap_or_default()
        } else {
            String::new()
        };
        // Prerequisites are linkable (lesson jump-off); related nodes only
        // orient the model, so they carry name + description, not ids.
        let entry = if is_prerequisite {
            format!(
                "- [{role}] {name} [[concept:{id}]]: {description}{excerpt}\n",
                name = n.name,
                description = n.description.as_deref().unwrap_or(""),
                excerpt = if excerpt.is_empty() {
                    String::new()
                } else {
                    format!(" — \"{excerpt}\"")
                },
            )
        } else {
            format!(
                "- [{role}] {name}{description}\n",
                name = n.name,
                description = n
                    .description
                    .as_deref()
                    .map(|d| format!(": {d}"))
                    .unwrap_or_default(),
            )
        };
        let cost = approx_tokens(&entry);
        if cost > remaining {
            trimmed = true;
            continue;
        }
        remaining -= cost;
        graph_block.push_str(&entry);
    }
    if !mastered_refs.is_empty() {
        let entry = format!(
            "Already mastered by the reader (reference, do not re-teach): {}\n",
            mastered_refs.join(", ")
        );
        let cost = approx_tokens(&entry);
        if cost <= remaining {
            remaining -= cost;
            graph_block.push_str(&entry);
        } else {
            trimmed = true;
        }
    }

    // --- Episodic memory (recent last-word) ---
    let mut episode_block = String::new();
    for episode in inputs.episodes.iter().rev().take(3) {
        let entry = format!("- ({}) {}\n", episode.kind, clip(&episode.summary, 300));
        let cost = approx_tokens(&entry);
        if cost > remaining {
            trimmed = true;
            break;
        }
        remaining -= cost;
        episode_block.push_str(&entry);
    }

    // --- Learner profile (compact ids/levels/style) ---
    let names: std::collections::HashMap<Uuid, String> =
        graph.nodes.iter().map(|n| (n.id, n.name.clone())).collect();
    let profile = crate::learning::profile_block(inputs.snapshot, &names).filter(|p| {
        let cost = approx_tokens(p);
        if cost <= remaining {
            remaining -= cost;
            true
        } else {
            trimmed = true;
            false
        }
    });

    // --- Thread history, newest kept ---
    let mut kept_thread: Vec<ChatMessage> = Vec::new();
    for message in thread.iter().rev() {
        let cost = approx_tokens(&message.content) + 8;
        if cost > remaining {
            trimmed = true;
            break;
        }
        remaining -= cost;
        kept_thread.push(message.clone());
    }
    kept_thread.reverse();

    // --- Compose ---
    let mut user_content = anchor;
    if !graph_block.is_empty() {
        user_content.push_str("\nConcept context (from the paper's knowledge graph):\n");
        user_content.push_str(&graph_block);
    }
    if !episode_block.is_empty() {
        user_content.push_str("\nReader's history with this part of the paper:\n");
        user_content.push_str(&episode_block);
    }
    if let Some(profile) = profile {
        user_content.push_str(&format!("\n{profile}\n"));
    }
    user_content.push_str(&format!("\nTask: {}\n", action.instruction()));
    if let Some(question) = question {
        user_content.push_str(&format!("Reader's question: {question}\n"));
    }

    let mut messages = vec![ChatMessage {
        role: "system".to_string(),
        content: SYSTEM_PREAMBLE.to_string(),
    }];
    messages.extend(kept_thread);
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_content,
    });
    let total = messages.iter().map(|m| approx_tokens(&m.content)).sum();
    Some(AssembledContext {
        messages,
        model_class: action.model_class(),
        approx_tokens: total,
        trimmed,
    })
}

// ---------------------------------------------------------------------------
// v3: execution artifacts as anchorable context (contextual-chat delta)
// ---------------------------------------------------------------------------

/// Assemble a discussion prompt anchored to an experiment: definition +
/// selected runs' parameters and metrics (+ the user's predictions where
/// recorded) — concrete numbers, never whole-repo or whole-paper dumps.
/// Newest runs survive trimming first; thread history trims after runs.
pub fn assemble_experiment(
    paper_title: &str,
    experiment: &crate::experiments::Experiment,
    runs: &[crate::experiments::ExperimentRun],
    question: Option<&str>,
    thread: &[ChatMessage],
    budget_tokens: usize,
) -> AssembledContext {
    let mut trimmed = false;
    let params: Vec<String> = experiment
        .parameters
        .iter()
        .map(|p| format!("{} ({}, default {})", p.name, p.kind, p.default))
        .collect();
    let anchor = format!(
        "Paper: {paper_title}\n\nExperiment \"{name}\" over the implementation of object \
         [[object:{object}]].\nTweakable parameters: {params}.\n",
        name = experiment.name,
        object = experiment.object_id,
        params = if params.is_empty() {
            "none".to_string()
        } else {
            params.join("; ")
        },
    );

    let system_and_anchor = approx_tokens(SYSTEM_PREAMBLE) + approx_tokens(&anchor) + 200;
    let mut remaining = budget_tokens.saturating_sub(system_and_anchor);

    // Runs, newest first into the budget (then re-ordered oldest→newest).
    let mut run_lines: Vec<String> = Vec::new();
    for run in runs.iter().rev() {
        let metrics: Vec<String> = run
            .metrics
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let params: Vec<String> = run.params.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let entry = format!(
            "- run {at} [{status}] params: {params} → metrics: {metrics}{prediction}\n",
            at = run.at,
            status = run.status,
            params = params.join(", "),
            metrics = if metrics.is_empty() {
                "(none captured)".to_string()
            } else {
                metrics.join(", ")
            },
            prediction = run
                .prediction
                .as_deref()
                .map(|p| format!(" | user predicted: \"{}\"", clip(p, 200)))
                .unwrap_or_default(),
        );
        let cost = approx_tokens(&entry);
        if cost > remaining {
            trimmed = true;
            break;
        }
        remaining -= cost;
        run_lines.push(entry);
    }
    run_lines.reverse();

    let mut kept_thread: Vec<ChatMessage> = Vec::new();
    for message in thread.iter().rev() {
        let cost = approx_tokens(&message.content) + 8;
        if cost > remaining {
            trimmed = true;
            break;
        }
        remaining -= cost;
        kept_thread.push(message.clone());
    }
    kept_thread.reverse();

    let mut user_content = anchor;
    if !run_lines.is_empty() {
        user_content.push_str("\nRecorded runs:\n");
        for line in &run_lines {
            user_content.push_str(line);
        }
    }
    user_content.push_str(
        "\nTask: Discuss these experimental results with the reader. Ground every claim in \
         the recorded numbers; when a prediction is present, compare it to what was observed.\n",
    );
    if let Some(question) = question {
        user_content.push_str(&format!("Reader's question: {question}\n"));
    }

    let mut messages = vec![ChatMessage {
        role: "system".to_string(),
        content: SYSTEM_PREAMBLE.to_string(),
    }];
    messages.extend(kept_thread);
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_content,
    });
    let total = messages.iter().map(|m| approx_tokens(&m.content)).sum();
    AssembledContext {
        messages,
        model_class: ModelClass::Strong,
        approx_tokens: total,
        trimmed,
    }
}

/// v4: a hypothesis card as an anchorable discussion context — the card's
/// fields plus its novelty verdict and evidence *titles* (never whole
/// papers), under the standard budget/trimming rules.
pub fn assemble_card(
    paper_title: &str,
    card: &crate::extension::HypothesisCard,
    question: Option<&str>,
    thread: &[ChatMessage],
    budget_tokens: usize,
) -> AssembledContext {
    let mut trimmed = false;
    let novelty = card
        .novelty
        .as_ref()
        .map(|n| {
            let evidence: Vec<String> = n
                .evidence
                .iter()
                .take(6)
                .map(|e| {
                    format!(
                        "{} ({}, sim {:.2})",
                        e.title,
                        e.year.map(|y| y.to_string()).unwrap_or_default(),
                        e.similarity
                    )
                })
                .collect();
            format!(
                "Novelty verdict: {:?}. Evidence: {}\n",
                n.verdict,
                evidence.join("; ")
            )
        })
        .unwrap_or_else(|| "Novelty: not yet checked.\n".to_string());
    let anchor = format!(
        "Paper: {paper_title}\n\nHypothesis card under discussion:\n\
         Claim: {claim}\nRationale: {rationale}\nRequired experiment: {experiment}\n\
         Expected evidence: {expected}\n{novelty}",
        claim = clip(&card.claim, 600),
        rationale = clip(&card.rationale, 800),
        experiment = clip(&card.required_experiment, 800),
        expected = clip(&card.expected_evidence, 600),
    );

    let system_and_anchor = approx_tokens(SYSTEM_PREAMBLE) + approx_tokens(&anchor) + 200;
    let mut remaining = budget_tokens.saturating_sub(system_and_anchor);
    let mut kept_thread: Vec<ChatMessage> = Vec::new();
    for message in thread.iter().rev() {
        let cost = approx_tokens(&message.content) + 8;
        if cost > remaining {
            trimmed = true;
            break;
        }
        remaining -= cost;
        kept_thread.push(message.clone());
    }
    kept_thread.reverse();

    let mut user_content = anchor;
    user_content.push_str(
        "\nTask: Discuss this hypothesis rigorously — strengths, objections, what evidence \
         would change your mind. Treat the novelty verdict as an estimate bounded by its evidence.\n",
    );
    if let Some(question) = question {
        user_content.push_str(&format!("Reader's question: {question}\n"));
    }
    let mut messages = vec![ChatMessage {
        role: "system".to_string(),
        content: SYSTEM_PREAMBLE.to_string(),
    }];
    messages.extend(kept_thread);
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_content,
    });
    let total = messages.iter().map(|m| approx_tokens(&m.content)).sum();
    AssembledContext {
        messages,
        model_class: ModelClass::Strong,
        approx_tokens: total,
        trimmed,
    }
}

fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let clipped: String = text.chars().take(max_chars).collect();
        format!("{clipped}…")
    }
}

/// Convenience: find an object and its thread-relevant metadata.
pub fn find_object(tree: &SemanticTreeDocument, object_id: Uuid) -> Option<&Object> {
    tree.objects.iter().find(|o| o.id == object_id)
}

/// Type-specific actions offered for an object type (drives the panel UI).
pub fn actions_for(object_type: ObjectType) -> Vec<Action> {
    let mut actions = vec![Action::Explain, Action::Ask];
    match object_type {
        ObjectType::Equation => actions.extend([
            Action::VariableBreakdown,
            Action::StepByStep,
            Action::Intuition,
            Action::Derivation,
            Action::Assumptions,
            Action::Prerequisites,
            Action::CommonMistakes,
        ]),
        ObjectType::Figure => actions.extend([
            Action::FigureDescribe,
            Action::FigureInterpret,
            Action::Assumptions,
            Action::Prerequisites,
            Action::CommonMistakes,
        ]),
        ObjectType::Table => actions.extend([Action::TableSummarize, Action::TableQuery]),
        _ => {}
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::BBox;
    use crate::objects::{Content, Relationship, RelationshipType, TreeNode};

    fn object(id: Uuid, object_type: ObjectType, text: &str, label: &str) -> Object {
        Object {
            id,
            object_type,
            regions: vec![BBox {
                page: 0,
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }],
            content: Content {
                text: text.to_string(),
                latex: None,
                caption: None,
            },
            semantic_label: Some(label.to_string()),
            relationships: Vec::new(),
            embedding: None,
            content_hash: crate::bundle::sha256_bytes(text.as_bytes()),
            confidence: 0.9,
        }
    }

    fn figure_with_equation() -> (SemanticTreeDocument, Uuid, Uuid) {
        let eq_id = Uuid::new_v4();
        let fig_id = Uuid::new_v4();
        let equation = object(
            eq_id,
            ObjectType::Equation,
            "Attention(Q,K,V) = softmax(QK^T/sqrt(dk))V",
            "Equation 12",
        );
        let mut figure = object(
            fig_id,
            ObjectType::Figure,
            "Figure 5: attention weights visualization.",
            "Figure 5",
        );
        figure.relationships.push(Relationship {
            relationship_type: RelationshipType::DependsOn,
            target: eq_id,
            confidence: None,
        });
        let tree = SemanticTreeDocument {
            pipeline_version: "0.1.0".to_string(),
            objects: vec![equation, figure],
            tree: vec![
                TreeNode {
                    object: eq_id,
                    children: Vec::new(),
                },
                TreeNode {
                    object: fig_id,
                    children: Vec::new(),
                },
            ],
        };
        (tree, fig_id, eq_id)
    }

    #[test]
    fn relationship_content_is_included() {
        let (tree, fig_id, eq_id) = figure_with_equation();
        let ctx = assemble(
            &tree,
            "Attention Is All You Need",
            fig_id,
            Action::Ask,
            Some("what equation does this depend on?"),
            &[],
            None,
            4000,
        )
        .unwrap();
        let user = &ctx.messages.last().unwrap().content;
        assert!(user.contains("softmax(QK^T"), "{user}");
        assert!(user.contains(&format!("[[object:{eq_id}]]")));
        assert!(!ctx.trimmed);
        assert_eq!(ctx.model_class, ModelClass::Strong);
    }

    #[test]
    fn budget_trims_relationships_then_thread() {
        let (tree, fig_id, _) = figure_with_equation();
        let thread: Vec<ChatMessage> = (0..30)
            .map(|i| ChatMessage {
                role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
                content: format!("message {i} {}", "x".repeat(400)),
            })
            .collect();

        // Generous budget: everything fits except maybe oldest messages.
        let generous = assemble(
            &tree,
            "T",
            fig_id,
            Action::Ask,
            Some("q"),
            &thread,
            None,
            100_000,
        )
        .unwrap();
        assert!(!generous.trimmed);
        assert_eq!(generous.messages.len(), 2 + thread.len());

        // Tight budget: thread shrinks, newest kept, cap respected.
        let tight = assemble(
            &tree,
            "T",
            fig_id,
            Action::Ask,
            Some("q"),
            &thread,
            None,
            800,
        )
        .unwrap();
        assert!(tight.trimmed);
        assert!(tight.approx_tokens <= 900, "{}", tight.approx_tokens);
        let kept: Vec<&String> = tight.messages.iter().map(|m| &m.content).collect();
        // Newest thread message survives; oldest doesn't.
        assert!(kept.iter().any(|c| c.contains("message 29")));
        assert!(!kept.iter().any(|c| c.contains("message 0 ")));
    }

    #[test]
    fn routing_and_type_actions() {
        assert_eq!(Action::CitationCard.model_class(), ModelClass::Light);
        assert_eq!(Action::StepByStep.model_class(), ModelClass::Strong);
        let equation_actions = actions_for(ObjectType::Equation);
        assert!(equation_actions.contains(&Action::VariableBreakdown));
        assert!(!actions_for(ObjectType::Paragraph).contains(&Action::TableQuery));
    }

    fn learner_graph() -> (
        SemanticTreeDocument,
        crate::concepts::KnowledgeGraph,
        Uuid, // anchored object
        Uuid, // anchor concept
        Uuid, // prerequisite concept
        Uuid, // dependent concept
    ) {
        use crate::concepts::{concept_id, ConceptEdge, ConceptNode, EdgeKind, KnowledgeGraph};
        let anchor_obj = Uuid::new_v4();
        let prereq_obj = Uuid::new_v4();
        let tree = SemanticTreeDocument {
            pipeline_version: "0.1.0".to_string(),
            objects: vec![
                object(
                    anchor_obj,
                    ObjectType::Section,
                    "Scaled dot-product attention computes softmax(QK^T/sqrt(dk))V.",
                    "3.2.1",
                ),
                object(
                    prereq_obj,
                    ObjectType::Paragraph,
                    "The softmax function normalizes scores into a distribution.",
                    "softmax",
                ),
            ],
            tree: Vec::new(),
        };
        let attention = concept_id("p", "Scaled Dot-Product Attention");
        let softmax = concept_id("p", "Softmax");
        let multihead = concept_id("p", "Multi-Head Attention");
        let graph = KnowledgeGraph {
            pipeline_version: "0.1.0".to_string(),
            extraction: "llm".to_string(),
            nodes: vec![
                ConceptNode {
                    id: attention,
                    name: "Scaled Dot-Product Attention".to_string(),
                    description: Some("Attention via scaled dot products.".to_string()),
                    object_ids: vec![anchor_obj],
                    confidence: 0.9,
                },
                ConceptNode {
                    id: softmax,
                    name: "Softmax".to_string(),
                    description: Some("Normalizes scores to probabilities.".to_string()),
                    object_ids: vec![prereq_obj],
                    confidence: 0.8,
                },
                ConceptNode {
                    id: multihead,
                    name: "Multi-Head Attention".to_string(),
                    description: Some("Parallel attention heads.".to_string()),
                    object_ids: vec![],
                    confidence: 0.8,
                },
            ],
            edges: vec![
                ConceptEdge {
                    from: softmax,
                    to: attention,
                    kind: EdgeKind::PrerequisiteOf,
                    confidence: 0.8,
                },
                ConceptEdge {
                    from: multihead,
                    to: attention,
                    kind: EdgeKind::DependsOn,
                    confidence: 0.8,
                },
            ],
        };
        (tree, graph, anchor_obj, attention, softmax, multihead)
    }

    #[test]
    fn link_query_matches_specific_names_first() {
        let (_, graph, _, attention, softmax, _) = learner_graph();
        let linked = link_query(
            &graph,
            "why is softmax used in scaled dot-product attention?",
        );
        assert_eq!(linked[0], attention, "longest (most specific) name first");
        assert!(linked.contains(&softmax));
        assert!(link_query(&graph, "unrelated question about databases").is_empty());
    }

    #[test]
    fn graph_assembly_prioritizes_unmastered_prerequisites() {
        let (tree, graph, anchor_obj, _, softmax, multihead) = learner_graph();
        let snapshot = crate::learning::LearnerSnapshot::default();
        let inputs = GraphInputs {
            graph: &graph,
            snapshot: &snapshot,
            episodes: &[],
            node_globals: None,
        };
        let ctx = assemble_graph(
            &tree,
            "T",
            anchor_obj,
            Action::Explain,
            None,
            &[],
            None,
            4000,
            &inputs,
        )
        .unwrap();
        let user = &ctx.messages.last().unwrap().content;
        assert!(
            user.contains("prerequisite (unmastered)") && user.contains("Softmax"),
            "{user}"
        );
        assert!(
            user.contains("normalizes scores into a distribution")
                || user.contains("Normalizes scores"),
            "prerequisite grounded with excerpt/description: {user}"
        );
        assert!(user.contains("Multi-Head Attention"), "dependent included");
        assert!(
            !user.contains("Related objects:"),
            "no v1 relationship dump"
        );
        let softmax_pos = user.find("Softmax").unwrap();
        let multihead_pos = user.find("Multi-Head Attention").unwrap();
        assert!(
            softmax_pos < multihead_pos,
            "prerequisites come before dependents"
        );
        let _ = (softmax, multihead);
    }

    #[test]
    fn mastered_prerequisite_becomes_reference_id() {
        let (tree, graph, anchor_obj, _, softmax, _) = learner_graph();
        let mut snapshot = crate::learning::LearnerSnapshot::default();
        snapshot.mastery.push(crate::learning::ConceptMastery {
            concept: softmax,
            score: 0.9,
            signals: 5,
            estimated: false,
            ease: 2.5,
            interval_days: 6.0,
            repetitions: 3,
            last_review: crate::bundle::now_rfc3339(),
            due: false,
            consecutive_failures: 0,
        });
        let inputs = GraphInputs {
            graph: &graph,
            snapshot: &snapshot,
            episodes: &[],
            node_globals: None,
        };
        let ctx = assemble_graph(
            &tree,
            "T",
            anchor_obj,
            Action::Explain,
            None,
            &[],
            None,
            4000,
            &inputs,
        )
        .unwrap();
        let user = &ctx.messages.last().unwrap().content;
        assert!(
            user.contains("do not re-teach") && user.contains("Softmax"),
            "{user}"
        );
        assert!(
            !user.contains("prerequisite (unmastered)"),
            "mastered prerequisite is not expanded: {user}"
        );
    }

    #[test]
    fn episodes_and_profile_attach_and_fallback_when_no_coverage() {
        let (tree, graph, anchor_obj, ..) = learner_graph();
        let snapshot = crate::learning::LearnerSnapshot::default();
        let episodes = vec![crate::learning::EpisodeEvent {
            paper_id: "p".to_string(),
            object: Some(anchor_obj),
            concept: None,
            kind: "confusion".to_string(),
            summary: "confused the scaling factor with a learned parameter".to_string(),
            covered_turns: Some(4),
            at: crate::bundle::now_rfc3339(),
        }];
        let inputs = GraphInputs {
            graph: &graph,
            snapshot: &snapshot,
            episodes: &episodes,
            node_globals: None,
        };
        let ctx = assemble_graph(
            &tree,
            "T",
            anchor_obj,
            Action::Explain,
            None,
            &[],
            None,
            4000,
            &inputs,
        )
        .unwrap();
        let user = &ctx.messages.last().unwrap().content;
        assert!(user.contains("confused the scaling factor"), "{user}");

        // Anchor with no graph coverage → None → caller falls back to v1.
        let stray = Uuid::new_v4();
        let mut tree2 = tree.clone();
        tree2.objects.push(object(
            stray,
            ObjectType::Paragraph,
            "completely unrelated text with no concept overlap whatsoever",
            "stray",
        ));
        assert!(assemble_graph(
            &tree2,
            "T",
            stray,
            Action::Explain,
            None,
            &[],
            None,
            4000,
            &inputs,
        )
        .is_none());
    }

    #[test]
    fn graph_assembly_respects_budget_and_trim_order() {
        let (tree, graph, anchor_obj, ..) = learner_graph();
        let snapshot = crate::learning::LearnerSnapshot::default();
        let inputs = GraphInputs {
            graph: &graph,
            snapshot: &snapshot,
            episodes: &[],
            node_globals: None,
        };
        let thread: Vec<ChatMessage> = (0..20)
            .map(|i| ChatMessage {
                role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
                content: format!("message {i} {}", "x".repeat(400)),
            })
            .collect();
        let tight = assemble_graph(
            &tree,
            "T",
            anchor_obj,
            Action::Ask,
            Some("q"),
            &thread,
            None,
            700,
            &inputs,
        )
        .unwrap();
        assert!(tight.trimmed);
        assert!(tight.approx_tokens <= 800, "{}", tight.approx_tokens);
        let joined: String = tight.messages.iter().map(|m| m.content.as_str()).collect();
        assert!(
            joined.contains("softmax(QK^T"),
            "anchor never trimmed: {joined}"
        );
    }

    #[test]
    fn experiment_context_includes_numbers_and_trims_old_runs() {
        use crate::experiments::{Experiment, ExperimentRun, ParameterSpec};
        use std::collections::BTreeMap;
        let experiment = Experiment {
            id: Uuid::new_v4(),
            name: "LR sweep".to_string(),
            object_id: Uuid::new_v4(),
            language: crate::implementations::Language::Python,
            parameters: vec![ParameterSpec {
                name: "learning_rate".into(),
                kind: "number".into(),
                default: "0.01".into(),
            }],
            created_at: crate::bundle::now_rfc3339(),
        };
        let run = |i: usize, loss: f64| ExperimentRun {
            run_id: Uuid::new_v4(),
            params: BTreeMap::from([("learning_rate".to_string(), format!("0.{i}"))]),
            metrics: BTreeMap::from([("loss".to_string(), loss)]),
            stdout_tail: String::new(),
            duration_ms: 5,
            status: "completed".to_string(),
            prediction: (i == 9).then(|| "loss will diverge".to_string()),
            run_by: None,
            at: format!("2026-07-02T00:00:0{i}Z"),
        };
        let runs: Vec<ExperimentRun> = (0..10).map(|i| run(i, 2.0 - i as f64 * 0.1)).collect();

        let ctx = assemble_experiment(
            "T",
            &experiment,
            &runs,
            Some("why did the last run improve?"),
            &[],
            100_000,
        );
        let user = &ctx.messages.last().unwrap().content;
        assert!(
            user.contains("loss=1.1"),
            "concrete numbers present: {user}"
        );
        assert!(user.contains("user predicted"), "prediction rides along");
        assert!(!ctx.trimmed);

        // Tight budget: newest runs survive, oldest trimmed, anchor intact.
        let tight = assemble_experiment("T", &experiment, &runs, None, &[], 450);
        assert!(tight.trimmed);
        let user = &tight.messages.last().unwrap().content;
        assert!(user.contains("LR sweep"), "anchor never trimmed");
        assert!(user.contains("00:09"), "newest run kept: {user}");
        assert!(!user.contains("00:00Z"), "oldest run trimmed: {user}");
    }

    #[test]
    fn table_data_is_authoritative_context() {
        let table_id = Uuid::new_v4();
        let tree = SemanticTreeDocument {
            pipeline_version: "0.1.0".to_string(),
            objects: vec![object(
                table_id,
                ObjectType::Table,
                "Table 2: BLEU scores.",
                "Table 2",
            )],
            tree: vec![TreeNode {
                object: table_id,
                children: Vec::new(),
            }],
        };
        let data = serde_json::json!({
            "columns": ["Model", "BLEU EN-DE"],
            "rows": [["ByteNet", 23.75], ["Transformer (big)", 28.4]]
        });
        let ctx = assemble(
            &tree,
            "T",
            table_id,
            Action::TableQuery,
            Some("which row is best on BLEU EN-DE?"),
            &[],
            Some(&data),
            4000,
        )
        .unwrap();
        let user = &ctx.messages.last().unwrap().content;
        assert!(user.contains("Transformer (big)"));
        assert!(user.contains("strictly from the table data"));
    }
}
