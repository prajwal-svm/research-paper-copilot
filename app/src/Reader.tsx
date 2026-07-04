import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@/platform";
import * as pdfjs from "pdfjs-dist";
import type { PDFDocumentProxy } from "pdfjs-dist";
import {
  HomeIcon,
  EraserIcon,
  FileTextIcon,
  FlaskConicalIcon,
  FolderGit2Icon,
  GraduationCapIcon,
  LightbulbIcon,
  ChevronsLeftIcon,
  ChevronsRightIcon,
  GripVerticalIcon,
  PuzzleIcon,
  UsersIcon,
  HighlighterIcon,
  PenLineIcon,
  ScanTextIcon,
  SearchIcon,
  SquareDashedMousePointerIcon,
  TextIcon,
  WaypointsIcon,
  XIcon,
  ZoomInIcon,
  ZoomOutIcon,
} from "lucide-react";
import { Dock, DockIcon } from "@/components/ui/dock";
import { Spinner } from "@/components/ui/spinner";
import ReadingMode from "./ReadingMode";

// The concept mindmap loads on demand with the other panes.
const GraphView = lazy(() => import("./GraphView"));
// Recharts + the sandbox workbench load only when experiments open.
const ExperimentWorkbench = lazy(() => import("./ExperimentWorkbench"));
// Reproduction wizard + repo browser (CodeMirror) load on demand too.
const ReproductionPane = lazy(() => import("./ReproductionPane"));
// Extension mode (v4) loads on demand (BlockNote inside).
const ExtensionMode = lazy(() => import("./ExtensionMode"));
const PluginPane = lazy(() => import("./PluginPane"));
const CommunityPane = lazy(() => import("./CommunityPane"));
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import ObjectPanel from "./ObjectPanel";
import PaperMarkdownDialog from "./PaperMarkdownDialog";
import InkLayer, { INK_COLORS, type InkStroke, type InkTool } from "./InkLayer";
import { AnnotationsMenu, type Bookmark, type Note } from "./Annotations";
import { CitationTargets, type CitationsDocument } from "./CitationLayer";
import type {
  AdHocSelection,
  PaperObject,
  SearchResults,
  SemanticTree,
} from "./types";

// Vite bundles the worker; keeps rendering off the main thread.
pdfjs.GlobalWorkerOptions.workerSrc = new URL(
  "pdfjs-dist/build/pdf.worker.min.mjs",
  import.meta.url,
).toString();

/** How many pages beyond the viewport get real canvases. */
const OVERSCAN = 2;
const PAGE_GAP = 16;

interface PageSlot {
  index: number;
  width: number; // CSS px at current scale
  height: number;
  top: number; // offset within the scroll content
}

/** The user's current focus: an extracted object or an ad-hoc selection. */
export type Selection =
  | { kind: "object"; object: PaperObject }
  | { kind: "ad-hoc"; selection: AdHocSelection };

/**
 * Virtualized canvas reader with an interactive object overlay: every page
 * has a fixed-size slot (stable scrollbar); slots near the viewport hold a
 * live canvas, a pdf.js text layer (native text selection), and transparent
 * hover/click targets positioned from the extracted objects' regions.
 */
const BASE_SCALE = 1.35; // ~fit-width for A4/letter in a 900px column
const MIN_ZOOM = 0.5;
const MAX_ZOOM = 2.2;

