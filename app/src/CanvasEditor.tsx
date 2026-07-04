import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, listen } from "@/platform";
import {
  Excalidraw,
  convertToExcalidrawElements,
} from "@excalidraw/excalidraw";
import type { ExcalidrawImperativeAPI } from "@excalidraw/excalidraw/types";
import type { ExcalidrawElementSkeleton } from "@excalidraw/excalidraw/data/transform";
import "@excalidraw/excalidraw/index.css";
import { canvasSummary, dataUrlToBase64, exportSceneDataUrl } from "./canvas/shared";
import { HomeIcon, PlusIcon, SparklesIcon, WandSparklesIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from "@/components/ui/input-group";
import { ScrollArea } from "@/components/ui/scroll-area";
import { MessageResponse } from "@/components/ai-elements/message";
import {
  Confirmation,
  ConfirmationAction,
  ConfirmationActions,
  ConfirmationRequest,
  ConfirmationTitle,
} from "@/components/ai-elements/confirmation";
import NoProviderNotice from "./ai/NoProviderNotice";
import { useAppDark } from "./chrome/ThemeToggle";
import type { PaperObject, PaperSummary, SemanticTree, WorkspaceItem } from "./types";

interface CanvasDoc {
  item: WorkspaceItem;
  scene: string;
  thumbnail: string;
}

/** A pinned reference marked on an Excalidraw element's customData. */
interface PinRef {
  paper_id: string;
  object_id: string | null;
  label: string;
}

interface AiStreamEvent {
  request_id: string;
  token?: string;
  done?: boolean;
  error?: string;
  cancelled?: boolean;
}

/** Full-page standalone canvas: freeform Excalidraw with pinned paper
 * content, Ask-AI, and AI-proposed edits gated by confirmation. */
export default function CanvasEditor({
  canvasId,
  onBack,
  onOpenPaper,
}: {
  canvasId: string;
  onBack: () => void;
  onOpenPaper: (paperId: string) => void;
}) {
  const [doc, setDoc] = useState<CanvasDoc | null | undefined>(undefined);

  useEffect(() => {
    invoke<CanvasDoc | null>("workspace_canvas_get", { id: canvasId })
      .then(setDoc)
      .catch(() => setDoc(null));
  }, [canvasId]);

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
        <p className="text-sm text-muted-foreground">This canvas no longer exists.</p>
        <Button variant="outline" onClick={onBack}>
          <HomeIcon data-icon="inline-start" />
          Back to library
        </Button>
      </div>
    );
  }
  return <LoadedCanvas key={doc.item.id} doc={doc} onBack={onBack} onOpenPaper={onOpenPaper} />;
}

function parseScene(sceneJson: string) {
  try {
    const parsed = JSON.parse(sceneJson);
    return {
      elements: Array.isArray(parsed.elements) ? parsed.elements : [],
      appState: parsed.appState ?? {},
      files: parsed.files ?? {},
    };
  } catch {
    return { elements: [], appState: {}, files: {} };
  }
}

/** Collect pinned refs from element customData for backlink reconciliation. */
function collectPins(elements: readonly Record<string, any>[]): PinRef[] {
  const pins: PinRef[] = [];
  for (const el of elements) {
    const ref = el.customData?.ref as PinRef | undefined;
    if (ref?.paper_id && !el.isDeleted) pins.push(ref);
  }
  return pins;
}

