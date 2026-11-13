import { defineConfig } from "vite";

// Top-level await (the signer, finding the document) needs a modern target
export default defineConfig({
  build: { target: "es2022" },
});
