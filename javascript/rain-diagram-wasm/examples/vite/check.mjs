// Serve the production build (`dist/`) and load it in headless Chromium,
// checking the page actually ran the Wasm. Run after `vite build`:
// `CHROMIUM=chromium node check.mjs` (or `nix run .#ci-vite` from the repo root).

import { execFile } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "dist");
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm" };
const expected = "rendered 3 events";

const server = http.createServer((req, res) => {
  const url = decodeURI(req.url.split("?")[0]);
  const file = path.join(root, url.endsWith("/") ? `${url}index.html` : url);
  if (!file.startsWith(root)) return res.writeHead(403).end();
  fs.readFile(file, (error, body) =>
    error
      ? res.writeHead(404).end()
      : res.writeHead(200, { "Content-Type": TYPES[path.extname(file)] ?? "application/octet-stream" }).end(body),
  );
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const { port } = server.address();

const assets = fs.readdirSync(path.join(root, "assets"));
if (!assets.some((f) => f.endsWith(".wasm"))) throw new Error(`no .wasm asset in dist/assets: ${assets.join(", ")}`);

const profile = fs.mkdtempSync(path.join(import.meta.dirname, ".chromium-"));
try {
  // --virtual-time-budget lets the page finish loading the Wasm before the dump
  const dom = await new Promise((resolve, reject) =>
    execFile(
      process.env.CHROMIUM ?? "chromium",
      ["--headless=new", "--no-sandbox", "--disable-gpu", `--user-data-dir=${profile}`, "--virtual-time-budget=10000", "--dump-dom", `http://127.0.0.1:${port}/`],
      // A desktop session's LD_LIBRARY_PATH can break Nix's Chromium
      { timeout: 60_000, maxBuffer: 16 * 1024 * 1024, env: { ...process.env, LD_LIBRARY_PATH: undefined } },
      (error, stdout, stderr) => (error ? reject(new Error(`${error.message}\n${stderr}`)) : resolve(stdout)),
    ),
  );
  const status = dom.match(/<p id="status">([^<]*)<\/p>/)?.[1];
  if (status !== expected) throw new Error(`expected "${expected}", got ${JSON.stringify(status)}`);
  if (!dom.includes('<svg') ) throw new Error("no SVG in the page");
  console.log(`vite example: ${status}, wasm assets: ${assets.filter((f) => f.endsWith(".wasm")).join(", ")}`);
} finally {
  server.close();
  fs.rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