export default function Reader({
  paperId,
  onBack,
  onOpenPaper,
  initialPane,
}: {
  paperId: string;
  onBack: () => void;
  /** Cross-paper navigation ("seen in paper X"). */
  onOpenPaper?: (paperId: string) => void;
  /** Land on a specific pane (omnibar: canvas view opens "graph"). */
  initialPane?: "pdf" | "graph" | "lessons" | "experiments" | "repro" | "extend" | "plugins" | "community";
}) {
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [pageSizes, setPageSizes] = useState<{ width: number; height: number }[]>([]);
  const [zoom, setZoom] = useState(1);
  const [visible, setVisible] = useState<[number, number]>([0, OVERSCAN]);
  const [tree, setTree] = useState<SemanticTree | null>(null);
  const [citations, setCitations] = useState<CitationsDocument | null>(null);
  const [notes, setNotes] = useState<Note[]>([]);
  const [bookmarks, setBookmarks] = useState<Bookmark[]>([]);
  const [selected, setSelected] = useState<Selection | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rawView, setRawView] = useState(false);
  const [extractionNotice, setExtractionNotice] = useState<string | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [markdownOpen, setMarkdownOpen] = useState(false);
  const [paneMode, setPaneMode] = useState<
    "pdf" | "graph" | "lessons" | "experiments" | "repro" | "extend" | "plugins" | "community"
  >(initialPane ?? "pdf");
  const [codeTarget, setCodeTarget] = useState<{ file: string; line: number } | null>(null);
  const [flashId, setFlashId] = useState<string | null>(null);
  const [regionMode, setRegionMode] = useState(false);
  const [inkStrokes, setInkStrokes] = useState<InkStroke[]>([]);
  const [inkTool, setInkTool] = useState<InkTool | null>(null);
  const [inkColor, setInkColor] = useState<string>(INK_COLORS[0]);
  const sessionStrokes = useRef<string[]>([]);
  const [marquee, setMarquee] = useState<{
    page: number;
    x0: number;
    y0: number;
    x1: number;
    y1: number;
  } | null>(null);
  const restoredScroll = useRef<number | null>(null);
  const saveTimer = useRef<number | undefined>(undefined);
  const scrollRef = useRef<HTMLDivElement>(null);
  const scale = BASE_SCALE * zoom;

  // Page slots derived from scale-1 page sizes; zoom just rescales geometry.
  const slots = useMemo<PageSlot[]>(() => {
    let top = 0;
    return pageSizes.map((size, index) => {
      const slot = {
        index,
        width: size.width * scale,
        height: size.height * scale,
        top,
      };
      top += size.height * scale + PAGE_GAP;
      return slot;
    });
  }, [pageSizes, scale]);

  // Zoom, preserving the reader's relative position in the document.
  const applyZoom = useCallback(
    (next: number) => {
      const clamped = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, next));
      const el = scrollRef.current;
      if (el && clamped !== zoom) {
        const target = el.scrollTop * (clamped / zoom);
        requestAnimationFrame(() => {
          el.scrollTop = target;
        });
      }
      setZoom(clamped);
    },
    [zoom],
  );

  // Load the document + extracted objects.
  useEffect(() => {
    let cancelled = false;
    let loaded: PDFDocumentProxy | null = null;
    (async () => {
      try {
        const bytes = await invoke<number[]>("read_original_pdf", { id: paperId });
        const doc = await pdfjs.getDocument({ data: new Uint8Array(bytes) }).promise;
        if (cancelled) {
          doc.loadingTask.destroy();
          return;
        }
        loaded = doc;
        // Slot geometry from page metadata only (no rendering), at scale 1.
        const sizes: { width: number; height: number }[] = [];
        for (let i = 1; i <= doc.numPages; i++) {
          const page = await doc.getPage(i);
          const viewport = page.getViewport({ scale: 1 });
          sizes.push({ width: viewport.width, height: viewport.height });
        }
        if (cancelled) return;
        setDoc(doc);
        setPageSizes(sizes);
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    // Object layer is optional: absent while ingestion runs → raw view.
    invoke<SemanticTree | null>("read_artifact", {
      id: paperId,
      artifact: "semantic_tree.json",
    })
      .then((t) => !cancelled && setTree(t))
      .catch(() => {});
    invoke<CitationsDocument | null>("read_artifact", {
      id: paperId,
      artifact: "citations.json",
    })
      .then((c) => !cancelled && setCitations(c))
      .catch(() => {});
    refreshAnnotations();
    invoke<InkStroke[]>("ink_list", { paperId })
      .then((s) => !cancelled && setInkStrokes(s))
      .catch(() => {});
    // Restore last reading state (position, panels) if present.
    invoke<{ scroll_top?: number; raw_view?: boolean } | null>("read_artifact", {
      id: paperId,
      artifact: "reading_state.json",
    })
      .then((state) => {
        if (cancelled || !state) return;
        if (typeof state.scroll_top === "number") restoredScroll.current = state.scroll_top;
        if (state.raw_view) setRawView(true);
      })
      .catch(() => {});
    // Raw view is the automatic default when extraction failed or degraded;
    // the reason comes from the pipeline's plain-language failure records.
    invoke<Record<string, unknown> | null>("read_artifact", {
      id: paperId,
      artifact: "metadata.json",
    })
      .then((metadata) => {
        if (cancelled || !metadata) return;
        const stages = (metadata.pipeline as { stages?: Record<string, { status?: string; failure_reason?: string }> })
          ?.stages ?? {};
        const layout = stages.layout;
        const objects = stages.objects;
        if (layout?.status === "failed" || objects?.status === "failed") {
          setRawView(true);
          setExtractionNotice(
            layout?.failure_reason ??
              objects?.failure_reason ??
              "Object extraction failed for this paper — showing the raw PDF.",
          );
        } else if (layout?.status === "degraded") {
          setExtractionNotice(layout.failure_reason ?? "Object extraction is limited for this paper.");
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      loaded?.loadingTask.destroy();
    };
  }, [paperId]);

  const notedIds = useMemo(() => new Set(notes.map((n) => n.object_id)), [notes]);

  const refreshAnnotations = useCallback(() => {
    invoke<Note[]>("notes_list", { paperId }).then(setNotes).catch(() => {});
    invoke<Bookmark[]>("bookmarks_list", { paperId })
      .then(setBookmarks)
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [paperId]);

  // Objects indexed by page for the overlay. Sentences are skipped (their
  // paragraph is the hover target); sections/citations have tiny regions and
  // join in later tasks.
  const objectsByPage = useMemo(() => {
    const byPage = new Map<number, { object: PaperObject; regionIndex: number }[]>();
    if (!tree) return byPage;
    for (const object of tree.objects) {
      if (object.type === "sentence" || object.type === "citation") continue;
      object.regions?.forEach((region, regionIndex) => {
        const list = byPage.get(region.page) ?? [];
        list.push({ object, regionIndex });
        byPage.set(region.page, list);
      });
    }
    return byPage;
  }, [tree]);

  // Track which slots intersect the viewport (+overscan).
  const onScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el || slots.length === 0) return;
    const viewTop = el.scrollTop;
    const viewBottom = viewTop + el.clientHeight;
    let first = slots.findIndex((s) => s.top + s.height >= viewTop);
    if (first === -1) first = slots.length - 1;
    let last = first;
    while (last + 1 < slots.length && slots[last + 1].top <= viewBottom) last++;
    setVisible(([prevFirst, prevLast]) => {
      const nextFirst = Math.max(0, first - OVERSCAN);
      const nextLast = Math.min(slots.length - 1, last + OVERSCAN);
      return prevFirst === nextFirst && prevLast === nextLast
        ? [prevFirst, prevLast]
        : [nextFirst, nextLast];
    });
  }, [slots]);

  useEffect(onScroll, [onScroll]);

  // Apply the restored scroll position once geometry exists.
  useEffect(() => {
    if (slots.length === 0 || restoredScroll.current === null) return;
    scrollRef.current?.scrollTo({ top: restoredScroll.current });
    restoredScroll.current = null;
  }, [slots]);

  // Persist reading state, debounced against scroll bursts.
  const persistState = useCallback(() => {
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      invoke("save_reading_state", {
        id: paperId,
        readingState: {
          scroll_top: scrollRef.current?.scrollTop ?? 0,
          raw_view: rawView,
          selected_object_id: selected?.kind === "object" ? selected.object.id : null,
        },
      }).catch(() => {});
    }, 400);
  }, [paperId, rawView, selected]);

  useEffect(persistState, [persistState]);

  // Ad-hoc selection objects from native text selection over the text layer.
  const onMouseUp = useCallback(() => {
    const domSelection = window.getSelection();
    if (!domSelection || domSelection.isCollapsed) return;
    const text = domSelection.toString().trim();
    if (text.length < 3) return;

    // Selection rects → page-local PDF-point regions.
    const regions: AdHocSelection["regions"] = [];
    for (const slot of slots) {
      const pageEl = document.querySelector<HTMLElement>(`[data-page-slot="${slot.index}"]`);
      if (!pageEl) continue;
      const pageRect = pageEl.getBoundingClientRect();
      for (const rect of domSelection.getRangeAt(0).getClientRects()) {
        const intersects =
          rect.left < pageRect.right &&
          rect.right > pageRect.left &&
          rect.top < pageRect.bottom &&
          rect.bottom > pageRect.top;
        if (!intersects || rect.width < 1 || rect.height < 1) continue;
        regions.push({
          page: slot.index,
          x: (rect.left - pageRect.left) / scale,
          y: (rect.top - pageRect.top) / scale,
          width: rect.width / scale,
          height: rect.height / scale,
        });
      }
    }
    if (regions.length === 0) return;
    setSelected({
      kind: "ad-hoc",
      selection: { id: crypto.randomUUID(), type: "selection", text, regions },
    });
  }, [slots, scale]);

  // Navigate to an object: scroll its first region into view and flash it.
  const goToObject = useCallback(
    (objectId: string) => {
      const object = tree?.objects.find((o) => o.id === objectId);
      const region = object?.regions?.[0];
      const el = scrollRef.current;
      if (!object || !region || !el) return;
      const slot = slots[region.page];
      if (!slot) return;
      el.scrollTo({ top: slot.top + region.y * scale - 120, behavior: "auto" });
      setSelected({ kind: "object", object });
      setFlashId(objectId);
      window.setTimeout(() => setFlashId(null), 1200);
    },
    [tree, slots, scale],
  );

  // Ink persistence: commit on pointer-up, erase, session-scoped undo (⌘Z).
  const commitStroke = useCallback(
    (stroke: InkStroke) => {
      setInkStrokes((all) => [...all, stroke]);
      sessionStrokes.current.push(stroke.stroke_id);
      invoke("ink_add", { paperId, stroke }).catch(() => {});
    },
    [paperId],
  );
  const eraseStroke = useCallback(
    (strokeId: string) => {
      setInkStrokes((all) => all.filter((s) => s.stroke_id !== strokeId));
      invoke("ink_delete", { paperId, strokeId }).catch(() => {});
    },
    [paperId],
  );

  // Cmd/Ctrl+F opens in-paper search; Escape exits search/region/draw mode;
  // ⌘Z undoes the last stroke drawn this session.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "f") {
        e.preventDefault();
        setSearchOpen(true);
      }
      if ((e.metaKey || e.ctrlKey) && e.key === "z" && sessionStrokes.current.length > 0) {
        e.preventDefault();
        const last = sessionStrokes.current.pop()!;
        eraseStroke(last);
      }
      if (e.key === "Escape") {
        setSearchOpen(false);
        setRegionMode(false);
        setMarquee(null);
        setInkTool(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [eraseStroke]);

  // Region marquee (screenshot-crop-style selection): drag a rectangle over
  // any part of a page — diagrams included — to make an ad-hoc selection.
  // Active via the toolbar toggle or by holding Alt while dragging.
  const beginMarquee = useCallback(
    (e: React.MouseEvent) => {
      if (!regionMode && !e.altKey) return;
      if (e.button !== 0) return;
      const pageEl = (e.target as HTMLElement).closest<HTMLElement>("[data-page-slot]");
      if (!pageEl) return;
      e.preventDefault();
      const page = Number(pageEl.dataset.pageSlot);
      const pageRect = pageEl.getBoundingClientRect();
      const startX = e.clientX - pageRect.left;
      const startY = e.clientY - pageRect.top;
      setMarquee({ page, x0: startX, y0: startY, x1: startX, y1: startY });

      const onMove = (move: MouseEvent) => {
        setMarquee({
          page,
          x0: startX,
          y0: startY,
          x1: Math.min(Math.max(move.clientX - pageRect.left, 0), pageRect.width),
          y1: Math.min(Math.max(move.clientY - pageRect.top, 0), pageRect.height),
        });
      };
      const onUp = (up: MouseEvent) => {
        window.removeEventListener("mousemove", onMove);
        window.removeEventListener("mouseup", onUp);
        const endX = Math.min(Math.max(up.clientX - pageRect.left, 0), pageRect.width);
        const endY = Math.min(Math.max(up.clientY - pageRect.top, 0), pageRect.height);
        setMarquee(null);
        setRegionMode(false);
        // CSS px → PDF points (top-left origin), normalized corners.
        const rect = {
          page,
          x: Math.min(startX, endX) / scale,
          y: Math.min(startY, endY) / scale,
          width: Math.abs(endX - startX) / scale,
          height: Math.abs(endY - startY) / scale,
        };
        if (rect.width < 8 || rect.height < 8) return; // accidental click
        // Gather text from every object the rectangle touches, reading order.
        const touched = (objectsByPage.get(page) ?? [])
          .filter(({ object, regionIndex }) => {
            const r = object.regions![regionIndex];
            return (
              r.x < rect.x + rect.width &&
              rect.x < r.x + r.width &&
              r.y < rect.y + rect.height &&
              rect.y < r.y + r.height
            );
          })
          .sort((a, b) => a.object.regions![a.regionIndex].y - b.object.regions![b.regionIndex].y);
        const seen = new Set<string>();
        const text = touched
          .filter(({ object }) => (seen.has(object.id) ? false : seen.add(object.id)))
          .map(({ object }) =>
            object.semantic_label
              ? `${object.semantic_label}: ${object.content.text}`
              : object.content.text,
          )
          .join("\n\n")
          .slice(0, 4000);
        setSelected({
          kind: "ad-hoc",
          selection: {
            id: crypto.randomUUID(),
            type: "selection",
            text: text || "(a visual region of the page with no extracted text)",
            regions: [rect],
          },
        });
      };
      window.addEventListener("mousemove", onMove);
      window.addEventListener("mouseup", onUp);
    },
    [regionMode, scale, objectsByPage],
  );

  if (error) {
    return (
      <div className="flex h-screen flex-col">
        <ReaderBar
          onBack={onBack}
          rawView={rawView}
          onToggleRaw={() => setRawView((v) => !v)}
          hasObjectLayer={false}
        />
        <Alert variant="destructive" className="m-4">
          <AlertTitle>Could not open paper</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      </div>
    );
  }

  const totalHeight = slots.length
    ? slots[slots.length - 1].top + slots[slots.length - 1].height
    : 0;

  return (
    <div className="flex h-screen flex-col">
      <ReaderBar
        onBack={onBack}
        rawView={rawView}
        onToggleRaw={() => setRawView((v) => !v)}
        hasObjectLayer={tree !== null}
        onOpenSearch={() => setSearchOpen(true)}
        regionMode={regionMode}
        onToggleRegionMode={() => setRegionMode((v) => !v)}
        zoom={zoom}
        onZoom={applyZoom}
        inkTool={inkTool}
        onToggleInk={() => setInkTool((t) => (t ? null : "pen"))}
        paneMode={paneMode}
        onToggleGraph={() => setPaneMode((m) => (m === "graph" ? "pdf" : "graph"))}
        onToggleLessons={() => setPaneMode((m) => (m === "lessons" ? "pdf" : "lessons"))}
        onToggleExperiments={() =>
          setPaneMode((m) => (m === "experiments" ? "pdf" : "experiments"))
        }
        onToggleRepro={() => setPaneMode((m) => (m === "repro" ? "pdf" : "repro"))}
        onToggleExtend={() => setPaneMode((m) => (m === "extend" ? "pdf" : "extend"))}
        onTogglePlugins={() => setPaneMode((m) => (m === "plugins" ? "pdf" : "plugins"))}
        onToggleCommunity={() => setPaneMode((m) => (m === "community" ? "pdf" : "community"))}
        onOpenMarkdown={() => setMarkdownOpen(true)}
        annotations={
          <AnnotationsMenu
            paperId={paperId}
            bookmarks={bookmarks}
            iconOnly
            labelFor={(id) =>
              tree?.objects.find((o) => o.id === id)?.semantic_label ??
              tree?.objects.find((o) => o.id === id)?.content.text.slice(0, 50)
            }
            onNavigate={goToObject}
          />
        }
      />
      <PaperMarkdownDialog
        paperId={paperId}
        open={markdownOpen}
        onOpenChange={setMarkdownOpen}
      />
      {searchOpen && (
        <SearchPanel
          paperId={paperId}
          onNavigate={(id) => {
            goToObject(id);
          }}
          onClose={() => setSearchOpen(false)}
        />
      )}
      {extractionNotice && (
        <Alert className="m-2">
          <AlertTitle>Limited extraction</AlertTitle>
          <AlertDescription>{extractionNotice}</AlertDescription>
        </Alert>
      )}
      {inkTool && (
        <InkPalette
          tool={inkTool}
          color={inkColor}
          onTool={setInkTool}
          onColor={setInkColor}
        />
      )}
      <div className="relative min-h-0 flex-1">
        {/* Split view: selecting an object pushes the PDF left and opens a
            resizable right pane (chat, context, annotations). */}
        <ResizablePanelGroup>
          <ResizablePanel defaultSize="68%" minSize="35%" className="relative">
            {/* Drag strip scoped to the PDF pane: drags the window, hosts the
                traffic lights, never overlays the right panel's controls. */}
            <div data-tauri-drag-region className="absolute inset-x-0 top-0 z-20 h-9" />
            {paneMode === "graph" && (
              <div className="absolute inset-0 z-10 bg-background">
                <Suspense
                  fallback={
                    <div className="flex h-full items-center justify-center">
                      <Spinner />
                    </div>
                  }
                >
                  <GraphView
                    paperId={paperId}
                    onOpenConcept={(node) => {
                      setPaneMode("pdf");
                      const target = node.object_ids[0];
                      if (target) {
                        // Layout is virtualized; give the PDF pane a frame to
                        // mount before scrolling (still well under 300 ms).
                        window.setTimeout(() => goToObject(target), 30);
                      }
                    }}
                  />
                </Suspense>
              </div>
            )}
            {paneMode === "experiments" && (
              <div className="absolute inset-0 z-10 bg-background">
                <Suspense
                  fallback={
                    <div className="flex h-full items-center justify-center">
                      <Spinner />
                    </div>
                  }
                >
                  <ExperimentWorkbench
                    paperId={paperId}
                    labelFor={(id) =>
                      tree?.objects.find((o) => o.id === id)?.semantic_label ??
                      tree?.objects.find((o) => o.id === id)?.content.text.slice(0, 50)
                    }
                  />
                </Suspense>
              </div>
            )}
            {paneMode === "extend" && (
              <div className="absolute inset-0 z-10 bg-background">
                <Suspense
                  fallback={
                    <div className="flex h-full items-center justify-center">
                      <Spinner />
                    </div>
                  }
                >
                  <ExtensionMode
                    paperId={paperId}
                    labelFor={(id) =>
                      tree?.objects.find((o) => o.id === id)?.semantic_label ??
                      tree?.objects.find((o) => o.id === id)?.content.text.slice(0, 50)
                    }
                    onNavigateObject={(objectId) => {
                      setPaneMode("pdf");
                      window.setTimeout(() => goToObject(objectId), 30);
                    }}
                  />
                </Suspense>
              </div>
            )}
            {paneMode === "repro" && (
              <div className="absolute inset-0 z-10 bg-background">
                <Suspense
                  fallback={
                    <div className="flex h-full items-center justify-center">
                      <Spinner />
                    </div>
                  }
                >
                  <ReproductionPane
                    paperId={paperId}
                    target={codeTarget}
                    labelFor={(id) =>
                      tree?.objects.find((o) => o.id === id)?.semantic_label ??
                      tree?.objects.find((o) => o.id === id)?.content.text.slice(0, 50)
                    }
                    onNavigateObject={(objectId) => {
                      setPaneMode("pdf");
                      window.setTimeout(() => goToObject(objectId), 30);
                    }}
                  />
                </Suspense>
              </div>
            )}
            {paneMode === "community" && (
              <div className="absolute inset-0 z-10 bg-background">
                <Suspense
                  fallback={
                    <div className="flex h-full items-center justify-center">
                      <Spinner />
                    </div>
                  }
                >
                  <CommunityPane paperId={paperId} />
                </Suspense>
              </div>
            )}
            {paneMode === "plugins" && (
              <div className="absolute inset-0 z-10 bg-background">
                <Suspense
                  fallback={
                    <div className="flex h-full items-center justify-center">
                      <Spinner />
                    </div>
                  }
                >
                  <PluginPane paperId={paperId} />
                </Suspense>
              </div>
            )}
            {paneMode === "lessons" && (
              <div className="absolute inset-0 z-10 bg-background">
                <ReadingMode
                  paperId={paperId}
                  labelFor={(id) =>
                    tree?.objects.find((o) => o.id === id)?.semantic_label ??
                    tree?.objects.find((o) => o.id === id)?.content.text.slice(0, 50)
                  }
                  notes={notes}
                  onEscapeToObject={(objectId) => {
                    // Escape to the paper (<300 ms); the lesson cursor is
                    // persisted, so toggling back resumes the same step.
                    setPaneMode("pdf");
                    window.setTimeout(() => goToObject(objectId), 30);
                  }}
                  onNavigateObject={(objectId) => {
                    setPaneMode("pdf");
                    window.setTimeout(() => goToObject(objectId), 30);
                  }}
                  onHighlight={async (text, anchorObjectId) => {
                    const anchor = tree?.objects.find((o) => o.id === anchorObjectId);
                    if (!anchor) return;
                    await invoke("note_save", {
                      paperId,
                      noteId: crypto.randomUUID(),
                      objectId: anchorObjectId,
                      anchorHash: anchor.content_hash,
                      markdown: "> " + text,
                    }).catch(() => {});
                    refreshAnnotations();
                  }}
                  onQuote={(text) =>
                    setSelected({
                      kind: "ad-hoc",
                      selection: {
                        id: crypto.randomUUID(),
                        type: "selection",
                        text,
                        regions: [],
                      },
                    })
                  }
                />
              </div>
            )}
            <div
              className={
                "h-full overflow-y-auto bg-muted/60" +
                (regionMode ? " region-mode" : "") +
                (inkTool ? " draw-mode" : "")
              }
              ref={scrollRef}
              onScroll={() => {
                onScroll();
                persistState();
              }}
              onMouseDown={beginMarquee}
              onMouseUp={marquee || regionMode ? undefined : onMouseUp}
            >
              <div
                className="relative mx-auto w-fit min-w-96 pt-4"
                style={{ height: totalHeight }}
              >
                {doc &&
                  slots.slice(visible[0], visible[1] + 1).map((slot) => (
                    <PageView
                      key={slot.index}
                      doc={doc}
                      slot={slot}
                      paperId={paperId}
                      scale={scale}
                      objects={rawView ? [] : (objectsByPage.get(slot.index) ?? [])}
                      citations={rawView ? null : citations}
                      notedIds={notedIds}
                      selected={selected}
                      flashId={flashId}
                      marquee={marquee?.page === slot.index ? marquee : null}
                      ink={
                        <InkLayer
                          page={slot.index}
                          scale={scale}
                          strokes={inkStrokes}
                          tool={inkTool}
                          color={inkColor}
                          onCommit={commitStroke}
                          onErase={eraseStroke}
                        />
                      }
                      onSelect={(object) => setSelected({ kind: "object", object })}
                    />
                  ))}
              </div>
            </div>
          </ResizablePanel>
          {/* The object panel accompanies the PDF and reading mode (quote in
              chat); canvas/graph and other panes keep their full width. */}
          {(paneMode === "pdf" || paneMode === "lessons") && selected && (
            <>
              <ResizableHandle withHandle />
              <ResizablePanel defaultSize="32%" minSize="300px" maxSize="55%">
                <ObjectPanel
                  paperId={paperId}
                  selection={selected}
                  tree={tree}
                  notes={notes}
                  bookmarks={bookmarks}
                  onAnnotationsChanged={refreshAnnotations}
                  onNavigate={goToObject}
                  onClose={() => setSelected(null)}
                  onOpenPaper={onOpenPaper}
                  onShowInCode={(file, line) => {
                    setCodeTarget({ file, line });
                    setPaneMode("repro");
                  }}
                />
              </ResizablePanel>
            </>
          )}
        </ResizablePanelGroup>
      </div>
    </div>
  );
}