function LoadedCanvas({
  doc,
  onBack,
  onOpenPaper,
}: {
  doc: CanvasDoc;
  onBack: () => void;
  onOpenPaper: (paperId: string) => void;
}) {
  const dark = useAppDark();
  const apiRef = useRef<ExcalidrawImperativeAPI | null>(null);
  const saveTimer = useRef<number | undefined>(undefined);
  const [title, setTitle] = useState(doc.item.title);
  const initial = parseScene(doc.scene);

  // Autosave scene + thumbnail + pinned-ref reconciliation.
  const persist = useCallback(async () => {
    const api = apiRef.current;
    if (!api) return;
    const elements = api.getSceneElements();
    const scene = JSON.stringify({
      elements,
      appState: { viewBackgroundColor: api.getAppState().viewBackgroundColor },
      files: api.getFiles(),
    });
    const thumbnail = (await exportSceneDataUrl(api, 400)) ?? "";
    invoke("workspace_canvas_save", { id: doc.item.id, scene, thumbnail }).catch(() => {});
    invoke("workspace_canvas_refs_sync", {
      id: doc.item.id,
      pins: collectPins(elements as unknown as Record<string, any>[]).map((p) => ({
        paper_id: p.paper_id,
        object_id: p.object_id,
        label: p.label,
      })),
    }).catch(() => {});
  }, [doc.item.id]);

  const scheduleSave = useCallback(() => {
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => void persist(), 900);
  }, [persist]);

  useEffect(
    () => () => {
      window.clearTimeout(saveTimer.current);
      void persist();
    },
    [persist],
  );

  // ---- Pin paper content -------------------------------------------------
  const [pinOpen, setPinOpen] = useState(false);

  async function pinPaper(paper: PaperSummary) {
    const api = apiRef.current;
    if (!api) return;
    const skeleton: ExcalidrawElementSkeleton = {
      type: "rectangle",
      x: 100,
      y: 100,
      width: 220,
      height: 80,
      label: { text: paper.title.slice(0, 60) },
      customData: { ref: { paper_id: paper.id, object_id: null, label: paper.title } },
    } as ExcalidrawElementSkeleton;
    const created = convertToExcalidrawElements([skeleton]);
    api.updateScene({ elements: [...api.getSceneElements(), ...created] });
    setPinOpen(false);
    scheduleSave();
  }

  async function pinObject(paper: PaperSummary, object: PaperObject) {
    const api = apiRef.current;
    if (!api) return;
    const ref = { paper_id: paper.id, object_id: object.id, label: object.semantic_label ?? object.type };
    if (object.type === "figure") {
      const dataUrl = await invoke<string | null>("paper_figure_data_url", {
        paperId: paper.id,
        objectId: object.id,
      }).catch(() => null);
      if (dataUrl) {
        const fileId = crypto.randomUUID();
        api.addFiles([
          {
            id: fileId as never,
            dataURL: dataUrl as never,
            mimeType: "image/png",
            created: 1,
          } as never,
        ]);
        const skeleton = {
          type: "image",
          x: 120,
          y: 120,
          width: 260,
          height: 200,
          fileId,
          customData: { ref },
        } as unknown as ExcalidrawElementSkeleton;
        const created = convertToExcalidrawElements([skeleton]);
        api.updateScene({ elements: [...api.getSceneElements(), ...created] });
        setPinOpen(false);
        scheduleSave();
        return;
      }
    }
    // Equation / non-image: a labeled card.
    const skeleton: ExcalidrawElementSkeleton = {
      type: "rectangle",
      x: 120,
      y: 120,
      width: 240,
      height: 90,
      label: { text: ref.label.slice(0, 60) },
      customData: { ref },
    } as ExcalidrawElementSkeleton;
    const created = convertToExcalidrawElements([skeleton]);
    api.updateScene({ elements: [...api.getSceneElements(), ...created] });
    setPinOpen(false);
    scheduleSave();
  }

  // Open the referenced paper from the current selection.
  function openSelectedRef() {
    const api = apiRef.current;
    if (!api) return;
    const selected = api.getAppState().selectedElementIds ?? {};
    for (const el of api.getSceneElements() as unknown as Record<string, any>[]) {
      if (selected[el.id] && el.customData?.ref?.paper_id) {
        onOpenPaper(el.customData.ref.paper_id);
        return;
      }
    }
  }

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
          placeholder="Untitled canvas"
          onChange={(e) => setTitle(e.target.value)}
          onBlur={() =>
            invoke("workspace_item_rename", {
              id: doc.item.id,
              title: title.trim() || "Untitled canvas",
            }).catch(() => {})
          }
        />
        <Button variant="outline" size="sm" onClick={() => setPinOpen(true)}>
          <PlusIcon data-icon="inline-start" />
          Pin paper
        </Button>
        <Button variant="outline" size="sm" onClick={openSelectedRef} title="Open the selected pinned paper">
          Open ref
        </Button>
        <CanvasAi apiRef={apiRef} onApplied={scheduleSave} />
      </header>

      <div className="min-h-0 flex-1">
        <Excalidraw
          theme={dark ? "dark" : "light"}
          excalidrawAPI={(api) => (apiRef.current = api)}
          initialData={{ elements: initial.elements, appState: initial.appState, files: initial.files }}
          onChange={scheduleSave}
        />
      </div>

      <PinDialog open={pinOpen} onOpenChange={setPinOpen} onPinPaper={pinPaper} onPinObject={pinObject} />
    </div>
  );
}

