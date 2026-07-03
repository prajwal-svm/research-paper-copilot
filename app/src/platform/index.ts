/**
 * Platform adapter (v5 platform-parity): the single boundary between the
 * shared frontend and the host platform. Desktop = Tauri IPC; web = the
 * WASM core (installed via `setWebInvoke` by the web entry). No view may
 * import `@tauri-apps/*` directly — everything goes through here.
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  open as tauriOpen,
  save as tauriSave,
  type OpenDialogOptions,
  type SaveDialogOptions,
} from "@tauri-apps/plugin-dialog";

export type Platform = "desktop" | "web";

export const platform: Platform =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window ? "desktop" : "web";

type InvokeFn = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/** Web command handler, installed by the web entry (WASM-core bridge). */
let webInvoke: InvokeFn = async (command) => {
  throw new Error(
    `"${command}" is not available on web yet — the web core bridge hasn't been installed`,
  );
};

export function setWebInvoke(handler: InvokeFn) {
  webInvoke = handler;
}

/** Run a backend command on whichever platform hosts us. */
export const invoke: InvokeFn = (command, args) =>
  platform === "desktop" ? tauriInvoke(command, args) : webInvoke(command, args);

/** Subscribe to backend events. Web: events surface via the same bus the
 * web bridge feeds (no-op until installed). */
export async function listen<T>(
  event: string,
  handler: (event: { payload: T }) => void,
): Promise<UnlistenFn> {
  if (platform === "desktop") {
    return tauriListen<T>(event, handler);
  }
  const listeners = webListeners.get(event) ?? new Set();
  listeners.add(handler as (event: { payload: unknown }) => void);
  webListeners.set(event, listeners);
  return () => {
    listeners.delete(handler as (event: { payload: unknown }) => void);
  };
}

const webListeners = new Map<string, Set<(event: { payload: unknown }) => void>>();

/** Emit into web-side listeners (used by the web bridge). */
export function emitWebEvent(event: string, payload: unknown) {
  for (const handler of webListeners.get(event) ?? []) {
    handler({ payload });
  }
}

/** Native file-open dialog. Web: an <input type=file> flow lands with the
 * web bridge; until then returns null (feature degrades explicitly). */
export async function openFileDialog(
  options: OpenDialogOptions,
): Promise<string | string[] | null> {
  if (platform === "desktop") {
    return tauriOpen(options);
  }
  return null;
}

export async function saveFileDialog(options: SaveDialogOptions): Promise<string | null> {
  if (platform === "desktop") {
    return tauriSave(options);
  }
  return null;
}

/** OS-level file drag-and-drop (desktop only; web uses HTML5 DnD). */
export async function onFileDrop(handlers: {
  onOver?: () => void;
  onLeave?: () => void;
  onDrop: (paths: string[]) => void;
}): Promise<UnlistenFn> {
  if (platform !== "desktop") {
    return () => {};
  }
  return getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "over") handlers.onOver?.();
    if (event.payload.type === "leave") handlers.onLeave?.();
    if (event.payload.type === "drop") handlers.onDrop(event.payload.paths);
  });
}

export interface Capability {
  id: string;
  label: string;
  availability: "native" | "web" | "web_via_runner";
  web_note?: string;
}

let matrixCache: Capability[] | null = null;

/** Capability parity matrix (single source of truth in the Rust core). */
export async function capabilities(): Promise<Capability[]> {
  if (!matrixCache) {
    matrixCache = await invoke<Capability[]>("capability_matrix").catch(() => []);
  }
  return matrixCache;
}

/** True when a feature is fully usable on the current platform. */
export async function isAvailable(id: string): Promise<boolean> {
  if (platform === "desktop") return true;
  const capability = (await capabilities()).find((c) => c.id === id);
  return capability?.availability === "web";
}
