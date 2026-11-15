import { defineConfig } from "vite";

// What Patchwork's importmap provides, so the tool uses Patchwork's copies:
// @inkandswitch/patchwork-bootloader/externals (0.9.7), less what this tool
// never imports. Everything else is bundled.
const EXTERNALS = [
  "@automerge/automerge",
  "@automerge/automerge/slim",
  "@automerge/automerge-repo",
  "@automerge/automerge-repo/slim",
  "@codemirror/commands",
  "@codemirror/language",
  "@codemirror/state",
  "@codemirror/view",
];

export default defineConfig({
  // Patchwork serves the module from wherever it keeps it
  base: "./",
  build: {
    target: "es2022",
    emptyOutDir: true,
    // The entry is read in a worker: no preload helper touching `document`
    modulePreload: false,
    rolldownOptions: {
      input: { index: "index.js" },
      external: EXTERNALS,
      output: {
        format: "es",
        entryFileNames: "[name].js",
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash][extname]",
      },
      // Patchwork reads `plugins` from the entry
      preserveEntrySignatures: "strict",
    },
  },
});
