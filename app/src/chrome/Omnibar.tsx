import { useCallback, useEffect, useState } from "react";
import { invoke } from "@/platform";
import { toast } from "sonner";
import {
  FileTextIcon,
  ImportIcon,
  LibraryIcon,
  LinkIcon,
  MoonIcon,
  PenToolIcon,
  SearchIcon,
  SettingsIcon,
  TelescopeIcon,
  UsersIcon,
} from "lucide-react";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from "@/components/ui/command";
import { cn } from "@/lib/utils";
import { toggleTheme } from "./ThemeToggle";
import { pickAndImportPdf } from "./importActions";
import ImportArxivDialog from "./ImportArxivDialog";
import type { PaperSummary } from "../types";

const OPEN_EVENT = "omnibar:open";

/** Open the omnibar from anywhere (used by the Search trigger pill). */
export function openOmnibar() {
  window.dispatchEvent(new CustomEvent(OPEN_EVENT));
}

/** Wide "Search ⌘K" pill that opens the omnibar. */
export function OmnibarTrigger({ className }: { className?: string }) {
  return (
    <button
      type="button"
      onClick={openOmnibar}
      className={cn(
        "flex h-8 w-full max-w-md items-center gap-2 rounded-md border bg-background px-3 text-sm text-muted-foreground transition-colors hover:bg-accent",
        className,
      )}
    >
      <SearchIcon className="size-4" />
      <span className="flex-1 text-left">Search</span>
      <kbd className="pointer-events-none rounded border bg-muted px-1.5 font-mono text-[10px] text-muted-foreground">
        ⌘K
      </kbd>
    </button>
  );
}

/** Every-token-must-match scoring for plain (non-slash) queries. */
function tokenScore(hay: string, q: string): number {
  const tokens = q.split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return 1;
  return tokens.every((t) => hay.includes(t)) ? 1 : 0;
}

/**
 * cmdk filter with slash-command routing. Slash names live in each item's
 * keywords ("/import pdf", "/open", …). "/verb rest" shows commands whose
 * name matches the verb; content entries (papers, canvases) match when
 * their verb (/open, /excalidraw) matches and "rest" matches their text.
 */
function omniFilter(value: string, search: string, keywords?: string[]): number {
  const q = search.trim().toLowerCase();
  const hay = `${value} ${(keywords ?? []).join(" ")}`.toLowerCase();
  if (!q) return 1;
  if (!q.startsWith("/")) return tokenScore(hay, q);

  const full = q.slice(1);
  const [verb = "", ...restParts] = full.split(/\s+/);
  const rest = restParts.join(" ");
  const slashNames = (keywords ?? [])
    .filter((k) => k.startsWith("/"))
    .map((k) => k.slice(1).toLowerCase());

  if (value.startsWith("cmd:")) {
    let best = 0;
    for (const name of slashNames) {
      if (name === full) best = Math.max(best, 2);
      else if (name.startsWith(full)) best = Math.max(best, 1.5);
      else if (full.startsWith(name)) best = Math.max(best, 0.5);
    }
    return best;
  }
  // Content entries: first slash keyword is the routing verb.
  const routeVerb = slashNames[0];
  if (!routeVerb || !verb || !routeVerb.startsWith(verb)) return 0;
  return rest ? tokenScore(hay, rest) : 1;
}

/**
 * Universal command palette (⌘K, available in every view): searches
 * papers and canvases, and runs slash commands — /import pdf,
 * /import arxiv, /open, /excalidraw, /theme, /settings, /library,
 * /research.
 */
