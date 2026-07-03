import { useState } from "react";
import { invoke } from "@/platform";
import { ImportIcon, SearchIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  HoverCard,
  HoverCardContent,
  HoverCardTrigger,
} from "@/components/ui/hover-card";
import { Spinner } from "@/components/ui/spinner";
import type { BBox } from "./types";

export interface CitationEntry {
  id: string;
  marker?: string;
  raw_text: string;
  resolved: {
    source: string;
    title?: string;
    authors?: string[];
    year?: number;
    venue?: string;
    arxiv_id?: string;
    doi?: string;
    url?: string;
  } | null;
  mentions?: { object_id: string; bbox?: BBox }[];
}

export interface CitationsDocument {
  pipeline_version: string;
  entries: CitationEntry[];
}

/**
 * Citation hover targets for one page (task 6.3): hovering a "[n]" marker
 * shows a card with resolved metadata (or the raw bibliography entry — never
 * a blank card) and offers "import as paper". Static data renders instantly,
 * comfortably inside the 150 ms cached budget.
 */
export function CitationTargets({
  page,
  scale,
  citations,
  sourcePaperId,
  onImported,
}: {
  page: number;
  scale: number;
  citations: CitationsDocument | null;
  /** Paper being read — imports record a citing→cited backlink from it. */
  sourcePaperId?: string;
  onImported?: (paperId: string) => void;
}) {
  if (!citations) return null;
  const targets = citations.entries.flatMap((entry) =>
    (entry.mentions ?? [])
      .filter((m) => m.bbox && m.bbox.page === page)
      .map((m, i) => ({ entry, bbox: m.bbox!, key: `${entry.id}-${i}` })),
  );
  return (
    <>
      {targets.map(({ entry, bbox, key }) => (
        <HoverCard key={key} openDelay={120} closeDelay={150}>
          <HoverCardTrigger asChild>
            <span
              className="citation-target"
              style={{
                left: bbox.x * scale,
                top: bbox.y * scale,
                width: bbox.width * scale,
                height: bbox.height * scale,
              }}
            />
          </HoverCardTrigger>
          <HoverCardContent className="w-80" side="top">
            <CitationCard entry={entry} sourcePaperId={sourcePaperId} onImported={onImported} />
          </HoverCardContent>
        </HoverCard>
      ))}
    </>
  );
}

function CitationCard({
  entry,
  sourcePaperId,
  onImported,
}: {
  entry: CitationEntry;
  sourcePaperId?: string;
  onImported?: (paperId: string) => void;
}) {
  const [importing, setImporting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const resolved = entry.resolved;
  const importable = resolved?.arxiv_id ?? resolved?.doi;

  async function importAsPaper() {
    if (!importable) return;
    setImporting(true);
    setMessage(null);
    try {
      const id = await invoke<string>("import_url", {
        input: importable,
        sourcePaperId: sourcePaperId ?? null,
      });
      setMessage("Added to your library — ingestion is running.");
      onImported?.(id);
    } catch (e) {
      setMessage(String(e));
    } finally {
      setImporting(false);
    }
  }

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        {entry.marker && <Badge variant="secondary">{entry.marker}</Badge>}
        {resolved?.year && (
          <span className="text-xs text-muted-foreground">{resolved.year}</span>
        )}
      </div>
      {resolved?.title ? (
        <>
          <p className="text-sm font-medium">{resolved.title}</p>
          {resolved.authors && resolved.authors.length > 0 && (
            <p className="text-xs text-muted-foreground">
              {resolved.authors.slice(0, 4).join(", ")}
              {resolved.authors.length > 4 ? " et al." : ""}
              {resolved.venue ? ` · ${resolved.venue}` : ""}
            </p>
          )}
        </>
      ) : (
        // Unresolvable: the raw bibliography entry, never a blank card.
        <p className="text-sm text-muted-foreground">{entry.raw_text.slice(0, 220)}</p>
      )}
      <div className="flex items-center gap-2">
        {importable ? (
          <Button size="sm" disabled={importing} onClick={importAsPaper}>
            {importing && <Spinner data-icon="inline-start" />}
            {!importing && <ImportIcon data-icon="inline-start" />}
            Import as paper
          </Button>
        ) : (
          <Button size="sm" variant="outline" asChild>
            <a
              href={`https://scholar.google.com/scholar?q=${encodeURIComponent(
                entry.raw_text.slice(0, 120),
              )}`}
              target="_blank"
              rel="noreferrer"
            >
              <SearchIcon data-icon="inline-start" />
              Search for it
            </a>
          </Button>
        )}
      </div>
      {message && <p className="text-xs text-muted-foreground">{message}</p>}
    </div>
  );
}
