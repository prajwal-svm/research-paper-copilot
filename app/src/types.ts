// Mirrors copilot-core's serialized types crossing the Tauri boundary.

export type IngestionStatus = "ready" | "processing" | "degraded" | "failed";

export interface PaperSummary {
  id: string;
  title: string;
  authors: string[];
  status: IngestionStatus;
  imported_at?: string;
  last_opened?: string;
  arxiv_id?: string;
  doi?: string;
  published_at?: string;
  starred: boolean;
  priority?: "high" | "medium" | "low";
}

export type PipelineStage = "layout" | "objects" | "enrichment" | "embeddings";

export type PipelineProgressEvent =
  | { kind: "stage_started"; stage: PipelineStage }
  | { kind: "stage_completed"; stage: PipelineStage }
  | { kind: "stage_skipped"; stage: PipelineStage }
  | { kind: "stage_degraded"; stage: PipelineStage; reason: string }
  | { kind: "stage_failed"; stage: PipelineStage; reason: string }
  | { kind: "pipeline_finished"; usable: boolean };

export interface IngestionProgress {
  paper_id: string;
  event: PipelineProgressEvent;
}

// ---- .research bundle artifacts (subset the reader consumes) ----

export interface BBox {
  page: number;
  x: number;
  y: number;
  width: number;
  height: number;
}

export type ObjectType =
  | "section"
  | "paragraph"
  | "sentence"
  | "equation"
  | "figure"
  | "table"
  | "citation"
  | "definition"
  | "algorithm"
  | "experiment"
  | "dataset"
  | "metric"
  | "claim"
  | "limitation"
  | "future_work"
  | "selection";

export interface PaperObject {
  id: string;
  type: ObjectType;
  regions?: BBox[];
  content: { text: string; latex?: string; caption?: string };
  semantic_label?: string;
  relationships?: { type: string; target: string; confidence?: number }[];
  embedding: { index: number } | null;
  content_hash: string;
  confidence: number;
}

export interface SemanticTree {
  pipeline_version: string;
  objects: PaperObject[];
  tree: { object: string; children?: unknown[] }[];
}

/** An ad-hoc object created from a manual text selection (client-side only
 * until it gets an anchored note/chat). */
export interface AdHocSelection {
  id: string;
  type: "selection";
  text: string;
  regions: BBox[];
}

export interface SearchHit {
  object_id: string;
  snippet: string;
  score: number;
}

export interface SearchResults {
  exact: SearchHit[];
  semantic: SearchHit[];
  semantic_available: boolean;
}
