import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@/platform";
import { RefreshCwIcon, SearchCodeIcon } from "lucide-react";

/**
 * App-wide custom context menu: right-click / ctrl+click anywhere opens it
 * instead of the webview's default (inspect/reload) menu. Areas that need
 * their own menu can stop propagation of `contextmenu`.
 *
 * Dismissal listens to `mousedown`, not `click`: on macOS a ctrl+click
 * gesture fires `contextmenu` and then a `click` from the same press, which
 * would close the menu the instant it opened. `mousedown` fires before
 * `contextmenu`, so the opening gesture can never dismiss its own menu.
 */
export default function AppContextMenu() {
  const [position, setPosition] = useState<{ x: number; y: number } | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onContextMenu = (e: MouseEvent) => {
      // Always suppress the webview default; show ours.
      e.preventDefault();
      const menuWidth = 176;
      const menuHeight = 88;
      setPosition({
        x: Math.min(e.clientX, window.innerWidth - menuWidth - 8),
        y: Math.min(e.clientY, window.innerHeight - menuHeight - 8),
      });
    };
    const onMouseDown = (e: MouseEvent) => {
      // Presses inside the menu are handled by the items themselves.
      if (menuRef.current?.contains(e.target as Node)) return;
      setPosition(null);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setPosition(null);
    };
    const onBlur = () => setPosition(null);
    window.addEventListener("contextmenu", onContextMenu);
    window.addEventListener("mousedown", onMouseDown, true);
    window.addEventListener("blur", onBlur);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("contextmenu", onContextMenu);
      window.removeEventListener("mousedown", onMouseDown, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  const item = useCallback(
    (label: string, icon: React.ReactNode, action: () => void) => (
      <button
        className="flex w-full cursor-pointer items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm hover:bg-accent"
        onClick={() => {
          setPosition(null);
          action();
        }}
      >
        {icon}
        {label}
      </button>
    ),
    [],
  );

  if (!position) return null;
  return (
    <div
      ref={menuRef}
      className="fixed z-50 w-44 rounded-md border bg-popover p-1 text-popover-foreground shadow-md"
      style={{ left: position.x, top: position.y }}
      onContextMenu={(e) => {
        // Right-click on the open menu itself shouldn't re-open/move it.
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      {item("Reload", <RefreshCwIcon className="size-4 opacity-70" />, () =>
        window.location.reload(),
      )}
      {import.meta.env.DEV &&
        item("Inspect (dev)", <SearchCodeIcon className="size-4 opacity-70" />, () => {
          invoke("open_devtools").catch(() => {});
        })}
    </div>
  );
}