function ReaderBar({
  onBack,
  rawView,
  onToggleRaw,
  hasObjectLayer,
  onOpenSearch,
  regionMode,
  onToggleRegionMode,
  zoom,
  onZoom,
  inkTool,
  onToggleInk,
  annotations,
  paneMode,
  onToggleGraph,
  onToggleLessons,
  onToggleExperiments,
  onToggleRepro,
  onToggleExtend,
  onTogglePlugins,
  onToggleCommunity,
  onOpenMarkdown,
}: {
  onBack: () => void;
  rawView: boolean;
  onToggleRaw: () => void;
  hasObjectLayer: boolean;
  onOpenSearch?: () => void;
  regionMode?: boolean;
  onToggleRegionMode?: () => void;
  zoom?: number;
  onZoom?: (zoom: number) => void;
  inkTool?: InkTool | null;
  onToggleInk?: () => void;
  annotations?: React.ReactNode;
  paneMode?: "pdf" | "graph" | "lessons" | "experiments" | "repro" | "extend" | "plugins" | "community";
  onToggleGraph?: () => void;
  onToggleLessons?: () => void;
  onToggleExperiments?: () => void;
  onToggleRepro?: () => void;
  onToggleExtend?: () => void;
  onTogglePlugins?: () => void;
  onToggleCommunity?: () => void;
  /** Open the parsed-markdown view of the paper. */
  onOpenMarkdown?: () => void;
}) {
  // The dock can block the chat composer — it's draggable by its grip
  // (double-click the grip to reset), position persisted per machine.
  const [dockOffset, setDockOffset] = useState<{ x: number; y: number }>(() => {
    try {
      return JSON.parse(localStorage.getItem("reader-dock-offset") ?? "") as {
        x: number;
        y: number;
      };
    } catch {
      return { x: 0, y: 0 };
    }
  });
  const dragState = useRef<{ startX: number; startY: number; baseX: number; baseY: number } | null>(
    null,
  );
  // Motion's layout projection and text selection both fight the drag:
  // disable them for its duration, animate only collapse/expand.
  const [draggingDock, setDraggingDock] = useState(false);
  function onGripPointerDown(event: React.PointerEvent) {
    event.preventDefault();
    (event.target as HTMLElement).setPointerCapture(event.pointerId);
    dragState.current = {
      startX: event.clientX,
      startY: event.clientY,
      baseX: dockOffset.x,
      baseY: dockOffset.y,
    };
    setDraggingDock(true);
    document.body.style.userSelect = "none";
  }
  function onGripPointerMove(event: React.PointerEvent) {
    const drag = dragState.current;
    if (!drag) return;
    event.preventDefault();
    setDockOffset({
      x: drag.baseX + (event.clientX - drag.startX),
      y: Math.min(0, drag.baseY + (event.clientY - drag.startY)),
    });
  }
  function onGripPointerUp() {
    dragState.current = null;
    setDraggingDock(false);
    document.body.style.userSelect = "";
    localStorage.setItem("reader-dock-offset", JSON.stringify(dockOffset));
  }

  // Collapsed, the dock shrinks to the grip + an expand chevron — the PDF
  // gets the space back. Persisted per machine, like the drag offset.
  const [dockCollapsed, setDockCollapsed] = useState(
    () => localStorage.getItem("reader-dock-collapsed") === "1",
  );
  function toggleDockCollapsed() {
    setDockCollapsed((collapsed) => {
      localStorage.setItem("reader-dock-collapsed", collapsed ? "0" : "1");
      return !collapsed;
    });
  }

  return (
    <>
      {/* No toolbar: window dragging is handled by an overlay strip inside
          the PDF pane (see Reader) and by the object panel's header. */}
      <div
        className="pointer-events-none fixed inset-x-0 bottom-4 z-20 flex justify-center"
        style={{ transform: "translate(" + dockOffset.x + "px, " + dockOffset.y + "px)" }}
      >
        <Dock
          className="pointer-events-auto mt-0 bg-background/80"
          iconMagnification={52}
          iconSize={36}
          layout={!draggingDock}
        >
          <div
            className="flex cursor-grab touch-none items-center self-stretch px-0.5 select-none active:cursor-grabbing"
            title="Drag to move the toolbar (double-click to reset)"
            onPointerDown={onGripPointerDown}
            onPointerMove={onGripPointerMove}
            onPointerUp={onGripPointerUp}
            onPointerCancel={onGripPointerUp}
            onDoubleClick={() => {
              setDockOffset({ x: 0, y: 0 });
              localStorage.setItem("reader-dock-offset", JSON.stringify({ x: 0, y: 0 }));
            }}
          >
            <GripVerticalIcon className="text-muted-foreground size-4" />
          </div>
          {!dockCollapsed && (
          <div className="contents *:animate-in *:fade-in *:zoom-in-75 *:duration-300">
          <DockIcon>
            <DockTip label="Back to library">
              <Button variant="ghost" size="icon" className="size-full" onClick={onBack}>
                <HomeIcon />
              </Button>
            </DockTip>
          </DockIcon>
          <DockGroupSeparator />
          {onToggleRegionMode && (
            <DockIcon>
              <DockTip label="Select a region — drag a rectangle over anything (⌥+drag)">
                <Button
                  variant={regionMode ? "secondary" : "ghost"}
                  size="icon"
                  className="size-full"
                  onClick={onToggleRegionMode}
                >
                  <SquareDashedMousePointerIcon />
                </Button>
              </DockTip>
            </DockIcon>
          )}
          {zoom !== undefined && onZoom && (
            <>
              <DockIcon>
                <DockTip label="Zoom out">
                  <Button
                    variant="ghost"
                    size="icon"
                    className="size-full"
                    disabled={zoom <= MIN_ZOOM}
                    onClick={() => onZoom(zoom - 0.15)}
                  >
                    <ZoomOutIcon />
                  </Button>
                </DockTip>
              </DockIcon>
              <DockIcon>
                <DockTip label={`Zoom in (${Math.round(zoom * 100)}%)`}>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="size-full"
                    disabled={zoom >= MAX_ZOOM}
                    onClick={() => onZoom(zoom + 0.15)}
                  >
                    <ZoomInIcon />
                  </Button>
                </DockTip>
              </DockIcon>
            </>
          )}
          {onToggleInk && (
            <DockIcon>
              <DockTip label={inkTool ? "Stop drawing (Esc)" : "Draw — pen, highlighter, eraser"}>
                <Button
                  variant={inkTool ? "secondary" : "ghost"}
                  size="icon"
                  className="size-full"
                  onClick={onToggleInk}
                >
                  <PenLineIcon />
                </Button>
              </DockTip>
            </DockIcon>
          )}
          {annotations && <DockIcon>{annotations}</DockIcon>}
          {onOpenSearch && (
            <DockIcon>
              <DockTip label="Search in paper (⌘F)">
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-full"
                  onClick={onOpenSearch}
                >
                  <SearchIcon />
                </Button>
              </DockTip>
            </DockIcon>
          )}
          <DockIcon>
            <DockTip label={rawView ? "Raw view (object layer off)" : "Interactive view"}>
              <Button
                variant={rawView ? "secondary" : "ghost"}
                size="icon"
                className="size-full"
                onClick={onToggleRaw}
                disabled={!hasObjectLayer && !rawView}
              >
                {rawView ? <FileTextIcon /> : <ScanTextIcon />}
              </Button>
            </DockTip>
          </DockIcon>
          {onOpenMarkdown && (
            <DockIcon>
              <DockTip label="View as Markdown — the parsed paper, refinable with AI">
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-full"
                  onClick={onOpenMarkdown}
                >
                  <TextIcon />
                </Button>
              </DockTip>
            </DockIcon>
          )}
          <DockGroupSeparator />
          {onToggleGraph && (
            <DockIcon>
              <DockTip label={paneMode === "graph" ? "Back to the paper" : "Concept map"}>
                <Button
                  variant={paneMode === "graph" ? "secondary" : "ghost"}
                  size="icon"
                  className="size-full"
                  onClick={onToggleGraph}
                >
                  <WaypointsIcon />
                </Button>
              </DockTip>
            </DockIcon>
          )}
          {onToggleExperiments && (
            <DockIcon>
              <DockTip
                label={paneMode === "experiments" ? "Back to the paper" : "Experiments — run and tweak"}
              >
                <Button
                  variant={paneMode === "experiments" ? "secondary" : "ghost"}
                  size="icon"
                  className="size-full"
                  onClick={onToggleExperiments}
                >
                  <FlaskConicalIcon />
                </Button>
              </DockTip>
            </DockIcon>
          )}
          {onToggleExtend && (
            <DockIcon>
              <DockTip
                label={paneMode === "extend" ? "Back to the paper" : "Extend — hypotheses to draft"}
              >
                <Button
                  variant={paneMode === "extend" ? "secondary" : "ghost"}
                  size="icon"
                  className="size-full"
                  onClick={onToggleExtend}
                >
                  <LightbulbIcon />
                </Button>
              </DockTip>
            </DockIcon>
          )}
          {onToggleCommunity && (
            <DockIcon>
              <DockTip
                label={paneMode === "community" ? "Back to the paper" : "Community — registry, proposals"}
              >
                <Button
                  variant={paneMode === "community" ? "secondary" : "ghost"}
                  size="icon"
                  className="size-full"
                  onClick={onToggleCommunity}
                >
                  <UsersIcon />
                </Button>
              </DockTip>
            </DockIcon>
          )}
          {onTogglePlugins && (
            <DockIcon>
              <DockTip
                label={paneMode === "plugins" ? "Back to the paper" : "Plugins — export, panels"}
              >
                <Button
                  variant={paneMode === "plugins" ? "secondary" : "ghost"}
                  size="icon"
                  className="size-full"
                  onClick={onTogglePlugins}
                >
                  <PuzzleIcon />
                </Button>
              </DockTip>
            </DockIcon>
          )}
          {onToggleRepro && (
            <DockIcon>
              <DockTip
                label={paneMode === "repro" ? "Back to the paper" : "Reproduce — repo, runs, report"}
              >
                <Button
                  variant={paneMode === "repro" ? "secondary" : "ghost"}
                  size="icon"
                  className="size-full"
                  onClick={onToggleRepro}
                >
                  <FolderGit2Icon />
                </Button>
              </DockTip>
            </DockIcon>
          )}
          {onToggleLessons && (
            <DockIcon>
              <DockTip
                label={paneMode === "lessons" ? "Back to the paper" : "Reading mode — learn as a course"}
              >
                <Button
                  variant={paneMode === "lessons" ? "secondary" : "ghost"}
                  size="icon"
                  className="size-full"
                  onClick={onToggleLessons}
                >
                  <GraduationCapIcon />
                </Button>
              </DockTip>
            </DockIcon>
          )}
          </div>
          )}
          <DockIcon>
            <DockTip label={dockCollapsed ? "Expand toolbar" : "Collapse toolbar"}>
              <Button
                variant="ghost"
                size="icon"
                className="size-full"
                onClick={toggleDockCollapsed}
              >
                {dockCollapsed ? <ChevronsRightIcon /> : <ChevronsLeftIcon />}
              </Button>
            </DockTip>
          </DockIcon>
        </Dock>
      </div>
    </>
  );
}

