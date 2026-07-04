import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, listen } from "@/platform";
import {
  BlockNoteSchema,
  defaultInlineContentSpecs,
} from "@blocknote/core";
import {
  createReactInlineContentSpec,
  SuggestionMenuController,
  useCreateBlockNote,
} from "@blocknote/react";
import { BlockNoteView } from "@blocknote/shadcn";
import "@blocknote/shadcn/style.css";
import {
  ArrowUpRightIcon,
  HomeIcon,
  SparklesIcon,
  XIcon,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { MessageResponse } from "@/components/ai-elements/message";
import NoProviderNotice from "./ai/NoProviderNotice";
import type { PaperSummary, SemanticTree, WorkspaceItem } from "./types";

interface NoteDoc {
  item: WorkspaceItem;
  content: string;
  markdown: string;
}

// Mention chips navigate via a window event — the inline-content spec is
// module-level and can't close over component props.
export const OPEN_PAPER_EVENT = "rpc-note-open-paper";

const MentionInline = createReactInlineContentSpec(
  {
    type: "mention",
    propSchema: {
      label: { default: "" },
      paperId: { default: "" },
      objectId: { default: "" },
    },
    content: "none",
  },
  {
    render: (props) => (
      <span
        className="mx-0.5 cursor-pointer rounded bg-accent px-1 text-sm font-medium text-accent-foreground hover:underline"
        title="Open in reader"
        onClick={() =>
          window.dispatchEvent(
            new CustomEvent(OPEN_PAPER_EVENT, {
              detail: {
                paperId: props.inlineContent.props.paperId,
                objectId: props.inlineContent.props.objectId,
              },
            }),
          )
        }
      >
        @{props.inlineContent.props.label}
      </span>
    ),
  },
);

const schema = BlockNoteSchema.create({
  inlineContentSpecs: {
    ...defaultInlineContentSpecs,
    mention: MentionInline,
  },
});

/** Every mention in the document, for refs reconciliation. */
function collectMentions(
  blocks: unknown[],
): { paper_id: string | null; object_id: string | null; label: string }[] {
  const found: { paper_id: string | null; object_id: string | null; label: string }[] = [];
  const walk = (nodes: unknown[]) => {
    for (const node of nodes) {
      const block = node as {
        type?: string;
        props?: Record<string, string>;
        content?: unknown;
        children?: unknown[];
      };
      if (block.type === "mention" && block.props) {
        found.push({
          paper_id: block.props.paperId || null,
          object_id: block.props.objectId || null,
          label: block.props.label ?? "",
        });
        continue;
      }
      if (Array.isArray(block.content)) walk(block.content);
      if (Array.isArray(block.children)) walk(block.children);
    }
  };
  walk(blocks);
  return found;
}

interface AiStreamEvent {
  request_id: string;
  token?: string;
  done?: boolean;
  error?: string;
  cancelled?: boolean;
}

/** Streaming note-AI state (improve/summarize/expand/continue). */
function useNoteAi() {
  const [text, setText] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const activeRequest = useRef<string | null>(null);

  useEffect(() => {
    const unlisten = listen<AiStreamEvent>("ai-stream", ({ payload }) => {
      if (payload.request_id !== activeRequest.current) return;
      if (payload.token) setText((t) => t + payload.token);
      if (payload.done || payload.cancelled) setStreaming(false);
      if (payload.error) {
        setStreaming(false);
        setError(payload.error);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const start = useCallback((action: string, text: string) => {
    const requestId = crypto.randomUUID();
    activeRequest.current = requestId;
    setText("");
    setError(null);
    setStreaming(true);
    invoke<string>("note_ai", { requestId, action, text }).catch((e) => {
      if (activeRequest.current === requestId) {
        setStreaming(false);
        setError(String(e));
      }
    });
  }, []);

  const cancel = useCallback(() => {
    if (activeRequest.current) {
      invoke("ai_cancel", { requestId: activeRequest.current }).catch(() => {});
    }
  }, []);

  const reset = useCallback(() => {
    activeRequest.current = null;
    setText("");
    setError(null);
    setStreaming(false);
  }, []);

  return { text, streaming, error, start, cancel, reset };
}

/** Full-page workspace note: AFFiNE-grade block editing on BlockNote. */
export default function NoteEditor({
  noteId,
  onBack,
  onOpenPaper,
}: {
  noteId: string;
  onBack: () => void;
  onOpenPaper: (paperId: string) => void;
}) {
  const [doc, setDoc] = useState<NoteDoc | null | undefined>(undefined);

  useEffect(() => {
    invoke<NoteDoc | null>("workspace_note_get", { id: noteId })
      .then(setDoc)
      .catch(() => setDoc(null));
  }, [noteId]);

  // Mention chip clicks → reader navigation.
  useEffect(() => {
    const onOpen = (event: Event) => {
      const detail = (event as CustomEvent).detail as { paperId?: string };
      if (detail?.paperId) onOpenPaper(detail.paperId);
    };
    window.addEventListener(OPEN_PAPER_EVENT, onOpen);
    return () => window.removeEventListener(OPEN_PAPER_EVENT, onOpen);
  }, [onOpenPaper]);

  if (doc === undefined) {
    return (
      <div className="flex h-screen items-center justify-center">
        <Spinner />
      </div>
    );
  }
  if (doc === null) {
    return (
      <div className="flex h-screen flex-col items-center justify-center gap-3">
        <p className="text-sm text-muted-foreground">This note no longer exists.</p>
        <Button variant="outline" onClick={onBack}>
          <HomeIcon data-icon="inline-start" />
          Back to library
        </Button>
      </div>
    );
  }
  return <LoadedNote key={doc.item.id} doc={doc} onBack={onBack} />;
}

function LoadedNote({ doc, onBack }: { doc: NoteDoc; onBack: () => void }) {
  const initialContent = (() => {
    try {
      const parsed = JSON.parse(doc.content);
      return Array.isArray(parsed) && parsed.length > 0 ? parsed : undefined;
    } catch {
      return undefined;
    }
  })();
  const editor = useCreateBlockNote({ schema, initialContent });
  const [title, setTitle] = useState(doc.item.title);
  const saveTimer = useRef<number | undefined>(undefined);
  const ai = useNoteAi();
  const [aiOpen, setAiOpen] = useState(false);
  const [selectionBar, setSelectionBar] = useState<{ x: number; y: number } | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);

  // Autosave: debounce content persistence + refs reconciliation; the
  // store is the truth, there is no save button.
  const persist = useCallback(() => {
    const content = JSON.stringify(editor.document);
    const markdown = editor.blocksToMarkdownLossy(editor.document);
    invoke("workspace_note_save", { id: doc.item.id, content, markdown }).catch(() => {});
    invoke("workspace_note_refs_sync", {
      id: doc.item.id,
      mentions: collectMentions(editor.document as unknown[]).map(
        ({ paper_id, object_id, label }) => ({
          paper_id,
          object_id: object_id || null,
          label,
        }),
      ),
    }).catch(() => {});
  }, [editor, doc.item.id]);

  const scheduleSave = useCallback(() => {
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(persist, 800);
  }, [persist]);

  useEffect(
    () => () => {
      // Flush on unmount so a quick close never loses the last edit.
      window.clearTimeout(saveTimer.current);
      persist();
    },
    [persist],
  );

  // Mention suggestions: papers first; objects from the top-matching
  // papers' semantic trees (fetched lazily, cached per paper).
  const papersCache = useRef<PaperSummary[] | null>(null);
  const treeCache = useRef<Map<string, SemanticTree | null>>(new Map());
  const mentionItems = useCallback(
    async (query: string) => {
      if (!papersCache.current) {
        papersCache.current = await invoke<PaperSummary[]>("list_papers").catch(() => []);
      }
      const q = query.toLowerCase();
      const papers = (papersCache.current ?? []).filter((p) =>
        p.title.toLowerCase().includes(q),
      );
      const insert = (label: string, paperId: string, objectId?: string) => {
        editor.insertInlineContent([
          { type: "mention", props: { label, paperId, objectId: objectId ?? "" } },
          " ",
        ]);
      };
      const items = papers.slice(0, 5).map((paper) => ({
        title: paper.title,
        subtext: "paper",
        onItemClick: () => insert(paper.title, paper.id),
      }));
      // Drill-in: labeled objects from the top matches.
      if (q.length >= 2) {
        for (const paper of (papersCache.current ?? []).slice(0, 10)) {
          if (!treeCache.current.has(paper.id)) {
            treeCache.current.set(
              paper.id,
              await invoke<SemanticTree | null>("read_artifact", {
                paperId: paper.id,
                artifact: "semantic_tree.json",
              }).catch(() => null),
            );
          }
          const tree = treeCache.current.get(paper.id);
          for (const object of tree?.objects ?? []) {
            if (
              object.semantic_label &&
              object.semantic_label.toLowerCase().includes(q) &&
              items.length < 12
            ) {
              items.push({
                title: object.semantic_label,
                subtext: paper.title.slice(0, 40),
                onItemClick: () =>
                  insert(object.semantic_label!, paper.id, object.id),
              });
            }
          }
        }
      }
      return items;
    },
    [editor],
  );

  // Select-to-AI: same floating-toolbar pattern as reading mode
  // (mousedown is prevented so the selection survives the click).
  function onEditorMouseUp() {
    const selection = window.getSelection();
    const wrap = wrapRef.current;
    if (!selection || selection.isCollapsed || !wrap || aiOpen) {
      setSelectionBar(null);
      return;
    }
    if (!wrap.contains(selection.anchorNode)) return;
    const rect = selection.getRangeAt(0).getBoundingClientRect();
    const base = wrap.getBoundingClientRect();
    setSelectionBar({
      x: rect.left - base.left + rect.width / 2,
      y: rect.top - base.top,
    });
  }

  function runAi(action: string) {
    const selected = editor.getSelectedText();
    const text =
      selected.trim() ||
      editor.blocksToMarkdownLossy(editor.document).slice(-4_000);
    setSelectionBar(null);
    setAiOpen(true);
    ai.start(action, text);
  }

  function acceptAi() {
    if (ai.text.trim()) {
      editor.insertInlineContent(ai.text.trim());
      scheduleSave();
    }
    setAiOpen(false);
    ai.reset();
  }

  const noProvider = ai.error?.startsWith("No AI provider configured") ?? false;

  return (
    <div className="flex h-screen flex-col">
      <header
        data-tauri-drag-region
        className="flex flex-none items-center gap-2 border-b px-4 py-2 pl-20"
      >
        <Button variant="ghost" size="icon-sm" onClick={onBack} title="Back to library">
          <HomeIcon />
        </Button>
        <input
          className="min-w-0 flex-1 bg-transparent text-lg font-semibold outline-none"
          value={title}
          placeholder="Untitled"
          onChange={(e) => setTitle(e.target.value)}
          onBlur={() => {
            const next = title.trim() || "Untitled";
            invoke("workspace_item_rename", { id: doc.item.id, title: next }).catch(
              () => {},
            );
          }}
        />
      </header>

      <div
        ref={wrapRef}
        className="relative min-h-0 flex-1 overflow-y-auto"
        onMouseUp={onEditorMouseUp}
      >
        <div className="mx-auto max-w-3xl py-6">
          <BlockNoteView
            editor={editor}
            theme={document.documentElement.classList.contains("dark") ? "dark" : "light"}
            onChange={scheduleSave}
          >
            <SuggestionMenuController triggerCharacter="@" getItems={mentionItems} />
          </BlockNoteView>
        </div>

        {selectionBar && (
          <div
            className="absolute z-20 flex -translate-x-1/2 -translate-y-full gap-0.5 rounded-md border bg-background p-0.5 shadow-md"
            style={{ left: selectionBar.x, top: Math.max(0, selectionBar.y - 6) }}
            onMouseDown={(e) => e.preventDefault()}
          >
            {[
              ["improve", "Improve"],
              ["summarize", "Summarize"],
              ["expand", "Expand"],
            ].map(([action, label]) => (
              <Button key={action} variant="ghost" size="sm" onClick={() => runAi(action)}>
                <SparklesIcon data-icon="inline-start" />
                {label}
              </Button>
            ))}
          </div>
        )}

        {aiOpen && (
          <div
            className="absolute inset-x-4 bottom-4 z-20 mx-auto max-w-2xl rounded-lg border bg-background p-3 shadow-lg"
            onMouseDown={(e) => e.preventDefault()}
          >
            <div className="mb-2 flex items-center gap-2 text-sm text-muted-foreground">
              <SparklesIcon className="size-4" />
              AI suggestion
              {ai.streaming && <Spinner className="size-3.5" />}
              <span className="flex-1" />
              <button
                className="hover:text-foreground"
                onClick={() => {
                  ai.cancel();
                  setAiOpen(false);
                  ai.reset();
                }}
              >
                <XIcon className="size-4" />
              </button>
            </div>
            {noProvider ? (
              <NoProviderNotice actionDescription="use AI in notes" />
            ) : ai.error ? (
              <p className="text-sm text-destructive">{ai.error}</p>
            ) : (
              <div className="max-h-56 overflow-y-auto text-sm">
                <MessageResponse>{ai.text || "…"}</MessageResponse>
              </div>
            )}
            {!ai.error && (
              <div className="mt-2 flex justify-end gap-1.5">
                {ai.streaming && (
                  <Button variant="ghost" size="sm" onClick={() => ai.cancel()}>
                    Stop
                  </Button>
                )}
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    setAiOpen(false);
                    ai.reset();
                  }}
                >
                  Discard
                </Button>
                <Button size="sm" disabled={!ai.text.trim()} onClick={acceptAi}>
                  <ArrowUpRightIcon data-icon="inline-start" />
                  Insert
                </Button>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
