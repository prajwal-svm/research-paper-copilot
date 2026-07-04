import { exportToBlob } from "@excalidraw/excalidraw";
import type { ExcalidrawImperativeAPI } from "@excalidraw/excalidraw/types";

// Self-hosted Excalidraw fonts (copied to public/excalidraw-assets at build
// time) — without this the runtime fetches fonts from a CDN, which breaks
// the app's offline-first posture. Importing this module sets the path once.
declare global {
  interface Window {
    EXCALIDRAW_ASSET_PATH?: string | string[];
  }
}
if (typeof window !== "undefined") {
  window.EXCALIDRAW_ASSET_PATH = "/excalidraw-assets/";
}

/**
 * Text description of a scene so non-vision models can reason about it too:
 * labeled shapes and arrow connections. Shared by the per-paper concept map
 * (GraphView) and standalone canvases.
 */
export function canvasSummary(
  elements: readonly Record<string, any>[],
): string {
  const byId = new Map(elements.map((el) => [el.id, el]));
  const textOf = (el?: Record<string, any>): string => {
    if (!el) return "?";
    if (el.type === "text") return el.text ?? "?";
    const bound = (el.boundElements ?? []).find(
      (b: { type: string }) => b.type === "text",
    );
    const label = bound && byId.get(bound.id);
    return label?.text ?? el.type;
  };
  const lines: string[] = [];
  for (const el of elements) {
    if (el.isDeleted) continue;
    if (el.type === "arrow") {
      const from = textOf(byId.get(el.startBinding?.elementId));
      const to = textOf(byId.get(el.endBinding?.elementId));
      lines.push(`arrow: "${from}" -> "${to}"`);
    } else if (el.type === "text" && !el.containerId) {
      lines.push(`text: "${el.text}"`);
    } else if (["rectangle", "ellipse", "diamond"].includes(el.type as string)) {
      lines.push(`${el.type}: "${textOf(el)}"`);
    }
  }
  return "The user's diagram canvas:\n" + lines.join("\n");
}

/** Export the current scene to a PNG data URL. `null` if the scene is empty
 * or export fails (callers proceed with the text summary alone). */
export async function exportSceneDataUrl(
  api: ExcalidrawImperativeAPI,
  maxWidthOrHeight = 1600,
): Promise<string | null> {
  try {
    const blob = await exportToBlob({
      elements: api.getSceneElements(),
      appState: { exportBackground: true, viewBackgroundColor: "#ffffff" },
      files: api.getFiles(),
      mimeType: "image/png",
      maxWidthOrHeight,
    });
    return await new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onloadend = () => resolve(String(reader.result));
      reader.onerror = reject;
      reader.readAsDataURL(blob);
    });
  } catch {
    return null;
  }
}

/** Base64 payload of a data URL (no `data:...;base64,` prefix). */
export function dataUrlToBase64(dataUrl: string): string {
  return dataUrl.slice(dataUrl.indexOf(",") + 1);
}
