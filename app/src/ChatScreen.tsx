import { useCallback, useEffect, useState } from "react";
import { invoke } from "@/platform";
import { HomeIcon, MessagesSquareIcon, PlusIcon, SearchIcon, Trash2Icon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import GlobalChatView from "./GlobalChatView";
import type { WorkspaceItem } from "./types";

/** Full-screen chat: searchable sidebar of chats + the conversation pane. */
export default function ChatScreen({
  chatId,
  onBack,
  onOpenChat,
}: {
  chatId: string;
  onBack: () => void;
  onOpenChat: (id: string) => void;
}) {
  const [chats, setChats] = useState<WorkspaceItem[]>([]);
  const [query, setQuery] = useState("");

  const reload = useCallback(() => {
    invoke<WorkspaceItem[]>("workspace_items_list", { kind: "chat" })
      .then(setChats)
      .catch(() => {});
  }, []);
  useEffect(reload, [reload]);

  async function newChat() {
    const item = await invoke<WorkspaceItem>("workspace_chat_create", {}).catch(() => null);
    if (item) {
      reload();
      onOpenChat(item.id);
    }
  }

  async function deleteChat(id: string) {
    await invoke("workspace_item_delete", { id }).catch(() => {});
    reload();
    if (id === chatId) onBack();
  }

  const filtered = chats.filter((c) => c.title.toLowerCase().includes(query.toLowerCase()));

  return (
    <div className="flex h-screen">
      <aside className="flex w-64 flex-none flex-col border-r bg-muted/30">
        <div data-tauri-drag-region className="flex items-center gap-2 border-b px-3 py-2 pl-20">
          <Button variant="ghost" size="icon-sm" onClick={onBack} title="Back to library">
            <HomeIcon />
          </Button>
          <span className="flex-1 text-sm font-medium">Chats</span>
          <Button variant="ghost" size="icon-sm" onClick={newChat} title="New chat">
            <PlusIcon />
          </Button>
        </div>
        <div className="relative px-2 py-2">
          <SearchIcon className="absolute left-4 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            className="h-8 pl-7"
            placeholder="Search chats…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <ScrollArea className="min-h-0 flex-1">
          <ul className="flex flex-col gap-0.5 p-2">
            {filtered.map((chat) => (
              <li key={chat.id} className="group flex items-center">
                <button
                  className={
                    "flex min-w-0 flex-1 items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-accent " +
                    (chat.id === chatId ? "bg-accent" : "")
                  }
                  onClick={() => onOpenChat(chat.id)}
                >
                  <MessagesSquareIcon className="size-3.5 flex-none text-muted-foreground" />
                  <span className="truncate">{chat.title}</span>
                </button>
                <button
                  className="opacity-0 transition-opacity hover:text-destructive group-hover:opacity-100"
                  title="Delete chat"
                  onClick={() => deleteChat(chat.id)}
                >
                  <Trash2Icon className="size-3.5" />
                </button>
              </li>
            ))}
          </ul>
        </ScrollArea>
      </aside>
      <main className="min-w-0 flex-1">
        <GlobalChatView key={chatId} chatId={chatId} onTitleChange={reload} />
      </main>
    </div>
  );
}