export default function Omnibar({
  onOpenPaper,
  onGoLibrary,
  onGoResearch,
  onOpenSettings,
}: {
  /** Open a paper; pane "graph" lands on its canvas view. */
  onOpenPaper: (id: string, title: string, pane?: "graph" | "community") => void;
  onGoLibrary: () => void;
  onGoResearch: () => void;
  onOpenSettings: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [papers, setPapers] = useState<PaperSummary[]>([]);
  const [arxivOpen, setArxivOpen] = useState(false);

  // ⌘K / Ctrl+K everywhere + programmatic open (trigger pill).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    const onOpenEvent = () => setOpen(true);
    window.addEventListener("keydown", onKey);
    window.addEventListener(OPEN_EVENT, onOpenEvent);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener(OPEN_EVENT, onOpenEvent);
    };
  }, []);

  // Fresh index on every open.
  useEffect(() => {
    if (!open) return;
    setQuery("");
    invoke<PaperSummary[]>("list_papers")
      .then(setPapers)
      .catch(() => setPapers([]));
  }, [open]);

  const run = useCallback((action: () => void) => {
    setOpen(false);
    action();
  }, []);

  async function importPdf() {
    const error = await pickAndImportPdf();
    if (error) toast.error("PDF import failed", { description: error });
    else
      toast.success("Import started", {
        description: "The paper appears in the library as it processes.",
      });
  }

  return (
    <>
      <CommandDialog
        open={open}
        onOpenChange={setOpen}
        title="Search"
        description="Search papers and canvases or run a command"
        className="top-1/4 sm:max-w-2xl"
      >
        <Command filter={omniFilter}>
          <CommandInput
            placeholder="Type a command or search…  ( / for commands )"
            value={query}
            onValueChange={setQuery}
          />
          <CommandList className="max-h-96">
            <CommandEmpty>No results.</CommandEmpty>

            <CommandGroup heading="Commands">
              <CommandItem
                value="cmd:import-pdf"
                keywords={["/import", "/import pdf", "import", "pdf", "file"]}
                onSelect={() => run(importPdf)}
              >
                <ImportIcon />
                Import PDF…
              </CommandItem>
              <CommandItem
                value="cmd:import-arxiv"
                keywords={["/import", "/import arxiv", "import", "arxiv", "doi", "url"]}
                onSelect={() => run(() => setArxivOpen(true))}
              >
                <LinkIcon />
                Import from arXiv / DOI…
              </CommandItem>
              <CommandItem
                value="cmd:theme"
                keywords={["/theme", "theme", "dark", "light", "toggle"]}
                onSelect={() => run(toggleTheme)}
              >
                <MoonIcon />
                Toggle theme
              </CommandItem>
              <CommandItem
                value="cmd:settings"
                keywords={["/settings", "settings", "preferences", "providers", "sync"]}
                onSelect={() => run(onOpenSettings)}
              >
                <SettingsIcon />
                Open settings
              </CommandItem>
              <CommandItem
                value="cmd:library"
                keywords={["/library", "library", "home", "papers"]}
                onSelect={() => run(onGoLibrary)}
              >
                <LibraryIcon />
                Go to library
              </CommandItem>
              <CommandItem
                value="cmd:research"
                keywords={["/research", "research", "workspace", "reviews", "gaps"]}
                onSelect={() => run(onGoResearch)}
              >
                <TelescopeIcon />
                Open research workspace
              </CommandItem>
            </CommandGroup>

            {papers.length > 0 && (
              <>
                <CommandSeparator />
                <CommandGroup heading="Papers">
                  {papers.map((paper) => (
                    <CommandItem
                      key={paper.id}
                      value={`paper:${paper.title} ${paper.arxiv_id ?? ""}`}
                      keywords={["/open"]}
                      onSelect={() => run(() => onOpenPaper(paper.id, paper.title))}
                    >
                      <FileTextIcon />
                      <span className="truncate">{paper.title}</span>
                    </CommandItem>
                  ))}
                </CommandGroup>
              </>
            )}

            {/* Community: /publish and /pull open the paper's community pane. */}
            {query.trim() !== "" && papers.length > 0 && (
              <>
                <CommandSeparator />
                <CommandGroup heading="Community">
                  {papers.map((paper) => (
                    <CommandItem
                      key={`community-${paper.id}`}
                      value={`community:${paper.title}`}
                      keywords={["/publish", "/pull", "community", "registry", "publish", "pull"]}
                      onSelect={() => run(() => onOpenPaper(paper.id, paper.title, "community"))}
                    >
                      <UsersIcon />
                      <span className="truncate">Community: {paper.title}</span>
                    </CommandItem>
                  ))}
                </CommandGroup>
              </>
            )}

            {/* Canvas views: surfaced on demand, not in the default listing. */}
            {query.trim() !== "" && papers.length > 0 && (
              <>
                <CommandSeparator />
                <CommandGroup heading="Canvases">
                  {papers.map((paper) => (
                    <CommandItem
                      key={`canvas-${paper.id}`}
                      value={`canvas:${paper.title}`}
                      keywords={["/excalidraw", "canvas", "map", "mindmap"]}
                      onSelect={() => run(() => onOpenPaper(paper.id, paper.title, "graph"))}
                    >
                      <PenToolIcon />
                      <span className="truncate">Canvas: {paper.title}</span>
                    </CommandItem>
                  ))}
                </CommandGroup>
              </>
            )}
          </CommandList>
        </Command>
      </CommandDialog>

      <ImportArxivDialog open={arxivOpen} onOpenChange={setArxivOpen} />
    </>
  );
}