/** Pin picker: papers, drill into a paper's figures/equations. */
function PinDialog({
  open,
  onOpenChange,
  onPinPaper,
  onPinObject,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPinPaper: (paper: PaperSummary) => void;
  onPinObject: (paper: PaperSummary, object: PaperObject) => void;
}) {
  const [papers, setPapers] = useState<PaperSummary[]>([]);
  const [drill, setDrill] = useState<{ paper: PaperSummary; objects: PaperObject[] } | null>(null);

  useEffect(() => {
    if (!open) return;
    setDrill(null);
    invoke<PaperSummary[]>("list_papers").then(setPapers).catch(() => setPapers([]));
  }, [open]);

  async function openDrill(paper: PaperSummary) {
    const tree = await invoke<SemanticTree | null>("read_artifact", {
      paperId: paper.id,
      artifact: "semantic_tree.json",
    }).catch(() => null);
    const objects = (tree?.objects ?? []).filter(
      (o) => o.type === "figure" || o.type === "equation",
    );
    setDrill({ paper, objects });
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="p-0 sm:max-w-lg">
        <DialogHeader className="px-4 pt-4">
          <DialogTitle>{drill ? drill.paper.title : "Pin paper content"}</DialogTitle>
          <DialogDescription>
            {drill
              ? "Pick a figure or equation, or pin the whole paper."
              : "Pick a paper to pin, or drill into its figures and equations."}
          </DialogDescription>
        </DialogHeader>
        <Command>
          <CommandInput placeholder={drill ? "Search figures & equations…" : "Search papers…"} />
          <CommandList>
            <CommandEmpty>Nothing found.</CommandEmpty>
            {!drill ? (
              <CommandGroup heading="Papers">
                {papers.map((paper) => (
                  <CommandItem
                    key={paper.id}
                    value={paper.title}
                    onSelect={() => onPinPaper(paper)}
                  >
                    <span className="truncate">{paper.title}</span>
                    <button
                      className="ml-auto text-xs text-muted-foreground hover:text-foreground"
                      onClick={(e) => {
                        e.stopPropagation();
                        openDrill(paper);
                      }}
                    >
                      figures & equations →
                    </button>
                  </CommandItem>
                ))}
              </CommandGroup>
            ) : (
              <CommandGroup heading="Figures & equations">
                <CommandItem value={`whole ${drill.paper.title}`} onSelect={() => onPinPaper(drill.paper)}>
                  Pin the whole paper
                </CommandItem>
                {drill.objects.map((object) => (
                  <CommandItem
                    key={object.id}
                    value={`${object.semantic_label ?? object.type} ${object.id}`}
                    onSelect={() => onPinObject(drill.paper, object)}
                  >
                    <span className="truncate">
                      {object.semantic_label ?? object.type} ({object.type})
                    </span>
                  </CommandItem>
                ))}
              </CommandGroup>
            )}
          </CommandList>
        </Command>
      </DialogContent>
    </Dialog>
  );
}

