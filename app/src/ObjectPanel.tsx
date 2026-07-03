import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@/platform";
import {
  BookOpenIcon,
  PencilIcon,
  RefreshCwIcon,
  SendIcon,
  ThumbsDownIcon,
  ThumbsUpIcon,
  Trash2Icon,
  XIcon,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from "@/components/ui/input-group";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import MarkdownEditor from "./MarkdownEditorLazy";

// CodeMirror + sandbox plumbing load only when an equation panel opens.
const ImplementationPanel = lazy(() => import("./ImplementationPanel"));
import NoProviderNotice from "./ai/NoProviderNotice";
import ObjectLinkedText from "./ai/ObjectLinkedText";
import { ObjectAnnotations, type Bookmark, type Note } from "./Annotations";
import { useAiStream, type AiAction } from "./ai/useAiStream";
import type { Selection } from "./Reader";
import type { PaperObject, SemanticTree } from "./types";

export interface StoredChatMessage {
  id?: string;
  role: string;
  content: string;
  action?: string;
  incomplete?: boolean;
  edited?: boolean;
  at: string;
}

/**
 * One chat message with hover edit/delete (both roles). Editing opens the
 * block markdown editor; saving appends a correction event — the original
 * text is never erased from the journal.
 */
function ChatMessageView({
  message,
  isLast,
  labelFor,
  onNavigate,
  onEdit,
  onDelete,
}: {
  message: StoredChatMessage;
  isLast: boolean;
  labelFor: (objectId: string) => string | undefined;
  onNavigate: (objectId: string) => void;
  onEdit: (content: string) => Promise<void>;
  onDelete: () => Promise<void>;
}) {
  const [editing, setEditing] = useState(false);
  const draft = useRef(message.content);

  if (editing) {
    return (
      <div className="flex flex-col gap-1.5">
        <MarkdownEditor
          initialMarkdown={message.content}
          onMarkdownChange={(md) => (draft.current = md)}
          autoFocus
        />
        <div className="flex gap-1.5 self-end">
          <Button variant="ghost" size="sm" onClick={() => setEditing(false)}>
            Cancel
          </Button>
          <Button
            size="sm"
            onClick={async () => {
              await onEdit(draft.current.trim());
              setEditing(false);
            }}
          >
            Save
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div
      className={
        "group relative " +
        (message.role === "user"
          ? "self-end rounded-lg bg-accent px-3 py-1.5 text-sm"
          : "text-sm")
      }
    >
      <ObjectLinkedText
        text={message.content}
        labelFor={labelFor}
        onNavigate={onNavigate}
      />
      <div className="absolute -top-2 right-1 hidden gap-0.5 rounded-md border bg-background p-0.5 shadow-sm group-hover:flex">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon-sm"
              disabled={!message.id}
              onClick={() => setEditing(true)}
            >
              <PencilIcon />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Edit message</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon-sm" disabled={!message.id} onClick={onDelete}>
              <Trash2Icon />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Delete message</TooltipContent>
        </Tooltip>
      </div>
      <span className="flex items-center gap-1.5">
        {message.incomplete && (
          <Badge variant="outline" className="mt-1">
            incomplete
          </Badge>
        )}
        {message.edited && (
          <Badge variant="outline" className="mt-1">
            edited
          </Badge>
        )}
      </span>
      {message.role === "assistant" && !message.incomplete && isLast && <AnswerThumbs />}
    </div>
  );
}

/** Per-answer feedback (task 8.4): one tap, content-free, opt-in telemetry. */
function AnswerThumbs() {
  const [voted, setVoted] = useState<"up" | "down" | null>(null);
  if (voted) {
    return <span className="text-xs text-muted-foreground">Thanks for the feedback.</span>;
  }
  const vote = (direction: "up" | "down") => {
    setVoted(direction);
    invoke("telemetry_record", {
      kind: direction === "up" ? "answer_thumbs_up" : "answer_thumbs_down",
    }).catch(() => {});
  };
  return (
    <div className="mt-1 flex gap-1">
      <Button variant="ghost" size="icon-sm" title="Good answer" onClick={() => vote("up")}>
        <ThumbsUpIcon />
      </Button>
      <Button variant="ghost" size="icon-sm" title="Bad answer" onClick={() => vote("down")}>
        <ThumbsDownIcon />
      </Button>
    </div>
  );
}

/** Actions per object type (mirrors copilot-core context::actions_for). */
function actionsFor(object: PaperObject): { action: AiAction; label: string }[] {
  const base: { action: AiAction; label: string }[] = [{ action: "explain", label: "Explain" }];
  switch (object.type) {
    case "equation":
      return [
        ...base,
        { action: "variable_breakdown", label: "Variables" },
        { action: "step_by_step", label: "Step by step" },
        { action: "intuition", label: "Intuition" },
        { action: "derivation", label: "Derivation" },
        { action: "assumptions", label: "Assumptions" },
        { action: "prerequisites", label: "Prerequisites" },
        { action: "common_mistakes", label: "Common mistakes" },
      ];
    case "figure":
      return [
        ...base,
        { action: "figure_describe", label: "Describe" },
        { action: "figure_interpret", label: "Interpret" },
        { action: "assumptions", label: "Assumptions" },
        { action: "prerequisites", label: "Prerequisites" },
        { action: "common_mistakes", label: "Common mistakes" },
      ];
    case "table":
      return [
        ...base,
        { action: "table_summarize", label: "Summarize" },
        { action: "table_query", label: "Query data" },
      ];
    default:
      return base;
  }
}

const ACTION_DESCRIPTION: Record<string, string> = {
  explain: "explain this object",
  ask: "ask about this object",
  variable_breakdown: "break down the variables",
  step_by_step: "walk through the steps",
  intuition: "give the intuition",
  derivation: "derive it from first principles",
  assumptions: "list the assumptions",
  prerequisites: "list the prerequisites",
  common_mistakes: "list common mistakes",
  figure_describe: "describe this figure",
  figure_interpret: "interpret this figure",
  table_summarize: "summarize this table",
  table_query: "query the table data",
};

/**
 * Anchored interaction panel (task 6.1): opens beside the reader without
 * reflowing pages; scrolling stays live. Structured so v2 can add tabs
 * (deep-dive, quiz, …) without redesign — the action row is the tab strip's
 * v1 form. Cached (pre-generated) content renders immediately; AI actions
 * stream; no-provider and mid-stream failure states are designed, not raw
 * errors.
 */
interface SeenElsewhere {
  concept: string;
  paper_id: string;
  paper_title: string;
  node: string;
  object: string | null;
}

export default function ObjectPanel({
  paperId,
  selection,
  tree,
  notes,
  bookmarks,
  onAnnotationsChanged,
  onNavigate,
  onClose,
  onOpenSettings,
  onOpenPaper,
  onShowInCode,
}: {
  paperId: string;
  selection: Selection;
  tree: SemanticTree | null;
  notes: Note[];
  bookmarks: Bookmark[];
  onAnnotationsChanged: () => void;
  onNavigate: (objectId: string) => void;
  onClose: () => void;
  onOpenSettings?: () => void;
  /** Cross-paper navigation for "seen in paper X". */
  onOpenPaper?: (paperId: string) => void;
  /** Object→code navigation ("show in code", v3 code-understanding). */
  onShowInCode?: (file: string, line: number) => void;
}) {
  const object = selection.kind === "object" ? selection.object : null;
  const [pregenerated, setPregenerated] = useState<string | null>(null);
  const [seenElsewhere, setSeenElsewhere] = useState<SeenElsewhere[]>([]);
  const [codeLinks, setCodeLinks] = useState<
    { file: string; start_line: number; function?: string; confidence: number }[]
  >([]);
  const [history, setHistory] = useState<StoredChatMessage[]>([]);
  const [question, setQuestion] = useState("");
  const [activeAction, setActiveAction] = useState<AiAction | null>(null);
  const stream = useAiStream(paperId);

  // Satisfaction instrumentation (opt-in, content-free): object interactions
  // and time-to-first-wow. No paper content ever leaves the event kind.
  useEffect(() => {
    if (!object) return;
    invoke("telemetry_record", { kind: "object_interaction" }).catch(() => {});
    invoke("telemetry_record", { kind: "first_object_interaction" }).catch(() => {});
  }, [object?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  // Cached enrichment + persisted conversation (resume-on-open): both are
  // local file reads, well under the 100 ms cached-open budget.
  useEffect(() => {
    setPregenerated(null);
    setHistory([]);
    stream.reset();
    setActiveAction(null);
    if (!object) return;
    invoke<string | null>("read_pregenerated", {
      paperId,
      objectId: object.id,
    })
      .then(setPregenerated)
      .catch(() => {});
    invoke<StoredChatMessage[]>("chat_history", {
      paperId,
      objectId: object.id,
    })
      .then(setHistory)
      .catch(() => {});
    // Cross-paper: concepts on this object already known from other papers.
    setSeenElsewhere([]);
    invoke<SeenElsewhere[]>("object_seen_elsewhere", {
      paperId,
      objectId: object.id,
    })
      .then(setSeenElsewhere)
      .catch(() => {});
    // Code-understanding: repository locations implementing this object.
    setCodeLinks([]);
    invoke<{ code_map: { links: { object: string; file: string; start_line: number; function?: string; confidence: number }[] } | null }>(
      "repro_artifacts",
      { paperId },
    )
      .then((a) =>
        setCodeLinks((a.code_map?.links ?? []).filter((l) => l.object === object.id)),
      )
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [paperId, object?.id, selection]);

  // When a stream finishes, fold it into the persisted history view.
  useEffect(() => {
    if (!object || (!stream.done && !stream.error)) return;
    invoke<StoredChatMessage[]>("chat_history", {
      paperId,
      objectId: object.id,
    })
      .then((h) => {
        setHistory(h);
        if (stream.done) {
          stream.reset();
          setActiveAction(null);
        }
      })
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stream.done, stream.error]);

  const labelFor = useMemo(() => {
    return (objectId: string) =>
      tree?.objects.find((o) => o.id === objectId)?.semantic_label ??
      tree?.objects
        .find((o) => o.id === objectId)
        ?.content.text.slice(0, 40);
  }, [tree]);

  const title =
    selection.kind === "object"
      ? (selection.object.semantic_label ?? selection.object.type)
      : "Selection";
  const bodyText =
    selection.kind === "object"
      ? selection.object.content.text
      : selection.selection.text;

  // Ad-hoc selections (text drag / region marquee) anchor by their own id
  // and pass their gathered text; extracted objects anchor by object id.
  function run(action: AiAction, q?: string) {
    setActiveAction(action);
    if (selection.kind === "object") {
      stream.start(selection.object.id, action, q);
    } else {
      stream.start(selection.selection.id, action, q, selection.selection.text);
    }
  }

  const noProvider =
    stream.error !== null && stream.error.startsWith("No AI provider configured");

  return (
    <aside className="flex h-full flex-col border-l bg-background">
      <div
        data-tauri-drag-region
        className="flex flex-none items-center justify-between gap-2 border-b px-3 py-2"
      >
        <div className="flex min-w-0 items-center gap-2">
          <Badge variant="secondary">
            {selection.kind === "object" ? selection.object.type : "selection"}
          </Badge>
          <span className="truncate text-sm font-medium">{title}</span>
          {selection.kind === "object" && selection.object.confidence < 0.7 && (
            <Badge variant="outline">low confidence</Badge>
          )}
        </div>
        <Button variant="ghost" size="icon-sm" onClick={onClose} title="Close panel">
          <XIcon />
        </Button>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-3 p-3">
          {/* Non-AI data: always available, no key required. */}
          <p className="max-h-32 overflow-y-auto text-sm text-muted-foreground">
            {bodyText.slice(0, 600)}
            {bodyText.length > 600 ? "…" : ""}
          </p>

          {object && object.relationships && object.relationships.length > 0 && (
            <div className="flex flex-wrap items-center gap-1">
              {object.relationships.slice(0, 6).map((rel, i) => (
                <Button
                  key={i}
                  variant="outline"
                  size="sm"
                  onClick={() => onNavigate(rel.target)}
                  title={rel.type}
                >
                  {labelFor(rel.target) ?? rel.type}
                </Button>
              ))}
            </div>
          )}

          {/* Cross-paper: this concept appears in other library papers. */}
          {seenElsewhere.length > 0 && (
            <div className="flex flex-wrap items-center gap-1">
              <span className="text-xs text-muted-foreground">Seen in</span>
              {seenElsewhere.slice(0, 4).map((s) => (
                <Tooltip key={`${s.paper_id}-${s.node}`}>
                  <TooltipTrigger asChild>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => onOpenPaper?.(s.paper_id)}
                    >
                      <BookOpenIcon data-icon="inline-start" />
                      {s.paper_title}
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>
                    “{s.concept}” also appears in this paper — open it
                  </TooltipContent>
                </Tooltip>
              ))}
            </div>
          )}

          {/* Code-understanding: where this object lives in the repo. */}
          {onShowInCode && codeLinks.length > 0 && (
            <div className="flex flex-wrap items-center gap-1">
              <span className="text-xs text-muted-foreground">In the code</span>
              {codeLinks.slice(0, 3).map((link, i) => (
                <Button
                  key={i}
                  variant="outline"
                  size="sm"
                  className={link.confidence < 0.6 ? "border-dashed" : ""}
                  onClick={() => onShowInCode(link.file, link.start_line)}
                >
                  {link.function ?? link.file.split("/").pop()} · L{link.start_line}
                </Button>
              ))}
            </div>
          )}

          {/* Notes + bookmark: no key required, instant local writes. */}
          {object && (
            <ObjectAnnotations
              paperId={paperId}
              object={object}
              notes={notes}
              bookmarked={bookmarks.some((b) => b.object_id === object.id)}
              onChanged={onAnnotationsChanged}
            />
          )}

          <Separator />

          {/* Action strip — becomes the tab strip in v2. Ad-hoc selections
              get Explain; extracted objects get their type-specific set. */}
          <div className="flex flex-wrap gap-1.5">
            {(object
              ? actionsFor(object)
              : [{ action: "explain" as AiAction, label: "Explain" }]
            ).map(({ action, label }) => (
              <Button
                key={action}
                variant={activeAction === action ? "secondary" : "outline"}
                size="sm"
                disabled={stream.streaming}
                onClick={() => run(action)}
              >
                {label}
              </Button>
            ))}
          </div>

          {/* Implementation mode (v3): equations become runnable code. */}
          {object && object.type === "equation" && (
            <>
              <Separator />
              <Suspense fallback={<Skeleton className="h-8 w-full" />}>
                <ImplementationPanel paperId={paperId} objectId={object.id} />
              </Suspense>
            </>
          )}

          {/* Pre-generated enrichment: cached, instant, works with no key. */}
          {pregenerated && !activeAction && history.length === 0 && (
            <ObjectLinkedText
              text={pregenerated}
              labelFor={labelFor}
              onNavigate={onNavigate}
            />
          )}

          {/* Persisted conversation, resumed on open. Empty assistant turns
              (failed streams from reasoning-budget exhaustion) are hidden.
              Every message — yours and the AI's — is editable and deletable
              (append-only corrections; originals stay in the journal). */}
          {history
            .filter((m) => m.role !== "assistant" || m.content.trim().length > 0)
            .map((message, i, visible) => (
              <ChatMessageView
                key={message.id ?? i}
                message={message}
                isLast={i === visible.length - 1}
                labelFor={labelFor}
                onNavigate={onNavigate}
                onEdit={async (content) => {
                  if (!object || !message.id) return;
                  const updated = await invoke<StoredChatMessage[]>("chat_edit", {
                    paperId,
                    objectId: object.id,
                    messageId: message.id,
                    content,
                  }).catch(() => null);
                  if (updated) setHistory(updated);
                }}
                onDelete={async () => {
                  if (!object || !message.id) return;
                  const updated = await invoke<StoredChatMessage[]>("chat_delete", {
                    paperId,
                    objectId: object.id,
                    messageId: message.id,
                  }).catch(() => null);
                  if (updated) setHistory(updated);
                }}
              />
            ))}

          {/* Streaming response. */}
          {activeAction && (
            <div className="flex flex-col gap-2">
              {stream.text && (
                <ObjectLinkedText
                  text={stream.text}
                  labelFor={labelFor}
                  onNavigate={onNavigate}
                />
              )}
              {stream.streaming && (
                <div className="flex items-center gap-2 text-sm text-muted-foreground">
                  <Spinner /> thinking…
                  {stream.host && (
                    <Badge variant="outline" title="Where this request is being sent">
                      → {stream.host}
                    </Badge>
                  )}
                  <Button variant="ghost" size="sm" onClick={() => stream.cancel()}>
                    Stop
                  </Button>
                </div>
              )}
              {stream.cancelled && (
                <span className="text-xs text-muted-foreground">
                  Stopped — the partial answer above is saved.
                </span>
              )}
              {noProvider && (
                <NoProviderNotice
                  actionDescription={ACTION_DESCRIPTION[activeAction] ?? "run this action"}
                  onOpenSettings={onOpenSettings}
                />
              )}
              {stream.error && !noProvider && (
                <Alert variant="destructive">
                  <AlertTitle>
                    {stream.text ? "Answer incomplete" : "Something went wrong"}
                  </AlertTitle>
                  <AlertDescription className="flex items-center justify-between gap-2">
                    <span className="min-w-0 truncate">{stream.error}</span>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => run(activeAction, question || undefined)}
                    >
                      <RefreshCwIcon data-icon="inline-start" />
                      Retry
                    </Button>
                  </AlertDescription>
                </Alert>
              )}
            </div>
          )}
        </div>
      </ScrollArea>

      {/* Ask anything — free-form, anchored to this object or selection. */}
      <div className="flex-none border-t p-2">
        <InputGroup>
          <InputGroupInput
            placeholder={
              object?.type === "table"
                ? "Ask about this table's data…"
                : "Ask anything about this…"
            }
            value={question}
            disabled={stream.streaming}
            onChange={(e) => setQuestion(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && question.trim()) {
                run(object?.type === "table" ? "table_query" : "ask", question.trim());
              }
            }}
          />
          <InputGroupAddon align="inline-end">
            <InputGroupButton
              size="icon-xs"
              disabled={stream.streaming || !question.trim()}
              onClick={() =>
                run(object?.type === "table" ? "table_query" : "ask", question.trim())
              }
              title="Ask"
            >
              <SendIcon />
            </InputGroupButton>
          </InputGroupAddon>
        </InputGroup>
      </div>
    </aside>
  );
}
