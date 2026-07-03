import { useEffect, useState } from "react";
import { invoke } from "@/platform";
import { Trash2Icon } from "lucide-react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { FieldDescription } from "@/components/ui/field";
import { Separator } from "@/components/ui/separator";

interface ConceptMastery {
  concept: string;
  score: number;
  signals: number;
  estimated: boolean;
}

interface LearnerSnapshot {
  mastery: ConceptMastery[];
  preferences: Record<string, string>;
  episodes: number;
}

const STORES = [
  { id: "mastery", label: "Mastery", hint: "quiz and tutor outcomes per concept" },
  { id: "preferences", label: "Preferences", hint: "learning-style signals" },
  { id: "episodes", label: "Episodes", hint: "summaries of past confusions/insights" },
] as const;

/** Learner-model inspection + reset (per store and wholesale). Everything
 * here lives in `learning_state/` on this machine only — deleting it never
 * touches papers, notes, or chats. */
export default function LearningData() {
  const [snapshot, setSnapshot] = useState<LearnerSnapshot | null>(null);

  const refresh = () => {
    invoke<LearnerSnapshot>("learning_snapshot").then(setSnapshot).catch(() => {});
  };
  useEffect(refresh, []);

  async function reset(store?: string) {
    await invoke("learning_reset", { store: store ?? null }).catch(() => {});
    refresh();
  }

  const tracked = snapshot?.mastery.length ?? 0;
  const confident = snapshot?.mastery.filter((m) => !m.estimated).length ?? 0;

  const storeCount = (id: string) => {
    if (!snapshot) return 0;
    if (id === "mastery") return tracked;
    if (id === "preferences") return Object.keys(snapshot.preferences).length;
    return snapshot.episodes;
  };

  return (
    <div className="flex flex-col gap-4">
      <div>
        <FieldDescription>
          Sent to a provider only as a compact profile when you invoke an AI
          action.{" "}
          {tracked > 0
            ? `${tracked} concept${tracked === 1 ? "" : "s"} tracked (${confident} past the estimate threshold), ${snapshot?.episodes ?? 0} episode${snapshot?.episodes === 1 ? "" : "s"}.`
            : "Nothing recorded yet."}
        </FieldDescription>
      </div>
      <div className="flex flex-col gap-2">
        {STORES.map((store) => (
          <div key={store.id} className="flex items-center gap-2">
            <span className="text-sm">{store.label}</span>
            <Badge variant="secondary">{storeCount(store.id)}</Badge>
            <span className="text-muted-foreground flex-1 truncate text-xs">{store.hint}</span>
            <ResetConfirm
              title={`Reset ${store.label.toLowerCase()}?`}
              description={`Deletes all recorded ${store.label.toLowerCase()} data. Papers, notes, and chats are not affected. This cannot be undone.`}
              onConfirm={() => reset(store.id)}
            >
              <Button variant="ghost" size="icon-sm" disabled={storeCount(store.id) === 0}>
                <Trash2Icon />
              </Button>
            </ResetConfirm>
          </div>
        ))}
      </div>
      <Separator />
      <ResetConfirm
        title="Reset all learning data?"
        description="Deletes mastery, preferences, and episode memory entirely. The dashboard returns to its cold-start state. Papers, notes, and chats are not affected. This cannot be undone."
        onConfirm={() => reset()}
      >
        <Button
          variant="outline"
          size="sm"
          className="self-start"
          disabled={tracked === 0 && (snapshot?.episodes ?? 0) === 0}
        >
          <Trash2Icon data-icon="inline-start" />
          Reset all learning data
        </Button>
      </ResetConfirm>
    </div>
  );
}

function ResetConfirm({
  title,
  description,
  onConfirm,
  children,
}: {
  title: string;
  description: string;
  onConfirm: () => void;
  children: React.ReactNode;
}) {
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>{children}</AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction onClick={onConfirm}>Reset</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
