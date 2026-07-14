/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { copyFileSync, mkdirSync } from "node:fs";
import path from "path";

export default defineConfig({
  base: process.env.GOLDENEYE_UI_BASE_PATH || "/",
  plugins: [
    react(),
    tailwindcss(),
    {
      name: "goldeneye-legal-assets",
      closeBundle() {
        const legalDir = path.resolve(__dirname, "dist/legal");
        mkdirSync(legalDir, { recursive: true });
        for (const name of ["LICENSE", "UPSTREAM_LICENSE", "THIRD_PARTY_LICENSES.md"]) {
          copyFileSync(path.resolve(__dirname, name), path.join(legalDir, name));
        }
      },
    },
  ],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
  },
  build: {
    outDir: "dist",
    assetsDir: "assets",
    sourcemap: false,
    rollupOptions: {
      output: {
        manualChunks: undefined,
      },
    },
  },
  server: {
    port: 5173,
    proxy: {
      "/rpc": "http://127.0.0.1:9749",
      "/api": "http://127.0.0.1:9749",
    },
  },
});
