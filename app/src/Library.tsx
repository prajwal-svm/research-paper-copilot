import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, onFileDrop, platform } from "@/platform";
import { listen } from "@/platform";
import { openFileDialog } from "@/platform";
import {
  ChevronDownIcon,
  EllipsisVerticalIcon,
  FileCode2Icon,
  FileTextIcon,
  FolderOpenIcon,
  ImportIcon,
  LinkIcon,
  StarIcon,
  TelescopeIcon,
  Trash2Icon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import Settings from "./Settings";
import ThemeToggle from "./chrome/ThemeToggle";
import ImportArxivDialog from "./chrome/ImportArxivDialog";
import { OmnibarTrigger } from "./chrome/Omnibar";
import type { IngestionProgress, PaperSummary, PipelineStage } from "./types";

const STAGE_LABEL: Record<PipelineStage, string> = {
  layout: "Analyzing layout",
  objects: "Extracting objects",
  enrichment: "Parsing equations, figures, citations",
  embeddings: "Building search index",
};

const STATUS_LABEL: Record<PaperSummary["status"], string> = {
  ready: "Ready",
  processing: "Processing…",
  degraded: "Ready (some limitations)",
  failed: "Raw view only",
};

const STATUS_VARIANT: Record<
  PaperSummary["status"],
  "default" | "secondary" | "outline" | "destructive"
> = {
  ready: "secondary",
  processing: "outline",
  degraded: "outline",
  failed: "destructive",
};

const PRIORITIES = ["high", "medium", "low"] as const;

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
function PaperCard({
  paper,
  stageLabel,
  onOpen,
  onDelete,
  onChanged,
}: {
  paper: PaperSummary;
  stageLabel?: string;
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
  const showStatus = paper.status !== "ready" || stageLabel;

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
          <span className="font-medium">{paper.title}</span>
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
        {paper.authors.length > 0 && (
          <span className="line-clamp-1 text-sm text-muted-foreground">
            {paper.authors.join(", ")}
          </span>
        )}
        {meta.length > 0 && (
          <span className="text-xs text-muted-foreground/80">{meta.join(" · ")}</span>
        )}
        {(showStatus || paper.priority) && (
          <span className="mt-0.5 flex items-center gap-1.5">
            {showStatus && (
              <Badge variant={STATUS_VARIANT[paper.status]}>
                {stageLabel ?? STATUS_LABEL[paper.status]}
              </Badge>
            )}
            {paper.priority && (
              <Badge variant="outline">priority: {paper.priority}</Badge>
            )}
          </span>
        )}
      </button>

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
            <DropdownMenuLabel>Reading priority</DropdownMenuLabel>
            <DropdownMenuGroup>
              {PRIORITIES.map((priority) => (
                <DropdownMenuItem
                  key={priority}
                  onClick={() => setPriority(priority)}
                >
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

export default function Library({
  onOpen,
  onOpenResearch,
}: {
  onOpen: (id: string, title?: string) => void;
  onOpenResearch?: () => void;
}) {
  const [papers, setPapers] = useState<PaperSummary[]>([]);
  const [activeStages, setActiveStages] = useState<Record<string, string>>({});
  const [arxivOpen, setArxivOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<PaperSummary | null>(null);
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
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = listen<IngestionProgress>("ingestion-progress", ({ payload }) => {
      const { paper_id, event } = payload;
      if (event.kind === "stage_started") {
        setActiveStages((s) => ({ ...s, [paper_id]: STAGE_LABEL[event.stage] }));
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

  async function confirmDelete() {
    if (!pendingDelete) return;
    try {
      await invoke("delete_paper", { id: pendingDelete.id });
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setPendingDelete(null);
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
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" className="bg-background">
                <ImportIcon data-icon="inline-start" />
                Import
                <ChevronDownIcon data-icon="inline-end" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuGroup>
                <DropdownMenuItem onClick={pickAndImport}>
                  <FileTextIcon />
                  PDF
                </DropdownMenuItem>
                <DropdownMenuItem onClick={() => setArxivOpen(true)}>
                  <LinkIcon />
                  Link
                </DropdownMenuItem>
                <DropdownMenuItem onClick={pickAndImportLatex}>
                  <FileCode2Icon />
                  LaTeX
                </DropdownMenuItem>
              </DropdownMenuGroup>
            </DropdownMenuContent>
          </DropdownMenu>
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

      {papers.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <FileTextIcon />
            </EmptyMedia>
            <EmptyTitle>No papers yet</EmptyTitle>
            <EmptyDescription>
              Drop a PDF here, or paste an arXiv link above.
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <ul className="flex flex-col gap-2 pr-14">
          {papers.map((paper) => (
            <PaperCard
              key={paper.id}
              paper={paper}
              stageLabel={activeStages[paper.id]}
              onOpen={() => openPaper(paper)}
              onDelete={() => setPendingDelete(paper)}
              onChanged={refresh}
            />
          ))}
        </ul>
      )}

      {dragOver && (
        <div className="pointer-events-none fixed inset-x-0 bottom-8 text-center font-semibold text-primary">
          Drop PDF to import
        </div>
      )}

      <AlertDialog
        open={pendingDelete !== null}
        onOpenChange={(open) => !open && setPendingDelete(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete “{pendingDelete?.title}”?</AlertDialogTitle>
            <AlertDialogDescription>
              Only the .research bundle in your library is removed. The original
              PDF on disk is untouched.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDelete}>Delete</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <ImportArxivDialog open={arxivOpen} onOpenChange={setArxivOpen} />
      </div>
    </div>
  );
}
