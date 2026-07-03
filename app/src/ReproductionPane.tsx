import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@/platform";
import { listen } from "@/platform";
import CodeMirror, { type ReactCodeMirrorRef } from "@uiw/react-codemirror";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { EditorView } from "@codemirror/view";
import CapabilityBanner from "./chrome/CapabilityBanner";
import {
  CheckCircle2Icon,
  CircleDashedIcon,
  CircleSlashIcon,
  PlayIcon,
  SquareIcon,
  XCircleIcon,
} from "lucide-react";
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { MessageResponse } from "@/components/ai-elements/message";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Switch } from "@/components/ui/switch";

const STEPS = ["clone", "env", "explain", "map", "run", "verify", "report"] as const;
const STEP_LABEL: Record<string, string> = {
  clone: "Clone repository",
  env: "Detect environment",
  explain: "Explain architecture",
  map: "Map code ↔ paper",
  run: "Verification run",
  verify: "Verify metrics",
  report: "Write report",
};

interface StepRecord {
  status: string;
  detail?: string;
}

interface ReproView {
  state: { steps: Record<string, StepRecord> };
  repo: { remote: string; commit?: string; curated: boolean } | null;
  plan: { kind: string; setup_commands: string[] } | null;
  report: string | null;
  next_step: string | null;
}

interface CodeLink {
  file: string;
  function?: string;
  start_line: number;
  end_line: number;
  object: string;
  confidence: number;
}

/**
 * Reproduction mode + repo browser (v3): a staged, observable, resumable
 * pipeline (every step shows its exact commands and logs; failures offer
 * retry) plus a read-only source browser with code↔paper links. Browsing
 * works offline and without a container runtime; only Run needs one.
 */