/** Ask-about + AI-proposed-edits, sharing the canvas API. */
function CanvasAi({
  apiRef,
  onApplied,
}: {
  apiRef: React.RefObject<ExcalidrawImperativeAPI | null>;
  onApplied: () => void;
}) {
  const [mode, setMode] = useState<"ask" | "edit" | null>(null);
  const [prompt, setPrompt] = useState("");
  const [answer, setAnswer] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [proposed, setProposed] = useState<unknown[] | null>(null);
  const activeRequest = useRef<string | null>(null);
  const editRaw = useRef("");

  useEffect(() => {
    const unlisten = listen<AiStreamEvent>("ai-stream", ({ payload }) => {
      if (payload.request_id !== activeRequest.current) return;
      if (payload.token) {
        setAnswer((a) => a + payload.token);
        editRaw.current += payload.token;
      }
      if (payload.error) {
        setStreaming(false);
        setError(payload.error);
      }
      if (payload.cancelled) setStreaming(false);
      if (payload.done) {
        setStreaming(false);
        if (mode === "edit") tryProposeElements(editRaw.current);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [mode]);

  function tryProposeElements(raw: string) {
    try {
      const start = raw.indexOf("[");
      const end = raw.lastIndexOf("]");
      const skeletons = JSON.parse(raw.slice(start, end + 1));
      const elements = convertToExcalidrawElements(skeletons);
      setProposed(elements);
    } catch {
      setError("The AI's proposed elements couldn't be parsed. Try rephrasing.");
    }
  }

  async function run(which: "ask" | "edit") {
    const api = apiRef.current;
    const question = prompt.trim();
    if (!api || !question || streaming) return;
    const summary = canvasSummary(
      api.getSceneElements() as unknown as Record<string, any>[],
    );
    const requestId = crypto.randomUUID();
    activeRequest.current = requestId;
    setAnswer("");
    editRaw.current = "";
    setProposed(null);
    setError(null);
    setStreaming(true);
    const dataUrl = await exportSceneDataUrl(api);
    const image = dataUrl
      ? { media_type: "image/png", data_b64: dataUrlToBase64(dataUrl) }
      : null;
    // Ask and edit share one streaming command; ask asks for prose, edit
    // asks for element skeletons (parsed into a confirmation on done).
    const instruction =
      which === "ask"
        ? `Answer this question about the canvas, in prose (no JSON): ${question}`
        : question;
    invoke("canvas_ai_edit", { requestId, instruction, summary, image }).catch((e) => {
      setStreaming(false);
      setError(String(e));
    });
  }

  function approve() {
    const api = apiRef.current;
    if (api && proposed) {
      api.updateScene({
        elements: [...api.getSceneElements(), ...(proposed as never[])],
      });
      onApplied();
    }
    close();
  }

  function close() {
    if (activeRequest.current) invoke("ai_cancel", { requestId: activeRequest.current }).catch(() => {});
    setMode(null);
    setPrompt("");
    setAnswer("");
    setProposed(null);
    setError(null);
    setStreaming(false);
  }

  const noProvider = error?.startsWith("No AI provider configured") ?? false;

  return (
    <>
      <Button variant="outline" size="sm" onClick={() => setMode("ask")}>
        <SparklesIcon data-icon="inline-start" />
        Ask AI
      </Button>
      <Button variant="outline" size="sm" onClick={() => setMode("edit")}>
        <WandSparklesIcon data-icon="inline-start" />
        AI edit
      </Button>

      <Dialog open={mode !== null} onOpenChange={(o) => !o && close()}>
        <DialogContent className="flex max-h-[80vh] flex-col sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{mode === "edit" ? "Ask AI to add to the canvas" : "Ask AI about this canvas"}</DialogTitle>
            <DialogDescription>
              {mode === "edit"
                ? "Describe what to draw. You approve the AI's additions before they're placed."
                : "The AI sees your diagram (image + structure) and answers."}
            </DialogDescription>
          </DialogHeader>
          <InputGroup className="flex-none">
            <InputGroupInput
              placeholder={mode === "edit" ? "Add a data-flow diagram of the training loop…" : "What's missing from this map?"}
              value={prompt}
              disabled={streaming}
              onChange={(e) => setPrompt(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && mode && run(mode)}
            />
            <InputGroupAddon align="inline-end">
              <InputGroupButton size="icon-xs" disabled={streaming || !prompt.trim()} onClick={() => mode && run(mode)} title="Run">
                <SparklesIcon />
              </InputGroupButton>
            </InputGroupAddon>
          </InputGroup>

          {noProvider ? (
            <NoProviderNotice actionDescription="use AI on the canvas" />
          ) : error ? (
            <p className="text-sm text-destructive">{error}</p>
          ) : (
            (answer || streaming) && (
              <ScrollArea className="min-h-0 flex-1 rounded-md border">
                <div className="p-3 text-sm">
                  {mode === "ask" ? (
                    <MessageResponse>{answer || "…"}</MessageResponse>
                  ) : (
                    <p className="text-muted-foreground">
                      {streaming ? "Designing elements…" : "Proposed additions ready below."}
                    </p>
                  )}
                  {streaming && <Spinner className="mt-2 size-3.5" />}
                </div>
              </ScrollArea>
            )
          )}

          {mode === "edit" && proposed && (
            <Confirmation state="approval-requested" approval={{ id: "canvas-edit" }}>
              <ConfirmationRequest>
                <ConfirmationTitle>
                  Add {(proposed as unknown[]).length} element
                  {(proposed as unknown[]).length === 1 ? "" : "s"} to the canvas?
                </ConfirmationTitle>
              </ConfirmationRequest>
              <ConfirmationActions>
                <ConfirmationAction variant="outline" onClick={close}>
                  Reject
                </ConfirmationAction>
                <ConfirmationAction onClick={approve}>Approve</ConfirmationAction>
              </ConfirmationActions>
            </Confirmation>
          )}
        </DialogContent>
      </Dialog>
    </>
  );
}
