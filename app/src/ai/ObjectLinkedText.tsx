import { Fragment, useMemo } from "react";
import { MessageResponse } from "@/components/ai-elements/message";

/**
 * Renders streamed AI markdown, turning `[[object:UUID]]` citations into
 * clickable links that navigate the reader to the cited object.
 */
export default function ObjectLinkedText({
  text,
  labelFor,
  onNavigate,
}: {
  text: string;
  /** Human label for an object id (e.g. "Equation 1", "Section 3.2"). */
  labelFor: (objectId: string) => string | undefined;
  onNavigate: (objectId: string) => void;
}) {
  const parts = useMemo(() => splitObjectRefs(text), [text]);
  return (
    <div className="ai-response">
      {parts.map((part, i) =>
        part.kind === "text" ? (
          <Fragment key={i}>
            <MessageResponse>{part.value}</MessageResponse>
          </Fragment>
        ) : (
          <button
            key={i}
            className="mx-0.5 inline cursor-pointer rounded bg-accent px-1 text-sm font-medium text-accent-foreground hover:underline"
            onClick={() => onNavigate(part.value)}
            title="Go to this part of the paper"
          >
            {labelFor(part.value) ?? "source"}
          </button>
        ),
      )}
    </div>
  );
}

const OBJECT_REF = /\[\[object:([0-9a-f-]{36})\]\]/g;

function splitObjectRefs(
  text: string,
): { kind: "text" | "ref"; value: string }[] {
  const parts: { kind: "text" | "ref"; value: string }[] = [];
  let last = 0;
  for (const match of text.matchAll(OBJECT_REF)) {
    if (match.index! > last) {
      parts.push({ kind: "text", value: text.slice(last, match.index) });
    }
    parts.push({ kind: "ref", value: match[1] });
    last = match.index! + match[0].length;
  }
  if (last < text.length) {
    parts.push({ kind: "text", value: text.slice(last) });
  }
  return parts;
}
