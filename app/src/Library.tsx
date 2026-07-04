import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, onFileDrop, platform } from "@/platform";
import { listen } from "@/platform";
import { openFileDialog } from "@/platform";
import {
  CheckIcon,
  ChevronDownIcon,
  CircleIcon,
  EllipsisVerticalIcon,
  FileCode2Icon,
  FileTextIcon,
  FolderOpenIcon,
  FrameIcon,
  ImportIcon,
  LinkIcon,
  ListChecksIcon,
  MessagesSquareIcon,
  NotebookTextIcon,
  RefreshCwIcon,
  StarIcon,
  TelescopeIcon,
  Trash2Icon,
  TriangleAlertIcon,
  XIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ButtonGroup } from "@/components/ui/button-group";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Spinner } from "@/components/ui/spinner";
import PaperMarkdownDialog from "./PaperMarkdownDialog";
import Settings from "./Settings";
import ThemeToggle from "./chrome/ThemeToggle";
import ImportArxivDialog from "./chrome/ImportArxivDialog";
import { OmnibarTrigger } from "./chrome/Omnibar";
import type {
  IngestionProgress,
  PaperSummary,
  PipelineStage,
  WorkspaceItem,
} from "./types";

const STAGE_LABEL: Record<PipelineStage, string> = {
  layout: "Analyzing layout",
  objects: "Extracting objects",
  enrichment: "Parsing equations, figures, citations",
  concepts: "Building concept map",
  embeddings: "Building search index",
};

const STAGE_ORDER: PipelineStage[] = [
  "layout",
  "objects",
  "enrichment",
  "concepts",
  "embeddings",
];

/** One stage's state as the import progress panel shows it. */
interface StageState {
  status: string; // pending | running | complete | skipped | failed | …
  reason?: string;
  progress?: string; // "12/340" during long stages
}

type PaperStages = Partial<Record<PipelineStage, StageState>>;

const PRIORITIES = ["high", "medium", "low"] as const;

// ---- Unified library: one recency-sorted list of all content kinds ----

type LibraryFilter = "all" | "research" | "note" | "canvas" | "chat";

const FILTERS: { id: LibraryFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "research", label: "Research" },
  { id: "note", label: "Notes" },
  { id: "canvas", label: "Canvases" },
  { id: "chat", label: "Threads" },
];

const KIND_LABEL: Record<string, string> = {
  note: "Note",
  canvas: "Canvas",
  chat: "Thread",
};

function kindIcon(kind: string) {
  if (kind === "note") return NotebookTextIcon;
  if (kind === "canvas") return FrameIcon;
  if (kind === "chat") return MessagesSquareIcon;
  return FileTextIcon;
}

type LibraryEntry =
  | { type: "paper"; recency: string; paper: PaperSummary }
  | { type: "item"; recency: string; item: WorkspaceItem };

/** Reading-priority indicator colors: urgency reads at a glance. */
const PRIORITY_COLOR: Record<string, string> = {
  high: "#e5484d",
  medium: "#f5a623",
  low: "#006bff",
};

function PriorityDot({ priority }: { priority: string }) {
  return (
    <span
      aria-hidden
      className="inline-block size-2 rounded-full"
      style={{ backgroundColor: PRIORITY_COLOR[priority] ?? "#8e8e93" }}
    />
  );
}

