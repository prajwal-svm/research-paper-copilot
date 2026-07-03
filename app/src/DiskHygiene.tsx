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
import { Button } from "@/components/ui/button";
import { FieldDescription } from "@/components/ui/field";

interface CacheUsage {
  path: string;
  bytes: number;
  repos: number;
}

/** Repo-cache usage + cleanup (v3). Clearing is safe: bundles keep their
 * references and reports; clones re-download on demand. Container images
 * are managed by Docker/Podman themselves (`docker system prune`). */
export default function DiskHygiene() {
  const [usage, setUsage] = useState<CacheUsage | null>(null);

  const refresh = () => {
    invoke<CacheUsage>("repos_cache_usage").then(setUsage).catch(() => {});
  };
  useEffect(refresh, []);

  const mb = usage ? usage.bytes / (1024 * 1024) : 0;

  return (
    <div className="flex flex-col gap-2">
      <div>
        <FieldDescription>
          Cloned repositories live in a library-level cache ({usage?.repos ?? 0}{" "}
          repo{usage?.repos === 1 ? "" : "s"}, {mb.toFixed(1)} MB). Clearing it
          loses nothing — bundles keep their references and reports, and clones
          re-download on demand. Container images are cleaned with{" "}
          <code>docker system prune</code>.
        </FieldDescription>
      </div>
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button
            variant="outline"
            size="sm"
            className="self-start"
            disabled={!usage || usage.repos === 0}
          >
            <Trash2Icon data-icon="inline-start" />
            Clear repo cache
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Clear the repo cache?</AlertDialogTitle>
            <AlertDialogDescription>
              Deletes {usage?.repos ?? 0} cached clone{usage?.repos === 1 ? "" : "s"} (
              {mb.toFixed(1)} MB). Reproduction references, code maps, and
              reports stay in your papers; repos re-clone when needed.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={async () => {
                await invoke("repos_cache_clear").catch(() => {});
                refresh();
              }}
            >
              Clear cache
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
