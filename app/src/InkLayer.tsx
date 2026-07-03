import { useCallback, useRef, useState } from "react";
import { getStroke } from "perfect-freehand";

export type InkTool = "pen" | "highlighter" | "eraser";

export interface InkStroke {
  stroke_id: string;
  page: number;
  tool: string;
  color: string;
  /** Base size in PDF points. */
  size: number;
  /** [x, y, pressure] in PDF points, top-left origin. */
  points: [number, number, number][];
  at: string;
}

export const INK_COLORS = ["#3b82f6", "#ef4444", "#22c55e", "#eab308"] as const;

const TOOL_DEFAULTS: Record<Exclude<InkTool, "eraser">, { size: number; opacity: number }> = {
  pen: { size: 2.2, opacity: 1 },
  highlighter: { size: 12, opacity: 0.35 },
};

/** perfect-freehand outline → SVG path. */
function strokePath(points: [number, number, number][], size: number): string {
  const outline = getStroke(
    points.map(([x, y, p]) => [x, y, p]),
    {
      size,
      thinning: 0.55,
      smoothing: 0.6,
      streamline: 0.45,
      simulatePressure: points.every(([, , p]) => p === 0.5),
    },
  );
  if (outline.length === 0) return "";
  const [first, ...rest] = outline;
  return `M ${first[0].toFixed(2)} ${first[1].toFixed(2)} ${rest
    .map(([x, y]) => `L ${x.toFixed(2)} ${y.toFixed(2)}`)
    .join(" ")} Z`;
}

/**
 * Freehand ink overlay for one page (perfect-freehand): buttery
 * variable-width strokes, stored in PDF points so they survive zoom.
 * Live stroke renders locally at 60fps; persistence happens on pointer-up.
 */
export default function InkLayer({
  page,
  scale,
  strokes,
  tool,
  color,
  onCommit,
  onErase,
}: {
  page: number;
  scale: number;
  strokes: InkStroke[];
  /** Active tool, or null when not in draw mode (render-only). */
  tool: InkTool | null;
  color: string;
  onCommit: (stroke: InkStroke) => void;
  onErase: (strokeId: string) => void;
}) {
  const [live, setLive] = useState<[number, number, number][]>([]);
  const drawing = useRef(false);
  const svgRef = useRef<SVGSVGElement>(null);

  const toPdf = useCallback(
    (e: React.PointerEvent): [number, number, number] => {
      const rect = svgRef.current!.getBoundingClientRect();
      return [
        (e.clientX - rect.left) / scale,
        (e.clientY - rect.top) / scale,
        e.pressure > 0 ? e.pressure : 0.5,
      ];
    },
    [scale],
  );

  const eraseAt = useCallback(
    (x: number, y: number) => {
      const threshold = 6; // PDF points
      for (const stroke of strokes) {
        if (
          stroke.points.some(
            ([px, py]) => Math.hypot(px - x, py - y) < threshold + stroke.size / 2,
          )
        ) {
          onErase(stroke.stroke_id);
        }
      }
    },
    [strokes, onErase],
  );

  const onPointerDown = (e: React.PointerEvent) => {
    if (!tool || e.button !== 0) return;
    e.stopPropagation();
    (e.target as Element).setPointerCapture(e.pointerId);
    drawing.current = true;
    const point = toPdf(e);
    if (tool === "eraser") {
      eraseAt(point[0], point[1]);
    } else {
      setLive([point]);
    }
  };

  const onPointerMove = (e: React.PointerEvent) => {
    if (!tool || !drawing.current) return;
    const point = toPdf(e);
    if (tool === "eraser") {
      eraseAt(point[0], point[1]);
      return;
    }
    setLive((prev) => {
      const last = prev[prev.length - 1];
      // Distance-filter to keep point counts small without losing fidelity.
      if (last && Math.hypot(point[0] - last[0], point[1] - last[1]) < 1.2) return prev;
      return [...prev, point];
    });
  };

  const onPointerUp = () => {
    if (!tool || !drawing.current) return;
    drawing.current = false;
    if (tool !== "eraser" && live.length > 1) {
      const defaults = TOOL_DEFAULTS[tool];
      onCommit({
        stroke_id: crypto.randomUUID(),
        page,
        tool,
        color,
        size: defaults.size,
        points: live,
        at: new Date().toISOString(),
      });
    }
    setLive([]);
  };

  const pageStrokes = strokes.filter((s) => s.page === page);
  if (!tool && pageStrokes.length === 0) return null;

  return (
    <svg
      ref={svgRef}
      className="ink-layer"
      style={{ pointerEvents: tool ? "auto" : "none" }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerLeave={onPointerUp}
    >
      <g transform={`scale(${scale})`}>
        {pageStrokes.map((stroke) => (
          <path
            key={stroke.stroke_id}
            d={strokePath(stroke.points, stroke.size)}
            fill={stroke.color}
            fillOpacity={stroke.tool === "highlighter" ? 0.35 : 1}
            style={
              stroke.tool === "highlighter" ? { mixBlendMode: "multiply" } : undefined
            }
          />
        ))}
        {live.length > 1 && tool && tool !== "eraser" && (
          <path
            d={strokePath(live, TOOL_DEFAULTS[tool].size)}
            fill={color}
            fillOpacity={tool === "highlighter" ? 0.35 : 1}
            style={tool === "highlighter" ? { mixBlendMode: "multiply" } : undefined}
          />
        )}
      </g>
    </svg>
  );
}
