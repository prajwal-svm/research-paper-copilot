import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@/platform";
import {
  BookmarkIcon,
  BookOpenIcon,
  BracesIcon,
  ChevronDownIcon,
  FileTextIcon,
  ImageIcon,
  MusicIcon,
  NotebookPenIcon,
  PencilIcon,
  RefreshCwIcon,
  TextQuoteIcon,
  XIcon,
  Trash2Icon,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  PromptInput,
  PromptInputActionAddAttachments,
  PromptInputActionMenu,
  PromptInputActionMenuContent,
  PromptInputActionMenuTrigger,
  PromptInputAttachments,
  usePromptInputAttachments,
  PromptInputBody,
  PromptInputFooter,
  PromptInputSpeechButton,
  PromptInputSubmit,
  PromptInputTextarea,
  PromptInputTools,
  type PromptInputMessage,
} from "@/components/ai-elements/prompt-input";
import { Suggestion } from "@/components/ai-elements/suggestion";
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
  labelFor,
  onNavigate,
  onEdit,
  onDelete,
}: {
  message: StoredChatMessage;
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
      <div className="absolute -bottom-2 right-1 z-10 hidden gap-0.5 rounded-md border bg-background p-0.5 shadow-sm group-hover:flex">
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
    </div>
  );
}

/** Pastel chip palette for related suggestions — border tint only, cycled
 * round-robin by index (lemon chiffon → celadon). */
const CHIP_COLORS = [
  "#fbf8cc", // lemon chiffon
  "#fde4cf", // powder petal
  "#ffcfd2", // cotton rose
  "#f1c0e8", // pink orchid
  "#cfbaf0", // mauve
  "#a3c4f3", // baby blue ice
  "#90dbf4", // frosted blue
  "#8eecf5", // electric aqua
  "#98f5e1", // aquamarine
  "#b9fbc0", // celadon
];

// Border in the full pastel; background in the same hue at ~15% opacity so
// the label stays readable on both themes.
const chipStyle = (index: number) => {
  const color = CHIP_COLORS[index % CHIP_COLORS.length];
  return { borderColor: color, backgroundColor: `${color}26` };
};

/** Icon by attachment type: {} for JSON, image, audio, document fallback. */
function attachmentIcon(mediaType: string | undefined, name: string) {
  if (mediaType?.startsWith("image/")) return ImageIcon;
  if (mediaType?.startsWith("audio/") || /\.(mp3|wav|m4a|ogg)$/i.test(name)) return MusicIcon;
  if (mediaType === "application/json" || /\.json$/i.test(name)) return BracesIcon;
  return FileTextIcon;
}

/** Composer attachment: images as square thumbnails with a floating ✕;
 * other files as rounded mimetype pills. */
function ComposerAttachment({
  data,
}: {
  data: { id: string; url?: string; mediaType?: string; filename?: string };
}) {
  const attachments = usePromptInputAttachments();
  const isImage = Boolean(data.mediaType?.startsWith("image/") && data.url);
  const name = data.filename || (isImage ? "image" : "file");
  const Icon = attachmentIcon(data.mediaType, name);

  const removeButton = (
    <button
      type="button"
      aria-label="Remove attachment"
      className="absolute -top-1.5 -right-1.5 flex size-5 items-center justify-center rounded-full border bg-background shadow-sm hover:text-destructive"
      onClick={() => attachments.remove(data.id)}
    >
      <XIcon className="size-3" />
    </button>
  );

  if (isImage) {
    return (
      <div className="relative">
        <img
          src={data.url}
          alt={name}
          className="size-14 rounded-xl border object-cover"
        />
        {removeButton}
      </div>
    );
  }
  return (
    <div className="relative flex items-center gap-2 rounded-full border px-3 py-1.5 text-sm">
      <Icon className="size-4 shrink-0 text-muted-foreground" />
      <span className="max-w-40 truncate">{name}</span>
      {removeButton}
    </div>
  );
}

