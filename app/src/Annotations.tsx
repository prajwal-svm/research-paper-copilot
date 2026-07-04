import { useRef, useState } from "react";
import { invoke } from "@/platform";
import { saveFileDialog } from "@/platform";
import { BookmarkIcon, DownloadIcon, Trash2Icon } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { MessageResponse } from "@/components/ai-elements/message";
import MarkdownEditor from "./MarkdownEditorLazy";
import type { PaperObject } from "./types";

export interface Note {
  note_id: string;
  object_id: string;
  anchor_hash: string;
  markdown: string;
  updated_at: string;
}

export interface Bookmark {
  object_id: string;
  anchor_hash: string;
  added_at: string;
}

/** Notes list + editor for one object (task 7.1), inside the panel. */
export function ObjectAnnotations({
  paperId,
  object,
  notes,
  onChanged,
}: {
  paperId: string;
  object: PaperObject;
  notes: Note[];
  onChanged: () => void;
}) {
  const [editing, setEditing] = useState<{ noteId: string; initial: string } | null>(null);
  const draft = useRef("");
  const objectNotes = notes.filter((n) => n.object_id === object.id);

  async function saveNote() {
    if (!editing || !draft.current.trim()) {
      setEditing(null);
      return;
    }
    await invoke("note_save", {
      paperId,
      noteId: editing.noteId,
      objectId: object.id,
      anchorHash: object.content_hash,
      markdown: draft.current.trim(),
    }).catch(() => {});
    setEditing(null);
    onChanged();
  }

  return (
    <div className="flex flex-col gap-2">
      {objectNotes.map((note) =>
        editing?.noteId === note.note_id ? null : (
          <div key={note.note_id} className="group rounded-md border p-2 text-sm">
            <MessageResponse>{note.markdown}</MessageResponse>
            <div className="mt-1 flex gap-1 opacity-0 transition-opacity group-hover:opacity-100">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  draft.current = note.markdown;
                  setEditing({ noteId: note.note_id, initial: note.markdown });
                }}
              >
                Edit
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={async () => {
                  await invoke("note_delete", { paperId, noteId: note.note_id }).catch(() => {});
                  onChanged();
                }}
              >
                <Trash2Icon data-icon="inline-start" />
                Delete
              </Button>
            </div>
          </div>
        ),
      )}

      {editing && (
        <div className="flex flex-col gap-1.5">
          <MarkdownEditor
            key={editing.noteId}
            initialMarkdown={editing.initial}
            onMarkdownChange={(md) => (draft.current = md)}
            autoFocus
          />
          <div className="flex gap-1.5">
            <Button size="sm" onClick={saveNote}>
              Save
            </Button>
            <Button variant="ghost" size="sm" onClick={() => setEditing(null)}>
              Cancel
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

/** Bookmarks list + Markdown export (tasks 7.1/7.2), lives in the reader bar. */
export function AnnotationsMenu({
  paperId,
  paperTitle,
  bookmarks,
  labelFor,
  onNavigate,
  iconOnly,
}: {
  paperId: string;
  paperTitle?: string;
  bookmarks: Bookmark[];
  labelFor: (objectId: string) => string | undefined;
  onNavigate: (objectId: string) => void;
  iconOnly?: boolean;
}) {
  async function exportMarkdown() {
    const dest = await saveFileDialog({
      defaultPath: `${(paperTitle ?? "notes").slice(0, 40)}-notes.md`,
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (typeof dest === "string") {
      await invoke("export_annotations", { paperId, destPath: dest }).catch(() => {});
    }
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        {iconOnly ? (
          <Button variant="ghost" size="icon" title="Bookmarks and notes">
            <BookmarkIcon />
          </Button>
        ) : (
          <Button variant="ghost" size="sm" title="Bookmarks and notes">
            <BookmarkIcon data-icon="inline-start" />
            Bookmarks
          </Button>
        )}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-72">
        <DropdownMenuLabel>Bookmarks</DropdownMenuLabel>
        <DropdownMenuGroup>
          {bookmarks.length === 0 && (
            <DropdownMenuItem disabled>No bookmarks yet</DropdownMenuItem>
          )}
          {bookmarks.map((bookmark) => (
            <DropdownMenuItem
              key={bookmark.object_id}
              onClick={() => onNavigate(bookmark.object_id)}
            >
              <span className="truncate">
                {labelFor(bookmark.object_id) ?? "Bookmarked object"}
              </span>
            </DropdownMenuItem>
          ))}
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuItem onClick={exportMarkdown}>
            <DownloadIcon />
            Export notes as Markdown…
          </DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
