import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Tauri expects a fixed dev port; fail rather than silently shift.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  clearScreen: false,
  // Heavy deps behind React.lazy panes: pre-bundle them up front so opening
  // a pane mid-session never triggers Vite re-optimization (the "504
  // Outdated Optimize Dep" / failed module import on first open).
  optimizeDeps: {
    include: [
      "@excalidraw/excalidraw",
      "@blocknote/core",
      "@blocknote/react",
      "@blocknote/shadcn",
      "@uiw/react-codemirror",
      "@codemirror/lang-python",
      "@codemirror/lang-rust",
      "recharts",
    ],
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"],
    },
  },
  build: {
    target: "es2022",
    sourcemap: true,
  },
});
