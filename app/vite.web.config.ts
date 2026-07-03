import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Web build (v5 platform-parity): same plugins, same alias, same view
// modules — a separate entry (web.html → main.web.tsx) and output dir.
// Desktop (vite.config.ts) is untouched.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  build: {
    target: "es2022",
    outDir: "dist-web",
    sourcemap: true,
    rollupOptions: {
      input: path.resolve(__dirname, "web.html"),
    },
  },
});
