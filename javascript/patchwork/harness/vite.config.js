import { defineConfig } from "vite";

// See ../../editor/vite.config.js for why these two aren't pre-bundled
export default defineConfig({
  base: "./",
  build: { target: "es2022" },
  optimizeDeps: { exclude: ["@automerge/automerge", "@automerge/subduction"] },
});