export default function ReproductionPane({
  paperId,
  labelFor,
  target,
  onNavigateObject,
}: {
  paperId: string;
  labelFor: (objectId: string) => string | undefined;
  /** Object→code navigation target ("show in code"). */
  target?: { file: string; line: number } | null;
  onNavigateObject: (objectId: string) => void;
}) {
  const [view, setView] = useState<ReproView | null>(null);
  const [tab, setTab] = useState<"steps" | "files" | "architecture" | "report">("steps");
  const [remote, setRemote] = useState("");
  const [running, setRunning] = useState(false);
  const [log, setLog] = useState<string[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [consentOpen, setConsentOpen] = useState(false);
  const [networkConsent, setNetworkConsent] = useState(false);
  const [runCommand, setRunCommand] = useState("");
  const [reported, setReported] = useState("");
  const [architecture, setArchitecture] = useState<string | null>(null);
  const [codeMap, setCodeMap] = useState<CodeLink[]>([]);
  const activeRun = useRef<string | null>(null);

  const refresh = () => {
    invoke<ReproView>("repro_state", { paperId }).then(setView).catch(() => {});
    invoke<{ architecture: string | null; code_map: { links: CodeLink[] } | null }>(
      "repro_artifacts",
      { paperId },
    )
      .then((a) => {
        setArchitecture(a.architecture);
        setCodeMap(a.code_map?.links ?? []);
      })
      .catch(() => {});
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(refresh, [paperId]);

  useEffect(() => {
    if (target) setTab("files");
  }, [target]);

  useEffect(() => {
    const unlisten = listen<{
      paper_id: string;
      step: string;
      line?: string;
      done?: boolean;
      error?: string;
    }>("repro-progress", ({ payload }) => {
      if (payload.paper_id !== paperId) return;
      if (payload.line !== undefined) setLog((l) => [...l.slice(-500), payload.line!]);
      if (payload.done || payload.error) {
        setRunning(false);
        if (payload.error === "network_consent_required") {
          setNetworkConsent(true);
          setConsentOpen(true);
        } else if (payload.error) setNotice(payload.error);
        refresh();
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [paperId]);

  async function advance() {
    setNotice(null);
    if (view?.next_step === "run" && runCommand.trim()) {
      const metrics: Record<string, number> = {};
      for (const pair of reported.split(",")) {
        const [k, v] = pair.split("=").map((s) => s.trim());
        if (k && v && Number.isFinite(Number(v))) metrics[k] = Number(v);
      }
      await invoke("repro_configure_run", {
        paperId,
        runCommand: runCommand.trim(),
        reported: metrics,
      }).catch(() => {});
    }
    const runId = crypto.randomUUID();
    activeRun.current = runId;
    setLog([]);
    setRunning(true);
    try {
      await invoke("repro_advance", { paperId, runId });
    } catch (e) {
      setRunning(false);
      if (String(e).includes("consent_required")) setConsentOpen(true);
      else setNotice(String(e));
    }
  }

  if (!view) return null;

  if (!view.repo) {
    return (
      <div className="mx-auto flex max-w-md flex-col gap-3 p-6 pt-16">
      <CapabilityBanner id="reproduction" />
        <h2 className="text-base font-semibold">Reproduce this paper</h2>
        <p className="text-muted-foreground text-sm">
          Link the paper's GitHub repository. The pipeline clones it, sets up
          the environment, maps code to the paper, runs a verification-scale
          check, and writes an honest report.
        </p>
        <Field>
          <FieldLabel htmlFor="repo-remote">Repository URL</FieldLabel>
          <Input
            id="repo-remote"
            placeholder="https://github.com/…"
            value={remote}
            onChange={(e) => setRemote(e.target.value)}
          />
        </Field>
        <Button
          className="self-start"
          disabled={!remote.trim().startsWith("http")}
          onClick={async () => {
            await invoke("repro_set_repo", { paperId, remote: remote.trim() }).catch(() => {});
            refresh();
          }}
        >
          Link repository
        </Button>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex flex-none items-center gap-2 border-b px-3 pb-2 pt-10">
        {(["steps", "files", "architecture", "report"] as const).map((t) => (
          <Button
            key={t}
            variant={tab === t ? "secondary" : "ghost"}
            size="sm"
            onClick={() => setTab(t)}
          >
            {t}
          </Button>
        ))}
        <span className="text-muted-foreground ml-auto truncate text-xs">
          {view.repo.remote}
          {view.repo.commit ? ` @ ${view.repo.commit.slice(0, 8)}` : ""}
        </span>
        {!view.repo.curated && (
          <Badge variant="outline">unverified repo — steps may need manual help</Badge>
        )}
      </div>

      {tab === "steps" && (
        <ScrollArea className="min-h-0 flex-1">
          <div className="mx-auto flex max-w-2xl flex-col gap-3 p-4">
            <ol className="flex flex-col gap-1.5">
              {STEPS.map((step) => {
                const record = view.state.steps[step];
                const isNext = view.next_step === step;
                return (
                  <li key={step} className="flex items-center gap-2 text-sm">
                    {record?.status === "completed" ? (
                      <CheckCircle2Icon className="size-4 flex-none text-primary" />
                    ) : record?.status === "failed" ? (
                      <XCircleIcon className="text-destructive size-4 flex-none" />
                    ) : record?.status === "skipped" ? (
                      <CircleSlashIcon className="text-muted-foreground size-4 flex-none" />
                    ) : (
                      <CircleDashedIcon className="text-muted-foreground size-4 flex-none" />
                    )}
                    <span className={isNext ? "font-medium" : ""}>{STEP_LABEL[step]}</span>
                    {record?.detail && (
                      <span className="text-muted-foreground truncate text-xs">
                        {record.detail.slice(0, 80)}
                      </span>
                    )}
                    {isNext && running && <Badge variant="secondary">running…</Badge>}
                  </li>
                );
              })}
            </ol>

            {view.next_step === "run" && (
              <div className="flex flex-col gap-2 rounded-md border p-3">
                <Field>
                  <FieldLabel htmlFor="run-cmd">Verification command (runs in the sandbox)</FieldLabel>
                  <Input
                    id="run-cmd"
                    placeholder="python train.py --steps 100"
                    value={runCommand}
                    onChange={(e) => setRunCommand(e.target.value)}
                  />
                </Field>
                <Field>
                  <FieldLabel htmlFor="run-reported">
                    Reported metrics to compare (name=value, comma-separated)
                  </FieldLabel>
                  <Input
                    id="run-reported"
                    placeholder="loss=1.47, bleu=28.4"
                    value={reported}
                    onChange={(e) => setReported(e.target.value)}
                  />
                </Field>
                {view.plan && view.plan.setup_commands.length > 0 && (
                  <p className="text-muted-foreground text-xs">
                    Environment setup ({view.plan.kind}):{" "}
                    <code>{view.plan.setup_commands.join(" && ")}</code> — needs
                    network consent for downloads.
                  </p>
                )}
              </div>
            )}

            <div className="flex items-center gap-2">
              {view.next_step ? (
                <Button
                  disabled={running || (view.next_step === "run" && !runCommand.trim())}
                  onClick={advance}
                >
                  <PlayIcon data-icon="inline-start" />
                  {view.state.steps[view.next_step]?.status === "failed" ? "Retry: " : ""}
                  {STEP_LABEL[view.next_step]}
                </Button>
              ) : (
                <Badge variant="secondary">pipeline complete — see the report</Badge>
              )}
              {running && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() =>
                    activeRun.current &&
                    invoke("sandbox_kill", { runId: activeRun.current }).catch(() => {})
                  }
                >
                  <SquareIcon data-icon="inline-start" />
                  Stop
                </Button>
              )}
            </div>
            {notice && <p className="text-muted-foreground text-xs">{notice}</p>}
            {(running || log.length > 0) && (
              <pre className="bg-muted max-h-72 overflow-auto rounded-md p-2 text-xs">
                {log.join("\n") || "starting…"}
              </pre>
            )}
          </div>
        </ScrollArea>
      )}

      {tab === "files" && (
        <RepoBrowser
          paperId={paperId}
          codeMap={codeMap}
          labelFor={labelFor}
          target={target}
          onNavigateObject={onNavigateObject}
        />
      )}

      {tab === "architecture" && (
        <ScrollArea className="min-h-0 flex-1">
          <div className="mx-auto max-w-2xl p-4 text-sm">
            {architecture ? (
              <MessageResponse>{architecture}</MessageResponse>
            ) : (
              <p className="text-muted-foreground">
                Not generated yet — the Explain step writes this (needs an AI provider).
              </p>
            )}
          </div>
        </ScrollArea>
      )}

      {tab === "report" && (
        <ScrollArea className="min-h-0 flex-1">
          <div className="mx-auto max-w-2xl p-4 text-sm">
            {view.report ? (
              <MessageResponse>{view.report}</MessageResponse>
            ) : (
              <p className="text-muted-foreground">
                No report yet — it's written by the final pipeline step.
              </p>
            )}
          </div>
        </ScrollArea>
      )}

      <AlertDialog open={consentOpen} onOpenChange={setConsentOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Run this repository's pipeline in a sandbox?</AlertDialogTitle>
            <AlertDialogDescription className="flex flex-col gap-1.5">
              <span>
                <code>{view.repo.remote}</code> will be cloned into the library
                cache; its code runs only inside an isolated container:
              </span>
              <span>• Repo mounted read-only; memory/CPU/time limits; stoppable anytime</span>
              <span>• Network is OFF unless you allow it below (dependency downloads)</span>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="flex items-center gap-2">
            <Switch id="net-consent" checked={networkConsent} onCheckedChange={setNetworkConsent} />
            <label htmlFor="net-consent" className="text-sm">
              Allow network for this repo's runs (reason: dependency downloads)
            </label>
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel>Not now</AlertDialogCancel>
            <AlertDialogAction
              onClick={async () => {
                const scope = { kind: "repo", key: view.repo!.remote };
                await invoke("sandbox_grant", { paperId, scope }).catch(() => {});
                if (networkConsent) {
                  await invoke("sandbox_grant_network", {
                    paperId,
                    scope,
                    reason: "dependency downloads",
                  }).catch(() => {});
                }
                advance();
              }}
            >
              Approve
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

// ---------------------------------------------------------------------------

function RepoBrowser({
  paperId,
  codeMap,
  labelFor,
  target,
  onNavigateObject,
}: {
  paperId: string;
  codeMap: CodeLink[];
  labelFor: (objectId: string) => string | undefined;
  target?: { file: string; line: number } | null;
  onNavigateObject: (objectId: string) => void;
}) {
  const [files, setFiles] = useState<string[]>([]);
  const [selected, setSelected] = useState<string | null>(target?.file ?? null);
  const [content, setContent] = useState<string>("");
  const editorRef = useRef<ReactCodeMirrorRef>(null);

  useEffect(() => {
    invoke<string[]>("repro_list_files", { paperId })
      .then(setFiles)
      .catch(() => setFiles([]));
  }, [paperId]);

  useEffect(() => {
    if (target?.file) setSelected(target.file);
  }, [target]);

  useEffect(() => {
    if (!selected) return;
    invoke<string>("repro_read_file", { paperId, file: selected })
      .then(setContent)
      .catch((e) => setContent(`// could not read file: ${e}`));
  }, [paperId, selected]);

  // Object→code: scroll the editor to the target line once content loads.
  useEffect(() => {
    if (!target || target.file !== selected || !content) return;
    const view = editorRef.current?.view;
    if (!view) return;
    const line = Math.min(Math.max(target.line, 1), view.state.doc.lines);
    const pos = view.state.doc.line(line).from;
    view.dispatch({
      selection: { anchor: pos },
      effects: EditorView.scrollIntoView(pos, { y: "center" }),
    });
  }, [content, target, selected]);

  const fileLinks = useMemo(
    () => codeMap.filter((l) => l.file === selected),
    [codeMap, selected],
  );

  return (
    <div className="flex min-h-0 flex-1">
      <ScrollArea className="w-64 flex-none border-r">
        <ul className="p-2 text-xs">
          {files.length === 0 && (
            <li className="text-muted-foreground p-2">
              No clone yet — run the Clone step first.
            </li>
          )}
          {files.map((file) => (
            <li key={file}>
              <button
                className={
                  "w-full truncate rounded px-2 py-1 text-left hover:bg-accent " +
                  (file === selected ? "bg-accent" : "")
                }
                onClick={() => setSelected(file)}
              >
                {file}
                {codeMap.some((l) => l.file === file) && (
                  <span className="text-primary ml-1">●</span>
                )}
              </button>
            </li>
          ))}
        </ul>
      </ScrollArea>
      <div className="flex min-h-0 flex-1 flex-col">
        {/* Code→paper: this file's mapped objects, one click to the reader. */}
        {fileLinks.length > 0 && (
          <div className="flex flex-none flex-wrap gap-1 border-b p-2">
            {fileLinks.map((link, i) => (
              <Button
                key={i}
                variant="outline"
                size="sm"
                className={link.confidence < 0.6 ? "border-dashed" : ""}
                title={
                  `lines ${link.start_line}–${link.end_line}` +
                  (link.confidence < 0.6 ? " (low confidence)" : "")
                }
                onClick={() => onNavigateObject(link.object)}
              >
                {labelFor(link.object) ?? "paper object"} · L{link.start_line}
              </Button>
            ))}
          </div>
        )}
        {selected ? (
          <CodeMirror
            ref={editorRef}
            value={content}
            readOnly
            height="100%"
            className="min-h-0 flex-1 overflow-auto text-xs"
            theme={document.documentElement.classList.contains("dark") ? "dark" : "light"}
            extensions={[selected.endsWith(".rs") ? rust() : python()]}
          />
        ) : (
          <p className="text-muted-foreground p-4 text-sm">Select a file to view it.</p>
        )}
      </div>
    </div>
  );
}