function formatDate(iso?: string) {
  if (!iso) return null;
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

/** One library entry: content-only card body; actions appear on hover as a
 * separate stack sliding out of the card's right edge. */
function StageStatusIcon({ status }: { status: string }) {
  if (status === "running") return <Spinner className="size-3.5" />;
  if (status === "complete" || status === "skipped")
    return <CheckIcon className="size-3.5 text-primary" />;
  if (status === "failed") return <XIcon className="size-3.5 text-destructive" />;
  if (status === "degraded" || status === "partial")
    return <TriangleAlertIcon className="size-3.5 text-amber-500" />;
  return <CircleIcon className="size-3.5 text-muted-foreground/50" />;
}

/** Geist-style status dot: color says it all, hover tells the story. */
function PaperStatusDot({
  paper,
  stageLabel,
}: {
  paper: PaperSummary;
  stageLabel?: string;
}) {
  let color = "#50e3c2"; // ready — mint, distinct from the accent greens
  let label = "Ready";
  let pulse = false;
  if (paper.status === "failed") {
    color = "#e5484d";
    label = "Error — raw view only";
  } else if (paper.status === "degraded") {
    color = "#f5a623";
    label = "Ready (some limitations)";
  } else if (paper.status === "processing") {
    if (stageLabel) {
      color = "#f5a623";
      label = `Building — ${stageLabel}`;
      pulse = true;
    } else {
      color = "#006bff";
      label = "Queued";
    }
  }
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className={"inline-block size-2 shrink-0 rounded-full" + (pulse ? " animate-pulse" : "")}
          style={{ backgroundColor: color }}
          aria-label={label}
        />
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

/** The per-stage pipeline checklist (shared by the import-status modal). */
function ImportPipelinePanel({
  stages,
  stageLabel,
  onRetry,
}: {
  stages: PaperStages;
  stageLabel?: string;
  onRetry: () => void;
}) {
  return (
    <div className="space-y-2">
      {STAGE_ORDER.map((stage) => {
        const state = stages[stage] ?? { status: "pending" };
        return (
          <div key={stage} className="flex items-center gap-2 text-sm text-muted-foreground">
            <StageStatusIcon status={state.status} />
            <span className="min-w-0 flex-1">
              {STAGE_LABEL[stage]}
              {state.progress ? ` (${state.progress})` : ""}
              {state.reason && (
                <span
                  className="block truncate text-xs text-muted-foreground/80"
                  title={state.reason}
                >
                  {state.reason}
                </span>
              )}
            </span>
            {(state.status === "failed" || state.status === "degraded") && !stageLabel && (
              <Button
                variant="outline"
                size="sm"
                className="h-6 gap-1 px-2 text-xs"
                onClick={onRetry}
              >
                <RefreshCwIcon className="size-3" />
                Re-run
              </Button>
            )}
          </div>
        );
      })}
    </div>
  );
}

function PaperCard({
  paper,
  stageLabel,
  stages,
  onRetry,
  onOpen,
  onDelete,
  onChanged,
}: {
  paper: PaperSummary;
  stageLabel?: string;
  stages?: PaperStages;
  onRetry: () => void;
  onOpen: () => void;
  onDelete: () => void;
  onChanged: () => void;
}) {
  const meta = [
    paper.arxiv_id ? `arXiv:${paper.arxiv_id}` : null,
    paper.doi ? `DOI ${paper.doi}` : null,
    paper.published_at ? `published ${formatDate(paper.published_at)}` : null,
    paper.imported_at ? `added ${formatDate(paper.imported_at)}` : null,
  ].filter(Boolean);
  const [statusOpen, setStatusOpen] = useState(false);
  // Ready papers are never seeded into the live stage map (no need until
  // asked) — fetch the recorded stage states when the modal opens.
  const [fetchedStages, setFetchedStages] = useState<PaperStages | null>(null);
  function openStatus() {
    setStatusOpen(true);
    invoke<{ stage: PipelineStage; status: string; reason?: string }[]>(
      "pipeline_status",
      { paperId: paper.id },
    )
      .then((list) =>
        setFetchedStages(
          Object.fromEntries(
            list.map(({ stage, status, reason }) => [stage, { status, reason }]),
          ),
        ),
      )
      .catch(() => {});
  }
  const [markdownOpen, setMarkdownOpen] = useState(false);

  async function toggleStar() {
    await invoke("paper_toggle_star", { id: paper.id }).catch(() => {});
    onChanged();
  }

  async function setPriority(priority: string | null) {
    await invoke("paper_set_priority", { id: paper.id, priority }).catch(() => {});
    onChanged();
  }

  // Buttons stay visible while the ⋯ menu is open (the pointer leaves the
  // card when the menu renders, which would otherwise hide them mid-use).
  const [menuOpen, setMenuOpen] = useState(false);
  const forced = menuOpen
    ? "pointer-events-auto translate-x-0 scale-100 opacity-100"
    : "pointer-events-none translate-x-[-6px] scale-90 opacity-0 group-hover:pointer-events-auto group-hover:translate-x-0 group-hover:scale-100 group-hover:opacity-100";
  const hoverAction = `transition-all duration-200 ease-out ${forced}`;

  return (
    <li className="group relative rounded-lg border bg-card/90 shadow-xs backdrop-blur-sm">
      <button
        className="flex w-full cursor-pointer flex-col items-start gap-1 rounded-lg px-4 py-3 text-left hover:bg-accent/50"
        onClick={onOpen}
      >
        <span className="flex w-full items-center justify-between gap-2">
          <span className="truncate font-medium">{paper.title}</span>
          <span className="flex shrink-0 items-center gap-1.5">
          <Tooltip>
            <TooltipTrigger asChild>
              <span
                role="button"
                tabIndex={0}
                className={
                  "inline-flex size-7 shrink-0 cursor-pointer items-center justify-center rounded-md hover:bg-accent " +
                  (paper.starred ? "text-amber-400" : "text-muted-foreground/60")
                }
                onClick={(e) => {
                  e.stopPropagation();
                  toggleStar();
                }}
              >
                <StarIcon
                  className="size-4"
                  fill={paper.starred ? "currentColor" : "none"}
                />
              </span>
            </TooltipTrigger>
            <TooltipContent>{paper.starred ? "Unstar" : "Star"}</TooltipContent>
          </Tooltip>
          </span>
        </span>
        {paper.authors.length > 0 && (
          <span className="line-clamp-1 text-sm text-muted-foreground">
            {paper.authors.join(", ")}
          </span>
        )}
        <span className="flex items-center gap-2 text-xs text-muted-foreground/80">
          {meta.length > 0 && <span>{meta.join(" · ")}</span>}
          <PaperStatusDot paper={paper} stageLabel={stageLabel} />
        </span>
        {paper.priority && (
          <span className="mt-0.5 flex items-center gap-1.5">
            <Badge variant="outline" className="gap-1.5">
              <PriorityDot priority={paper.priority} />
              priority: {paper.priority}
            </Badge>
          </span>
        )}
      </button>

      {/* Import status modal (⋯ menu): the per-stage pipeline with re-run. */}
      <Dialog open={statusOpen} onOpenChange={setStatusOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Import status</DialogTitle>
            <DialogDescription className="min-w-0 break-words">
              {paper.title}
            </DialogDescription>
          </DialogHeader>
          <ImportPipelinePanel
            stages={{ ...fetchedStages, ...stages }}
            stageLabel={stageLabel}
            onRetry={onRetry}
          />
        </DialogContent>
      </Dialog>

      {/* Full parsed paper as markdown — shared with the reader toolbar. */}
      <PaperMarkdownDialog
        paperId={paper.id}
        title={paper.title}
        open={markdownOpen}
        onOpenChange={setMarkdownOpen}
      />

      {/* Hover actions: separate icon buttons floating beside the card, with
          clear space between card edge and buttons. */}
      <div className="absolute -right-12 top-1/2 flex -translate-y-1/2 flex-col gap-1.5">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="outline"
              size="icon-sm"
              className={`rounded-full bg-background shadow-sm ${hoverAction}`}
              onClick={() => invoke("reveal_paper", { id: paper.id })}
            >
              <FolderOpenIcon />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="right">Reveal on disk</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="outline"
              size="icon-sm"
              className={`rounded-full bg-background shadow-sm delay-[40ms] ${hoverAction}`}
              onClick={onDelete}
            >
              <Trash2Icon />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="right">Delete from library</TooltipContent>
        </Tooltip>
        <DropdownMenu onOpenChange={setMenuOpen}>
          <Tooltip>
            <TooltipTrigger asChild>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="outline"
                  size="icon-sm"
                  className={`rounded-full bg-background shadow-sm delay-[80ms] ${hoverAction}`}
                >
                  <EllipsisVerticalIcon />
                </Button>
              </DropdownMenuTrigger>
            </TooltipTrigger>
            <TooltipContent side="right">More options</TooltipContent>
          </Tooltip>
          <DropdownMenuContent
            side="right"
            align="start"
            // Returning focus to the trigger would pop its tooltip and pin
            // it until the next blur — don't.
            onCloseAutoFocus={(e) => e.preventDefault()}
          >
            <DropdownMenuItem onClick={openStatus}>
              <ListChecksIcon />
              Import status
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setMarkdownOpen(true)}>
              <FileTextIcon />
              View as Markdown
            </DropdownMenuItem>
            <DropdownMenuLabel>Reading priority</DropdownMenuLabel>
            <DropdownMenuGroup>
              {PRIORITIES.map((priority) => (
                <DropdownMenuItem
                  key={priority}
                  onClick={() => setPriority(priority)}
                >
                  <PriorityDot priority={priority} />
                  {priority}
                  {paper.priority === priority && " ✓"}
                </DropdownMenuItem>
              ))}
              <DropdownMenuItem onClick={() => setPriority(null)}>
                clear
              </DropdownMenuItem>
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </li>
  );
}

/** A workspace entity (note/canvas/thread) in the unified list. Opening
 * routes to its surface once the kind's feature lands. */
function WorkspaceItemCard({
  item,
  onOpen,
  onDelete,
}: {
  item: WorkspaceItem;
  onOpen?: () => void;
  onDelete: () => void;
}) {
  const Icon = kindIcon(item.kind);
  // Canvas cards show a thumbnail — fetched lazily (canvases are few).
  const [thumbnail, setThumbnail] = useState<string>("");
  useEffect(() => {
    if (item.kind !== "canvas") return;
    invoke<{ thumbnail: string } | null>("workspace_canvas_get", { id: item.id })
      .then((doc) => setThumbnail(doc?.thumbnail ?? ""))
      .catch(() => {});
  }, [item.id, item.kind, item.updated_at]);
  return (
    <li className="group relative rounded-lg border bg-card/90 shadow-xs backdrop-blur-sm">
      <div
        className={
          "flex w-full items-center gap-3 rounded-lg px-4 py-3 text-left" +
          (onOpen ? " cursor-pointer hover:bg-accent/50" : "")
        }
        onClick={onOpen}
      >
        {item.kind === "canvas" &&
          (thumbnail ? (
            <img
              src={thumbnail}
              alt=""
              className="h-12 w-16 shrink-0 rounded border bg-white object-cover"
            />
          ) : (
            <div className="flex h-12 w-16 shrink-0 items-center justify-center rounded border bg-muted">
              <Icon className="size-5 text-muted-foreground/60" />
            </div>
          ))}
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <span className="flex w-full items-center gap-2">
            <Icon className="size-4 shrink-0 text-muted-foreground" />
            <span className="truncate font-medium">{item.title}</span>
            <Badge variant="outline" className="ml-auto shrink-0">
              {KIND_LABEL[item.kind] ?? item.kind}
            </Badge>
          </span>
          <span className="text-xs text-muted-foreground/80">
            updated {formatDate(item.updated_at)}
          </span>
        </div>
      </div>
      <div className="absolute -right-12 top-1/2 flex -translate-y-1/2 flex-col gap-1.5">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="outline"
              size="icon-sm"
              className="pointer-events-none rounded-full bg-background opacity-0 shadow-sm transition-all group-hover:pointer-events-auto group-hover:opacity-100"
              onClick={onDelete}
            >
              <Trash2Icon />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="right">Delete</TooltipContent>
        </Tooltip>
      </div>
    </li>
  );
}

export default function Library({
  onOpen,
  onOpenResearch,
  onOpenNote,
  onOpenCanvas,
  onOpenChat,
}: {
  onOpen: (id: string, title?: string) => void;
  onOpenResearch?: () => void;
  onOpenNote?: (id: string) => void;
  onOpenCanvas?: (id: string) => void;
  onOpenChat?: (id: string) => void;
}) {
  const [papers, setPapers] = useState<PaperSummary[]>([]);
  const [activeStages, setActiveStages] = useState<Record<string, string>>({});
  // Per-paper, per-stage pipeline state: seeded from persisted metadata
  // (pipeline_status) so it survives view switches, overlaid live by events.
  const [paperStages, setPaperStages] = useState<Record<string, PaperStages>>({});
  const [arxivOpen, setArxivOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<
    { kind: "paper" | "item"; id: string; title: string } | null
  >(null);
  const [deleteConfirmText, setDeleteConfirmText] = useState("");
  // Workspace items (notes/canvases/threads) share the list with papers.
  const [items, setItems] = useState<WorkspaceItem[]>([]);
  const [filter, setFilter] = useState<LibraryFilter>(() => {
    const saved = localStorage.getItem("library-filter");
    return FILTERS.some((f) => f.id === saved) ? (saved as LibraryFilter) : "all";
  });
  useEffect(() => {
    localStorage.setItem("library-filter", filter);
  }, [filter]);
  const refreshTimer = useRef<number | undefined>(undefined);

  const refresh = useCallback(() => {
    invoke<PaperSummary[]>("list_papers")
      .then(setPapers)
      // On web (pre-bridge) the list simply isn't there yet — the empty
      // state + capability matrix carry the messaging; the error alert is
      // reserved for actual import failures.
      .catch((e) => {
        if (platform === "desktop") setError(String(e));
      });
    invoke<WorkspaceItem[]>("workspace_items_list", {})
      .then(setItems)
      .catch(() => setItems([]));
  }, []);

  useEffect(() => {
    refresh();
    const setStage = (paperId: string, stage: PipelineStage, state: StageState) =>
      setPaperStages((all) => ({
        ...all,
        [paperId]: { ...all[paperId], [stage]: state },
      }));
    const unlisten = listen<IngestionProgress>("ingestion-progress", ({ payload }) => {
      const { paper_id, event } = payload;
      if (event.kind === "stage_started") {
        setActiveStages((s) => ({ ...s, [paper_id]: STAGE_LABEL[event.stage] }));
        setStage(paper_id, event.stage, { status: "running" });
      }
      // Intra-stage progress (embeddings over many objects): append a
      // (done/total) count so the card never looks frozen mid-stage.
      if (event.kind === "stage_progress") {
        setActiveStages((s) => ({
          ...s,
          [paper_id]: `${STAGE_LABEL[event.stage]} (${event.done}/${event.total})`,
        }));
        setStage(paper_id, event.stage, {
          status: "running",
          progress: `${event.done}/${event.total}`,
        });
      }
      if (event.kind === "stage_completed" || event.kind === "stage_skipped") {
        setStage(paper_id, event.stage, { status: "complete" });
      }
      if (event.kind === "stage_degraded") {
        setStage(paper_id, event.stage, { status: "degraded", reason: event.reason });
      }
      if (event.kind === "stage_failed") {
        setStage(paper_id, event.stage, { status: "failed", reason: event.reason });
      }
      if (event.kind === "pipeline_finished") {
        setActiveStages((s) => {
          const next = { ...s };
          delete next[paper_id];
          return next;
        });
      }
      // Coalesce refreshes; events arrive in bursts.
      window.clearTimeout(refreshTimer.current);
      refreshTimer.current = window.setTimeout(refresh, 200);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [refresh]);

  // Seed persisted stage state for papers whose import isn't clean-finished,
  // so returning to the library shows where the pipeline stands. Live events
  // may land before (or while) the fetch resolves — they always win the
  // merge, and the fetch runs once per paper regardless (a mid-run progress
  // event must not suppress seeding the earlier stages' recorded states).
  const seeded = useRef(new Set<string>());
  useEffect(() => {
    for (const paper of papers) {
      if (paper.status === "ready" || seeded.current.has(paper.id)) continue;
      seeded.current.add(paper.id);
      invoke<{ stage: PipelineStage; status: string; reason?: string }[]>(
        "pipeline_status",
        { paperId: paper.id },
      )
        .then((stages) =>
          setPaperStages((all) => ({
            ...all,
            [paper.id]: {
              ...Object.fromEntries(
                stages.map(({ stage, status, reason }) => [stage, { status, reason }]),
              ),
              ...all[paper.id],
            },
          })),
        )
        .catch(() => {
          // Let a later pass retry (bundle may be mid-create).
          seeded.current.delete(paper.id);
        });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [papers]);

  // OS-level file drag-and-drop (desktop webview file drop, not HTML5 DnD).
  useEffect(() => {
    const unlisten = onFileDrop({
      onOver: () => setDragOver(true),
      onLeave: () => setDragOver(false),
      onDrop: (paths) => {
        setDragOver(false);
        for (const path of paths) {
          if (path.toLowerCase().endsWith(".pdf")) importFile(path);
        }
      },
    });
    return () => {
      unlisten.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function importFile(path: string) {
    setError(null);
    try {
      await invoke<string>("import_pdf_file", { path });
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function pickAndImportLatex() {
    const selected = await openFileDialog({
      multiple: false,
      filters: [{ name: "LaTeX", extensions: ["tex"] }],
    });
    if (typeof selected !== "string") return;
    setError(null);
    try {
      await invoke<string>("import_latex", { path: selected });
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function pickAndImport() {
    const selected = await openFileDialog({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (typeof selected === "string") await importFile(selected);
  }

  async function createNote() {
    try {
      const item = await invoke<WorkspaceItem>("workspace_note_create", {});
      refresh();
      onOpenNote?.(item.id);
    } catch (e) {
      setError(String(e));
    }
  }

  async function createCanvas() {
    try {
      const item = await invoke<WorkspaceItem>("workspace_canvas_create", {});
      refresh();
      onOpenCanvas?.(item.id);
    } catch (e) {
      setError(String(e));
    }
  }

  async function createChat() {
    try {
      const item = await invoke<WorkspaceItem>("workspace_chat_create", {});
      refresh();
      onOpenChat?.(item.id);
    } catch (e) {
      setError(String(e));
    }
  }

  async function confirmDelete() {
    if (!pendingDelete) return;
    try {
      if (pendingDelete.kind === "paper") {
        await invoke("delete_paper", { id: pendingDelete.id });
      } else {
        await invoke("workspace_item_delete", { id: pendingDelete.id });
      }
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setPendingDelete(null);
      setDeleteConfirmText("");
    }
  }

  async function openPaper(paper: PaperSummary) {
    try {
      await invoke("open_paper", { id: paper.id });
    } catch {
      // touch failure is non-fatal
    }
    onOpen(paper.id, paper.title);
  }

  return (
    <div className="relative min-h-screen overflow-hidden">
      {/* Background: CSS grid pattern starting at the top and fading toward
          the bottom, plus a ghost wordmark at the bottom edge. Purely
          decorative. */}
      <div aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="library-grid absolute inset-0" />

        {/* Ghost wordmark, clipped by the bottom edge. */}
        <span className="absolute inset-x-0 -bottom-12 select-none whitespace-nowrap text-center font-heading text-[clamp(6rem,11vw,10rem)] font-extrabold leading-none tracking-tight text-foreground/[0.03]">
          Research Copilot
        </span>
      </div>

      <div
        className={
          "relative mx-auto flex min-h-screen max-w-4xl flex-col gap-6 px-6 py-8" +
          (dragOver ? " ring-2 ring-inset ring-primary" : "")
        }
      >
      <header
        data-tauri-drag-region
        className="flex flex-wrap items-center justify-between gap-4"
      >
        <h1 data-tauri-drag-region className="pl-14 font-heading text-2xl font-semibold">
          Library
        </h1>
        <div className="flex flex-1 items-center justify-end gap-2">
          <OmnibarTrigger className="max-w-xl flex-1 bg-background" />
          {/* Split button: the main segment imports a file directly; the
              chevron opens the full menu (PDF / link / LaTeX). */}
          <ButtonGroup>
            <Button variant="outline" className="bg-background" onClick={pickAndImport}>
              <ImportIcon data-icon="inline-start" />
              Import
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="outline"
                  size="icon"
                  className="bg-background"
                  aria-label="More import options"
                >
                  <ChevronDownIcon />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuGroup>
                  <DropdownMenuItem onClick={pickAndImport}>
                    <FileTextIcon />
                    PDF file
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => setArxivOpen(true)}>
                    <LinkIcon />
                    Link (arXiv / DOI / PDF)
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={pickAndImportLatex}>
                    <FileCode2Icon />
                    LaTeX
                  </DropdownMenuItem>
                </DropdownMenuGroup>
              </DropdownMenuContent>
            </DropdownMenu>
          </ButtonGroup>
          {onOpenNote && (
            <Button variant="outline" className="bg-background" onClick={createNote}>
              <NotebookTextIcon data-icon="inline-start" />
              New note
            </Button>
          )}
          {onOpenCanvas && (
            <Button variant="outline" className="bg-background" onClick={createCanvas}>
              <FrameIcon data-icon="inline-start" />
              New canvas
            </Button>
          )}
          {onOpenChat && (
            <Button variant="outline" className="bg-background" onClick={createChat}>
              <MessagesSquareIcon data-icon="inline-start" />
              New chat
            </Button>
          )}
          {onOpenResearch && (
            <Button variant="outline" className="bg-background" onClick={onOpenResearch}>
              <TelescopeIcon data-icon="inline-start" />
              Research
            </Button>
          )}
          <Settings />
          <ThemeToggle />
        </div>
      </header>

      {error && (
        <Alert variant="destructive">
          <AlertTitle>Import problem</AlertTitle>
          <AlertDescription className="flex items-center justify-between gap-4">
            <span>{error}</span>
            <Button variant="ghost" size="sm" onClick={() => setError(null)}>
              Dismiss
            </Button>
          </AlertDescription>
        </Alert>
      )}

      {/* Filter chips: one workspace, five lenses. Selection persists. */}
      <div className="flex flex-wrap items-center gap-1.5">
        {FILTERS.map(({ id, label }) => (
          <Button
            key={id}
            variant={filter === id ? "secondary" : "outline"}
            size="sm"
            className="rounded-full bg-background"
            onClick={() => setFilter(id)}
          >
            {label}
          </Button>
        ))}
      </div>

      {(() => {
        // One recency-sorted list across papers and workspace items.
        const entries: LibraryEntry[] = [
          ...papers.map((paper) => ({
            type: "paper" as const,
            recency: paper.last_opened ?? paper.imported_at ?? "",
            paper,
          })),
          ...items.map((item) => ({
            type: "item" as const,
            recency: item.updated_at,
            item,
          })),
        ]
          .filter((entry) =>
            filter === "all"
              ? true
              : entry.type === "paper"
                ? filter === "research"
                : entry.item.kind === filter,
          )
          .sort((a, b) => b.recency.localeCompare(a.recency));

        if (entries.length === 0) {
          const unshipped =
            filter === "note" || filter === "canvas" || filter === "chat";
          return (
            <Empty>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <FileTextIcon />
                </EmptyMedia>
                <EmptyTitle>
                  {filter === "all" || filter === "research"
                    ? "No papers yet"
                    : `No ${FILTERS.find((f) => f.id === filter)?.label.toLowerCase()} yet`}
                </EmptyTitle>
                <EmptyDescription>
                  {filter === "all" || filter === "research"
                    ? "Drop a PDF here, or paste an arXiv link above."
                    : unshipped
                      ? `${KIND_LABEL[filter]}s arrive with the ${KIND_LABEL[filter].toLowerCase()}s feature — the workspace store is ready for them.`
                      : ""}
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          );
        }
        return (
          <ul className="flex flex-col gap-2 pr-14">
            {entries.map((entry) =>
              entry.type === "paper" ? (
                <PaperCard
                  key={entry.paper.id}
                  paper={entry.paper}
                  stageLabel={activeStages[entry.paper.id]}
                  stages={paperStages[entry.paper.id]}
                  onRetry={() => {
                    invoke("retry_ingestion", { paperId: entry.paper.id }).catch(
                      (e) => setError(String(e)),
                    );
                  }}
                  onOpen={() => openPaper(entry.paper)}
                  onDelete={() =>
                    setPendingDelete({
                      kind: "paper",
                      id: entry.paper.id,
                      title: entry.paper.title,
                    })
                  }
                  onChanged={refresh}
                />
              ) : (
                <WorkspaceItemCard
                  key={entry.item.id}
                  item={entry.item}
                  onOpen={
                    entry.item.kind === "note" && onOpenNote
                      ? () => onOpenNote(entry.item.id)
                      : entry.item.kind === "canvas" && onOpenCanvas
                        ? () => onOpenCanvas(entry.item.id)
                        : entry.item.kind === "chat" && onOpenChat
                          ? () => onOpenChat(entry.item.id)
                          : undefined
                  }
                  onDelete={() =>
                    setPendingDelete({
                      kind: "item",
                      id: entry.item.id,
                      title: entry.item.title,
                    })
                  }
                />
              ),
            )}
          </ul>
        );
      })()}

      {dragOver && (
        <div className="pointer-events-none fixed inset-x-0 bottom-8 text-center font-semibold text-primary">
          Drop PDF to import
        </div>
      )}

      <AlertDialog
        open={pendingDelete !== null}
        onOpenChange={(open) => {
          if (!open) {
            setPendingDelete(null);
            setDeleteConfirmText("");
          }
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {pendingDelete?.kind === "paper" ? "Delete paper" : "Delete item"}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {pendingDelete?.kind === "paper"
                ? `“${pendingDelete?.title}” — its notes, chats, and enrichment will be permanently deleted. The original PDF on disk is untouched.`
                : `“${pendingDelete?.title}” will be removed from the workspace.`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="flex items-center gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            <TriangleAlertIcon className="size-4 shrink-0" />
            Deleting “{pendingDelete?.title}” cannot be undone.
          </div>
          <div className="flex flex-col gap-1.5">
            <label htmlFor="delete-confirm" className="text-sm">
              To confirm, type the paper title{" "}
              <span className="font-semibold">“{pendingDelete?.title}”</span>
            </label>
            <Input
              id="delete-confirm"
              autoFocus
              value={deleteConfirmText}
              onChange={(e) => setDeleteConfirmText(e.target.value)}
            />
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              disabled={
                deleteConfirmText.trim() !== (pendingDelete?.title ?? "").trim()
              }
              onClick={confirmDelete}
            >
              {pendingDelete?.kind === "paper" ? "Delete paper" : "Delete item"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <ImportArxivDialog open={arxivOpen} onOpenChange={setArxivOpen} />
      </div>
    </div>
  );
}
