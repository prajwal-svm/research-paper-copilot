import { useCallback, useEffect, useState } from "react";
import { invoke } from "@/platform";
import { openFileDialog } from "@/platform";
import { toast } from "sonner";
import { DownloadIcon, PlayIcon, PuzzleIcon } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "@/components/ui/empty";

interface PluginView {
  manifest: {
    name: string;
    version: string;
    capabilities: string[];
    permissions: string[];
    description?: string;
  };
  status: { status: "compatible" } | { status: "incompatible"; reason: string };
  granted: string[];
}

const EXPORT_FORMATS = ["anki", "obsidian", "latex"] as const;

/** Third-party plugins over the scoped bundle view: exporters write files
 * to a chosen folder; panels render in a sandboxed iframe. Permissions are
 * explicit, revocable, and blocked calls are surfaced — never silent. */
export default function PluginPane({ paperId }: { paperId: string }) {
  const [plugins, setPlugins] = useState<PluginView[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [panelHtml, setPanelHtml] = useState<string | null>(null);
  const [blocked, setBlocked] = useState<string[]>([]);

  const refresh = useCallback(() => {
    invoke<PluginView[]>("plugin_list").then(setPlugins).catch(() => {});
  }, []);
  useEffect(refresh, [refresh]);

  async function setConsent(plugin: string, permission: string, granted: boolean) {
    await invoke("plugin_set_consent", { plugin, permission, granted }).catch((e) =>
      toast.error("Couldn't update permission", { description: String(e) }),
    );
    refresh();
  }

  async function exportTo(plugin: string, format: string) {
    const dir = await openFileDialog({ directory: true, multiple: false });
    if (typeof dir !== "string") return;
    setBusy(`${plugin}:${format}`);
    try {
      const written = await invoke<string[]>("plugin_export_to_dir", {
        plugin,
        paperId,
        format,
        destDir: dir,
      });
      toast.success(`Exported ${written.length} file(s)`, { description: written.join(", ") });
    } catch (e) {
      toast.error("Export failed", { description: String(e) });
    } finally {
      setBusy(null);
    }
  }

  async function runPanel(plugin: string) {
    setBusy(`${plugin}:panel`);
    try {
      const result = await invoke<{ output: { html?: string; raw?: string }; blocked: string[] }>(
        "plugin_run",
        { plugin, paperId, format: "panel" },
      );
      setBlocked(result.blocked);
      setPanelHtml(result.output.html ?? result.output.raw ?? "<p>Panel produced no HTML.</p>");
    } catch (e) {
      toast.error("Panel failed", { description: String(e) });
    } finally {
      setBusy(null);
    }
  }

  return (
    <ScrollArea className="h-full">
      <div className="mx-auto flex w-full max-w-2xl flex-col gap-4 p-6 pt-12">
        <h2 className="text-lg font-semibold">Plugins</h2>

        {plugins.length === 0 && (
          <Empty>
            <EmptyHeader>
              <EmptyTitle>No plugins installed</EmptyTitle>
              <EmptyDescription>
                Drop a plugin folder (plugin.json + plugin.wasm) into the app's
                plugins directory. Plugins read a scoped view of this paper —
                never your filesystem — and every extra permission is granted
                by you, revocably.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        )}

        {blocked.length > 0 && (
          <Alert>
            <AlertTitle>Blocked plugin access</AlertTitle>
            <AlertDescription>
              {blocked.join("; ")} — grant the permission below if this is
              intentional.
            </AlertDescription>
          </Alert>
        )}

        {plugins.map(({ manifest, status, granted }) => (
          <div key={manifest.name} className="flex flex-col gap-2 rounded-lg border p-4">
            <div className="flex items-center gap-2">
              <PuzzleIcon className="text-muted-foreground size-4" />
              <span className="font-medium">{manifest.name}</span>
              <Badge variant="outline">v{manifest.version}</Badge>
              {manifest.capabilities.map((c) => (
                <Badge key={c} variant="secondary">
                  {c}
                </Badge>
              ))}
            </div>
            {manifest.description && (
              <p className="text-muted-foreground text-sm">{manifest.description}</p>
            )}

            {status.status === "incompatible" ? (
              <Alert variant="destructive">
                <AlertTitle>Not loaded</AlertTitle>
                <AlertDescription>{status.reason}</AlertDescription>
              </Alert>
            ) : (
              <>
                {manifest.permissions.length > 0 && (
                  <div className="flex flex-col gap-1">
                    {manifest.permissions.map((permission) => (
                      <label
                        key={permission}
                        className="flex items-center justify-between text-sm"
                      >
                        <span>
                          Allow <code>{permission}</code> access
                        </span>
                        <Switch
                          checked={granted.includes(permission)}
                          onCheckedChange={(next) =>
                            setConsent(manifest.name, permission, next)
                          }
                        />
                      </label>
                    ))}
                  </div>
                )}
                <div className="flex flex-wrap gap-2">
                  {manifest.capabilities.includes("exporter") &&
                    EXPORT_FORMATS.map((format) => (
                      <Button
                        key={format}
                        variant="outline"
                        size="sm"
                        disabled={busy !== null}
                        onClick={() => exportTo(manifest.name, format)}
                      >
                        <DownloadIcon data-icon="inline-start" />
                        Export {format}
                      </Button>
                    ))}
                  {manifest.capabilities.includes("panel") && (
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={busy !== null}
                      onClick={() => runPanel(manifest.name)}
                    >
                      <PlayIcon data-icon="inline-start" />
                      Open panel
                    </Button>
                  )}
                </div>
              </>
            )}
          </div>
        ))}

        {panelHtml !== null && (
          <>
            <Separator />
            {/* Sandboxed: no same-origin, no top-navigation; scripts only
                inside the frame. The plugin never touches the app DOM. */}
            <iframe
              title="plugin panel"
              sandbox="allow-scripts"
              srcDoc={panelHtml}
              className="h-96 w-full rounded-md border bg-white"
            />
          </>
        )}
      </div>
    </ScrollArea>
  );
}
