import { useEffect, useRef, useState } from "react";
import { invoke } from "@/platform";
import { openFileDialog as openDialog } from "@/platform";
import {
  ArchiveIcon,
  DownloadIcon,
  FlaskConicalIcon,
  LightbulbIcon,
  PencilIcon,
  SearchCheckIcon,
  SparklesIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { MessageResponse } from "@/components/ai-elements/message";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import MarkdownEditor from "./MarkdownEditorLazy";

interface Weakness {
  id: string;
  kind: string;
  summary: string;
  object_ids: string[];
}

interface NoveltyEvidence {
  title: string;
  year?: number | null;
  source: string;
  identifier?: string | null;
  url?: string | null;
  similarity: number;
}

interface NoveltyResult {
  verdict: "appears_novel" | "adjacent_work_exists" | "likely_known" | "insufficient_evidence";
  evidence: NoveltyEvidence[];
  query: string;
}

interface HypothesisCard {
  id: string;
  claim: string;
  rationale: string;
  required_experiment: string;
  expected_evidence: string;
  weakness_ids: string[];
  novelty?: NoveltyResult | null;
  experiment_id?: string | null;
  upstream_changed: boolean;
}

interface ExtensionView {
  weaknesses: { generated_at: string; weaknesses: Weakness[] } | null;
  cards: HypothesisCard[];
  outline: string | null;
  draft: string | null;
}

const VERDICT_LABEL: Record<NoveltyResult["verdict"], string> = {
  appears_novel: "appears novel",
  adjacent_work_exists: "adjacent work exists",
  likely_known: "likely known",
  insufficient_evidence: "insufficient evidence",
};

/**
 * Extension mode (v4): weaknesses → hypotheses → novelty → outline → draft.
 * Staged and resumable; regenerating upstream flags (never destroys) your
 * cards and documents. Novelty verdicts render only with their evidence —
 * the UI cannot show a novelty claim without the works that back it.
 */
export default function ExtensionMode({
  paperId,
  labelFor,
  onNavigateObject,
}: {
  paperId: string;
  labelFor: (objectId: string) => string | undefined;
  onNavigateObject: (objectId: string) => void;
}) {
  const [view, setView] = useState<ExtensionView | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [tab, setTab] = useState<"pipeline" | "outline" | "draft">("pipeline");

  const refresh = () => {
    invoke<ExtensionView>("extension_state", { paperId }).then(setView).catch(() => {});
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(refresh, [paperId]);

  async function run(stage: string, command: string, extra: Record<string, unknown> = {}) {
    setBusy(stage);
    setNotice(null);
    try {
      const result = await invoke<{ removed_citations?: number; content?: string | null } | null>(
        command,
        { requestId: crypto.randomUUID(), paperId, ...extra },
      );
      if (result && typeof result === "object" && "removed_citations" in result) {
        if (result.content === null) {
          setNotice("Generation needs an AI provider (Settings). Existing documents stay editable.");
        } else if ((result.removed_citations ?? 0) > 0) {
          setNotice(
            `${result.removed_citations} unverifiable citation${result.removed_citations === 1 ? "" : "s"} removed — the surrounding claims need your review.`,
          );
        }
      }
      if (result === null) {
        setNotice("This stage needs an AI provider (Settings). Cached results stay usable.");
      }
    } catch (e) {
      setNotice(String(e));
    } finally {
      setBusy(null);
      refresh();
    }
  }

  if (!view) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner />
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex flex-none items-center gap-2 border-b px-3 pb-2 pt-10">
        {(["pipeline", "outline", "draft"] as const).map((t) => (
          <Button key={t} variant={tab === t ? "secondary" : "ghost"} size="sm" onClick={() => setTab(t)}>
            {t}
          </Button>
        ))}
        <span className="text-muted-foreground ml-auto text-xs">
          drafts are AI-assisted and provenance-marked on export
        </span>
      </div>

      {tab === "pipeline" && (
        <ScrollArea className="min-h-0 flex-1">
          <div className="mx-auto flex max-w-2xl flex-col gap-4 p-4">
            {/* Stage 1: weaknesses */}
            <div className="flex items-center gap-2">
              <h3 className="flex-1 text-sm font-semibold">1 · Weaknesses (object-grounded)</h3>
              <Button
                variant="outline"
                size="sm"
                disabled={busy !== null}
                onClick={() => run("weaknesses", "extension_weaknesses")}
              >
                {busy === "weaknesses" && <Spinner data-icon="inline-start" />}
                {view.weaknesses ? "Regenerate" : "Find weaknesses"}
              </Button>
            </div>
            {busy === "weaknesses" && !view.weaknesses && (
              <div className="flex flex-col gap-1.5">
                <Skeleton className="h-4 w-3/4" />
                <Skeleton className="h-4 w-2/3" />
              </div>
            )}
            {view.weaknesses?.weaknesses.map((weakness) => (
              <div key={weakness.id} className="rounded-md border p-2 text-sm">
                <Badge variant="outline" className="mb-1">
                  {weakness.kind.replace("_", " ")}
                </Badge>
                <p>{weakness.summary}</p>
                <div className="mt-1 flex flex-wrap gap-1">
                  {weakness.object_ids.map((objectId) => (
                    <Button
                      key={objectId}
                      variant="link"
                      size="sm"
                      className="h-auto p-0 text-xs"
                      onClick={() => onNavigateObject(objectId)}
                    >
                      {labelFor(objectId) ?? "source passage"}
                    </Button>
                  ))}
                </div>
              </div>
            ))}

            <Separator />

            {/* Stage 2–3: cards + novelty */}
            <div className="flex items-center gap-2">
              <h3 className="flex-1 text-sm font-semibold">2 · Hypothesis cards</h3>
              <Button
                variant="outline"
                size="sm"
                disabled={busy !== null || !view.weaknesses}
                onClick={() => run("cards", "extension_generate_cards")}
              >
                {busy === "cards" && <Spinner data-icon="inline-start" />}
                <LightbulbIcon data-icon="inline-start" />
                Propose hypotheses
              </Button>
            </div>
            {view.cards.map((card) => (
              <CardView
                key={card.id}
                paperId={paperId}
                card={card}
                busy={busy}
                onNoveltyCheck={() =>
                  run(`novelty-${card.id}`, "extension_novelty", { cardId: card.id })
                }
                onChanged={refresh}
              />
            ))}
            {view.cards.length === 0 && view.weaknesses && (
              <p className="text-muted-foreground text-sm">
                No cards yet — propose some from the weaknesses, or they'll appear here.
              </p>
            )}

            <Separator />

            {/* Stage 4–5: outline & draft */}
            <div className="flex items-center gap-2">
              <h3 className="flex-1 text-sm font-semibold">3 · Outline → draft (cited from a fixed bibliography)</h3>
              <Button
                variant="outline"
                size="sm"
                disabled={busy !== null || view.cards.length === 0}
                onClick={() => run("outline", "extension_draft", { stage: "outline" })}
              >
                {busy === "outline" && <Spinner data-icon="inline-start" />}
                {view.outline ? "Regenerate outline" : "Generate outline"}
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={busy !== null || view.cards.length === 0}
                onClick={() => run("draft", "extension_draft", { stage: "draft" })}
              >
                {busy === "draft" && <Spinner data-icon="inline-start" />}
                {view.draft ? "Regenerate draft" : "Generate draft"}
              </Button>
            </div>
            {notice && <p className="text-muted-foreground text-xs">{notice}</p>}
          </div>
        </ScrollArea>
      )}

      {(tab === "outline" || tab === "draft") && (
        <DocumentTab
          key={tab}
          paperId={paperId}
          name={tab === "outline" ? "outline.md" : "draft.md"}
          content={tab === "outline" ? view.outline : view.draft}
          exportable={tab === "draft"}
          onSaved={refresh}
        />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------

function CardView({
  paperId,
  card,
  busy,
  onNoveltyCheck,
  onChanged,
}: {
  paperId: string;
  card: HypothesisCard;
  busy: string | null;
  onNoveltyCheck: () => void;
  onChanged: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [fields, setFields] = useState({
    claim: card.claim,
    rationale: card.rationale,
    required_experiment: card.required_experiment,
    expected_evidence: card.expected_evidence,
  });

  async function save() {
    await invoke("extension_card_edit", {
      paperId,
      cardId: card.id,
      claim: fields.claim,
      rationale: fields.rationale,
      requiredExperiment: fields.required_experiment,
      expectedEvidence: fields.expected_evidence,
    }).catch(() => {});
    setEditing(false);
    onChanged();
  }

  return (
    <div className="flex flex-col gap-2 rounded-md border p-3 text-sm">
      <div className="flex items-start gap-2">
        {editing ? (
          <div className="flex flex-1 flex-col gap-2">
            {(
              [
                ["claim", "Claim"],
                ["rationale", "Rationale"],
                ["required_experiment", "Required experiment"],
                ["expected_evidence", "Expected evidence"],
              ] as const
            ).map(([key, label]) => (
              <Field key={key}>
                <FieldLabel htmlFor={`${card.id}-${key}`}>{label}</FieldLabel>
                <Input
                  id={`${card.id}-${key}`}
                  value={fields[key]}
                  onChange={(e) => setFields((f) => ({ ...f, [key]: e.target.value }))}
                />
              </Field>
            ))}
            <div className="flex gap-1.5">
              <Button size="sm" onClick={save}>
                Save
              </Button>
              <Button variant="ghost" size="sm" onClick={() => setEditing(false)}>
                Cancel
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex-1">
            <p className="font-medium">{card.claim}</p>
            <p className="text-muted-foreground mt-1">{card.rationale}</p>
            <p className="text-muted-foreground mt-1 text-xs">
              Experiment: {card.required_experiment} · Evidence: {card.expected_evidence}
            </p>
          </div>
        )}
        <div className="flex flex-none gap-0.5">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="icon-sm" onClick={() => setEditing(true)}>
                <PencilIcon />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Edit card</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={async () => {
                  await invoke("extension_card_archive", { paperId, cardId: card.id }).catch(
                    () => {},
                  );
                  onChanged();
                }}
              >
                <ArchiveIcon />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Archive card</TooltipContent>
          </Tooltip>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-1.5">
        {card.upstream_changed && <Badge variant="outline">upstream changed — review</Badge>}
        {card.experiment_id && (
          <Badge variant="secondary">
            <FlaskConicalIcon data-icon="inline-start" />
            experiment linked
          </Badge>
        )}
        <Button
          variant="outline"
          size="sm"
          disabled={busy !== null}
          onClick={onNoveltyCheck}
          title="Searches arXiv + Semantic Scholar with this claim (only the claim text is sent)"
        >
          {busy === `novelty-${card.id}` ? (
            <Spinner data-icon="inline-start" />
          ) : (
            <SearchCheckIcon data-icon="inline-start" />
          )}
          {card.novelty ? "Re-check novelty" : "Check novelty"}
        </Button>
      </div>

      {/* Verdict is NEVER rendered without its evidence. */}
      {card.novelty && (
        <div className="rounded-md bg-muted/60 p-2">
          <div className="flex items-center gap-2">
            <SparklesIcon className="size-3.5" />
            <span className="text-xs font-medium">
              {VERDICT_LABEL[card.novelty.verdict]}
            </span>
            <span className="text-muted-foreground text-xs">
              {card.novelty.verdict === "insufficient_evidence"
                ? "search returned nothing usable — retry when online"
                : `based on ${card.novelty.evidence.length} works`}
            </span>
          </div>
          <ul className="mt-1 flex flex-col gap-0.5">
            {card.novelty.evidence.slice(0, 5).map((evidence, i) => (
              <li key={i} className="text-muted-foreground flex items-center gap-1.5 text-xs">
                <span className="tabular-nums">{(evidence.similarity * 100).toFixed(0)}%</span>
                <span className="truncate">
                  {evidence.title}
                  {evidence.year ? ` (${evidence.year})` : ""}
                </span>
                {evidence.identifier && (
                  <Button
                    variant="link"
                    size="sm"
                    className="h-auto flex-none p-0 text-xs"
                    onClick={() =>
                      invoke("import_url", {
                        input: evidence.identifier,
                        sourcePaperId: paperId,
                      }).catch(() => {})
                    }
                  >
                    import
                  </Button>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------

function DocumentTab({
  paperId,
  name,
  content,
  exportable,
  onSaved,
}: {
  paperId: string;
  name: string;
  content: string | null;
  exportable: boolean;
  onSaved: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const draft = useRef(content ?? "");
  const [notice, setNotice] = useState<string | null>(null);

  if (!content && !editing) {
    return (
      <div className="flex flex-1 items-center justify-center p-6">
        <Empty>
          <EmptyHeader>
            <EmptyTitle>Nothing here yet</EmptyTitle>
            <EmptyDescription>
              Generate it from the pipeline tab — then edit it here. Editing
              and exporting work without an AI provider.
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      </div>
    );
  }

  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className="mx-auto flex max-w-2xl flex-col gap-3 p-4">
        <div className="flex items-center gap-1.5">
          {editing ? (
            <>
              <Button
                size="sm"
                onClick={async () => {
                  await invoke("extension_save_document", {
                    paperId,
                    name,
                    content: draft.current,
                  }).catch(() => {});
                  setEditing(false);
                  onSaved();
                }}
              >
                Save
              </Button>
              <Button variant="ghost" size="sm" onClick={() => setEditing(false)}>
                Cancel
              </Button>
            </>
          ) : (
            <Button variant="outline" size="sm" onClick={() => setEditing(true)}>
              <PencilIcon data-icon="inline-start" />
              Edit
            </Button>
          )}
          {exportable && !editing && (
            <Button
              variant="outline"
              size="sm"
              onClick={async () => {
                const dir = await openDialog({ directory: true });
                if (typeof dir === "string") {
                  try {
                    await invoke("extension_export", { paperId, destDir: dir });
                    setNotice(`Exported main.tex + references.bib to ${dir}`);
                  } catch (e) {
                    setNotice(String(e));
                  }
                }
              }}
            >
              <DownloadIcon data-icon="inline-start" />
              Export LaTeX
            </Button>
          )}
        </div>
        {notice && <p className="text-muted-foreground text-xs">{notice}</p>}
        {editing ? (
          <MarkdownEditor
            initialMarkdown={content ?? ""}
            onMarkdownChange={(md) => (draft.current = md)}
            autoFocus
          />
        ) : (
          <div className="text-sm">
            <MessageResponse>{content ?? ""}</MessageResponse>
          </div>
        )}
      </div>
    </ScrollArea>
  );
}
