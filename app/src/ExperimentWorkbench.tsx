import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@/platform";
import { listen } from "@/platform";
import { CartesianGrid, Line, LineChart, XAxis, YAxis } from "recharts";
import {
  FlaskConicalIcon,
  PlayIcon,
  PlusIcon,
  SendIcon,
  SquareIcon,
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
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from "@/components/ui/input-group";
import { MessageResponse } from "@/components/ai-elements/message";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Spinner } from "@/components/ui/spinner";
import CapabilityBanner from "./chrome/CapabilityBanner";

type Language = "python" | "pytorch" | "tensorflow" | "jax" | "rust";

interface ParameterSpec {
  name: string;
  kind: string;
  default: string;
}

interface Experiment {
  id: string;
  name: string;
  object_id: string;
  language: Language;
  parameters: ParameterSpec[];
}

interface ExperimentRun {
  run_id: string;
  params: Record<string, string>;
  metrics: Record<string, number>;
  stdout_tail: string;
  duration_ms: number;
  status: string;
  prediction?: string | null;
  at: string;
}

interface StoredChatMessage {
  role: string;
  content: string;
  incomplete?: boolean;
}

/**
 * Experiment mode (v3): tweak parameters → run in the sandbox → observe →
 * chart → discuss with the AI. Runs are append-only records in the bundle;
 * the chart derives from them live (nothing rendered is ever stored).
 * Predict–observe–explain: predictions are captured before the run.
 */
