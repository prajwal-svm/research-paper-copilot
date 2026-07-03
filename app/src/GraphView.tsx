import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@/platform";
import { Excalidraw, convertToExcalidrawElements } from "@excalidraw/excalidraw";
import type { ExcalidrawImperativeAPI } from "@excalidraw/excalidraw/types";
import type { ExcalidrawElementSkeleton } from "@excalidraw/excalidraw/data/transform";
import "@excalidraw/excalidraw/index.css";

// Self-hosted Excalidraw fonts (copied to public/excalidraw-assets at build
// time) — without this the runtime fetches fonts from a CDN, which breaks
// the app's offline-first posture. CJK fallback (Xiaolai) is excluded for
// size; those glyphs fall back to system fonts.
declare global {
  interface Window {
    EXCALIDRAW_ASSET_PATH?: string | string[];
  }
}
window.EXCALIDRAW_ASSET_PATH = "/excalidraw-assets/";
import { ExternalLinkIcon, RefreshCwIcon } from "lucide-react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Spinner } from "@/components/ui/spinner";

export interface ConceptNode {
  id: string;
  name: string;
  description?: string;
  object_ids: string[];
  confidence: number;
}

export interface ConceptEdge {
  from: string;
  to: string;
  kind: string;
  confidence: number;
}

export interface KnowledgeGraph {
  pipeline_version: string;
  extraction: "llm" | "heuristic";
  nodes: ConceptNode[];
  edges: ConceptEdge[];
}

// ---------------------------------------------------------------------------
// Concept DAG → Excalidraw scene (element skeletons + tidy tree layout)
// ---------------------------------------------------------------------------

const BLOCK_W = 220;
const BLOCK_H = 64;
const GAP_X = 140;
const GAP_Y = 28;

interface TreeNode {
  node: ConceptNode;
  children: TreeNode[];
}

/** One parent per concept (highest-confidence builds-on edge, cycle-safe);
 * every remaining edge still renders as a dashed arrow on the canvas. */
function buildForest(graph: KnowledgeGraph): { forest: TreeNode[]; parentOf: Map<string, string> } {
  const byId = new Map(graph.nodes.map((n) => [n.id, n]));
  const candidates = new Map<string, { parent: string; confidence: number }[]>();
  for (const edge of graph.edges) {
    const [child, parent] =
      edge.kind === "prerequisite_of" ? [edge.to, edge.from] : [edge.from, edge.to];
    if (child === parent || !byId.has(child) || !byId.has(parent)) continue;
    const list = candidates.get(child) ?? [];
    list.push({ parent, confidence: edge.confidence });
    candidates.set(child, list);
  }
  const parentOf = new Map<string, string>();
  const createsCycle = (child: string, parent: string) => {
    let cursor: string | undefined = parent;
    while (cursor) {
      if (cursor === child) return true;
      cursor = parentOf.get(cursor);
    }
    return false;
  };
  for (const [child, list] of candidates) {
    for (const c of [...list].sort((a, b) => b.confidence - a.confidence)) {
      if (!createsCycle(child, c.parent)) {
        parentOf.set(child, c.parent);
        break;
      }
    }
  }
  const childrenOf = new Map<string, string[]>();
  for (const [child, parent] of parentOf) {
    childrenOf.set(parent, [...(childrenOf.get(parent) ?? []), child]);
  }
  const build = (id: string): TreeNode => ({
    node: byId.get(id)!,
    children: (childrenOf.get(id) ?? []).map(build),
  });
  const forest = graph.nodes
    .filter((n) => !parentOf.has(n.id))
    .map((n) => build(n.id))
    .sort((a, b) => b.children.length - a.children.length);
  return { forest, parentOf };
}

