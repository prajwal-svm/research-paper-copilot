//! Extension mode (v4): weaknesses → hypotheses → novelty → related work →
//! outline → draft, as a staged, resumable, edit-tolerant pipeline under the
//! bundle's `research/` area.
//!
//! Integrity rules (the point of this module):
//! - every weakness must cite a real paper object or it is dropped at parse,
//! - hypothesis cards are append-only user data that upstream regeneration
//!   can flag but never destroy,
//! - drafts cite only from a pre-assembled bibliography; unknown keys are
//!   stripped and counted, and BibTeX comes from resolved metadata only,
//! - AI-drafted text carries provenance into the LaTeX source.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bundle::Bundle;
use crate::citations::CitationsDocument;
use crate::novelty::NoveltyResult;
use crate::objects::{ObjectType, SemanticTreeDocument};

pub const RESEARCH_DIR: &str = "research";

#[derive(Debug, thiserror::Error)]
pub enum ExtensionError {
    #[error(transparent)]
    Bundle(#[from] crate::bundle::BundleError),
    #[error("research: {0}")]
    Io(#[from] std::io::Error),
}

fn dir(bundle: &Bundle) -> PathBuf {
    bundle.root().join(RESEARCH_DIR)
}

// ---------------------------------------------------------------------------
// Weaknesses (derived, regenerable, object-grounded)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weakness {
    pub id: Uuid,
    /// "assumption" | "limitation" | "future_work" | "methodological"
    pub kind: String,
    pub summary: String,
    /// Non-empty by construction: parse rejects uncited weaknesses.
    pub object_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeaknessDoc {
    /// Revision tag — cards remember which revision they were built from,
    /// so regeneration flags them instead of touching them.
    pub generated_at: String,
    pub weaknesses: Vec<Weakness>,
}

pub fn weaknesses(bundle: &Bundle) -> Result<Option<WeaknessDoc>, ExtensionError> {
    Ok(std::fs::read(dir(bundle).join("weaknesses.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok()))
}

/// Sections likely to carry assumptions/limitations/future work, plus
/// equations (their stated conditions are assumptions).
fn weakness_candidates(tree: &SemanticTreeDocument) -> Vec<&crate::objects::Object> {
    let heading_hit = |label: &str, text: &str| {
        let hay =
            format!("{} {}", label, text.chars().take(120).collect::<String>()).to_lowercase();
        [
            "limitation",
            "future work",
            "discussion",
            "conclusion",
            "assumption",
            "threats to",
        ]
        .iter()
        .any(|k| hay.contains(k))
    };
    let body_hit = |text: &str| {
        let hay = text.to_lowercase();
        [
            "we assume",
            "limitation",
            "future work",
            "left for future",
            "does not handle",
            "beyond the scope",
        ]
        .iter()
        .any(|k| hay.contains(k))
    };
    tree.objects
        .iter()
        .filter(|o| match o.object_type {
            ObjectType::Section => {
                heading_hit(o.semantic_label.as_deref().unwrap_or(""), &o.content.text)
                    || body_hit(&o.content.text)
            }
            ObjectType::Paragraph => body_hit(&o.content.text),
            ObjectType::Equation => true,
            _ => false,
        })
        .take(24)
        .collect()
}

pub fn weakness_prompt(tree: &SemanticTreeDocument, paper_title: &str) -> String {
    let excerpts: String = weakness_candidates(tree)
        .iter()
        .map(|o| {
            format!(
                "[[object:{id}]] ({label}): {text}\n",
                id = o.id,
                label = o.semantic_label.as_deref().unwrap_or("passage"),
                text = o.content.text.chars().take(900).collect::<String>(),
            )
        })
        .collect();
    format!(
        "Identify concrete weaknesses of the paper \"{paper_title}\" — assumptions that may not \
         hold, stated limitations, deferred future work, methodological gaps. Base every item \
         ONLY on the excerpts below and cite the supporting excerpt id(s).\n\
         Respond with JSON only:\n\
         [{{\"kind\": \"assumption|limitation|future_work|methodological\", \
         \"summary\": \"one precise sentence\", \"objects\": [\"<uuid>\"]}}]\n\n\
         Paper excerpts:\n{excerpts}"
    )
}

/// Parse weaknesses; any item whose citations don't resolve to real objects
/// is dropped (never shown uncited).
pub fn parse_weaknesses(tree: &SemanticTreeDocument, raw: &str) -> Vec<Weakness> {
    let Some(json) = raw
        .find('[')
        .and_then(|s| raw.rfind(']').map(|e| &raw[s..=e]))
    else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return Vec::new();
    };
    parsed
        .into_iter()
        .filter_map(|v| {
            let object_ids: Vec<Uuid> = v["objects"]
                .as_array()?
                .iter()
                .filter_map(|o| o.as_str()?.parse().ok())
                .filter(|id| tree.objects.iter().any(|o| o.id == *id))
                .collect();
            if object_ids.is_empty() {
                return None; // uncited → dropped, per spec
            }
            let kind = v["kind"].as_str().unwrap_or("limitation");
            let kind =
                if ["assumption", "limitation", "future_work", "methodological"].contains(&kind) {
                    kind
                } else {
                    "limitation"
                };
            Some(Weakness {
                id: Uuid::new_v4(),
                kind: kind.to_string(),
                summary: v["summary"].as_str()?.trim().to_string(),
                object_ids,
            })
        })
        .filter(|w| !w.summary.is_empty())
        .collect()
}

/// Run the weaknesses stage. `None` from the LLM → `Ok(None)` (no-key
/// state); a cached document keeps serving.
pub fn run_weaknesses(
    bundle: &Bundle,
    tree: &SemanticTreeDocument,
    paper_title: &str,
    llm: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<WeaknessDoc>, ExtensionError> {
    let Some(raw) = llm(&weakness_prompt(tree, paper_title)) else {
        return weaknesses(bundle);
    };
    let parsed = parse_weaknesses(tree, &raw);
    if parsed.is_empty() {
        return weaknesses(bundle);
    }
    let doc = WeaknessDoc {
        generated_at: crate::bundle::now_rfc3339(),
        weaknesses: parsed,
    };
    std::fs::create_dir_all(dir(bundle))?;
    std::fs::write(
        dir(bundle).join("weaknesses.json"),
        serde_json::to_vec_pretty(&doc).expect("serializable"),
    )?;
    Ok(Some(doc))
}

// ---------------------------------------------------------------------------
// Hypothesis cards (append-only user data)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum CardEvent {
    Create {
        card: HypothesisCard,
        at: String,
    },
    Edit {
        id: Uuid,
        claim: String,
        rationale: String,
        required_experiment: String,
        expected_evidence: String,
        at: String,
    },
    Archive {
        id: Uuid,
        at: String,
    },
    SetNovelty {
        id: Uuid,
        novelty: NoveltyResult,
        at: String,
    },
    LinkExperiment {
        id: Uuid,
        experiment_id: Uuid,
        at: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HypothesisCard {
    pub id: Uuid,
    pub claim: String,
    pub rationale: String,
    pub required_experiment: String,
    pub expected_evidence: String,
    /// Source weaknesses this card grew from.
    #[serde(default)]
    pub weakness_ids: Vec<Uuid>,
    /// Weakness-doc revision the card was built against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_rev: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub novelty: Option<NoveltyResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<Uuid>,
    #[serde(default)]
    pub archived: bool,
    /// Computed at read: the weaknesses doc regenerated since this card.
    #[serde(default)]
    pub upstream_changed: bool,
    pub created_at: String,
}

fn cards_journal(bundle: &Bundle) -> crate::bundle::Journal {
    crate::bundle::Journal::at(dir(bundle).join("hypotheses.jsonl"))
}

/// Live cards (folded), with `upstream_changed` computed against the
/// current weaknesses revision. Archived cards excluded.
pub fn cards(bundle: &Bundle) -> Result<Vec<HypothesisCard>, ExtensionError> {
    let events: Vec<CardEvent> = cards_journal(bundle).read_all()?;
    let mut live: BTreeMap<Uuid, HypothesisCard> = BTreeMap::new();
    for event in events {
        match event {
            CardEvent::Create { card, .. } => {
                live.insert(card.id, card);
            }
            CardEvent::Edit {
                id,
                claim,
                rationale,
                required_experiment,
                expected_evidence,
                ..
            } => {
                if let Some(card) = live.get_mut(&id) {
                    card.claim = claim;
                    card.rationale = rationale;
                    card.required_experiment = required_experiment;
                    card.expected_evidence = expected_evidence;
                }
            }
            CardEvent::Archive { id, .. } => {
                live.remove(&id);
            }
            CardEvent::SetNovelty { id, novelty, .. } => {
                if let Some(card) = live.get_mut(&id) {
                    card.novelty = Some(novelty);
                }
            }
            CardEvent::LinkExperiment {
                id, experiment_id, ..
            } => {
                if let Some(card) = live.get_mut(&id) {
                    card.experiment_id = Some(experiment_id);
                }
            }
        }
    }
    let current_rev = weaknesses(bundle)?.map(|d| d.generated_at);
    let mut cards: Vec<HypothesisCard> = live.into_values().collect();
    for card in &mut cards {
        card.upstream_changed = match (&card.upstream_rev, &current_rev) {
            (Some(rev), Some(current)) => rev != current,
            _ => false,
        };
    }
    cards.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(cards)
}

pub fn create_card(bundle: &Bundle, mut card: HypothesisCard) -> Result<Uuid, ExtensionError> {
    if card.upstream_rev.is_none() {
        card.upstream_rev = weaknesses(bundle)?.map(|d| d.generated_at);
    }
    let id = card.id;
    cards_journal(bundle).append(&CardEvent::Create {
        card,
        at: crate::bundle::now_rfc3339(),
    })?;
    Ok(id)
}

pub fn edit_card(
    bundle: &Bundle,
    id: Uuid,
    claim: String,
    rationale: String,
    required_experiment: String,
    expected_evidence: String,
) -> Result<(), ExtensionError> {
    cards_journal(bundle).append(&CardEvent::Edit {
        id,
        claim,
        rationale,
        required_experiment,
        expected_evidence,
        at: crate::bundle::now_rfc3339(),
    })?;
    Ok(())
}

pub fn archive_card(bundle: &Bundle, id: Uuid) -> Result<(), ExtensionError> {
    cards_journal(bundle).append(&CardEvent::Archive {
        id,
        at: crate::bundle::now_rfc3339(),
    })?;
    Ok(())
}

pub fn set_card_novelty(
    bundle: &Bundle,
    id: Uuid,
    novelty: NoveltyResult,
) -> Result<(), ExtensionError> {
    cards_journal(bundle).append(&CardEvent::SetNovelty {
        id,
        novelty,
        at: crate::bundle::now_rfc3339(),
    })?;
    Ok(())
}

pub fn link_card_experiment(
    bundle: &Bundle,
    id: Uuid,
    experiment_id: Uuid,
) -> Result<(), ExtensionError> {
    cards_journal(bundle).append(&CardEvent::LinkExperiment {
        id,
        experiment_id,
        at: crate::bundle::now_rfc3339(),
    })?;
    Ok(())
}

/// Prompt for generating candidate cards from weaknesses; generated cards
/// are ADDED (never replacing user cards).
pub fn cards_prompt(weaknesses: &WeaknessDoc, paper_title: &str) -> String {
    let list: String = weaknesses
        .weaknesses
        .iter()
        .map(|w| format!("- {} [{}]: {}\n", w.id, w.kind, w.summary))
        .collect();
    format!(
        "From these weaknesses of \"{paper_title}\", propose research hypotheses worth pursuing.\n\
         Respond with JSON only:\n\
         [{{\"claim\": \"testable claim\", \"rationale\": \"why plausible\", \
         \"required_experiment\": \"concrete design\", \"expected_evidence\": \"what would confirm it\", \
         \"weaknesses\": [\"<weakness uuid>\"]}}]\n\nWeaknesses:\n{list}"
    )
}

pub fn parse_cards(weaknesses: &WeaknessDoc, raw: &str) -> Vec<HypothesisCard> {
    let Some(json) = raw
        .find('[')
        .and_then(|s| raw.rfind(']').map(|e| &raw[s..=e]))
    else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return Vec::new();
    };
    let known: Vec<Uuid> = weaknesses.weaknesses.iter().map(|w| w.id).collect();
    parsed
        .into_iter()
        .filter_map(|v| {
            Some(HypothesisCard {
                id: Uuid::new_v4(),
                claim: v["claim"].as_str()?.trim().to_string(),
                rationale: v["rationale"].as_str().unwrap_or("").to_string(),
                required_experiment: v["required_experiment"].as_str().unwrap_or("").to_string(),
                expected_evidence: v["expected_evidence"].as_str().unwrap_or("").to_string(),
                weakness_ids: v["weaknesses"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|w| w.as_str()?.parse().ok())
                            .filter(|id| known.contains(id))
                            .collect()
                    })
                    .unwrap_or_default(),
                upstream_rev: Some(weaknesses.generated_at.clone()),
                novelty: None,
                experiment_id: None,
                archived: false,
                upstream_changed: false,
                created_at: crate::bundle::now_rfc3339(),
            })
        })
        .filter(|c| !c.claim.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Fixed bibliography, outline & draft (cite-only-what-exists)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BibEntry {
    /// Citation key (e.g. "vaswani2017attention").
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
}

fn bib_key(title: &str, first_author: Option<&str>, year: Option<i32>) -> String {
    let author = first_author
        .and_then(|a| a.split_whitespace().last())
        .unwrap_or("anon")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>();
    let word = title
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .find(|w| w.len() > 3 && !["with", "from", "this", "that"].contains(&w.as_str()))
        .unwrap_or_else(|| "work".to_string())
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>();
    format!(
        "{author}{}{word}",
        year.map(|y| y.to_string()).unwrap_or_default()
    )
}

/// Assemble the fixed bibliography a draft may cite: this paper, its
/// resolved citations, and the novelty evidence attached to live cards.
/// The draft prompt receives exactly these keys — nothing else is citable.
pub fn assemble_bibliography(
    bundle: &Bundle,
    paper_title: &str,
    paper_authors: &[String],
) -> Result<Vec<BibEntry>, ExtensionError> {
    let mut entries: Vec<BibEntry> = Vec::new();
    let mut seen_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut push = |mut entry: BibEntry, entries: &mut Vec<BibEntry>| {
        let mut key = entry.key.clone();
        let mut n = 1;
        while seen_keys.contains(&key) {
            n += 1;
            key = format!("{}{n}", entry.key);
        }
        entry.key = key.clone();
        seen_keys.insert(key);
        entries.push(entry);
    };

    push(
        BibEntry {
            key: bib_key(paper_title, paper_authors.first().map(|s| s.as_str()), None),
            title: paper_title.to_string(),
            authors: paper_authors.to_vec(),
            year: None,
            venue: None,
            identifier: None,
        },
        &mut entries,
    );

    if let Some(citations) = bundle.read_derived_json::<CitationsDocument>("citations.json")? {
        for entry in citations.entries.iter().filter_map(|e| e.resolved.as_ref()) {
            let Some(title) = &entry.title else { continue };
            push(
                BibEntry {
                    key: bib_key(title, entry.authors.first().map(|s| s.as_str()), entry.year),
                    title: title.clone(),
                    authors: entry.authors.clone(),
                    year: entry.year,
                    venue: entry.venue.clone(),
                    identifier: entry.arxiv_id.clone().or(entry.doi.clone()),
                },
                &mut entries,
            );
        }
    }

    for card in cards(bundle)? {
        if let Some(novelty) = &card.novelty {
            for evidence in &novelty.evidence {
                push(
                    BibEntry {
                        key: bib_key(&evidence.title, None, evidence.year),
                        title: evidence.title.clone(),
                        authors: vec![],
                        year: evidence.year,
                        venue: None,
                        identifier: evidence.identifier.clone(),
                    },
                    &mut entries,
                );
            }
        }
    }
    Ok(entries)
}

/// Strip `\cite{...}` (and `[[cite:...]]`) referencing keys outside the
/// bibliography. Returns (cleaned text, removed count) — the count is shown
/// to the user, per spec.
pub fn strip_unknown_citations(text: &str, bibliography: &[BibEntry]) -> (String, usize) {
    let known: std::collections::HashSet<&str> =
        bibliography.iter().map(|b| b.key.as_str()).collect();
    let mut removed = 0;
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("\\cite{") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 6..];
        match after.find('}') {
            Some(end) => {
                let keys: Vec<&str> = after[..end].split(',').map(|k| k.trim()).collect();
                let kept: Vec<&str> = keys.iter().copied().filter(|k| known.contains(k)).collect();
                removed += keys.len() - kept.len();
                if !kept.is_empty() {
                    result.push_str(&format!("\\cite{{{}}}", kept.join(",")));
                }
                rest = &after[end + 1..];
            }
            None => {
                result.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    result.push_str(rest);
    (result, removed)
}

/// LaTeX export: main.tex (with provenance markers + draft label) and
/// references.bib built from resolved metadata only.
pub fn export_latex(draft_body: &str, title: &str, bibliography: &[BibEntry]) -> (String, String) {
    let main = format!(
        "% ============================================================\n\
         % DRAFT — AI-assisted. Generated by Research Paper Copilot.\n\
         % Provenance: passages marked `% ai-drafted` were produced by an\n\
         % AI model from user-approved hypotheses and are the author's\n\
         % responsibility to verify. Remove this block deliberately.\n\
         % ============================================================\n\
         \\documentclass{{article}}\n\
         \\usepackage[utf8]{{inputenc}}\n\
         \\usepackage{{natbib}}\n\
         \\title{{{title} (Draft)}}\n\
         \\begin{{document}}\n\
         \\maketitle\n\n\
         % ai-drafted: begin\n\
         {draft_body}\n\
         % ai-drafted: end\n\n\
         \\bibliographystyle{{plainnat}}\n\
         \\bibliography{{references}}\n\
         \\end{{document}}\n"
    );
    let bib: String = bibliography
        .iter()
        .map(|entry| {
            let authors = if entry.authors.is_empty() {
                "Unknown".to_string()
            } else {
                entry.authors.join(" and ")
            };
            format!(
                "@article{{{key},\n  title={{{title}}},\n  author={{{authors}}},{year}{venue}{note}\n}}\n\n",
                key = entry.key,
                title = entry.title,
                year = entry
                    .year
                    .map(|y| format!("\n  year={{{y}}},"))
                    .unwrap_or_default(),
                venue = entry
                    .venue
                    .as_deref()
                    .map(|v| format!("\n  journal={{{v}}},"))
                    .unwrap_or_default(),
                note = entry
                    .identifier
                    .as_deref()
                    .map(|i| format!("\n  note={{{i}}}"))
                    .unwrap_or_default(),
            )
        })
        .collect();
    (main, bib)
}

/// User-data documents for outline/draft with a tiny state file recording
/// the last generation revision (resume + provenance).
pub fn read_document(bundle: &Bundle, name: &str) -> Option<String> {
    std::fs::read_to_string(dir(bundle).join(name)).ok()
}

pub fn write_document(bundle: &Bundle, name: &str, content: &str) -> Result<(), ExtensionError> {
    std::fs::create_dir_all(dir(bundle))?;
    std::fs::write(dir(bundle).join(name), content)?;
    Ok(())
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Paper;
    use crate::layout::BBox;
    use crate::objects::{Content, Object};

    fn setup() -> (tempfile::TempDir, Bundle, SemanticTreeDocument, Uuid) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("paper.research");
        let bundle = Bundle::create(&root, b"%PDF-1.5 fake", Paper::new("T"), "file").unwrap();
        let limitation_id = Uuid::new_v4();
        let tree = SemanticTreeDocument {
            pipeline_version: "0.1.0".to_string(),
            objects: vec![Object {
                id: limitation_id,
                object_type: ObjectType::Section,
                regions: vec![BBox {
                    page: 0,
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                }],
                content: Content {
                    text: "Limitations: we assume fixed-length sequences; longer contexts are left for future work.".to_string(),
                    latex: None,
                    caption: None,
                },
                semantic_label: Some("7 Limitations".to_string()),
                relationships: vec![],
                embedding: None,
                content_hash: crate::bundle::sha256_bytes(b"lim"),
                confidence: 0.9,
            }],
            tree: vec![],
        };
        (tmp, bundle, tree, limitation_id)
    }

    #[test]
    fn uncited_weaknesses_are_dropped_at_parse() {
        let (_tmp, bundle, tree, limitation) = setup();
        let raw = format!(
            r#"[
              {{"kind": "limitation", "summary": "Assumes fixed-length sequences.", "objects": ["{limitation}"]}},
              {{"kind": "limitation", "summary": "Made up with no citation.", "objects": []}},
              {{"kind": "limitation", "summary": "Cites a nonexistent object.", "objects": ["{fake}"]}}
            ]"#,
            fake = Uuid::new_v4(),
        );
        let doc = run_weaknesses(&bundle, &tree, "T", &|_| Some(raw.clone()))
            .unwrap()
            .expect("generated");
        assert_eq!(doc.weaknesses.len(), 1, "only the cited weakness survives");
        assert_eq!(doc.weaknesses[0].object_ids, vec![limitation]);
        // No-key: cached doc keeps serving.
        let cached = run_weaknesses(&bundle, &tree, "T", &|_| None)
            .unwrap()
            .unwrap();
        assert_eq!(cached.weaknesses.len(), 1);
    }

    #[test]
    fn cards_survive_upstream_regeneration_flagged_not_destroyed() {
        let (_tmp, bundle, tree, limitation) = setup();
        let raw = format!(
            r#"[{{"kind": "limitation", "summary": "Fixed-length assumption.", "objects": ["{limitation}"]}}]"#
        );
        run_weaknesses(&bundle, &tree, "T", &|_| Some(raw.clone())).unwrap();
        let doc = weaknesses(&bundle).unwrap().unwrap();

        let generated = parse_cards(
            &doc,
            &format!(
                r#"[{{"claim": "Rotary gates remove the fixed-length assumption", "rationale": "r", "required_experiment": "e", "expected_evidence": "v", "weaknesses": ["{}"]}}]"#,
                doc.weaknesses[0].id
            ),
        );
        assert_eq!(generated.len(), 1);
        let card_id = create_card(&bundle, generated[0].clone()).unwrap();
        edit_card(
            &bundle,
            card_id,
            "Rotary gates (edited)".into(),
            "r".into(),
            "e".into(),
            "v".into(),
        )
        .unwrap();

        // Upstream regeneration (different content → new revision).
        std::thread::sleep(std::time::Duration::from_millis(5));
        run_weaknesses(&bundle, &tree, "T", &|_| Some(raw.clone())).unwrap();

        let live = cards(&bundle).unwrap();
        assert_eq!(live.len(), 1, "card not destroyed");
        assert_eq!(live[0].claim, "Rotary gates (edited)", "edit preserved");
        assert!(live[0].upstream_changed, "flagged for review");

        // Experiment linking round-trips; archive removes from live set.
        let experiment = Uuid::new_v4();
        link_card_experiment(&bundle, card_id, experiment).unwrap();
        assert_eq!(cards(&bundle).unwrap()[0].experiment_id, Some(experiment));
        archive_card(&bundle, card_id).unwrap();
        assert!(cards(&bundle).unwrap().is_empty());
    }

    #[test]
    fn unknown_citation_keys_are_stripped_and_counted() {
        let bibliography = vec![BibEntry {
            key: "vaswani2017attention".into(),
            title: "Attention Is All You Need".into(),
            authors: vec!["Ashish Vaswani".into()],
            year: Some(2017),
            venue: None,
            identifier: None,
        }];
        let draft = "Building on \\cite{vaswani2017attention}, and unlike \\cite{smith2023made}, \
                     we combine \\cite{vaswani2017attention,ghost2024} both.";
        let (cleaned, removed) = strip_unknown_citations(draft, &bibliography);
        assert_eq!(removed, 2);
        assert!(cleaned.contains("\\cite{vaswani2017attention}, and unlike , we combine"));
        assert!(cleaned.contains("\\cite{vaswani2017attention} both"));
        assert!(!cleaned.contains("smith2023made") && !cleaned.contains("ghost2024"));
    }

    #[test]
    fn latex_export_carries_provenance_and_resolved_bibtex_only() {
        let (_tmp, bundle, _tree, _) = setup();
        let bibliography =
            assemble_bibliography(&bundle, "T", &["Ada Lovelace".to_string()]).unwrap();
        assert!(!bibliography.is_empty(), "the paper itself is citable");

        let (main, bib) = export_latex(
            "We propose things. \\cite{lovelacework}",
            "T",
            &bibliography,
        );
        assert!(main.contains("DRAFT — AI-assisted"), "draft label present");
        assert!(main.contains("% ai-drafted: begin"), "provenance markers");
        assert!(main.contains("\\bibliography{references}"));
        assert!(bib.contains("@article{"), "bibtex from resolved metadata");
        assert!(bib.contains("Ada Lovelace"));
    }
}