export default function ExperimentWorkbench({
  paperId,
  labelFor,
}: {
  paperId: string;
  labelFor: (objectId: string) => string | undefined;
}) {
  const [experiments, setExperiments] = useState<Experiment[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const refresh = () => {
    invoke<Experiment[]>("experiment_list", { paperId })
      .then((list) => {
        setExperiments(list);
        setActiveId((current) => current ?? list[list.length - 1]?.id ?? null);
      })
      .catch(() => {});
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(refresh, [paperId]);

  const active = experiments.find((e) => e.id === activeId) ?? null;

  return (
    <div className="flex h-full min-h-0">
      <CapabilityBanner id="experiments" />
      <ScrollArea className="w-56 flex-none border-r">
        <div className="flex flex-col gap-1 p-2 pt-10">
          <Button variant="outline" size="sm" onClick={() => setCreating(true)}>
            <PlusIcon data-icon="inline-start" />
            New experiment
          </Button>
          {experiments.map((experiment) => (
            <button
              key={experiment.id}
              className={
                "flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-accent " +
                (experiment.id === activeId ? "bg-accent" : "")
              }
              onClick={() => setActiveId(experiment.id)}
            >
              <FlaskConicalIcon className="size-3.5 flex-none" />
              <span className="truncate">{experiment.name}</span>
            </button>
          ))}
        </div>
      </ScrollArea>

      {active ? (
        <ExperimentView key={active.id} paperId={paperId} experiment={active} labelFor={labelFor} />
      ) : (
        <div className="flex flex-1 items-center justify-center p-6">
          <Empty>
            <EmptyHeader>
              <EmptyTitle>No experiments yet</EmptyTitle>
              <EmptyDescription>
                Create one over any equation you've generated an implementation
                for — tweak parameters, run it sandboxed, and discuss what
                happens.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        </div>
      )}

      {creating && (
        <CreateExperimentDialog
          paperId={paperId}
          labelFor={labelFor}
          onClose={() => setCreating(false)}
          onCreated={(experiment) => {
            setCreating(false);
            refresh();
            setActiveId(experiment.id);
          }}
        />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------

function ExperimentView({
  paperId,
  experiment,
  labelFor,
}: {
  paperId: string;
  experiment: Experiment;
  labelFor: (objectId: string) => string | undefined;
}) {
  const [runs, setRuns] = useState<ExperimentRun[]>([]);
  const [values, setValues] = useState<Record<string, string>>(() =>
    Object.fromEntries(experiment.parameters.map((p) => [p.name, p.default])),
  );
  const [prediction, setPrediction] = useState("");
  const [running, setRunning] = useState(false);
  const [liveLog, setLiveLog] = useState<string[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [consentOpen, setConsentOpen] = useState(false);
  const activeRun = useRef<string | null>(null);

  const refreshRuns = () => {
    invoke<ExperimentRun[]>("experiment_runs", { paperId, experimentId: experiment.id })
      .then(setRuns)
      .catch(() => {});
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(refreshRuns, [paperId, experiment.id]);

  useEffect(() => {
    const unlisten = listen<{
      run_id: string;
      line?: string;
      outcome?: unknown;
      error?: string;
    }>("sandbox-progress", ({ payload }) => {
      if (payload.run_id !== activeRun.current) return;
      if (payload.line !== undefined) setLiveLog((l) => [...l.slice(-400), payload.line!]);
      if (payload.outcome || payload.error) {
        setRunning(false);
        if (payload.error === "consent_required") setConsentOpen(true);
        else if (payload.error) setNotice(payload.error);
        refreshRuns();
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [experiment.id]);

  async function run() {
    const consents = await invoke<[{ kind?: string; key?: string }, boolean, string][]>(
      "sandbox_consents",
      { paperId },
    ).catch(() => []);
    const granted = consents.some(
      ([scope]) => scope.kind === "experiment" && scope.key === experiment.id,
    );
    if (!granted) {
      setConsentOpen(true);
      return;
    }
    startRun();
  }

  function startRun() {
    const runId = crypto.randomUUID();
    activeRun.current = runId;
    setLiveLog([]);
    setRunning(true);
    setNotice(null);
    invoke("experiment_run", {
      runId,
      paperId,
      experimentId: experiment.id,
      params: values,
      prediction: prediction.trim() || null,
    }).catch((e) => {
      setRunning(false);
      setNotice(String(e));
    });
    setPrediction("");
  }

  // Chart data derives live from runs (never stored): x = run index,
  // one line per metric key.
  const metricKeys = useMemo(() => {
    const keys = new Set<string>();
    for (const run of runs) Object.keys(run.metrics).forEach((k) => keys.add(k));
    return [...keys].slice(0, 4);
  }, [runs]);
  const chartData = useMemo(
    () =>
      runs
        .filter((r) => r.status === "completed")
        .map((run, i) => ({
          run: i + 1,
          ...run.metrics,
        })),
    [runs],
  );
  const chartConfig: ChartConfig = Object.fromEntries(
    metricKeys.map((key, i) => [key, { label: key, color: `var(--chart-${i + 1})` }]),
  );

  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className="mx-auto flex max-w-2xl flex-col gap-4 p-6 pt-10">
        <div className="flex items-center gap-2">
          <h2 className="flex-1 truncate text-base font-semibold">{experiment.name}</h2>
          <Badge variant="outline">{labelFor(experiment.object_id) ?? "implementation"}</Badge>
        </div>

        <div className="flex flex-wrap items-end gap-2">
          {experiment.parameters.map((parameter) => (
            <Field key={parameter.name} className="w-36">
              <FieldLabel htmlFor={`param-${parameter.name}`}>{parameter.name}</FieldLabel>
              <Input
                id={`param-${parameter.name}`}
                value={values[parameter.name] ?? ""}
                onChange={(e) =>
                  setValues((v) => ({ ...v, [parameter.name]: e.target.value }))
                }
              />
            </Field>
          ))}
          <Button disabled={running} onClick={run}>
            <PlayIcon data-icon="inline-start" />
            Run
          </Button>
          {running && (
            <Button
              variant="ghost"
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
        <Field>
          <FieldLabel htmlFor="prediction">
            Prediction (optional — recorded before the run)
          </FieldLabel>
          <Input
            id="prediction"
            placeholder="What do you expect to happen?"
            value={prediction}
            onChange={(e) => setPrediction(e.target.value)}
          />
        </Field>
        {notice && <p className="text-muted-foreground text-xs">{notice}</p>}
        {(running || liveLog.length > 0) && (
          <pre className="bg-muted max-h-32 overflow-auto rounded-md p-2 text-xs">
            {liveLog.join("\n") || "starting sandbox…"}
          </pre>
        )}

        {chartData.length > 1 && metricKeys.length > 0 && (
          <ChartContainer config={chartConfig} className="h-56 w-full">
            <LineChart data={chartData}>
              <CartesianGrid vertical={false} />
              <XAxis dataKey="run" tickLine={false} axisLine={false} />
              <YAxis tickLine={false} axisLine={false} width={40} />
              <ChartTooltip content={<ChartTooltipContent />} />
              {metricKeys.map((key) => (
                <Line
                  key={key}
                  dataKey={key}
                  type="monotone"
                  stroke={`var(--color-${key})`}
                  dot={false}
                  isAnimationActive={false}
                />
              ))}
            </LineChart>
          </ChartContainer>
        )}

        {runs.length > 0 && (
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead>
                <tr className="text-muted-foreground text-left">
                  <th className="py-1 pr-2">#</th>
                  <th className="py-1 pr-2">params</th>
                  <th className="py-1 pr-2">metrics</th>
                  <th className="py-1 pr-2">status</th>
                  <th className="py-1">prediction</th>
                </tr>
              </thead>
              <tbody>
                {runs.map((run, i) => (
                  <tr key={run.run_id} className="border-t align-top">
                    <td className="py-1 pr-2 tabular-nums">{i + 1}</td>
                    <td className="py-1 pr-2">
                      {Object.entries(run.params)
                        .map(([k, v]) => `${k}=${v}`)
                        .join(", ")}
                    </td>
                    <td className="py-1 pr-2 tabular-nums">
                      {Object.entries(run.metrics)
                        .map(([k, v]) => `${k}=${v}`)
                        .join(", ") || "—"}
                    </td>
                    <td className="py-1 pr-2">
                      {run.status === "completed" ? (
                        run.status
                      ) : (
                        <Badge variant="outline">{run.status}</Badge>
                      )}
                    </td>
                    <td className="py-1">{run.prediction ?? ""}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        <Separator />
        <Discussion paperId={paperId} experimentId={experiment.id} />
      </div>

      <AlertDialog open={consentOpen} onOpenChange={setConsentOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Run this experiment in a sandbox?</AlertDialogTitle>
            <AlertDialogDescription className="flex flex-col gap-1.5">
              <span>“{experiment.name}” will run in an isolated container:</span>
              <span>• No network access</span>
              <span>• Only the implementation's folder mounted, read-only</span>
              <span>• Parameters passed as environment variables</span>
              <span>• Memory, CPU, and time limits; stoppable anytime</span>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Not now</AlertDialogCancel>
            <AlertDialogAction
              onClick={async () => {
                await invoke("sandbox_grant", {
                  paperId,
                  scope: { kind: "experiment", key: experiment.id },
                }).catch(() => {});
                startRun();
              }}
            >
              Run in sandbox
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </ScrollArea>
  );
}

// ---------------------------------------------------------------------------

function Discussion({ paperId, experimentId }: { paperId: string; experimentId: string }) {
  const [history, setHistory] = useState<StoredChatMessage[]>([]);
  const [question, setQuestion] = useState("");
  const [streamText, setStreamText] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const activeRequest = useRef<string | null>(null);

  const refresh = () => {
    invoke<StoredChatMessage[]>("chat_history", { paperId, objectId: experimentId })
      .then(setHistory)
      .catch(() => {});
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(refresh, [paperId, experimentId]);

  useEffect(() => {
    const unlisten = listen<{
      request_id: string;
      token?: string;
      done?: boolean;
      error?: string;
      cancelled?: boolean;
    }>("ai-stream", ({ payload }) => {
      if (payload.request_id !== activeRequest.current) return;
      if (payload.token) setStreamText((t) => t + payload.token);
      if (payload.done || payload.cancelled || payload.error) {
        setStreaming(false);
        setStreamText("");
        if (payload.error) setError(payload.error);
        refresh();
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [experimentId]);

  function ask(text: string) {
    const requestId = crypto.randomUUID();
    activeRequest.current = requestId;
    setError(null);
    setStreamText("");
    setStreaming(true);
    invoke("experiment_stream", {
      requestId,
      paperId,
      experimentId,
      question: text,
    }).catch((e) => {
      setStreaming(false);
      setError(String(e));
      refresh();
    });
  }

  return (
    <div className="flex flex-col gap-2">
      {history
        .filter((m) => m.content.trim().length > 0)
        .map((message, i) =>
          message.role === "user" ? (
            <p key={i} className="text-muted-foreground pl-4 text-sm">
              You: {message.content}
            </p>
          ) : (
            <div key={i} className="text-sm">
              <MessageResponse>{message.content}</MessageResponse>
            </div>
          ),
        )}
      {streamText && (
        <div className="text-sm">
          <MessageResponse>{streamText}</MessageResponse>
        </div>
      )}
      {streaming && !streamText && <Spinner />}
      {error && <p className="text-muted-foreground text-xs">{error}</p>}
      <div className="flex gap-1.5">
        <InputGroup>
          <InputGroupInput
            placeholder="Discuss the results…"
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && question.trim()) {
                ask(question.trim());
                setQuestion("");
              }
            }}
          />
          <InputGroupAddon align="inline-end">
            <InputGroupButton
              size="sm"
              disabled={streaming || !question.trim()}
              onClick={() => {
                ask(question.trim());
                setQuestion("");
              }}
            >
              <SendIcon />
            </InputGroupButton>
          </InputGroupAddon>
        </InputGroup>
        <Button
          variant="outline"
          size="sm"
          disabled={streaming}
          onClick={() =>
            ask(
              "Propose one concrete experiment: which parameter should I change and to what, " +
                "ask me to predict the outcome before running, and explain what the result would teach me.",
            )
          }
        >
          Propose an experiment
        </Button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------

function CreateExperimentDialog({
  paperId,
  labelFor,
  onClose,
  onCreated,
}: {
  paperId: string;
  labelFor: (objectId: string) => string | undefined;
  onClose: () => void;
  onCreated: (experiment: Experiment) => void;
}) {
  const [name, setName] = useState("");
  const [objectId, setObjectId] = useState("");
  const [language, setLanguage] = useState<Language>("python");
  const [params, setParams] = useState("learning_rate=0.01");
  const [error, setError] = useState<string | null>(null);

  async function create() {
    const parameters: ParameterSpec[] = params
      .split(",")
      .map((pair) => pair.trim())
      .filter(Boolean)
      .map((pair) => {
        const [n, d = ""] = pair.split("=");
        return { name: n.trim(), kind: "number", default: d.trim() };
      });
    try {
      const experiment = await invoke<Experiment>("experiment_create", {
        paperId,
        name: name.trim() || "Untitled experiment",
        objectId,
        language,
        parameters,
      });
      onCreated(experiment);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <AlertDialog open onOpenChange={(open) => !open && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>New experiment</AlertDialogTitle>
          <AlertDialogDescription>
            Anchored to an object you've generated an implementation for.
            Parameters reach the code as env vars (EXP_NAME); print{" "}
            <code>{'{"metric": value}'}</code> lines to record metrics.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <div className="flex flex-col gap-3">
          <Field>
            <FieldLabel htmlFor="exp-name">Name</FieldLabel>
            <Input id="exp-name" value={name} onChange={(e) => setName(e.target.value)} />
          </Field>
          <Field>
            <FieldLabel htmlFor="exp-object">Object id (equation with an implementation)</FieldLabel>
            <Input
              id="exp-object"
              placeholder="paste from the equation panel"
              value={objectId}
              onChange={(e) => setObjectId(e.target.value)}
            />
            {objectId && (
              <p className="text-muted-foreground text-xs">{labelFor(objectId) ?? ""}</p>
            )}
          </Field>
          <Field>
            <FieldLabel htmlFor="exp-lang">Language</FieldLabel>
            <div className="flex gap-1">
              {(["python", "pytorch", "tensorflow", "jax", "rust"] as Language[]).map((l) => (
                <Button
                  key={l}
                  variant={language === l ? "secondary" : "outline"}
                  size="sm"
                  onClick={() => setLanguage(l)}
                >
                  {l}
                </Button>
              ))}
            </div>
          </Field>
          <Field>
            <FieldLabel htmlFor="exp-params">Parameters (name=default, comma-separated)</FieldLabel>
            <Input id="exp-params" value={params} onChange={(e) => setParams(e.target.value)} />
          </Field>
          {error && <p className="text-destructive text-xs">{error}</p>}
        </div>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction disabled={!objectId.trim()} onClick={create}>
            Create
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
