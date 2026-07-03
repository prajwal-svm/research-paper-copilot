import { useEffect, useRef } from "react";
import { useCreateBlockNote } from "@blocknote/react";
import { BlockNoteView } from "@blocknote/shadcn";
import "@blocknote/shadcn/style.css";

/**
 * Block-based markdown editor (BlockNote, shadcn flavor) used everywhere
 * markdown is edited: notes and chat messages (yours and the AI's).
 * Content round-trips as markdown — the stored format never changes, so
 * journals and exports stay plain `.md`-compatible.
 */
export default function MarkdownEditor({
  initialMarkdown,
  onMarkdownChange,
  autoFocus = false,
}: {
  initialMarkdown: string;
  /** Fires (debounced by BlockNote's change cadence) with fresh markdown. */
  onMarkdownChange: (markdown: string) => void;
  autoFocus?: boolean;
}) {
  const editor = useCreateBlockNote();
  const loaded = useRef(false);

  useEffect(() => {
    if (loaded.current) return;
    loaded.current = true;
    const blocks = editor.tryParseMarkdownToBlocks(initialMarkdown);
    editor.replaceBlocks(editor.document, blocks);
    if (autoFocus) editor.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const dark = document.documentElement.classList.contains("dark");

  return (
    <div className="markdown-editor min-h-24 rounded-md border">
      <BlockNoteView
        editor={editor}
        theme={dark ? "dark" : "light"}
        onChange={() => {
          onMarkdownChange(editor.blocksToMarkdownLossy(editor.document));
        }}
      />
    </div>
  );
}
