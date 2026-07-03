import { useEffect, useState } from "react";
import { invoke } from "@/platform";
import { listen } from "@/platform";
import { openFileDialog as openDialog } from "@/platform";
import { CloudIcon, FolderIcon, RefreshCwIcon, Trash2Icon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";

interface SyncStatus {
  configured: boolean;
  backend?: string | null;
  destination?: string | null;
  last_generation: number;
  conflicts: string[];
  trash: string[];
}

/**
 * Cloud sync setup (add-cloud-sync): bring-your-own storage, no accounts.
 * Recommended: a free Cloudflare R2 bucket (10 GB, ciphertext only);
 * equally supported: your own MinIO (e.g. on Coolify) or a plain folder
 * (iCloud Drive/Dropbox/Syncthing). Everything uploaded is encrypted on
 * this machine with your passphrase — which is unrecoverable if lost.
 */
export default function SyncSettings() {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [backend, setBackend] = useState<"s3" | "folder">("s3");
  const [endpoint, setEndpoint] = useState("");
  const [bucket, setBucket] = useState("");
  const [region, setRegion] = useState("auto");
  const [folder, setFolder] = useState("");
  const [accessKey, setAccessKey] = useState("");
  const [secretKey, setSecretKey] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [busy, setBusy] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [log, setLog] = useState<string | null>(null);

  const refresh = () => {
    invoke<SyncStatus>("sync_status").then(setStatus).catch(() => {});
  };
  useEffect(refresh, []);

  useEffect(() => {
    const unlisten = listen<{
      line?: string;
      outcome?: { pushed_blobs: number; pulled_files: number; conflict_copies: string[]; generation: number };
      error?: string;
    }>("sync-progress", ({ payload }) => {
      if (payload.line) setLog(payload.line);
      if (payload.outcome) {
        setSyncing(false);
        setLog(null);
        setMessage(
          `Synced (generation ${payload.outcome.generation}): ↑${payload.outcome.pushed_blobs} ↓${payload.outcome.pulled_files}` +
            (payload.outcome.conflict_copies.length > 0
              ? ` — ${payload.outcome.conflict_copies.length} conflict cop${payload.outcome.conflict_copies.length === 1 ? "y" : "ies"} created`
              : ""),
        );
        refresh();
      }
      if (payload.error) {
        setSyncing(false);
        setLog(null);
        setMessage(payload.error);
        refresh();
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  async function configure() {
    setBusy(true);
    setMessage(null);
    try {
      const destination = await invoke<string>("sync_configure", {
        backend,
        endpoint: endpoint.trim() || null,
        bucket: bucket.trim() || null,
        region: region.trim() || null,
        folder: folder.trim() || null,
        accessKey: accessKey.trim() || null,
        secretKey: secretKey.trim() || null,
        passphrase,
      });
      setMessage(`Sync configured — encrypted blobs will be stored at ${destination}.`);
      setPassphrase("");
      setAccessKey("");
      setSecretKey("");
      refresh();
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }

  function syncNow() {
    setSyncing(true);
    setMessage(null);
    invoke("sync_now").catch((e) => {
      setSyncing(false);
      setMessage(String(e));
    });
  }

  return (
    <div className="flex flex-col gap-3">
      <FieldDescription>
        <strong>A lost passphrase cannot be recovered</strong> — that's the
        point. Recommended: a free Cloudflare R2 bucket; your own MinIO (e.g.
        on Coolify) and plain folders (iCloud Drive, Dropbox, Syncthing) work
        the same way.
      </FieldDescription>

      {status?.configured ? (
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <Badge variant="secondary">
              <CloudIcon data-icon="inline-start" />
              {status.destination}
            </Badge>
            <span className="text-muted-foreground text-xs">
              generation {status.last_generation}
            </span>
          </div>
          <div className="flex items-center gap-1.5">
            <Button size="sm" disabled={syncing} onClick={syncNow}>
              {syncing ? <Spinner data-icon="inline-start" /> : <RefreshCwIcon data-icon="inline-start" />}
              Sync now
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={async () => {
                try {
                  const removed = await invoke<number>("sync_clean_remote");
                  setMessage(`Cleaned ${removed} unreferenced remote objects.`);
                } catch (e) {
                  setMessage(String(e));
                }
              }}
            >
              Clean remote…
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={async () => {
                await invoke("sync_disable").catch(() => {});
                refresh();
              }}
            >
              Disable
            </Button>
          </div>
          {log && <p className="text-muted-foreground truncate text-xs">{log}</p>}
          {status.conflicts.length > 0 && (
            <div className="text-xs">
              <p className="font-medium">
                {status.conflicts.length} conflict cop{status.conflicts.length === 1 ? "y" : "ies"}{" "}
                to review (both versions were kept):
              </p>
              <ul className="text-muted-foreground mt-0.5 flex flex-col gap-0.5">
                {status.conflicts.slice(0, 5).map((c) => (
                  <li key={c} className="truncate">
                    {c}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {status.trash.length > 0 && (
            <p className="text-muted-foreground text-xs">
              <Trash2Icon className="mr-1 inline size-3" />
              {status.trash.length} deleted paper{status.trash.length === 1 ? "" : "s"} in the local
              trash (grace period) — restore by moving out of <code>.trash/</code>.
            </p>
          )}
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          <div className="flex gap-1">
            <Button
              variant={backend === "s3" ? "secondary" : "outline"}
              size="sm"
              onClick={() => setBackend("s3")}
            >
              <CloudIcon data-icon="inline-start" />
              R2 / S3 / MinIO
            </Button>
            <Button
              variant={backend === "folder" ? "secondary" : "outline"}
              size="sm"
              onClick={() => setBackend("folder")}
            >
              <FolderIcon data-icon="inline-start" />
              Folder
            </Button>
          </div>

          {backend === "s3" ? (
            <>
              <Field>
                <FieldLabel htmlFor="sync-endpoint">Endpoint</FieldLabel>
                <Input
                  id="sync-endpoint"
                  placeholder="https://<account>.r2.cloudflarestorage.com or http://minio.local:9000"
                  value={endpoint}
                  onChange={(e) => setEndpoint(e.target.value)}
                />
              </Field>
              <div className="flex gap-2">
                <Field className="flex-1">
                  <FieldLabel htmlFor="sync-bucket">Bucket</FieldLabel>
                  <Input id="sync-bucket" value={bucket} onChange={(e) => setBucket(e.target.value)} />
                </Field>
                <Field className="w-28">
                  <FieldLabel htmlFor="sync-region">Region</FieldLabel>
                  <Input id="sync-region" value={region} onChange={(e) => setRegion(e.target.value)} />
                </Field>
              </div>
              <div className="flex gap-2">
                <Field className="flex-1">
                  <FieldLabel htmlFor="sync-access">Access key</FieldLabel>
                  <Input
                    id="sync-access"
                    value={accessKey}
                    onChange={(e) => setAccessKey(e.target.value)}
                  />
                </Field>
                <Field className="flex-1">
                  <FieldLabel htmlFor="sync-secret">Secret key</FieldLabel>
                  <Input
                    id="sync-secret"
                    type="password"
                    value={secretKey}
                    onChange={(e) => setSecretKey(e.target.value)}
                  />
                </Field>
              </div>
            </>
          ) : (
            <Field>
              <FieldLabel htmlFor="sync-folder">Folder (iCloud Drive, Dropbox, USB…)</FieldLabel>
              <div className="flex gap-1.5">
                <Input
                  id="sync-folder"
                  value={folder}
                  onChange={(e) => setFolder(e.target.value)}
                  placeholder="/Users/you/Library/Mobile Documents/…"
                />
                <Button
                  variant="outline"
                  size="sm"
                  onClick={async () => {
                    const dir = await openDialog({ directory: true });
                    if (typeof dir === "string") setFolder(dir);
                  }}
                >
                  Choose…
                </Button>
              </div>
              <FieldDescription>
                Folder transports are eventually-consistent — prefer syncing
                from one device at a time.
              </FieldDescription>
            </Field>
          )}

          <Field>
            <FieldLabel htmlFor="sync-passphrase">Library passphrase</FieldLabel>
            <Input
              id="sync-passphrase"
              type="password"
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
            />
            <FieldDescription>
              Encrypts everything before upload. Other devices join with the
              same storage + this passphrase. <strong>If you lose it, the
              synced copy is unrecoverable</strong> — there is no reset.
            </FieldDescription>
          </Field>
          <Button
            className="self-start"
            disabled={busy || passphrase.trim().length < 8}
            onClick={configure}
          >
            {busy && <Spinner data-icon="inline-start" />}
            Enable sync
          </Button>
        </div>
      )}
      {message && <p className="text-muted-foreground text-xs">{message}</p>}
    </div>
  );
}
