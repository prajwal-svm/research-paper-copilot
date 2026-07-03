import { invoke } from "@/platform";
import { openFileDialog } from "@/platform";

/**
 * OS file picker → PDF import. Returns null on success or cancel,
 * an error message otherwise. Safe to call from any view: the library
 * refreshes itself from pipeline events.
 */
export async function pickAndImportPdf(): Promise<string | null> {
  const selected = await openFileDialog({
    multiple: false,
    filters: [{ name: "PDF", extensions: ["pdf"] }],
  });
  if (typeof selected !== "string") return null;
  try {
    await invoke<string>("import_pdf_file", { path: selected });
    return null;
  } catch (e) {
    return String(e);
  }
}
