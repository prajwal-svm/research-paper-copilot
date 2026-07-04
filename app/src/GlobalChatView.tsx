import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, listen, openFileDialog } from "@/platform";
import {
  CopyIcon,
  FileIcon,
  FileTextIcon,
  LinkIcon,
  RefreshCwIcon,
  SendIcon,
  Trash2Icon,
  XIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { Conversation, ConversationContent } from "@/components/ai-elements/conversation";
import { Message, MessageContent, MessageResponse } from "@/components/ai-elements/message";
import { Suggestion, Suggestions } from "@/components/ai-elements/suggestion";
import NoProviderNotice from "./ai/NoProviderNotice";
import type { PaperSummary, SemanticTree } from "./types";

interface ChatMessageRow {
  id: string;
  chat_id: string;
  role: string;
  content: string;
  incomplete: boolean;
  edited: boolean;
  created_at: string;
}

/** A reference attached to the next message, resolved server-side on send. */
type ChatRef =
  | { kind: "paper"; paper_id: string; label: string }
  | { kind: "object"; paper_id: string; object_id: string; label: string }
  | { kind: "url"; url: string; label: string }
  | { kind: "pdf"; path: string; label: string };

interface AiStreamEvent {
  request_id: string;
  token?: string;
  done?: boolean;
  error?: string;
  cancelled?: boolean;
}

const SLASH_ACTIONS = [
  { name: "/summarize", desc: "Summarize the conversation", prompt: "Summarize our conversation so far concisely." },
  { name: "/explain", desc: "Explain in simple terms", prompt: "Explain the last topic in simple terms." },
  { name: "/search", desc: "Turn this into a web-style question", prompt: "Rephrase my question as a focused, searchable query and answer it." },
];

/**
 * Host-agnostic global chat: renders one chat thread with AI Elements and
 * the mention/slash composer. Used by both the full-screen surface and the
 * overlay — hosts own layout only.
 */
export default function GlobalChatView({
  chatId,
  onTitleChange,
}: {
  chatId: string;
  /** Fired after the first exchange so hosts can refresh the sidebar title. */
  onTitleChange?: () => void;
}) {
  const [messages, setMessages] = useState<ChatMessageRow[]>([]);
  const [input, setInput] = useState("");
  const [refs, setRefs] = useState<ChatRef[]>([]);
  const [streamText, setStreamText] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const activeRequest = useRef<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  const reload = useCallback(() => {
    invoke<ChatMessageRow[]>("workspace_chat_messages", { chatId })
      .then(setMessages)
      .catch(() => {});
  }, [chatId]);
  useEffect(reload, [reload]);

  useEffect(() => {
    const unlisten = listen<AiStreamEvent>("ai-stream", ({ payload }) => {
      if (payload.request_id !== activeRequest.current) return;
      if (payload.token) setStreamText((t) => t + payload.token);
      if (payload.error) {
        setStreaming(false);
        setError(payload.error);
      }
      if (payload.cancelled || payload.done) {
        setStreaming(false);
        setStreamText("");
        reload();
        if (payload.done) onTitleChange?.();
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [reload, onTitleChange]);

  function send(overrideText?: string) {
    const content = (overrideText ?? input).trim();
    if (!content || streaming) return;
    const requestId = crypto.randomUUID();
    activeRequest.current = requestId;
    setStreamText("");
    setError(null);
    setStreaming(true);
    // Persist backlinks for the chat, and pass refs for server-side context.
    invoke("workspace_chat_refs_sync", {
      id: chatId,
      refs: refs.map((r) => ({
        paper_id: "paper_id" in r ? r.paper_id : null,
        object_id: "object_id" in r ? r.object_id : null,
        label: r.label,
      })),
    }).catch(() => {});
    invoke<string>("chat_stream", {
      requestId,
      chatId,
      content,
      refs: refs.map(toContextRef),
      images: null,
    }).catch((e) => {
      if (activeRequest.current === requestId) {
        setStreaming(false);
        setError(String(e));
      }
    });
    setInput("");
    setRefs([]);
  }

  const noProvider = error?.startsWith("No AI provider configured") ?? false;

  async function copyMessage(text: string) {
    await navigator.clipboard.writeText(text).catch(() => {});
  }

  async function deleteMessage(id: string) {
    await invoke("workspace_chat_delete_message", { messageId: id }).catch(() => {});
    reload();
  }

  return (
    <div className="flex h-full flex-col">
      <Conversation className="min-h-0 flex-1">
        <ConversationContent>
          {messages.map((m) => (
            <Message key={m.id} from={m.role === "assistant" ? "assistant" : "user"}>
              <MessageContent>
                <MessageResponse>{m.content}</MessageResponse>
                {m.incomplete && (
                  <Badge variant="outline" className="mt-1">
                    incomplete
                  </Badge>
                )}
                {m.edited && (
                  <Badge variant="outline" className="mt-1">
                    edited
                  </Badge>
                )}
                <div className="mt-1 flex gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                  <Button variant="ghost" size="icon-sm" title="Copy" onClick={() => copyMessage(m.content)}>
                    <CopyIcon />
                  </Button>
                  {m.role === "assistant" && (
                    <Button variant="ghost" size="icon-sm" title="Retry" onClick={() => {
                      const prev = messages[messages.indexOf(m) - 1];
                      if (prev) send(prev.content);
                    }}>
                      <RefreshCwIcon />
                    </Button>
                  )}
                  <Button variant="ghost" size="icon-sm" title="Delete" onClick={() => deleteMessage(m.id)}>
                    <Trash2Icon />
                  </Button>
                </div>
              </MessageContent>
            </Message>
          ))}
          {streaming && (
            <Message from="assistant">
              <MessageContent>
                {streamText ? <MessageResponse>{streamText}</MessageResponse> : null}
                <div className="mt-1 flex items-center gap-2 text-sm text-muted-foreground">
                  <Spinner /> thinking…
                  <Button variant="ghost" size="sm" onClick={() => {
                    if (activeRequest.current) invoke("ai_cancel", { requestId: activeRequest.current }).catch(() => {});
                  }}>
                    Stop
                  </Button>
                </div>
              </MessageContent>
            </Message>
          )}
          {noProvider && <NoProviderNotice actionDescription="chat" />}
          {error && !noProvider && <p className="text-sm text-destructive">{error}</p>}
          {messages.length > 0 && !streaming && (
            <Suggestions>
              <Suggestion suggestion="Summarize this" onClick={(s) => send(s)} />
              <Suggestion suggestion="What are the key takeaways?" onClick={(s) => send(s)} />
            </Suggestions>
          )}
        </ConversationContent>
      </Conversation>

      <Composer
        input={input}
        setInput={setInput}
        refs={refs}
        setRefs={setRefs}
        textareaRef={textareaRef}
        streaming={streaming}
        onSend={() => send()}
        onSlash={(action) => send(action.prompt)}
      />
    </div>
  );
}

function toContextRef(r: ChatRef) {
  switch (r.kind) {
    case "paper":
      return { kind: "paper", paper_id: r.paper_id };
    case "object":
      return { kind: "object", paper_id: r.paper_id, object_id: r.object_id };
    case "url":
      return { kind: "url", url: r.url };
    case "pdf":
      return { kind: "pdf", path: r.path };
  }
}

/** The composer: attached-ref chips, a textarea, and @/slash popovers. */
function Composer({
  input,
  setInput,
  refs,
  setRefs,
  textareaRef,
  streaming,
  onSend,
  onSlash,
}: {
  input: string;
  setInput: (v: string) => void;
  refs: ChatRef[];
  setRefs: React.Dispatch<React.SetStateAction<ChatRef[]>>;
  textareaRef: React.RefObject<HTMLTextAreaElement | null>;
  streaming: boolean;
  onSend: () => void;
  onSlash: (action: (typeof SLASH_ACTIONS)[number]) => void;
}) {
  // Which popover is open, based on the token under the caret.
  const [popover, setPopover] = useState<"mention" | "slash" | null>(null);

  function onChange(value: string) {
    setInput(value);
    const trimmed = value.trimStart();
    if (trimmed.startsWith("/") && !value.includes("\n")) setPopover("slash");
    else if (/(^|\s)@$/.test(value)) setPopover("mention");
    else setPopover(null);
  }

  function addRef(ref: ChatRef) {
    setRefs((current) => [...current, ref]);
    // Drop the trigger char.
    setInput(input.replace(/@$/, "").replace(/^\/$/, ""));
    setPopover(null);
    textareaRef.current?.focus();
  }

  async function attachFile(kind: "url" | "pdf" | "file") {
    if (kind === "url") {
      const url = window.prompt?.("Paste a URL to reference:");
      if (url) addRef({ kind: "url", url, label: url.replace(/^https?:\/\//, "").slice(0, 40) });
      return;
    }
    const selected = await openFileDialog({
      multiple: false,
      filters: kind === "pdf" ? [{ name: "PDF", extensions: ["pdf"] }] : undefined,
    });
    const path = typeof selected === "string" ? selected : selected?.[0];
    if (path) {
      const name = path.split("/").pop() ?? path;
      addRef({ kind: "pdf", path, label: name });
    }
  }

  return (
    <div className="relative flex-none border-t p-2">
      {popover === "slash" && (
        <div className="absolute bottom-full left-2 mb-1 w-72 rounded-lg border bg-popover shadow-md">
          <Command>
            <CommandList>
              <CommandGroup heading="Actions">
                {SLASH_ACTIONS.map((action) => (
                  <CommandItem
                    key={action.name}
                    value={action.name}
                    onSelect={() => {
                      setInput("");
                      setPopover(null);
                      onSlash(action);
                    }}
                  >
                    <span className="font-medium">{action.name}</span>
                    <span className="ml-2 text-xs text-muted-foreground">{action.desc}</span>
                  </CommandItem>
                ))}
              </CommandGroup>
            </CommandList>
          </Command>
        </div>
      )}
      {popover === "mention" && (
        <MentionPopover onPick={addRef} onClose={() => setPopover(null)} />
      )}

      {refs.length > 0 && (
        <div className="mb-2 flex flex-wrap gap-1.5">
          {refs.map((r, i) => (
            <Badge key={i} variant="secondary" className="gap-1">
              {r.kind === "url" ? <LinkIcon className="size-3" /> : r.kind === "pdf" ? <FileTextIcon className="size-3" /> : <FileIcon className="size-3" />}
              <span className="max-w-40 truncate">{r.label}</span>
              <button className="hover:text-destructive" onClick={() => setRefs((c) => c.filter((_, j) => j !== i))}>
                <XIcon className="size-3" />
              </button>
            </Badge>
          ))}
        </div>
      )}

      <div className="flex items-end gap-2">
        <Textarea
          ref={textareaRef}
          className="max-h-40 min-h-11 flex-1 resize-none"
          placeholder="Message… (@ to reference, / for actions)"
          value={input}
          disabled={streaming}
          onChange={(e) => onChange(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !popover) {
              e.preventDefault();
              onSend();
            }
            if (e.key === "Escape") setPopover(null);
          }}
        />
        <div className="flex flex-col gap-1">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="icon-sm" onClick={() => attachFile("url")}>
                <LinkIcon />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Reference a URL</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="icon-sm" onClick={() => attachFile("pdf")}>
                <FileTextIcon />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Reference a PDF</TooltipContent>
          </Tooltip>
          <Button size="icon-sm" disabled={streaming || !input.trim()} onClick={onSend} title="Send">
            <SendIcon />
          </Button>
        </div>
      </div>
    </div>
  );
}

/** @-mention picker: papers, drill into their objects. */
function MentionPopover({
  onPick,
  onClose,
}: {
  onPick: (ref: ChatRef) => void;
  onClose: () => void;
}) {
  const [papers, setPapers] = useState<PaperSummary[]>([]);
  const [drill, setDrill] = useState<{ paper: PaperSummary; tree: SemanticTree | null } | null>(null);

  useEffect(() => {
    invoke<PaperSummary[]>("list_papers").then(setPapers).catch(() => setPapers([]));
  }, []);

  return (
    <div className="absolute bottom-full left-2 mb-1 w-80 rounded-lg border bg-popover shadow-md">
      <Command>
        <CommandInput placeholder={drill ? "Search objects…" : "Search papers…"} autoFocus />
        <CommandList>
          <CommandEmpty>Nothing found.</CommandEmpty>
          {!drill ? (
            <CommandGroup heading="Papers">
              {papers.map((paper) => (
                <CommandItem
                  key={paper.id}
                  value={paper.title}
                  onSelect={() => onPick({ kind: "paper", paper_id: paper.id, label: paper.title })}
                >
                  <span className="truncate">{paper.title}</span>
                  <button
                    className="ml-auto text-xs text-muted-foreground hover:text-foreground"
                    onClick={async (e) => {
                      e.stopPropagation();
                      const tree = await invoke<SemanticTree | null>("read_artifact", {
                        paperId: paper.id,
                        artifact: "semantic_tree.json",
                      }).catch(() => null);
                      setDrill({ paper, tree });
                    }}
                  >
                    sections →
                  </button>
                </CommandItem>
              ))}
            </CommandGroup>
          ) : (
            <CommandGroup heading={drill.paper.title.slice(0, 40)}>
              {(drill.tree?.objects ?? [])
                .filter((o) => o.semantic_label)
                .slice(0, 40)
                .map((object) => (
                  <CommandItem
                    key={object.id}
                    value={`${object.semantic_label} ${object.id}`}
                    onSelect={() =>
                      onPick({
                        kind: "object",
                        paper_id: drill.paper.id,
                        object_id: object.id,
                        label: object.semantic_label ?? object.type,
                      })
                    }
                  >
                    <span className="truncate">{object.semantic_label}</span>
                  </CommandItem>
                ))}
            </CommandGroup>
          )}
        </CommandList>
      </Command>
      <button className="sr-only" onClick={onClose}>
        close
      </button>
    </div>
  );
}