/** Tidy left→right tree layout: x by depth, y centered over the subtree. */
function layout(forest: TreeNode[]): Map<string, { x: number; y: number }> {
  const positions = new Map<string, { x: number; y: number }>();
  let cursorY = 0;
  const place = (item: TreeNode, depth: number): { top: number; bottom: number } => {
    const x = depth * (BLOCK_W + GAP_X);
    if (item.children.length === 0) {
      const y = cursorY;
      cursorY += BLOCK_H + GAP_Y;
      positions.set(item.node.id, { x, y });
      return { top: y, bottom: y + BLOCK_H };
    }
    const extents = item.children.map((child) => place(child, depth + 1));
    const top = extents[0].top;
    const bottom = extents[extents.length - 1].bottom;
    const y = (top + bottom) / 2 - BLOCK_H / 2;
    positions.set(item.node.id, { x, y });
    return { top: Math.min(top, y), bottom: Math.max(bottom, y + BLOCK_H) };
  };
  for (const root of forest) {
    place(root, 0);
    cursorY += GAP_Y * 2; // breathing room between root clusters
  }
  return positions;
}

// Elements always use light-theme colors: Excalidraw's dark theme is a
// render-time invert filter (black text renders white, etc.), so baking
// theme-specific colors into elements would double-invert.
function sceneFromGraph(graph: KnowledgeGraph): ExcalidrawElementSkeleton[] {
  const { forest, parentOf } = buildForest(graph);
  const positions = layout(forest);
  const stroke = "#495057";
  const accent = "#4c5eff";

  const skeletons: ExcalidrawElementSkeleton[] = [];
  for (const node of graph.nodes) {
    const pos = positions.get(node.id);
    if (!pos) continue;
    const lowConfidence = node.confidence < 0.6;
    skeletons.push({
      type: "rectangle",
      id: node.id,
      x: pos.x,
      y: pos.y,
      width: BLOCK_W,
      height: BLOCK_H,
      roundness: { type: 3 },
      strokeColor: lowConfidence ? stroke : accent,
      strokeStyle: lowConfidence ? "dashed" : "solid",
      backgroundColor: "transparent",
      label: {
        text: node.name.length > 60 ? `${node.name.slice(0, 59)}…` : node.name,
        fontSize: 14,
        strokeColor: "#1f2937",
      },
    });
  }
  // Parent-relation arrows (solid) + remaining relationship arrows (dashed).
  const drawn = new Set<string>();
  for (const [child, parent] of parentOf) {
    skeletons.push(arrowSkeleton(parent, child, positions, accent, "solid"));
    drawn.add(`${parent}->${child}`);
  }
  for (const edge of graph.edges) {
    const [child, parent] =
      edge.kind === "prerequisite_of" ? [edge.to, edge.from] : [edge.from, edge.to];
    if (drawn.has(`${parent}->${child}`) || !positions.has(parent) || !positions.has(child)) {
      continue;
    }
    drawn.add(`${parent}->${child}`);
    skeletons.push(arrowSkeleton(parent, child, positions, stroke, "dashed"));
  }
  return skeletons;
}

function arrowSkeleton(
  from: string,
  to: string,
  positions: Map<string, { x: number; y: number }>,
  color: string,
  style: "solid" | "dashed",
): ExcalidrawElementSkeleton {
  const a = positions.get(from)!;
  const b = positions.get(to)!;
  const x = a.x + BLOCK_W;
  const y = a.y + BLOCK_H / 2;
  return {
    type: "arrow",
    x,
    y,
    width: Math.max(b.x - x, 10),
    height: b.y + BLOCK_H / 2 - y,
    strokeColor: color,
    strokeStyle: style,
    start: { id: from },
    end: { id: to },
  };
}

// ---------------------------------------------------------------------------

/** App theme, reactively: tracks the `.dark` class the ThemeToggle flips. */
function useAppDark(): boolean {
  const [dark, setDark] = useState(() =>
    document.documentElement.classList.contains("dark"),
  );
  useEffect(() => {
    const observer = new MutationObserver(() =>
      setDark(document.documentElement.classList.contains("dark")),
    );
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    return () => observer.disconnect();
  }, []);
  return dark;
}

/**
 * Concept map on an Excalidraw canvas (v2 graph view, canvas edition):
 * generated from the paper's knowledge graph, then fully yours — move
 * blocks, edit text (double-click), add blocks/arrows/notes with the
 * toolbar, extend branches freely. The scene persists in the bundle as
 * user data; "Rebuild from graph" regenerates the machine layout after an
 * explicit confirmation (your canvas is never silently replaced).
 */
