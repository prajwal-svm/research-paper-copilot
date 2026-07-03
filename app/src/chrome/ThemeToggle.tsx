import { useEffect, useState } from "react";
import { AnimatedThemeToggler } from "@/components/ui/animated-theme-toggler";
import { cn } from "@/lib/utils";

type Theme = "light" | "dark";

const STORAGE_KEY = "rpc-theme";

function systemTheme(): Theme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function storedTheme(): Theme | null {
  const value = localStorage.getItem(STORAGE_KEY);
  return value === "light" || value === "dark" ? value : null;
}

export function applyTheme(theme: Theme) {
  document.documentElement.classList.toggle("dark", theme === "dark");
  document.documentElement.style.colorScheme = theme;
}

/** Apply persisted (or system) theme before first paint. */
export function initTheme() {
  applyTheme(storedTheme() ?? systemTheme());
}

/** Flip the theme programmatically (omnibar /theme). Persists like the button. */
export function toggleTheme() {
  const next: Theme = document.documentElement.classList.contains("dark") ? "light" : "dark";
  localStorage.setItem(STORAGE_KEY, next);
  applyTheme(next);
}

/**
 * Theme switcher with the MagicUI view-transition reveal (circle expanding
 * from the button). Controlled: persistence and `color-scheme` stay ours;
 * the component only animates the class flip.
 */
export default function ThemeToggle({ className }: { className?: string }) {
  const [theme, setTheme] = useState<Theme>(() => storedTheme() ?? systemTheme());

  // Stay in sync when the theme is flipped elsewhere (omnibar /theme).
  useEffect(() => {
    const observer = new MutationObserver(() =>
      setTheme(document.documentElement.classList.contains("dark") ? "dark" : "light"),
    );
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    return () => observer.disconnect();
  }, []);

  return (
    <AnimatedThemeToggler
      theme={theme}
      onThemeChange={(next) => {
        localStorage.setItem(STORAGE_KEY, next);
        document.documentElement.style.colorScheme = next;
        setTheme(next);
      }}
      className={cn(
        "inline-flex size-8 items-center justify-center rounded-md hover:bg-accent [&_svg]:size-4",
        className,
      )}
      title={`Switch to ${theme === "dark" ? "light" : "dark"} theme`}
    />
  );
}
