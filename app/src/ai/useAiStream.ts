import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@/platform";
import { listen } from "@/platform";

export type AiAction =
  | "explain"
  | "ask"
  | "variable_breakdown"
  | "step_by_step"
  | "intuition"
  | "derivation"
  | "assumptions"
  | "prerequisites"
  | "common_mistakes"
  | "figure_describe"
  | "figure_interpret"
  | "table_summarize"
  | "table_query"
  | "citation_card"
  | "hover_summary";

interface AiStreamEvent {
  request_id: string;
  token?: string;
  done?: boolean;
  error?: string;
  host?: string;
  cancelled?: boolean;
}

export interface AiStreamState {
  text: string;
  streaming: boolean;
  /** Set on failure; partial text above is preserved and labeled. */
  error: string | null;
  done: boolean;
  cancelled: boolean;
  /** Egress indicator: where paper content is being sent (actual host). */
  host: string | null;
}

const IDLE: AiStreamState = {
  text: "",
  streaming: false,
  error: null,
  done: false,
  cancelled: false,
  host: null,
};

/**
 * Streamed AI action anchored to a paper object. Tokens arrive over the
 * `ai-stream` Tauri event channel; partial output survives mid-stream
 * failures and user cancellation (cancel-anytime UX for slow reasoning
 * models).
 */
export function useAiStream(paperId: string) {
  const [state, setState] = useState<AiStreamState>(IDLE);
  const activeRequest = useRef<string | null>(null);

  useEffect(() => {
    const unlisten = listen<AiStreamEvent>("ai-stream", ({ payload }) => {
      if (payload.request_id !== activeRequest.current) return;
      setState((s) => {
        if (payload.host) return { ...s, host: payload.host };
        if (payload.token) return { ...s, text: s.text + payload.token };
        if (payload.done) return { ...s, streaming: false, done: true };
        if (payload.cancelled) return { ...s, streaming: false, cancelled: true, done: true };
        if (payload.error) return { ...s, streaming: false, error: payload.error };
        return s;
      });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const start = useCallback(
    (
      objectId: string,
      action: AiAction,
      question?: string,
      adhocText?: string,
      images?: { media_type: string; data_b64: string }[],
    ) => {
      const requestId = crypto.randomUUID();
      activeRequest.current = requestId;
      setState({ ...IDLE, streaming: true });
      invoke<string>("ai_stream", {
        requestId,
        paperId,
        objectId,
        action,
        question: question ?? null,
        adhocText: adhocText ?? null,
        images: images && images.length > 0 ? images : null,
      }).catch((e) => {
        // Errors before any event (e.g. no provider) land here.
        if (activeRequest.current === requestId) {
          setState((s) => ({ ...s, streaming: false, error: String(e) }));
        }
      });
      return requestId;
    },
    [paperId],
  );

  const cancel = useCallback(() => {
    const requestId = activeRequest.current;
    if (requestId) invoke("ai_cancel", { requestId }).catch(() => {});
  }, []);

  const reset = useCallback(() => {
    activeRequest.current = null;
    setState(IDLE);
  }, []);

  return { ...state, start, cancel, reset };
}