export default function GraphView({
  paperId,
  onOpenConcept,
}: {
  paperId: string;
  /** Open a selected concept block in the paper (<300 ms). */
  onOpenConcept?: (node: ConceptNode) => void;
}) {
  const [graph, setGraph] = useState<KnowledgeGraph | null | undefined>(undefined);
  const [savedScene, setSavedScene] = useState<
    | { elements?: unknown[]; appState?: Record<string, unknown>; files?: Record<string, unknown> }
    | null
    | undefined
  >(undefined);
  const [selectedConcept, setSelectedConcept] = useState<ConceptNode | null>(null);
  const [confirmRebuild, setConfirmRebuild] = useState(false);
  const apiRef = useRef<ExcalidrawImperativeAPI | null>(null);
  const dotsRef = useRef<HTMLDivElement | null>(null);
  const saveTimer = useRef<number | undefined>(undefined);
  const dark = useAppDark();

  useEffect(() => {
    invoke<KnowledgeGraph | null>("graph_get", { paperId })
      .then(setGraph)
      .catch(() => setGraph(null));
    invoke<{
      elements?: unknown[];
      appState?: Record<string, unknown>;
      files?: Record<string, unknown>;
    } | null>("canvas_get", { paperId })
      .then(setSavedScene)
      .catch(() => setSavedScene(null));
  }, [paperId]);

  const conceptById = useMemo(
    () => new Map((graph?.nodes ?? []).map((n) => [n.id, n])),
    [graph],
  );

  const generatedElements = useMemo(
    () => (graph && graph.nodes.length > 0 ? convertToExcalidrawElements(sceneFromGraph(graph)) : []),
    [graph],
  );

  // Debounced persistence: every edit lands in the bundle as user data.
  const persist = useCallback(() => {
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      const api = apiRef.current;
      if (!api) return;
      const elements = api.getSceneElements();
      const files = api.getFiles();
      const s = api.getAppState();
      const appState = {
        theme: s.theme,
        viewBackgroundColor: s.viewBackgroundColor,
        gridSize: s.gridSize,
        gridStep: s.gridStep,
        gridModeEnabled: s.gridModeEnabled,
        // Background schema marker: 3 = dotted CSS backdrop behind a
        // transparent canvas (Excalidraw's own grid renders lines, so
        // it stays off by default).
        canvasGrid: 3,
        scrollX: s.scrollX,
        scrollY: s.scrollY,
        zoom: s.zoom,
      };
      invoke("canvas_save", { paperId, scene: { elements, appState, files } }).catch(() => {});
    }, 800);
  }, [paperId]);

  // True dotted backdrop: Excalidraw's built-in grid draws (dashed)
  // lines, never dots, so it stays off. The canvas is transparent and
  // this layer behind it paints theme-aware dots, kept in lockstep with
  // pan/zoom on every onChange.
  const paintDots = useCallback(
    (s: {
      scrollX: number;
      scrollY: number;
      zoom: { value: number };
      theme: string;
      viewBackgroundColor: string;
    }) => {
      const el = dotsRef.current;
      if (!el) return;
      const z = s.zoom.value;
      const cell = 20 * z;
      const isDark = s.theme === "dark";
      el.style.backgroundColor =
        s.viewBackgroundColor === "transparent"
          ? isDark
            ? "#121212"
            : "#ffffff"
          : s.viewBackgroundColor;
      const dot = isDark ? "rgba(255,255,255,0.22)" : "rgba(0,0,0,0.22)";
      const r = Math.max(1, 1.1 * z);
      el.style.backgroundImage =
        cell >= 8 ? `radial-gradient(circle, ${dot} ${r}px, transparent ${r}px)` : "none";
      el.style.backgroundSize = `${cell}px ${cell}px`;
      el.style.backgroundPosition = `${(s.scrollX % 20) * z}px ${(s.scrollY % 20) * z}px`;
    },
    [],
  );

  // Track selection → "Open in paper" affordance for concept blocks.
  const handleChange = useCallback(
    (
      _elements: unknown,
      appState: {
        selectedElementIds?: Record<string, boolean>;
        scrollX: number;
        scrollY: number;
        zoom: { value: number };
        theme: string;
        viewBackgroundColor: string;
      },
    ) => {
      persist();
      paintDots(appState);
      const ids = Object.keys(appState.selectedElementIds ?? {});
      const concept = ids.length === 1 ? conceptById.get(ids[0]) : undefined;
      setSelectedConcept((current) =>
        concept?.id === current?.id ? current : (concept ?? null),
      );
    },
    [persist, paintDots, conceptById],
  );

  function rebuildFromGraph() {
    const api = apiRef.current;
    if (!api || !graph) return;
    api.updateScene({ elements: convertToExcalidrawElements(sceneFromGraph(graph)) });
    api.scrollToContent(undefined, { fitToContent: true });
    persist();
  }

  if (graph === undefined || savedScene === undefined) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner />
      </div>
    );
  }
  if ((!graph || graph.nodes.length === 0) && !savedScene) {
    return (
      <div className="flex h-full items-center justify-center">
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No concept map yet</EmptyTitle>
            <EmptyDescription>
              The map is generated when the paper finishes processing. Reopen
              the paper once ingestion completes.
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      </div>
    );
  }

  const initialElements =
    savedScene?.elements && savedScene.elements.length > 0
      ? (savedScene.elements as never[])
      : generatedElements;

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-none items-center gap-2 px-3 pb-2 pt-10">
        {graph?.extraction === "heuristic" && (
          <Badge variant="outline">limited map — no AI provider during processing</Badge>
        )}
        <span className="text-muted-foreground flex-1 text-xs">
          your canvas — move, edit (double-click), and add blocks with the toolbar; it saves into
          the paper bundle
        </span>
        {selectedConcept && (
          <Button size="sm" onClick={() => onOpenConcept?.(selectedConcept)}>
            <ExternalLinkIcon data-icon="inline-start" />
            Open “{selectedConcept.name.slice(0, 24)}
            {selectedConcept.name.length > 24 ? "…" : ""}” in paper
          </Button>
        )}
        {graph && graph.nodes.length > 0 && (
          <Button variant="ghost" size="sm" onClick={() => setConfirmRebuild(true)}>
            <RefreshCwIcon data-icon="inline-start" />
            Rebuild from graph
          </Button>
        )}
      </div>

      <div className="relative min-h-0 flex-1">
        {/* Dotted backdrop, visible through the transparent canvas. */}
        <div ref={dotsRef} aria-hidden className="pointer-events-none absolute inset-0" />
        <div className="absolute inset-0">
        <Excalidraw
          // Controlled: the canvas (and its render-time color inversion —
          // black text renders white in dark) always follows the app theme.
          theme={dark ? "dark" : "light"}
          excalidrawAPI={(api) => {
            apiRef.current = api;
            // First paint of the dot layer; onChange keeps it in sync.
            window.requestAnimationFrame(() => paintDots(api.getAppState()));
          }}
          initialData={{
            elements: initialElements,
            appState: {
              ...(savedScene?.appState ?? {}),
              // Migrate pre-dots saves (canvasGrid < 3): canvas
              // transparent so the dot layer shows through.
              ...(savedScene?.appState?.canvasGrid === 3
                ? {}
                : { viewBackgroundColor: "transparent" }),
              // The dot backdrop replaces Excalidraw's line grid, so the
              // grid never comes back on load — regardless of what any
              // earlier save recorded (hot-reload can stamp the marker
              // onto pre-migration state, so the marker can't be trusted
              // for this).
              gridModeEnabled: false,
            },
            files: (savedScene?.files ?? undefined) as never,
            scrollToContent: !savedScene?.appState,
          }}
          onChange={handleChange}
        />
        </div>
      </div>

      <AlertDialog open={confirmRebuild} onOpenChange={setConfirmRebuild}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Rebuild the map from the knowledge graph?</AlertDialogTitle>
            <AlertDialogDescription>
              This replaces the current canvas — including blocks you added
              and layout changes — with a fresh machine-generated map of the
              paper's {graph?.nodes.length ?? 0} concepts.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep my canvas</AlertDialogCancel>
            <AlertDialogAction onClick={rebuildFromGraph}>Rebuild</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
