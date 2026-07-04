/**
 * User-selectable primary (accent) color, applied as a CSS variable override
 * on the document root and persisted per machine. `--primary` in styles.css
 * resolves through `--app-primary`, so one property drives the whole app.
 */

export interface PrimaryColor {
  name: string;
  value: string;
}

export const PRIMARY_COLORS: PrimaryColor[] = [
  { name: "Gray", value: "#8e8e93" },
  { name: "Blue", value: "#006bff" },
  { name: "Purple", value: "#8b5cf6" },
  { name: "Pink", value: "#ec4899" },
  { name: "Red", value: "#e5484d" },
  { name: "Orange", value: "#ff9500" },
  { name: "Green", value: "#30a46c" },
  { name: "Teal", value: "#12a594" },
];

export const DEFAULT_PRIMARY = "#006bff";

const STORAGE_KEY = "primary-color";

export function currentPrimaryColor(): string {
  try {
    return localStorage.getItem(STORAGE_KEY) ?? DEFAULT_PRIMARY;
  } catch {
    return DEFAULT_PRIMARY;
  }
}

export function applyPrimaryColor(color: string) {
  document.documentElement.style.setProperty("--app-primary", color);
  try {
    localStorage.setItem(STORAGE_KEY, color);
  } catch {
    // Persistence is best-effort; the session still gets the color.
  }
}

/** Restore the saved color at startup (no-op for the default). */
export function initPrimaryColor() {
  const color = currentPrimaryColor();
  if (color !== DEFAULT_PRIMARY) {
    document.documentElement.style.setProperty("--app-primary", color);
  }
}
