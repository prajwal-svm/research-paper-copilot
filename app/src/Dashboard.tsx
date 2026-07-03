import { useEffect, useState } from "react";
import { invoke } from "@/platform";
import { ArrowLeftIcon, ArrowRightIcon, WaypointsIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import type { KnowledgeGraph } from "./GraphView";

interface ConceptMastery {
  concept: string;
  score: number;
  signals: number;
  estimated: boolean;
  due: boolean;
}

interface LearnerSnapshot {
  mastery: ConceptMastery[];
  episodes: number;
}

const SKIP_KEY = "rpc-skip-dashboard";

export function dashboardSkipped(): boolean {
  return localStorage.getItem(SKIP_KEY) === "1";
}

/**
 * Paper dashboard (v2): honest, mastery-derived progress shown before the
 * reader. All figures come from two local file reads (graph + learner
 * snapshot — well under the 500 ms budget). Until enough quiz signal exists
 * the numbers are labeled estimates, and nothing here ever gates content:
 * "Continue reading" is always one click and restores the exact position.
 */
export default function Dashboard({
  paperId,
  title,
  onContinue,
  onBack,
}: {
  paperId: string;
  title: string;
  /** Opens the reader, which restores the persisted reading position. */
  onContinue: () => void;
  onBack: () => void;
}) {
  const [graph, setGraph] = useState<KnowledgeGraph | null>(null);
  const [snapshot, setSnapshot] = useState<LearnerSnapshot | null>(null);
  const [skip, setSkip] = useState(dashboardSkipped());

  useEffect(() => {
    invoke<KnowledgeGraph | null>("graph_get", { paperId })
      .then(setGraph)
      .catch(() => {});
    invoke<LearnerSnapshot>("learning_snapshot").then(setSnapshot).catch(() => {});
  }, [paperId]);

  const conceptIds = new Set(graph?.nodes.map((n) => n.id) ?? []);
  const paperMastery =
    snapshot?.mastery.filter((m) => conceptIds.has(m.concept)) ?? [];
  const mastered = paperMastery.filter((m) => !m.estimated && m.score >= 0.6);
  const due = paperMastery.filter((m) => m.due);
  const totalSignals = paperMastery.reduce((a, m) => a + m.signals, 0);
  // Honest cold start: with little signal, everything is an estimate.
  const estimated = totalSignals < 3 * Math.max(1, mastered.length);

  return (
    <div className="flex h-screen flex-col">
      <div data-tauri-drag-region className="h-9 flex-none" />
      <div className="mx-auto flex w-full max-w-2xl flex-1 flex-col gap-6 overflow-y-auto px-6 pb-10">
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="icon-sm" onClick={onBack}>
            <ArrowLeftIcon />
          </Button>
          <h1 className="truncate text-lg font-semibold">{title}</h1>
        </div>

        <div className="grid grid-cols-2 gap-4 sm:grid-cols-3">
          <Card>
            <CardHeader>
              <CardTitle className="text-2xl tabular-nums">
                {graph ? graph.nodes.length : "—"}
              </CardTitle>
              <CardDescription>
                concepts in this paper
                {graph?.extraction === "heuristic" && " (limited graph)"}
              </CardDescription>
            </CardHeader>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle className="flex items-baseline gap-2 text-2xl tabular-nums">
                {mastered.length}
                {estimated && <Badge variant="outline">estimated</Badge>}
              </CardTitle>
              <CardDescription>
                {totalSignals === 0
                  ? "mastered — take a quiz to find out"
                  : "concepts mastered"}
              </CardDescription>
            </CardHeader>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle className="text-2xl tabular-nums">{due.length}</CardTitle>
              <CardDescription>due for review</CardDescription>
            </CardHeader>
          </Card>
        </div>

        <Card>
          <CardHeader>
            <CardTitle>Pick up where you left off</CardTitle>
            <CardDescription>
              Your exact reading position, notes, and conversations are
              restored. Progress here never locks anything — every section and
              lesson stays one click away.
            </CardDescription>
          </CardHeader>
          <CardContent className="flex items-center justify-between gap-4">
            <Button onClick={onContinue} autoFocus>
              Continue reading
              <ArrowRightIcon data-icon="inline-end" />
            </Button>
            {graph && graph.nodes.length > 0 && (
              <span className="text-muted-foreground flex items-center gap-1 text-xs">
                <WaypointsIcon className="size-3.5" />
                Concept graph available in the reader dock
              </span>
            )}
          </CardContent>
        </Card>

        <div className="flex items-center gap-2">
          <Switch
            id="skip-dashboard"
            checked={skip}
            onCheckedChange={(v) => {
              setSkip(v);
              localStorage.setItem(SKIP_KEY, v ? "1" : "0");
            }}
          />
          <label htmlFor="skip-dashboard" className="text-muted-foreground text-sm">
            Skip this screen and open papers directly
          </label>
        </div>
      </div>
    </div>
  );
}