/** Thin divider between dock groups (navigation · PDF tools · research). */
function DockGroupSeparator() {
  return <div aria-hidden className="mx-0.5 h-6 w-px self-center bg-border" />;
}

/** Floating ink tool palette (pen / highlighter / eraser + colors). */
function InkPalette({
  tool,
  color,
  onTool,
  onColor,
}: {
  tool: InkTool;
  color: string;
  onTool: (tool: InkTool) => void;
  onColor: (color: string) => void;
}) {
  return (
    <div className="pointer-events-none fixed inset-x-0 top-12 z-20 flex justify-center">
      <div className="pointer-events-auto flex items-center gap-1 rounded-full border bg-background/90 px-2 py-1 shadow-md backdrop-blur-md">
        {(
          [
            ["pen", PenLineIcon, "Pen"],
            ["highlighter", HighlighterIcon, "Highlighter"],
            ["eraser", EraserIcon, "Eraser"],
          ] as const
        ).map(([id, Icon, label]) => (
          <DockTip key={id} label={label}>
            <Button
              variant={tool === id ? "secondary" : "ghost"}
              size="icon-sm"
              onClick={() => onTool(id)}
            >
              <Icon />
            </Button>
          </DockTip>
        ))}
        <div className="mx-1 h-5 w-px bg-border" />
        {INK_COLORS.map((c) => (
          <button
            key={c}
            className={
              "size-5 cursor-pointer rounded-full border-2 transition-transform " +
              (color === c ? "scale-110 border-foreground" : "border-transparent")
            }
            style={{ background: c }}
            onClick={() => onColor(c)}
            aria-label={`Ink color ${c}`}
          />
        ))}
      </div>
    </div>
  );
}

