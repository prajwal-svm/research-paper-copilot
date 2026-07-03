import { useEffect, useRef, useState } from "react";
import { invoke } from "@/platform";
import { listen } from "@/platform";
import CodeMirror from "@uiw/react-codemirror";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import {
  CheckCircle2Icon,
  CircleDashedIcon,
  PlayIcon,
  RotateCcwIcon,
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
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";

type Language = "python" | "pytorch" | "tensorflow" | "jax" | "rust";

const LANGUAGES: { id: Language; label: string }[] = [
  { id: "python", label: "Python" },
  { id: "pytorch", label: "PyTorch" },
  { id: "tensorflow", label: "TensorFlow" },
  { id: "jax", label: "JAX" },
  { id: "rust", label: "Rust" },
];

interface ImplementationMeta {
  language: Language;
  provenance: string;
  user_edited: boolean;
  check_status: "unverified" | "passed" | "failed";
  guidance: string[];
  stale: boolean;
}

interface Implementation {
  meta: ImplementationMeta;
  code: string;
  checks?: string | null;
  last_output?: string | null;
}

interface RuntimeInfo {
  program: string;
  version: string;
}

interface SandboxEvent {
  run_id: string;
  line?: string;
  outcome?: { status: { kind: string; reason?: string; exit_code?: number } };
  error?: string;
}

/**
 * Implementation mode (v3): generate → edit → run, all inside the object
 * panel. Code lives in the bundle (`implementations/`), edits are never
 * silently regenerated, and every run goes through the consented sandbox —
 * no runtime and no key are both designed states, not errors.
 */
export default function ImplementationPanel({
  paperId,
  objectId,
}: {
  paperId: string;
  objectId: string;
}) {
  const [language, setLanguage] = useState<Language>("python");
  const [present, setPresent] = useState<Language[]>([]);
  const [impl, setImpl] = useState<Implementation | null>(null);
  const [generating, setGenerating] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [runtime, setRuntime] = useState<RuntimeInfo | null | undefined>(undefined);
  const [running, setRunning] = useState(false);
  const [liveLog, setLiveLog] = useState<string[]>([]);
  const [consentOpen, setConsentOpen] = useState(false);
  const pendingChecks = useRef(false);
  const activeRun = useRef<string | null>(null);
  const draft = useRef<string>("");
  const generateRequest = useRef<string | null>(null);

  const refresh = (lang: Language) => {
    invoke<{ implementation: Implementation | null; languages_present: Language[] }>(
      "implementation_get",
      { paperId, objectId, language: lang },
    )
      .then((v) => {
        setImpl(v.implementation);
        setPresent(v.languages_present);
        draft.current = v.implementation?.code ?? "";
      })
      .catch(() => {});
  };

  useEffect(() => {
    refresh(language);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [paperId, objectId, language]);

  useEffect(() => {
    invoke<RuntimeInfo | null>("sandbox_runtime_status")
      .then(setRuntime)
      .catch(() => setRuntime(null));
  }, []);

  useEffect(() => {
    const unlisten = listen<SandboxEvent>("sandbox-progress", ({ payload }) => {
      if (payload.run_id !== activeRun.current) return;
      if (payload.line !== undefined) setLiveLog((l) => [...l.slice(-400), payload.line!]);
      if (payload.outcome || payload.error) {
        setRunning(false);
        if (payload.error === "consent_required") setConsentOpen(true);
        else if (payload.error) setNotice(payload.error);
        refresh(language);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [language]);

  async function generate(force = false) {
    setGenerating(true);
    setNotice(null);
    const requestId = crypto.randomUUID();
    generateRequest.current = requestId;
    try {
      const result = await invoke<Implementation | null>("implementation_generate", {
        requestId,
        paperId,
        objectId,
        language,
        force,
      });
      if (result) {
        setImpl(result);
        draft.current = result.code;
        refresh(language);
      } else {
        setNotice("Generation needs an AI provider (Settings). Cached implementations still run without one.");
      }
    } catch (e) {
      setNotice(String(e));
    } finally {
      setGenerating(false);
    }
  }

  async function saveEdit() {
    if (!impl || draft.current === impl.code) return;
    await invoke("implementation_save_edit", {
      paperId,
      objectId,
      language,
      code: draft.current,
    }).catch(() => {});
    refresh(language);
  }

  async function run(withChecks: boolean) {
    await saveEdit();
    // First run in this paper needs explicit consent — check before running.
    const consents = await invoke<[unknown, boolean, string][]>("sandbox_consents", {
      paperId,
    }).catch(() => []);
    const granted = consents.some(
      ([scope]) => (scope as { kind?: string }).kind === "implementations",
    );
    if (!granted) {
      pendingChecks.current = withChecks;
      setConsentOpen(true);
      return;
    }
    startRun(withChecks);
  }

  function startRun(withChecks: boolean) {
    const runId = crypto.randomUUID();
    activeRun.current = runId;
    setLiveLog([]);
    setRunning(true);
    setNotice(null);
    invoke("implementation_run", {
      runId,
      paperId,
      objectId,
      language,
      withChecks,
    }).catch((e) => {
      setRunning(false);
      setNotice(String(e));
    });
  }

  function kill() {
    if (activeRun.current) {
      invoke("sandbox_kill", { runId: activeRun.current }).catch(() => {});
    }
  }

  const status = impl?.meta.check_status;

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap gap-1">
        {LANGUAGES.map((l) => (
          <Button
            key={l.id}
            variant={language === l.id ? "secondary" : "outline"}
            size="sm"
            onClick={() => setLanguage(l.id)}
          >
            {l.label}
            {present.includes(l.id) && <CheckCircle2Icon data-icon="inline-end" />}
          </Button>
        ))}
      </div>

      {impl ? (
        <>
          <div className="flex flex-wrap items-center gap-1.5">
            {status === "passed" ? (
              <Badge variant="secondary">
                <CheckCircle2Icon data-icon="inline-start" />
                verified
              </Badge>
            ) : status === "failed" ? (
              <Badge variant="destructive">
                <XCircleIcon data-icon="inline-start" />
                checks failed
              </Badge>
            ) : (
              <Badge variant="outline">
                <CircleDashedIcon data-icon="inline-start" />
                generated, not yet verified
              </Badge>
            )}
            {impl.meta.user_edited && <Badge variant="outline">edited</Badge>}
            {impl.meta.stale && <Badge variant="outline">source changed — review</Badge>}
          </div>

          <CodeMirror
            value={impl.code}
            height="260px"
            theme={document.documentElement.classList.contains("dark") ? "dark" : "light"}
            extensions={[language === "rust" ? rust() : python()]}
            onChange={(value) => (draft.current = value)}
            onBlur={saveEdit}
          />

          {impl.meta.guidance.length > 0 && (
            <ul className="text-muted-foreground flex flex-col gap-0.5 text-xs">
              {impl.meta.guidance.slice(0, 6).map((g, i) => (
                <li key={i}>• {g}</li>
              ))}
            </ul>
          )}

          {runtime === null ? (
            <p className="text-muted-foreground text-xs">
              Running code needs Docker or Podman installed — everything else
              here (viewing, editing) works without one.
            </p>
          ) : (
            <div className="flex items-center gap-1.5">
              <Button size="sm" disabled={running} onClick={() => run(false)}>
                <PlayIcon data-icon="inline-start" />
                Run
              </Button>
              {impl.checks && (
                <Button variant="outline" size="sm" disabled={running} onClick={() => run(true)}>
                  Run checks
                </Button>
              )}
              {running && (
                <Button variant="ghost" size="sm" onClick={kill}>
                  <SquareIcon data-icon="inline-start" />
                  Stop
                </Button>
              )}
              <Button
                variant="ghost"
                size="icon-sm"
                disabled={generating || running}
                onClick={() => generate(true)}
                title="Regenerate (replaces current code after confirmation)"
              >
                <RotateCcwIcon />
              </Button>
            </div>
          )}

          {(running || liveLog.length > 0 || impl.last_output) && (
            <pre className="bg-muted max-h-40 overflow-auto rounded-md p-2 text-xs">
              {running || liveLog.length > 0
                ? liveLog.join("\n") || "starting sandbox…"
                : impl.last_output}
            </pre>
          )}
        </>
      ) : generating ? (
        <div className="flex flex-col gap-2">
          <Skeleton className="h-4 w-2/3" />
          <Skeleton className="h-40 w-full" />
        </div>
      ) : (
        <Button
          variant="outline"
          size="sm"
          className="self-start"
          disabled={generating}
          onClick={() => generate(false)}
        >
          {generating && <Spinner data-icon="inline-start" />}
          Generate {LANGUAGES.find((l) => l.id === language)?.label} implementation
        </Button>
      )}
      {notice && <p className="text-muted-foreground text-xs">{notice}</p>}

      {/* Explicit consent: exactly what runs, what is mounted, network off. */}
      <AlertDialog open={consentOpen} onOpenChange={setConsentOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Run generated code in a sandbox?</AlertDialogTitle>
            <AlertDialogDescription className="flex flex-col gap-1.5">
              <span>
                This paper's implementations will run in an isolated container:
              </span>
              <span>• No network access</span>
              <span>• Only this paper's <code>implementations/</code> folder mounted, read-only</span>
              <span>• Memory, CPU, and time limits; stoppable anytime</span>
              <span>
                You're approving runs for this paper's implementations; revoke
                anytime in the paper's consent list.
              </span>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Not now</AlertDialogCancel>
            <AlertDialogAction
              onClick={async () => {
                await invoke("sandbox_grant", {
                  paperId,
                  scope: { kind: "implementations" },
                }).catch(() => {});
                startRun(pendingChecks.current);
              }}
            >
              Run in sandbox
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
