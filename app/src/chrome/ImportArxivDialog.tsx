import { useState } from "react";
import { invoke } from "@/platform";
import { CheckCircle2Icon, DownloadIcon } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";

/**
 * "Import from arXiv / DOI" modal: paste a URL, arXiv id, or DOI; shows
 * fetch progress, errors inline, and a success confirmation. The library
 * picks the new paper up from pipeline events.
 */
export default function ImportArxivDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);

  function reset() {
    setValue("");
    setBusy(false);
    setError(null);
    setSuccess(false);
  }

  async function submit() {
    const input = value.trim();
    if (!input || busy) return;
    setBusy(true);
    setError(null);
    setSuccess(false);
    try {
      await invoke<string>("import_url", { input });
      setSuccess(true);
      setValue("");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        onOpenChange(next);
        if (!next) reset();
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Import from arXiv / DOI</DialogTitle>
          <DialogDescription>
            Paste an arXiv URL, arXiv id (e.g. 1706.03762), or DOI. The PDF
            is fetched and processed into your library.
          </DialogDescription>
        </DialogHeader>

        <FieldGroup>
          <Field data-invalid={error ? true : undefined}>
            <FieldLabel htmlFor="arxiv-input">arXiv URL / id / DOI</FieldLabel>
            <Input
              id="arxiv-input"
              autoFocus
              placeholder="https://arxiv.org/abs/1706.03762"
              value={value}
              disabled={busy}
              aria-invalid={error ? true : undefined}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && submit()}
            />
          </Field>
        </FieldGroup>

        {error && (
          <Alert variant="destructive">
            <AlertTitle>Import failed</AlertTitle>
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        )}
        {success && (
          <Alert>
            <CheckCircle2Icon />
            <AlertTitle>Added to library</AlertTitle>
            <AlertDescription>
              Processing has started — the paper appears in the library as it
              ingests. You can import another or close this dialog.
            </AlertDescription>
          </Alert>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            {success ? "Done" : "Cancel"}
          </Button>
          <Button onClick={submit} disabled={busy || !value.trim()}>
            {busy ? (
              <Spinner data-icon="inline-start" />
            ) : (
              <DownloadIcon data-icon="inline-start" />
            )}
            {busy ? "Fetching…" : "Import"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
