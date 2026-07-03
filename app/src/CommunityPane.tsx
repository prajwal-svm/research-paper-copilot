import { useCallback, useEffect, useState } from "react";
import { invoke } from "@/platform";
import { toast } from "sonner";
import {
  CheckIcon,
  CloudDownloadIcon,
  CloudUploadIcon,
  GitPullRequestIcon,
  HistoryIcon,
  Undo2Icon,
  XIcon,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";

interface Proposal {
  id: string;
  base_revision: string;
  author: { id: string; display_name?: string };
  summary: string;
  created_at: string;
  status: "queued" | "submitted" | "merged" | "rejected";
  changes: unknown[];
}
interface ProvenanceEvent {
  at: string;
  actor: { id: string };
  event: string;
  proposal_id?: string;
  accepted?: boolean;
  reason?: string;
}
interface Overview {
  proposals: Proposal[];
  revision: string;
  events: ProvenanceEvent[];
  reputation: Record<
    string,
    { accepted: number; rejected: number; reverted: number; reviews_performed: number }
  >;
  my_trust: "new" | "trusted" | "maintainer";
}
interface RegistryCheck {
  eligible: boolean;
  canonical_id: string | null;
  layers: { version: number; publisher: string; created_at: string; artifacts: unknown[] }[];
}
interface PublishPreview {
  included: string[];
  excluded: [string, string][];
}
interface ChangePreview {
  path: string;
  kind: string;
  preview: string;
}

const STATUS_VARIANT: Record<Proposal["status"], "outline" | "secondary" | "destructive"> = {
  queued: "outline",
  submitted: "outline",
  merged: "secondary",
  rejected: "destructive",
};

/** Community pane (v5): the paper as a shared knowledge object — registry
 * pull/publish with consent + preview, PR-style proposals with review,
 * append-only provenance, and reputation derived from the public record. */
export default function CommunityPane({ paperId }: { paperId: string }) {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [registry, setRegistry] = useState<RegistryCheck | null>(null);
  const [registryError, setRegistryError] = useState<string | null>(null);
  const [identity, setIdentity] = useState("");
  const [summary, setSummary] = useState("");
  const [includeNotes, setIncludeNotes] = useState(true);
  const [includeCanvas, setIncludeCanvas] = useState(true);
  const [diff, setDiff] = useState<{ proposal: Proposal; changes: ChangePreview[] } | null>(null);
  const [publishPreview, setPublishPreview] = useState<PublishPreview | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => {
    invoke<Overview>("contribution_overview", { paperId }).then(setOverview).catch(() => {});
  }, [paperId]);
  useEffect(refresh, [refresh]);

  async function checkRegistry() {
    setRegistryError(null);
    try {
      setRegistry(await invoke<RegistryCheck>("registry_check", { paperId }));
    } catch (e) {
      // Offline/unconfigured — the local flow is unaffected (v4 behavior).
      setRegistryError(String(e));
    }
  }

  async function pull(version: number) {
    setBusy(true);
    try {
      const report = await invoke<{ added: string[]; merged_journals: string[]; kept_local: string[]; unresolved_anchors: string[] }>(
        "registry_pull",
        { paperId, version },
      );
      toast.success(`Pulled layer v${version}`, {
        description: `${report.added.length} added, ${report.merged_journals.length} journals merged, ${report.kept_local.length} kept local${report.unresolved_anchors.length ? `, ${report.unresolved_anchors.length} anchors unresolved` : ""}`,
      });
      refresh();
    } catch (e) {
      toast.error("Pull failed", { description: String(e) });
    } finally {
      setBusy(false);
    }
  }

  async function publish() {
    setBusy(true);
    try {
      const version = await invoke<number>("registry_publish", { paperId });
      toast.success(`Published as layer v${version}`);
      setPublishPreview(null);
      checkRegistry();
    } catch (e) {
      toast.error("Publish failed", { description: String(e) });
    } finally {
      setBusy(false);
    }
  }

  async function propose() {
    if (!summary.trim()) return;
    if (identity.trim()) {
      await invoke("contribution_identity_set", { name: identity.trim() }).catch(() => {});
    }
    try {
      await invoke("contribution_propose", {
        paperId,
        summary: summary.trim(),
        includeNotes,
        includeCanvas,
      });
      setSummary("");
      toast.success("Proposal queued", {
        description: "It submits when the registry is reachable; reviewers see it then.",
      });
      refresh();
    } catch (e) {
      toast.error("Couldn't create proposal", { description: String(e) });
    }
  }

  async function review(proposal: Proposal, accepted: boolean) {
    const reason = accepted ? null : window.prompt("Reason for rejecting?") ?? "not suitable";
    try {
      const outcome = await invoke<{ merged: boolean; conflicts: string[] }>(
        "contribution_review",
        { paperId, proposalId: proposal.id, accepted, reason },
      );
      if (accepted && !outcome.merged) {
        toast.error("Merge conflicts", { description: outcome.conflicts.join(", ") });
      }
      setDiff(null);
      refresh();
    } catch (e) {
      toast.error("Review failed", { description: String(e) });
    }
  }

  async function revert(proposal: Proposal) {
    await invoke("contribution_revert", { paperId, proposalId: proposal.id }).catch((e) =>
      toast.error("Revert failed", { description: String(e) }),
    );
    refresh();
  }

  async function openDiff(proposal: Proposal) {
    const changes = await invoke<ChangePreview[]>("contribution_diff", {
      paperId,
      proposalId: proposal.id,
    }).catch(() => []);
    setDiff({ proposal, changes });
  }

  return (
    <ScrollArea className="h-full">
      <div className="mx-auto flex w-full max-w-2xl flex-col gap-5 p-6 pt-12">
        <h2 className="text-lg font-semibold">Community</h2>

        {/* Registry: pull community enrichment / publish yours */}
        <div className="flex flex-col gap-2 rounded-lg border p-4">
          <div className="flex items-center gap-2">
            <h3 className="flex-1 text-sm font-medium">Knowledge registry</h3>
            <Button variant="outline" size="sm" onClick={checkRegistry} disabled={busy}>
              <CloudDownloadIcon data-icon="inline-start" />
              Check for enrichment
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={async () =>
                setPublishPreview(await invoke<PublishPreview>("registry_preview", { paperId }))
              }
            >
              <CloudUploadIcon data-icon="inline-start" />
              Publish…
            </Button>
          </div>
          {registryError && (
            <Alert>
              <AlertTitle>Registry unreachable</AlertTitle>
              <AlertDescription>
                {registryError} — everything local keeps working; try again when online.
              </AlertDescription>
            </Alert>
          )}
          {registry &&
            (!registry.eligible ? (
              <p className="text-muted-foreground text-sm">
                This paper has no DOI/arXiv id, so it can't join the shared registry —
                all local features work unchanged.
              </p>
            ) : registry.layers.length === 0 ? (
              <p className="text-muted-foreground text-sm">
                No community layers yet for {registry.canonical_id}. Be the first to publish.
              </p>
            ) : (
              <ul className="flex flex-col gap-1">
                {registry.layers.map((layer) => (
                  <li key={layer.version} className="flex items-center gap-2 text-sm">
                    <Badge variant="outline">v{layer.version}</Badge>
                    <span className="flex-1">
                      {layer.artifacts.length} artifacts · by {layer.publisher}
                    </span>
                    <Button size="sm" variant="outline" disabled={busy} onClick={() => pull(layer.version)}>
                      Pull (merges alongside your work)
                    </Button>
                  </li>
                ))}
              </ul>
            ))}
        </div>

        {/* Propose */}
        <div className="flex flex-col gap-3 rounded-lg border p-4">
          <h3 className="text-sm font-medium">Propose to community</h3>
          <FieldGroup className="gap-3">
            <Field>
              <FieldLabel htmlFor="contrib-name">Your contributor name</FieldLabel>
              <Input
                id="contrib-name"
                placeholder="shown in provenance forever"
                value={identity}
                onChange={(e) => setIdentity(e.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="contrib-summary">What did you improve?</FieldLabel>
              <Input
                id="contrib-summary"
                placeholder="e.g. clearer explanation of eq. 3 + fixed canvas layout"
                value={summary}
                onChange={(e) => setSummary(e.target.value)}
              />
            </Field>
          </FieldGroup>
          <div className="flex items-center gap-6 text-sm">
            <label className="flex items-center gap-2">
              Notes <Switch checked={includeNotes} onCheckedChange={setIncludeNotes} />
            </label>
            <label className="flex items-center gap-2">
              Concept canvas <Switch checked={includeCanvas} onCheckedChange={setIncludeCanvas} />
            </label>
            <Button className="ml-auto" onClick={propose} disabled={!summary.trim()}>
              <GitPullRequestIcon data-icon="inline-start" />
              Propose
            </Button>
          </div>
        </div>

        {/* Review queue */}
        {overview && overview.proposals.length > 0 && (
          <div className="flex flex-col gap-2 rounded-lg border p-4">
            <div className="flex items-center gap-2">
              <h3 className="flex-1 text-sm font-medium">Proposals</h3>
              <Badge variant="outline">your trust: {overview.my_trust}</Badge>
            </div>
            <ul className="flex flex-col gap-2">
              {overview.proposals.map((proposal) => (
                <li key={proposal.id} className="flex items-center gap-2 text-sm">
                  <Badge variant={STATUS_VARIANT[proposal.status]}>{proposal.status}</Badge>
                  <button
                    className="flex-1 truncate text-left underline-offset-2 hover:underline"
                    onClick={() => openDiff(proposal)}
                  >
                    {proposal.summary}
                  </button>
                  <span className="text-muted-foreground text-xs">
                    {proposal.author.display_name ?? proposal.author.id}
                  </span>
                  {(proposal.status === "queued" || proposal.status === "submitted") && (
                    <>
                      <Button size="icon-sm" variant="outline" title="Accept & merge" onClick={() => review(proposal, true)}>
                        <CheckIcon />
                      </Button>
                      <Button size="icon-sm" variant="outline" title="Reject" onClick={() => review(proposal, false)}>
                        <XIcon />
                      </Button>
                    </>
                  )}
                  {proposal.status === "merged" && (
                    <Button size="icon-sm" variant="outline" title="Revert this merge" onClick={() => revert(proposal)}>
                      <Undo2Icon />
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          </div>
        )}

        {/* Provenance inspector */}
        {overview && (
          <div className="flex flex-col gap-2 rounded-lg border p-4">
            <div className="flex items-center gap-2">
              <HistoryIcon className="text-muted-foreground size-4" />
              <h3 className="flex-1 text-sm font-medium">Provenance</h3>
              <span className="text-muted-foreground font-mono text-xs">
                rev {overview.revision.slice(0, 15)}
              </span>
            </div>
            {overview.events.length === 0 ? (
              <p className="text-muted-foreground text-sm">
                No community history yet — every propose, review, merge, and revert will be
                recorded here, signed and append-only.
              </p>
            ) : (
              <ul className="flex flex-col gap-1 text-sm">
                {overview.events.map((event, index) => (
                  <li key={index} className="flex items-center gap-2">
                    <span className="text-muted-foreground w-40 flex-none text-xs">
                      {new Date(event.at).toLocaleString()}
                    </span>
                    <span className="font-medium">{event.actor.id}</span>
                    <span>
                      {event.event}
                      {event.accepted === false && event.reason ? ` — ${event.reason}` : ""}
                    </span>
                  </li>
                ))}
              </ul>
            )}
            {overview && Object.keys(overview.reputation).length > 0 && (
              <>
                <Separator />
                <div className="flex flex-wrap gap-2">
                  {Object.entries(overview.reputation).map(([who, r]) => (
                    <Badge key={who} variant="secondary">
                      {who}: {r.accepted} accepted · {r.reviews_performed} reviews
                      {r.reverted > 0 ? ` · ${r.reverted} reverted` : ""}
                    </Badge>
                  ))}
                </div>
              </>
            )}
          </div>
        )}

        {/* Diff dialog */}
        <Dialog open={diff !== null} onOpenChange={(open) => !open && setDiff(null)}>
          <DialogContent className="max-h-[80vh] overflow-hidden sm:max-w-2xl">
            <DialogHeader>
              <DialogTitle>{diff?.proposal.summary}</DialogTitle>
              <DialogDescription>
                by {diff?.proposal.author.display_name ?? diff?.proposal.author.id} · base{" "}
                {diff?.proposal.base_revision.slice(0, 15)}
              </DialogDescription>
            </DialogHeader>
            <ScrollArea className="max-h-[55vh]">
              <div className="flex flex-col gap-3 pr-3">
                {diff?.changes.map((change) => (
                  <div key={change.path}>
                    <div className="flex items-center gap-2 text-sm">
                      <span className="font-mono">{change.path}</span>
                      <Badge variant="outline">{change.kind}</Badge>
                    </div>
                    <pre className="bg-muted/50 mt-1 overflow-x-auto rounded-md p-2 text-xs">
                      {change.preview}
                    </pre>
                  </div>
                ))}
              </div>
            </ScrollArea>
          </DialogContent>
        </Dialog>

        {/* Publish preview dialog: exactly what uploads, and what never will */}
        <Dialog open={publishPreview !== null} onOpenChange={(open) => !open && setPublishPreview(null)}>
          <DialogContent className="max-h-[80vh] overflow-hidden sm:max-w-lg">
            <DialogHeader>
              <DialogTitle>Publish enrichment</DialogTitle>
              <DialogDescription>
                Only enrichment leaves your machine — never the PDF, page images, extracted
                text, or personal state.
              </DialogDescription>
            </DialogHeader>
            <ScrollArea className="max-h-[50vh]">
              <div className="flex flex-col gap-2 pr-3 text-sm">
                <span className="font-medium">Will upload ({publishPreview?.included.length ?? 0})</span>
                {publishPreview?.included.map((path) => (
                  <span key={path} className="font-mono text-xs">{path}</span>
                ))}
                <Separator className="my-1" />
                <span className="font-medium">Held back</span>
                {publishPreview?.excluded.slice(0, 30).map(([path, reason]) => (
                  <span key={path} className="text-muted-foreground text-xs">
                    <span className="font-mono">{path}</span> — {reason}
                  </span>
                ))}
              </div>
            </ScrollArea>
            <Button onClick={publish} disabled={busy || (publishPreview?.included.length ?? 0) === 0}>
              <CloudUploadIcon data-icon="inline-start" />
              Publish {publishPreview?.included.length ?? 0} file(s)
            </Button>
          </DialogContent>
        </Dialog>
      </div>
    </ScrollArea>
  );
}
