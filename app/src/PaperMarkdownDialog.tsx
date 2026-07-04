import { useEffect, useState } from "react";
import { invoke } from "@/platform";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Spinner } from "@/components/ui/spinner";
import { MessageResponse } from "@/components/ai-elements/message";

/**
 * The parsed paper as markdown, shared by the library card menu and the
 * reader toolbar. Raw assembly is instant; the AI refinement is generated
 * once and cached in the bundle (research/paper-clean.md), so reopening —
 * from either surface — is instant thereafter.
 */
export default function PaperMarkdownDialog({
  paperId,
  title,
  open,
  onOpenChange,
}: {
  paperId: string;
  title?: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [markdown, setMarkdown] = useState<string | null>(null);
  const [refined, setRefined] = useState<string | null>(null);
  const [refining, setRefining] = useState(false);
  const [refineError, setRefineError] = useState<string | null>(null);
  const [showRaw, setShowRaw] = useState(false);

  useEffect(() => {
    if (!open) return;
    setMarkdown(null);
    setRefined(null);
    setRefineError(null);
    setShowRaw(false);
    invoke<string>("paper_markdown", { paperId })
      .then(setMarkdown)
      .catch((e) => setMarkdown(`_${String(e)}_`));
    // Cached AI refinement, if one was generated before.
    invoke<string | null>("paper_markdown_clean", { paperId, generate: false })
      .then(setRefined)
      .catch(() => {});
  }, [open, paperId]);

  function refine(regenerate: boolean) {
    setRefining(true);
    setRefineError(null);
    invoke<string | null>("paper_markdown_clean", {
      paperId,
      generate: true,
      regenerate,
    })
      .then((clean) => {
        setRefined(clean);
        setShowRaw(false);
      })
      .catch((e) => setRefineError(String(e)))
      .finally(() => setRefining(false));
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[85vh] flex-col sm:max-w-3xl">
        <DialogHeader className="flex-none">
          <DialogTitle className="min-w-0 break-words pr-8">
            {title ?? "Paper as Markdown"}
          </DialogTitle>
          <DialogDescription>
            Parsed markdown — sections, equations, figures, tables from the
            object layer.
          </DialogDescription>
        </DialogHeader>
        <ScrollArea className="min-h-0 flex-1 rounded-md border">
          <div className="p-4">
            {refined && !showRaw ? (
              <MessageResponse>{refined}</MessageResponse>
            ) : markdown === null ? (
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <Spinner /> Assembling markdown…
              </div>
            ) : (
              <MessageResponse>{markdown}</MessageResponse>
            )}
          </div>
        </ScrollArea>
        <div className="flex flex-none items-center gap-2">
          {refined === null ? (
            <Button
              variant="default"
              size="sm"
              disabled={refining || !markdown}
              onClick={() => refine(false)}
            >
              {refining && <Spinner data-icon="inline-start" />}
              {refining ? "Refining (takes a minute)…" : "Refine with AI"}
            </Button>
          ) : (
            <>
              <Button
                variant="outline"
                size="sm"
                disabled={refining}
                onClick={() => refine(true)}
              >
                {refining && <Spinner data-icon="inline-start" />}
                Regenerate
              </Button>
              <Button variant="ghost" size="sm" onClick={() => setShowRaw((v) => !v)}>
                {showRaw ? "Show refined" : "Show raw"}
              </Button>
              <Badge variant="secondary">AI-refined · cached</Badge>
            </>
          )}
          {refineError && (
            <span
              className="min-w-0 truncate text-xs text-destructive"
              title={refineError}
            >
              {refineError}
            </span>
          )}
          <span className="flex-1" />
          <Button
            variant="outline"
            size="sm"
            disabled={!markdown && !refined}
            onClick={() => {
              const content = refined && !showRaw ? refined : markdown;
              if (content) navigator.clipboard.writeText(content);
            }}
          >
            Copy markdown
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
