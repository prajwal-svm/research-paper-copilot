import { useEffect, useRef, useState } from "react";
import { invoke } from "@/platform";
import {
  ArrowLeftIcon,
  BookMarkedIcon,
  PencilIcon,
  PlusIcon,
  RefreshCwIcon,
  TelescopeIcon,
  UsersIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { MessageResponse } from "@/components/ai-elements/message";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Spinner } from "@/components/ui/spinner";
import MarkdownEditor from "./MarkdownEditorLazy";
import WorkspacesPanel from "./WorkspacesPanel";

interface Review {
  id: string;
  name: string;
  papers: string[];
  generated_at?: string | null;
}

interface RefreshSummary {
  previous_exists: boolean;
  added_lines: number;
  removed_lines: number;
  out_of_scope_citations_removed: number;
}

interface Gap {
  kind: string;
  score: number;
  statement: string;
  papers: string[];
  narrative?: string | null;
}

type GapReport =
  | {
      kind: "insufficient_coverage";
      papers_analyzed: number;
      concepts_analyzed: number;
      minimum_papers: number;
      minimum_concepts: number;
    }
  | { kind: "report"; generated_at: string; papers_analyzed: number; gaps: Gap[] };

/**
 * Library-level Research view (v4): living literature reviews (your edits
 * are never overwritten — regeneration updates the machine copy and shows
 * what changed) and structural gap reports (computed from the graph, only
 * narrated by the AI).
 */
