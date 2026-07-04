import { useEffect, useState } from "react";
import { invoke } from "@/platform";
import { ExpandIcon, XIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import GlobalChatView from "./GlobalChatView";
import type { WorkspaceItem } from "./types";

/**
 * The chat overlay: a right-side panel summonable from any view. Owns a
 * "current overlay chat" (created lazily on first open), and can expand to
 * the full-screen surface.
 */
export default function ChatOverlay({
  open,
  onClose,
  onExpand,
}: {
  open: boolean;
  onClose: () => void;
  /** Hand the active chat off to the full-screen surface. */
  onExpand: (chatId: string) => void;
}) {
  const [chatId, setChatId] = useState<string | null>(null);

  // Lazily create/reuse an overlay chat the first time it opens.
  useEffect(() => {
    if (!open || chatId) return;
    invoke<WorkspaceItem>("workspace_chat_create", {})
      .then((item) => setChatId(item.id))
      .catch(() => {});
  }, [open, chatId]);

  if (!open) return null;

  return (
    <div className="fixed inset-y-0 right-0 z-50 flex w-[420px] max-w-full flex-col border-l bg-background shadow-2xl">
      <div className="flex flex-none items-center gap-2 border-b px-3 py-2">
        <span className="flex-1 text-sm font-medium">Chat</span>
        {chatId && (
          <Button variant="ghost" size="icon-sm" title="Expand to full screen" onClick={() => onExpand(chatId)}>
            <ExpandIcon />
          </Button>
        )}
        <Button variant="ghost" size="icon-sm" title="Close" onClick={onClose}>
          <XIcon />
        </Button>
      </div>
      <div className="min-h-0 flex-1">
        {chatId && <GlobalChatView chatId={chatId} />}
      </div>
    </div>
  );
}
