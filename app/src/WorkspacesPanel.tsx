import { useCallback, useEffect, useState } from "react";
import { invoke } from "@/platform";
import { toast } from "sonner";
import { PlusIcon, RefreshCwIcon, Share2Icon, SendIcon, UsersIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "@/components/ui/empty";
import type { PaperSummary } from "./types";

interface Workspace {
  id: string;
  name: string;
  mode: string;
  created_at: string;
}
interface MemberView {
  member_id: string;
  name: string;
  role: string;
  present: boolean;
}
interface ThreadMessage {
  id: string;
  author_id: string;
  author_name: string;
  content: string;
  at: string;
}
interface Assignment {
  id: string;
  paper_ref: string;
  assigned_by: string;
  at: string;
}
interface CohortRow {
  member_id: string;
  completions: Record<string, [string, number | null]>;
}

/** Collaborative workspaces (v4 §7): shared libraries, object-anchored
 * threads with visible authorship, sync-cadence presence, reading-group
 * cohort progress (opt-in only), lab-mode shared experiments. */
export default function WorkspacesPanel() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [active, setActive] = useState<Workspace | null>(null);
  const [name, setName] = useState("");
  const [mode, setMode] = useState("reading_group");
  const [memberName, setMemberName] = useState("");

  const refresh = useCallback(() => {
    invoke<Workspace[]>("workspace_list").then(setWorkspaces).catch(() => {});
  }, []);
  useEffect(refresh, [refresh]);

  async function create() {
    if (!name.trim() || !memberName.trim()) return;
    try {
      const ws = await invoke<Workspace>("workspace_create", {
        name: name.trim(),
        mode,
        memberName: memberName.trim(),
      });
      setName("");
      refresh();
      setActive(ws);
    } catch (e) {
      toast.error("Couldn't create workspace", { description: String(e) });
    }
  }

  if (active) {
    return <WorkspaceDetail workspace={active} onBack={() => setActive(null)} />;
  }

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-6 p-6">
      <FieldGroup className="flex-row items-end gap-2">
        <Field className="flex-1">
          <FieldLabel htmlFor="ws-name">New workspace</FieldLabel>
          <Input
            id="ws-name"
            placeholder="e.g. Attention reading group"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </Field>
        <Field className="w-40">
          <FieldLabel htmlFor="ws-me">Your name</FieldLabel>
          <Input
            id="ws-me"
            placeholder="shown to members"
            value={memberName}
            onChange={(e) => setMemberName(e.target.value)}
          />
        </Field>
        <Select value={mode} onValueChange={setMode}>
          <SelectTrigger className="w-40">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="reading_group">Reading group</SelectItem>
              <SelectItem value="lab">Lab</SelectItem>
            </SelectGroup>
          </SelectContent>
        </Select>
        <Button onClick={create} disabled={!name.trim() || !memberName.trim()}>
          <PlusIcon data-icon="inline-start" />
          Create
        </Button>
      </FieldGroup>

      {workspaces.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No workspaces yet</EmptyTitle>
            <EmptyDescription>
              Create a reading group or lab. Sharing runs over your own
              sync remote — end-to-end encrypted; learner memory never leaves
              each member's machine.
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <ul className="flex flex-col gap-2">
          {workspaces.map((ws) => (
            <li key={ws.id}>
              <button
                className="hover:bg-accent/50 flex w-full items-center gap-3 rounded-lg border px-4 py-3 text-left"
                onClick={() => setActive(ws)}
              >
                <UsersIcon className="text-muted-foreground size-4" />
                <span className="flex-1 font-medium">{ws.name}</span>
                <Badge variant="outline">
                  {ws.mode === "lab" ? "lab" : "reading group"}
                </Badge>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function WorkspaceDetail({
  workspace,
  onBack,
}: {
  workspace: Workspace;
  onBack: () => void;
}) {
  const [members, setMembers] = useState<MemberView[]>([]);
  const [assignments, setAssignments] = useState<Assignment[]>([]);
  const [cohort, setCohort] = useState<CohortRow[]>([]);
  const [papers, setPapers] = useState<PaperSummary[]>([]);
  const [sharePick, setSharePick] = useState("");
  const [syncing, setSyncing] = useState(false);
  const [optedIn, setOptedIn] = useState(false);
  const [thread, setThread] = useState<ThreadMessage[]>([]);
  const [message, setMessage] = useState("");

  // Workspace-level discussion uses the workspace id as its anchor;
  // object-level threads reuse the same commands with an object UUID.
  const anchor = workspace.id;

  const refresh = useCallback(() => {
    invoke<MemberView[]>("workspace_members", { workspaceId: workspace.id })
      .then(setMembers)
      .catch(() => {});
    invoke<Assignment[]>("workspace_assignments", { workspaceId: workspace.id })
      .then(setAssignments)
      .catch(() => {});
    invoke<CohortRow[]>("workspace_cohort", { workspaceId: workspace.id })
      .then(setCohort)
      .catch(() => {});
    invoke<ThreadMessage[]>("workspace_thread", { workspaceId: workspace.id, anchor })
      .then(setThread)
      .catch(() => {});
    invoke<PaperSummary[]>("list_papers").then(setPapers).catch(() => {});
  }, [workspace.id, anchor]);
  useEffect(refresh, [refresh]);

  async function sync() {
    setSyncing(true);
    try {
      const outcome = await invoke<{ pulled_files: number; pushed_blobs: number }>(
        "workspace_sync",
        { workspaceId: workspace.id },
      );
      toast.success("Workspace synced", {
        description: `${outcome.pulled_files} pulled, ${outcome.pushed_blobs} pushed`,
      });
      refresh();
    } catch (e) {
      toast.error("Workspace sync failed", { description: String(e) });
    } finally {
      setSyncing(false);
    }
  }

  async function post() {
    if (!message.trim()) return;
    try {
      await invoke("workspace_thread_post", {
        workspaceId: workspace.id,
        anchor,
        content: message.trim(),
      });
      setMessage("");
      refresh();
    } catch (e) {
      toast.error("Couldn't post", { description: String(e) });
    }
  }

  async function share() {
    if (!sharePick) return;
    try {
      await invoke("workspace_share_paper", {
        workspaceId: workspace.id,
        paperId: sharePick,
      });
      toast.success("Paper shared into the workspace");
    } catch (e) {
      toast.error("Couldn't share paper", { description: String(e) });
    }
  }

  async function assign() {
    if (!sharePick) return;
    try {
      await invoke("workspace_assign", {
        workspaceId: workspace.id,
        paperRef: sharePick,
        quizNode: null,
      });
      refresh();
    } catch (e) {
      toast.error("Couldn't assign", { description: String(e) });
    }
  }

  async function setOptIn(next: boolean) {
    setOptedIn(next);
    const me = await invoke<{ member_id: string }>("workspace_whoami", {
      workspaceId: workspace.id,
    }).catch(() => null);
    if (!me) return;
    const event = next
      ? {
          op: "opt_in",
          member_id: me.member_id,
          shares: "assignment_completion,quiz_outcomes",
          at: new Date().toISOString(),
        }
      : { op: "opt_out", member_id: me.member_id, at: new Date().toISOString() };
    await invoke("workspace_progress", { workspaceId: workspace.id, event }).catch((e) =>
      toast.error("Couldn't update sharing", { description: String(e) }),
    );
    refresh();
  }

  const memberName = (id: string) =>
    members.find((m) => m.member_id === id)?.name ?? id.slice(0, 8);

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-5 p-6">
      <div className="flex items-center gap-2">
        <Button variant="ghost" size="sm" onClick={onBack}>
          ← All workspaces
        </Button>
        <h2 className="flex-1 text-lg font-semibold">{workspace.name}</h2>
        <Badge variant="outline">{workspace.mode === "lab" ? "lab" : "reading group"}</Badge>
        <Button size="sm" onClick={sync} disabled={syncing}>
          {syncing ? <Spinner data-icon="inline-start" /> : <RefreshCwIcon data-icon="inline-start" />}
          Sync
        </Button>
      </div>

      {/* Members + presence (sync-cadence) */}
      <div className="flex flex-wrap items-center gap-2">
        {members.map((m) => (
          <Badge key={m.member_id} variant={m.present ? "secondary" : "outline"}>
            {m.present ? "● " : ""}
            {m.name}
            {m.role === "instructor" ? " (instructor)" : ""}
          </Badge>
        ))}
      </div>

      {/* Share papers into the workspace */}
      <div className="flex items-end gap-2">
        <Field className="flex-1">
          <FieldLabel>Share a paper with the group</FieldLabel>
          <Select value={sharePick} onValueChange={setSharePick}>
            <SelectTrigger>
              <SelectValue placeholder="Pick a paper…" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {papers.map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {p.title}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        </Field>
        <Button variant="outline" onClick={share} disabled={!sharePick}>
          <Share2Icon data-icon="inline-start" />
          Share
        </Button>
        {workspace.mode === "reading_group" && (
          <Button variant="outline" onClick={assign} disabled={!sharePick}>
            Assign
          </Button>
        )}
      </div>
      {workspace.mode === "reading_group" && (
        <span className="text-muted-foreground -mt-3 text-xs">
          Assign uses the selected paper; members see it in the assignment list.
        </span>
      )}

      <Separator />

      {/* Discussion thread — authorship always visible */}
      <div className="flex flex-col gap-2">
        <h3 className="text-sm font-medium">Discussion</h3>
        <ScrollArea className="max-h-64 rounded-md border">
          <div className="flex flex-col gap-2 p-3">
            {thread.length === 0 && (
              <span className="text-muted-foreground text-sm">No messages yet.</span>
            )}
            {thread.map((m) => (
              <div key={m.id} className="text-sm">
                <span className="font-medium">{m.author_name}</span>{" "}
                <span className="text-muted-foreground text-xs">
                  {new Date(m.at).toLocaleString()}
                </span>
                <div>{m.content}</div>
              </div>
            ))}
          </div>
        </ScrollArea>
        <div className="flex gap-2">
          <Input
            placeholder="Write to the group…"
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && post()}
          />
          <Button onClick={post} disabled={!message.trim()}>
            <SendIcon data-icon="inline-start" />
            Post
          </Button>
        </div>
      </div>

      {workspace.mode === "reading_group" && (
        <>
          <Separator />
          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <h3 className="text-sm font-medium">Assignments & cohort progress</h3>
              <label className="flex items-center gap-2 text-xs">
                Share my progress (completion + quiz outcomes only)
                <Switch checked={optedIn} onCheckedChange={setOptIn} />
              </label>
            </div>
            {assignments.length === 0 ? (
              <span className="text-muted-foreground text-sm">No assignments yet.</span>
            ) : (
              <ul className="flex flex-col gap-1 text-sm">
                {assignments.map((a) => (
                  <li key={a.id} className="flex items-center gap-2">
                    <span className="flex-1">{a.paper_ref}</span>
                    <span className="text-muted-foreground text-xs">
                      assigned by {memberName(a.assigned_by)}
                    </span>
                  </li>
                ))}
              </ul>
            )}
            {cohort.length > 0 && (
              <ul className="flex flex-col gap-1 text-sm">
                {cohort.map((row) => (
                  <li key={row.member_id}>
                    <span className="font-medium">{memberName(row.member_id)}</span>{" "}
                    <span className="text-muted-foreground">
                      {Object.entries(row.completions)
                        .map(
                          ([aid, [status, quality]]) =>
                            `${assignments.find((a) => a.id === aid)?.paper_ref ?? aid.slice(0, 8)}: ${status}${quality != null ? ` (quiz ${quality}/5)` : ""}`,
                        )
                        .join(" · ")}
                    </span>
                  </li>
                ))}
              </ul>
            )}
            <span className="text-muted-foreground text-xs">
              Only opted-in members appear — mastery, episodes, and chats never leave a
              member's machine.
            </span>
          </div>
        </>
      )}
    </div>
  );
}
