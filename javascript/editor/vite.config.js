import { defineConfig } from "vite";

export default defineConfig({
  // Top-level await (the signer, finding the document) needs a modern target
  build: { target: "es2022" },
  // In dev, pre-bundling copies the Wasm packages into each dependent's
  // bundle, so their full and `slim` entries stop sharing one initialized
  // instance (a WebAssembly.Exception at startup). Left out, every import of
  // them resolves to the same module. (Their dependents stay pre-bundled:
  // they bring CommonJS dependencies.)
  optimizeDeps: {
    exclude: ["@automerge/automerge", "@automerge/subduction"],
  },
});
