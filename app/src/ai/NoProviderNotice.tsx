import { KeyRoundIcon } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

/**
 * The designed no-key state for AI entry points: explains what the action
 * would do, why a provider is needed, and points at setup — never a raw
 * error. Non-AI data (extracted content, notes, bookmarks) stays available
 * around it.
 */
export default function NoProviderNotice({
  actionDescription,
  onOpenSettings,
}: {
  actionDescription: string;
  onOpenSettings?: () => void;
}) {
  return (
    <Alert>
      <KeyRoundIcon />
      <AlertTitle>Connect an AI provider to {actionDescription}</AlertTitle>
      <AlertDescription>
        <p>
          This action sends the selected object (and only it, plus its related
          context) to a model you choose. Add an API key — Anthropic, OpenAI,
          or OpenRouter — or run models locally with Ollama; nothing is sent
          anywhere until you ask.
        </p>
        <button
          className="mt-1 cursor-pointer font-medium underline underline-offset-2"
          onClick={onOpenSettings}
        >
          Open provider settings
        </button>
      </AlertDescription>
    </Alert>
  );
}