/** Geist-style "Show More": centered pill on a divider line. */
function ShowMoreDivider({
  expanded,
  onToggle,
}: {
  expanded: boolean;
  onToggle: () => void;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="h-px flex-1 bg-border" />
      <Button
        variant="outline"
        size="sm"
        className="h-6 rounded-full px-3 text-xs font-normal text-muted-foreground"
        onClick={onToggle}
      >
        {expanded ? "Show Less" : "Show More"}
        <ChevronDownIcon
          className={"size-3 transition-transform " + (expanded ? "rotate-180" : "")}
        />
      </Button>
      <span className="h-px flex-1 bg-border" />
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
  // "Show More" for the excerpt + related area; collapses per selection.
  const [showAll, setShowAll] = useState(false);
  // Header "Add note" button → a modal editor (more room than the panel).
  const [noteOpen, setNoteOpen] = useState(false);
  const noteDraft = useRef("");
  const stream = useAiStream(paperId);

  async function saveNote() {
    const markdown = noteDraft.current.trim();
    if (!object || !markdown) {
      setNoteOpen(false);
      return;
    }
    await invoke("note_save", {
      paperId,
      noteId: crypto.randomUUID(),
      objectId: object.id,
      anchorHash: object.content_hash,
      markdown,
    }).catch(() => {});
    setNoteOpen(false);
    onAnnotationsChanged();
  }

  async function toggleBookmark() {
    if (!object) return;
    await invoke("bookmark_toggle", {
      paperId,
      objectId: object.id,
      anchorHash: object.content_hash,
    }).catch(() => {});
    onAnnotationsChanged();
  }

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
    setShowAll(false);
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

  const bodyText =
    selection.kind === "object"
      ? selection.object.content.text
      : selection.selection.text;

  // Ad-hoc selections (text drag / region marquee) anchor by their own id
  // and pass their gathered text; extracted objects anchor by object id.
  function run(action: AiAction, q?: string) {
    lastAsk.current = q;
    setActiveAction(action);
    if (selection.kind === "object") {
      stream.start(selection.object.id, action, q);
    } else {
      stream.start(selection.selection.id, action, q, selection.selection.text);
    }
  }

  const noProvider =
    stream.error !== null && stream.error.startsWith("No AI provider configured");

  // ---- Composer: attachments, model indicator, dictation ----
  const [model, setModel] = useState<{ provider: string; model: string; host: string } | null>(
    null,
  );
  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  // The question sent with the streaming request, kept for Retry after the
  // box is cleared on send.
  const lastAsk = useRef<string | undefined>(undefined);

  useEffect(() => {
    invoke<{ provider: string; model: string; host: string }>("active_model")
      .then(setModel)
      .catch(() => setModel(null));
  }, []);

  // Non-image attachments ride along as fenced context blocks, mirroring the
  // backend load_attachment contract: UTF-8 only, clamped to 60k chars.
  function decodeTextAttachment(data_b64: string): string | null {
    try {
      const bytes = Uint8Array.from(atob(data_b64), (c) => c.charCodeAt(0));
      const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
      return text.includes("\0") ? null : text;
    } catch {
      return null;
    }
  }

  function submitAsk(message: PromptInputMessage) {
    const TEXT_CAP = 60_000;
    const base = message.text.trim();
    const files = message.files.filter((f) => f.url?.startsWith("data:"));
    if (!base && files.length === 0) return;
    // Text-file attachments become fenced context blocks in the question;
    // images travel to the model as real image content.
    let q = base || "See the attached context.";
    const images: { media_type: string; data_b64: string }[] = [];
    for (const file of files) {
      const data_b64 = file.url.slice(file.url.indexOf(",") + 1);
      if (file.mediaType?.startsWith("image/")) {
        images.push({ media_type: file.mediaType, data_b64 });
        continue;
      }
      const text = decodeTextAttachment(data_b64);
      if (text === null) {
        window.alert?.(
          `${file.filename ?? "file"} is binary — attach images or text files`,
        );
        continue;
      }
      const truncated = text.length > TEXT_CAP;
      q +=
        "\n\n[file: " +
        (file.filename ?? "file") +
        (truncated ? " (truncated)" : "") +
        "]\n```\n" +
        text.slice(0, TEXT_CAP) +
        "\n```";
    }
    const action: AiAction = object?.type === "table" ? "table_query" : "ask";
    lastAsk.current = q;
    setActiveAction(action);
    if (selection.kind === "object") {
      stream.start(selection.object.id, action, q, undefined, images);
    } else {
      stream.start(selection.selection.id, action, q, selection.selection.text, images);
    }
    setQuestion("");
  }

  return (
    <aside className="flex h-full flex-col border-l bg-background">
      {/* Add-note modal: full-width editor, more room than the panel. */}
      <Dialog open={noteOpen} onOpenChange={setNoteOpen}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>Add note</DialogTitle>
          </DialogHeader>
          <div className="max-h-[60vh] min-h-40 overflow-y-auto">
            <MarkdownEditor
              initialMarkdown=""
              onMarkdownChange={(md) => (noteDraft.current = md)}
              autoFocus
            />
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setNoteOpen(false)}>
              Cancel
            </Button>
            <Button onClick={saveNote}>Save note</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <div
        data-tauri-drag-region
        className="flex flex-none items-center justify-between gap-2 border-b px-3 py-2"
      >
        <div className="flex min-w-0 items-center gap-1.5">
          {object && (
            <>
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  noteDraft.current = "";
                  setNoteOpen(true);
                }}
              >
                <NotebookPenIcon data-icon="inline-start" />
                Add note
              </Button>
              <Button
                variant={
                  bookmarks.some((b) => b.object_id === object.id)
                    ? "secondary"
                    : "outline"
                }
                size="sm"
                onClick={toggleBookmark}
              >
                <BookmarkIcon data-icon="inline-start" />
                {bookmarks.some((b) => b.object_id === object.id) ? "Bookmarked" : "Bookmark"}
              </Button>
            </>
          )}
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
          {/* Excerpt + related context, collapsible to get out of the way. */}
          <Collapsible defaultOpen className="flex flex-col gap-3">
            <CollapsibleTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className="-mx-1 justify-start gap-1 px-1 text-xs text-muted-foreground [&[data-state=closed]>svg]:-rotate-90"
              >
                <ChevronDownIcon className="transition-transform" />
                Excerpt & related
              </Button>
            </CollapsibleTrigger>
            <CollapsibleContent className="flex flex-col gap-3">
              {/* Non-AI data: always available, no key required. Full text
                  flows with the panel width; "Show More" reveals the rest. */}
              <p
                className={
                  "text-sm break-words text-muted-foreground" +
                  (showAll ? "" : " line-clamp-4")
                }
              >
                {showAll ? bodyText : bodyText.slice(0, 600)}
              </p>

              {object && object.relationships && object.relationships.length > 0 && (
                <div className="flex flex-wrap items-center gap-1">
                  {object.relationships.slice(0, showAll ? undefined : 6).map((rel, i) => (
                    <Suggestion
                      key={i}
                      suggestion={labelFor(rel.target) ?? rel.type}
                      className="h-auto max-w-full min-w-0 justify-start px-3 py-1 text-left whitespace-normal break-words"
                      style={chipStyle(i)}
                      onClick={() => onNavigate(rel.target)}
                      title={rel.type}
                    />
                  ))}
                </div>
              )}

              {/* Cross-paper: this concept appears in other library papers. */}
              {seenElsewhere.length > 0 && (
                <div className="flex flex-wrap items-center gap-1">
                  <span className="text-xs text-muted-foreground">Seen in</span>
                  {seenElsewhere.slice(0, showAll ? undefined : 4).map((s, i) => (
                    <Tooltip key={`${s.paper_id}-${s.node}`}>
                      <TooltipTrigger asChild>
                        <Suggestion
                          suggestion={s.paper_title}
                          className="h-auto max-w-full min-w-0 justify-start px-3 py-1 text-left whitespace-normal break-words"
                          style={chipStyle(i)}
                          onClick={() => onOpenPaper?.(s.paper_id)}
                        >
                          <BookOpenIcon data-icon="inline-start" />
                          {s.paper_title}
                        </Suggestion>
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
                  {codeLinks.slice(0, showAll ? undefined : 3).map((link, i) => (
                    <Suggestion
                      key={i}
                      suggestion={`${link.function ?? link.file.split("/").pop()} · L${link.start_line}`}
                      className={
                        "h-auto max-w-full min-w-0 justify-start text-left whitespace-normal break-words" +
                        (link.confidence < 0.6 ? " border-dashed" : "")
                      }
                      style={chipStyle(i)}
                      onClick={() => onShowInCode(link.file, link.start_line)}
                    />
                  ))}
                </div>
              )}

              {(bodyText.length > 300 ||
                (object?.relationships?.length ?? 0) > 6 ||
                seenElsewhere.length > 4 ||
                codeLinks.length > 3) && (
                <ShowMoreDivider
                  expanded={showAll}
                  onToggle={() => setShowAll((v) => !v)}
                />
              )}
            </CollapsibleContent>
          </Collapsible>

          {/* Notes + bookmark: no key required, instant local writes. */}
          {object && (
            <ObjectAnnotations
              paperId={paperId}
              object={object}
              notes={notes}
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
            .map((message, i) => (
              <ChatMessageView
                key={message.id ?? i}
                message={message}
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
                      onClick={() => run(activeAction, lastAsk.current)}
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

      {/* Composer (AI Elements PromptInput): multiline ask box — Enter sends,
          Shift+Enter breaks the line — with screenshots (paste), drag-drop and
          picker attachments, dictation where the webview supports it, and the
          model that will actually answer. */}
      <div className="flex-none border-t p-2">
        <PromptInput onSubmit={submitAsk} multiple>
          <PromptInputBody>
            {/* Quoted context (reading-mode quote / text-drag selection):
                shown as a dismissible chip; travels with the next question. */}
            {selection.kind === "ad-hoc" && (
              <div className="w-full px-3 pt-3">
                <div className="flex items-start gap-2 rounded-lg bg-muted px-3 py-2 text-sm text-muted-foreground">
                  <TextQuoteIcon className="mt-0.5 size-4 flex-none" />
                  <span className="line-clamp-2 min-w-0 flex-1">
                    {selection.selection.text}
                  </span>
                  <button
                    type="button"
                    className="flex-none hover:text-foreground"
                    title="Remove quote"
                    onClick={onClose}
                  >
                    <XIcon className="size-4" />
                  </button>
                </div>
              </div>
            )}
            <PromptInputAttachments>
              {(attachment) => <ComposerAttachment data={attachment} />}
            </PromptInputAttachments>
            <PromptInputTextarea
              ref={composerRef}
              placeholder={
                object?.type === "table"
                  ? "Ask about this table's data…"
                  : "Ask anything… (paste a screenshot)"
              }
              value={question}
              disabled={stream.streaming}
              onChange={(e) => setQuestion(e.currentTarget.value)}
            />
          </PromptInputBody>
          <PromptInputFooter>
            <PromptInputTools>
              <PromptInputActionMenu>
                <PromptInputActionMenuTrigger title="Attach" />
                <PromptInputActionMenuContent>
                  <PromptInputActionAddAttachments label="Attach images or files…" />
                </PromptInputActionMenuContent>
              </PromptInputActionMenu>
              <PromptInputSpeechButton
                textareaRef={composerRef}
                onTranscriptionChange={setQuestion}
                title="Dictate"
              />
              {model && (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Badge variant="secondary" className="max-w-40 cursor-default">
                      <span className="truncate">{model.model}</span>
                    </Badge>
                  </TooltipTrigger>
                  <TooltipContent>
                    via {model.host} — change providers in Settings
                  </TooltipContent>
                </Tooltip>
              )}
            </PromptInputTools>
            <PromptInputTools>
              <PromptInputSubmit
                title="Ask"
                status={stream.streaming ? "streaming" : undefined}
                disabled={stream.streaming}
              />
            </PromptInputTools>
          </PromptInputFooter>
        </PromptInput>
      </div>
    </aside>
  );
}
