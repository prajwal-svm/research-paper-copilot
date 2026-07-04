import { useEffect, useState } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
import { lazy, Suspense } from "react";
import { Toaster } from "@/components/ui/sonner";
import Dashboard, { dashboardSkipped } from "./Dashboard";

const ResearchView = lazy(() => import("./ResearchView"));
// BlockNote is heavy; the note surface loads on first open.
const NoteEditor = lazy(() => import("./NoteEditor"));
// Excalidraw is heavy; the canvas surface loads on first open.
const CanvasEditor = lazy(() => import("./CanvasEditor"));
const ChatScreen = lazy(() => import("./ChatScreen"));
import ChatOverlay from "./ChatOverlay";
import Library from "./Library";
import Reader from "./Reader";
import Settings from "./Settings";
import AppContextMenu from "./chrome/AppContextMenu";
import Omnibar from "./chrome/Omnibar";
import { initTheme } from "./chrome/ThemeToggle";

initTheme();

type ReaderPane = "pdf" | "graph" | "lessons" | "experiments" | "repro" | "extend" | "plugins" | "community";

type View =
  | { kind: "library" }
  | { kind: "research" }
  | { kind: "dashboard"; id: string; title: string }
  | { kind: "reader"; id: string; pane?: ReaderPane }
  | { kind: "note"; id: string }
  | { kind: "canvas"; id: string }
  | { kind: "chat"; id: string };

/** Top-level navigation: library → (dashboard) → reader. */
export default function App() {
  const [view, setView] = useState<View>({ kind: "library" });
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [chatOverlayOpen, setChatOverlayOpen] = useState(false);

  // ⌘⇧C / Ctrl+Shift+C: summon the chat overlay from anywhere.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "c") {
        e.preventDefault();
        setChatOverlayOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const openPaper = (id: string, title?: string, pane?: ReaderPane) => {
    // The dashboard never gates: skippable by preference, and papers opened
    // cross-paper ("seen in X") or into a specific pane land directly in
    // the reader.
    if (pane === undefined && title !== undefined && !dashboardSkipped()) {
      setView({ kind: "dashboard", id, title });
    } else {
      setView({ kind: "reader", id, pane });
    }
  };

  return (
    <TooltipProvider delayDuration={300}>
      {view.kind === "reader" ? (
        <Reader
          key={`${view.id}:${view.pane ?? "pdf"}`}
          paperId={view.id}
          initialPane={view.pane}
          onBack={() => setView({ kind: "library" })}
          onOpenPaper={(id) => openPaper(id)}
        />
      ) : view.kind === "dashboard" ? (
        <Dashboard
          paperId={view.id}
          title={view.title}
          onContinue={() => setView({ kind: "reader", id: view.id })}
          onBack={() => setView({ kind: "library" })}
        />
      ) : view.kind === "research" ? (
        <Suspense fallback={null}>
          <ResearchView
            onBack={() => setView({ kind: "library" })}
            onOpenPaper={(id) => openPaper(id)}
          />
        </Suspense>
      ) : view.kind === "note" ? (
        <Suspense fallback={null}>
          <NoteEditor
            noteId={view.id}
            onBack={() => setView({ kind: "library" })}
            onOpenPaper={(id) => openPaper(id)}
          />
        </Suspense>
      ) : view.kind === "canvas" ? (
        <Suspense fallback={null}>
          <CanvasEditor
            canvasId={view.id}
            onBack={() => setView({ kind: "library" })}
            onOpenPaper={(id) => openPaper(id)}
          />
        </Suspense>
      ) : view.kind === "chat" ? (
        <Suspense fallback={null}>
          <ChatScreen
            chatId={view.id}
            onBack={() => setView({ kind: "library" })}
            onOpenChat={(id) => setView({ kind: "chat", id })}
          />
        </Suspense>
      ) : (
        <Library
          onOpen={openPaper}
          onOpenResearch={() => setView({ kind: "research" })}
          onOpenNote={(id) => setView({ kind: "note", id })}
          onOpenCanvas={(id) => setView({ kind: "canvas", id })}
          onOpenChat={(id) => setView({ kind: "chat", id })}
        />
      )}

      {/* Chat overlay: summonable from any view (⌘⇧C / Omnibar). */}
      <ChatOverlay
        open={chatOverlayOpen}
        onClose={() => setChatOverlayOpen(false)}
        onExpand={(id) => {
          setChatOverlayOpen(false);
          setView({ kind: "chat", id });
        }}
      />

      {/* Universal ⌘K palette + the settings instance it opens. */}
      <Omnibar
        onOpenPaper={openPaper}
        onGoLibrary={() => setView({ kind: "library" })}
        onGoResearch={() => setView({ kind: "research" })}
        onOpenSettings={() => setSettingsOpen(true)}
        onOpenNote={(id) => setView({ kind: "note", id })}
        onOpenCanvas={(id) => setView({ kind: "canvas", id })}
        onOpenChat={(id) => setView({ kind: "chat", id })}
        onOpenChatOverlay={() => setChatOverlayOpen(true)}
      />
      <Settings open={settingsOpen} onOpenChange={setSettingsOpen} showTrigger={false} />
      <Toaster />
      <AppContextMenu />
    </TooltipProvider>
  );
}