export default function ResearchView({
  onBack,
  onOpenPaper,
}: {
  onBack: () => void;
  onOpenPaper: (paperId: string) => void;
}) {
  const [tab, setTab] = useState<"reviews" | "gaps" | "workspaces">("reviews");

  return (
    <div className="flex h-screen flex-col">
      <div data-tauri-drag-region className="flex h-12 flex-none items-end gap-2 px-4 pb-1">
        <Button variant="ghost" size="icon-sm" onClick={onBack}>
          <ArrowLeftIcon />
        </Button>
        <h1 className="text-base font-semibold">Research</h1>
        <div className="ml-4 flex gap-1">
          <Button
            variant={tab === "reviews" ? "secondary" : "ghost"}
            size="sm"
            onClick={() => setTab("reviews")}
          >
            <BookMarkedIcon data-icon="inline-start" />
            Literature reviews
          </Button>
          <Button
            variant={tab === "gaps" ? "secondary" : "ghost"}
            size="sm"
            onClick={() => setTab("gaps")}
          >
            <TelescopeIcon data-icon="inline-start" />
            Gap reports
          </Button>
          <Button
            variant={tab === "workspaces" ? "secondary" : "ghost"}
            size="sm"
            onClick={() => setTab("workspaces")}
          >
            <UsersIcon data-icon="inline-start" />
            Workspaces
          </Button>
        </div>
      </div>
      {tab === "reviews" ? (
        <Reviews />
      ) : tab === "gaps" ? (
        <Gaps onOpenPaper={onOpenPaper} />
      ) : (
        <WorkspacesPanel />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------

function Reviews() {
  const [reviews, setReviews] = useState<Review[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [detail, setDetail] = useState<{
    review: Review;
    generated: string | null;
    document: string | null;
  } | null>(null);
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState(false);
  const [summary, setSummary] = useState<RefreshSummary | null>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [query, setQuery] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const draft = useRef("");

  const refreshList = () => {
    invoke<Review[]>("review_list").then(setReviews).catch(() => {});
  };
  useEffect(refreshList, []);

  useEffect(() => {
    if (!activeId) return;
    setDetail(null);
    setEditing(false);
    setSummary(null);
    invoke<{ review: Review; generated: string | null; document: string | null }>("review_get", {
      id: activeId,
    })
      .then((d) => {
        setDetail(d);
        draft.current = d.document ?? "";
      })
      .catch(() => {});
  }, [activeId]);

  async function regenerate() {
    if (!activeId) return;
    setBusy(true);
    setNotice(null);
    try {
      const result = await invoke<RefreshSummary | null>("review_regenerate", {
        requestId: crypto.randomUUID(),
        id: activeId,
      });
      if (result === null) {
        setNotice("Synthesis needs an AI provider (Settings). Your document stays editable.");
      } else {
        setSummary(result);
      }
      const d = await invoke<{ review: Review; generated: string | null; document: string | null }>(
        "review_get",
        { id: activeId },
      );
      setDetail(d);
      draft.current = d.document ?? "";
    } catch (e) {
      setNotice(String(e));
    } finally {
      setBusy(false);
      refreshList();
    }
  }

  return (
    <div className="flex min-h-0 flex-1">
      <ScrollArea className="w-64 flex-none border-r">
        <div className="flex flex-col gap-1 p-2">
          <Button variant="outline" size="sm" onClick={() => setCreating(true)}>
            <PlusIcon data-icon="inline-start" />
            New review
          </Button>
          {reviews.map((review) => (
            <button
              key={review.id}
              className={
                "rounded-md px-2 py-1.5 text-left text-sm hover:bg-accent " +
                (review.id === activeId ? "bg-accent" : "")
              }
              onClick={() => setActiveId(review.id)}
            >
              <span className="block truncate">{review.name}</span>
              <span className="text-muted-foreground text-xs">
                {review.papers.length} papers
              </span>
            </button>
          ))}
        </div>
      </ScrollArea>

      <div className="flex min-h-0 flex-1 flex-col">
        {creating && (
          <div className="flex flex-none flex-wrap items-end gap-2 border-b p-3">
            <Field className="w-56">
              <FieldLabel htmlFor="rev-name">Name</FieldLabel>
              <Input id="rev-name" value={name} onChange={(e) => setName(e.target.value)} />
            </Field>
            <Field className="w-64">
              <FieldLabel htmlFor="rev-query">Concept scope (empty = whole library)</FieldLabel>
              <Input
                id="rev-query"
                placeholder="e.g. attention"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </Field>
            <Button
              size="sm"
              onClick={async () => {
                try {
                  const review = await invoke<Review>("review_create", {
                    name: name.trim() || "Untitled review",
                    query: query.trim(),
                  });
                  setCreating(false);
                  setName("");
                  setQuery("");
                  refreshList();
                  setActiveId(review.id);
                } catch (e) {
                  setNotice(String(e));
                }
              }}
            >
              Create
            </Button>
            <Button variant="ghost" size="sm" onClick={() => setCreating(false)}>
              Cancel
            </Button>
          </div>
        )}

        {detail ? (
          <ScrollArea className="min-h-0 flex-1">
            <div className="mx-auto flex max-w-2xl flex-col gap-3 p-4">
              <div className="flex items-center gap-1.5">
                <h2 className="flex-1 truncate text-base font-semibold">{detail.review.name}</h2>
                {editing ? (
                  <>
                    <Button
                      size="sm"
                      onClick={async () => {
                        await invoke("review_save_document", {
                          id: detail.review.id,
                          content: draft.current,
                        }).catch(() => {});
                        setEditing(false);
                        setDetail({ ...detail, document: draft.current });
                      }}
                    >
                      Save
                    </Button>
                    <Button variant="ghost" size="sm" onClick={() => setEditing(false)}>
                      Cancel
                    </Button>
                  </>
                ) : (
                  <>
                    <Button variant="outline" size="sm" onClick={() => setEditing(true)}>
                      <PencilIcon data-icon="inline-start" />
                      Edit
                    </Button>
                    <Button variant="outline" size="sm" disabled={busy} onClick={regenerate}>
                      {busy ? (
                        <Spinner data-icon="inline-start" />
                      ) : (
                        <RefreshCwIcon data-icon="inline-start" />
                      )}
                      Refresh synthesis
                    </Button>
                  </>
                )}
              </div>
              {summary && summary.previous_exists && (
                <Badge variant="outline" className="self-start">
                  synthesis updated: +{summary.added_lines} / −{summary.removed_lines} lines — your
                  document was not modified; merge what you want
                </Badge>
              )}
              {notice && <p className="text-muted-foreground text-xs">{notice}</p>}

              {editing ? (
                <MarkdownEditor
                  initialMarkdown={detail.document ?? ""}
                  onMarkdownChange={(md) => (draft.current = md)}
                  autoFocus
                />
              ) : detail.document ? (
                <div className="text-sm">
                  <MessageResponse>{detail.document}</MessageResponse>
                </div>
              ) : (
                <p className="text-muted-foreground text-sm">
                  No synthesis yet — refresh to generate one (needs an AI provider).
                </p>
              )}

              {summary && summary.previous_exists && detail.generated && (
                <details className="text-sm">
                  <summary className="text-muted-foreground cursor-pointer text-xs">
                    View the latest machine synthesis (for merging)
                  </summary>
                  <div className="mt-2 rounded-md border p-2">
                    <MessageResponse>{detail.generated}</MessageResponse>
                  </div>
                </details>
              )}
            </div>
          </ScrollArea>
        ) : (
          <div className="flex flex-1 items-center justify-center p-6">
            <Empty>
              <EmptyHeader>
                <EmptyTitle>Living literature reviews</EmptyTitle>
                <EmptyDescription>
                  Scoped to concepts from your library's knowledge graph. The
                  machine synthesis and your edits are kept separate — a
                  refresh never overwrites your writing.
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          </div>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------

function Gaps({ onOpenPaper }: { onOpenPaper: (paperId: string) => void }) {
  const [report, setReport] = useState<GapReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    invoke<GapReport | null>("gaps_latest").then(setReport).catch(() => {});
  }, []);

  async function generate() {
    setBusy(true);
    setNotice(null);
    try {
      setReport(await invoke<GapReport>("gaps_generate"));
    } catch (e) {
      setNotice(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className="mx-auto flex max-w-2xl flex-col gap-3 p-4">
        <div className="flex items-center gap-2">
          <p className="text-muted-foreground flex-1 text-sm">
            Gaps are computed from your library's graph structure — the AI
            only narrates them, it cannot invent them.
          </p>
          <Button variant="outline" size="sm" disabled={busy} onClick={generate}>
            {busy ? <Spinner data-icon="inline-start" /> : <TelescopeIcon data-icon="inline-start" />}
            Generate report
          </Button>
        </div>
        {notice && <p className="text-muted-foreground text-xs">{notice}</p>}

        {report?.kind === "insufficient_coverage" && (
          <Empty>
            <EmptyHeader>
              <EmptyTitle>Library too small for honest gap claims</EmptyTitle>
              <EmptyDescription>
                {report.papers_analyzed} papers / {report.concepts_analyzed} concepts analyzed —
                gap analysis needs at least {report.minimum_papers} papers and{" "}
                {report.minimum_concepts} concepts to say anything defensible.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        )}

        {report?.kind === "report" &&
          report.gaps.map((gap, i) => (
            <div key={i} className="flex flex-col gap-1.5 rounded-md border p-3 text-sm">
              <div className="flex items-center gap-2">
                <Badge variant="outline">{gap.kind.replace(/_/g, " ")}</Badge>
                <span className="text-muted-foreground text-xs tabular-nums">
                  score {gap.score.toFixed(1)}
                </span>
              </div>
              <p className="font-medium">{gap.statement}</p>
              {gap.narrative && <p className="text-muted-foreground">{gap.narrative}</p>}
              <div className="flex flex-wrap gap-1">
                {gap.papers.slice(0, 6).map((paperId) => (
                  <Button
                    key={paperId}
                    variant="link"
                    size="sm"
                    className="h-auto p-0 text-xs"
                    onClick={() => onOpenPaper(paperId)}
                  >
                    {paperId}
                  </Button>
                ))}
              </div>
            </div>
          ))}
        {report?.kind === "report" && report.gaps.length === 0 && (
          <p className="text-muted-foreground text-sm">
            No structural gaps found — the analyzed concepts co-occur densely.
          </p>
        )}
      </div>
    </ScrollArea>
  );
}