/** Dock icon tooltip (shadcn Tooltip, shown above the dock). */
function DockTip({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent side="top" sideOffset={10}>
        {label}
      </TooltipContent>
    </Tooltip>
  );
}

/** In-paper search: exact + semantic, debounced, offline. */
function SearchPanel({
  paperId,
  onNavigate,
  onClose,
}: {
  paperId: string;
  onNavigate: (objectId: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResults | null>(null);
  const debounce = useRef<number | undefined>(undefined);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    window.clearTimeout(debounce.current);
    if (query.trim().length < 2) {
      setResults(null);
      return;
    }
    debounce.current = window.setTimeout(() => {
      invoke<SearchResults>("search_paper", { id: paperId, query: query.trim() })
        .then(setResults)
        .catch(() => setResults(null));
    }, 150);
  }, [query, paperId]);

  const hits = results
    ? [
        ...results.exact.map((h) => ({ ...h, kind: "exact" as const })),
        ...results.semantic
          .filter((s) => !results.exact.some((e) => e.object_id === s.object_id))
          .map((h) => ({ ...h, kind: "semantic" as const })),
      ]
    : [];

  return (
    <div className="flex flex-none flex-col gap-2 border-b py-2 pl-20 pr-4">
      <div className="flex items-center gap-2">
        <Input
          ref={inputRef}
          placeholder="Search this paper (exact + semantic)…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <Button variant="ghost" size="icon-sm" onClick={onClose} title="Close search">
          <XIcon />
        </Button>
      </div>
      {results && (
        <div className="flex max-h-64 flex-col gap-1 overflow-y-auto">
          {hits.length === 0 && (
            <span className="px-1 py-2 text-sm text-muted-foreground">No matches.</span>
          )}
          {hits.map((hit) => (
            <button
              key={`${hit.kind}-${hit.object_id}`}
              className="flex cursor-pointer items-baseline gap-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-accent"
              onClick={() => onNavigate(hit.object_id)}
            >
              <Badge variant={hit.kind === "exact" ? "secondary" : "outline"}>
                {hit.kind}
              </Badge>
              <span className="min-w-0 flex-1 truncate text-muted-foreground">
                {hit.snippet}
              </span>
            </button>
          ))}
          {!results.semantic_available && (
            <span className="px-1 pb-1 text-xs text-muted-foreground">
              Semantic search will be available once this paper finishes indexing.
            </span>
          )}
        </div>
      )}
    </div>
  );
}

function PageView({
  doc,
  slot,
  paperId,
  scale,
  objects,
  citations,
  notedIds,
  selected,
  flashId,
  marquee,
  ink,
  onSelect,
}: {
  doc: PDFDocumentProxy;
  slot: PageSlot;
  paperId: string;
  scale: number;
  objects: { object: PaperObject; regionIndex: number }[];
  citations: CitationsDocument | null;
  notedIds: Set<string>;
  selected: Selection | null;
  flashId: string | null;
  marquee: { x0: number; y0: number; x1: number; y1: number } | null;
  ink?: React.ReactNode;
  onSelect: (object: PaperObject) => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const textLayerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    let renderTask: ReturnType<import("pdfjs-dist").PDFPageProxy["render"]> | null = null;
    let textLayer: pdfjs.TextLayer | null = null;
    (async () => {
      const page = await doc.getPage(slot.index + 1);
      if (cancelled) return;
      const canvas = canvasRef.current;
      if (!canvas) return;
      const dpr = window.devicePixelRatio || 1;
      const viewport = page.getViewport({ scale: scale * dpr });
      canvas.width = viewport.width;
      canvas.height = viewport.height;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      renderTask = page.render({ canvas, canvasContext: ctx, viewport });
      try {
        await renderTask.promise;
      } catch {
        // Cancelled mid-render on scroll-away — expected.
      }

      // Text layer at CSS scale for native selection.
      const container = textLayerRef.current;
      if (!container || cancelled) return;
      container.replaceChildren();
      container.style.setProperty("--scale-factor", String(scale));
      textLayer = new pdfjs.TextLayer({
        textContentSource: page.streamTextContent(),
        container,
        viewport: page.getViewport({ scale }),
      });
      try {
        await textLayer.render();
      } catch {
        // Cancelled — expected.
      }
    })();
    return () => {
      cancelled = true;
      renderTask?.cancel();
      textLayer?.cancel();
    };
  }, [doc, slot.index, scale]);

  return (
    <div
      className="page-slot"
      data-page-slot={slot.index}
      style={{ top: slot.top, width: slot.width, height: slot.height }}
    >
      <canvas ref={canvasRef} style={{ width: slot.width, height: slot.height }} />
      <div ref={textLayerRef} className="textLayer" />
      <CitationTargets
        page={slot.index}
        scale={scale}
        citations={citations}
        sourcePaperId={paperId}
      />
      {ink}
      {/* Live marquee while dragging a region selection. */}
      {marquee && (
        <div
          className="marquee-rect"
          style={{
            left: Math.min(marquee.x0, marquee.x1),
            top: Math.min(marquee.y0, marquee.y1),
            width: Math.abs(marquee.x1 - marquee.x0),
            height: Math.abs(marquee.y1 - marquee.y0),
          }}
        />
      )}
      {/* Persistent highlight for the active ad-hoc region selection. */}
      {selected?.kind === "ad-hoc" &&
        selected.selection.regions
          .filter((r) => r.page === slot.index)
          .map((r, i) => (
            <div
              key={i}
              className="adhoc-region"
              style={{
                left: r.x * scale,
                top: r.y * scale,
                width: r.width * scale,
                height: r.height * scale,
              }}
            />
          ))}
      {/* Object overlay: transparent hover/click targets from extracted regions. */}
      {objects.map(({ object, regionIndex }) => {
        const region = object.regions![regionIndex];
        const isSelected =
          selected?.kind === "object" && selected.object.id === object.id;
        return (
          <button
            key={`${object.id}-${regionIndex}`}
            className={
              "object-target" +
              (isSelected ? " object-target-selected" : "") +
              (flashId === object.id ? " object-target-flash" : "") +
              (notedIds.has(object.id) ? " object-target-noted" : "") +
              (object.confidence < 0.7 ? " object-target-low-confidence" : "")
            }
            style={{
              left: region.x * scale,
              top: region.y * scale,
              width: region.width * scale,
              height: region.height * scale,
            }}
            title={object.semantic_label ?? object.type}
            onClick={() => onSelect(object)}
          />
        );
      })}
    </div>
  );
}
