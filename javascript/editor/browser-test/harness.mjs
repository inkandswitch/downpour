// What the editor's browser tests share: a static server for a built site,
// and headless Chromium driven over the DevTools protocol (Node's built-in
// WebSocket; no dependencies). Each `browser()` is its own profile, so its
// own person: own storage, own signer. `tab()`s in one browser share them.
//
// Sync goes through the public Subduction server unless DOWNPOUR_SYNC names
// another (passed to the page as `?sync=`).

import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";

const TYPES = { ".css": "text/css", ".html": "text/html", ".js": "text/javascript", ".json": "application/json", ".wasm": "application/wasm" };

/** Serve `dir` on a free port; resolves to its base URL and a `close`. */
export const serve = async (dir) => {
  const server = http.createServer((req, res) => {
    const url = decodeURIComponent(new URL(req.url, "http://x").pathname);
    const file = path.join(dir, url.endsWith("/") ? `${url}index.html` : url);
    if (!file.startsWith(dir)) return res.writeHead(403).end();
    fs.readFile(file, (error, body) =>
      error ? res.writeHead(404).end() : res.writeHead(200, { "Content-Type": TYPES[path.extname(file)] ?? "application/octet-stream" }).end(body),
    );
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { url: `http://127.0.0.1:${server.address().port}/`, close: () => server.close() };
};

/** The page's query string for the sync server under test. */
export const sync = process.env.DOWNPOUR_SYNC ? `?sync=${encodeURIComponent(process.env.DOWNPOUR_SYNC)}` : "";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** A headless Chromium with a fresh profile. */
export const browser = async (name) => {
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), `downpour-${name}-`));
  const env = { ...process.env };
  delete env.LD_LIBRARY_PATH; // Nix's, for node, breaks Chromium
  const chromium = spawn(
    process.env.CHROMIUM ?? "chromium",
    ["--headless=new", "--no-sandbox", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank"],
    { env, stdio: ["ignore", "ignore", "pipe"] },
  );
  const endpoint = await new Promise((resolve, reject) => {
    let log = "";
    chromium.stderr.on("data", (chunk) => {
      log += chunk;
      const found = log.match(/DevTools listening on (ws:\S+)/);
      if (found) resolve(found[1]);
    });
    chromium.on("exit", (code) => reject(new Error(`${name}: Chromium exited (${code}): ${log}`)));
  });
  const port = new URL(endpoint).port;

  /** A page in this browser. */
  const tab = async (label = name) => {
    const target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
    const socket = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => {
      socket.addEventListener("open", resolve, { once: true });
      socket.addEventListener("error", reject, { once: true });
    });
    let next = 0;
    const pending = new Map();
    const errors = [];
    socket.addEventListener("message", ({ data }) => {
      const message = JSON.parse(data);
      if (message.method === "Runtime.exceptionThrown") {
        const details = message.params.exceptionDetails;
        errors.push(details.exception?.description ?? details.text);
      }
      const waiter = pending.get(message.id);
      if (!waiter) return;
      pending.delete(message.id);
      message.error ? waiter.reject(new Error(message.error.message)) : waiter.resolve(message.result);
    });
    const send = (method, params = {}) =>
      new Promise((resolve, reject) => {
        const id = ++next;
        pending.set(id, { resolve, reject });
        socket.send(JSON.stringify({ id, method, params }));
      });
    await send("Runtime.enable");
    const evaluate = async (expression) => {
      const { result, exceptionDetails } = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
      if (exceptionDetails) throw new Error(`${label}: ${exceptionDetails.exception?.description ?? exceptionDetails.text}`);
      return result.value;
    };
    /** Poll `expression` until it is truthy; its value. */
    const until = async (expression, what, ms = 30_000) => {
      const end = Date.now() + ms;
      while (Date.now() < end) {
        const value = await evaluate(expression).catch(() => undefined);
        if (value) return value;
        await sleep(200);
      }
      throw new Error(`${label}: timed out waiting for ${what}${errors.length ? `; page errors: ${errors.join(" | ")}` : ""}`);
    };
    const open = (url) => send("Page.navigate", { url });
    return { errors, evaluate, open, send, until, close: () => socket.close() };
  };

  return {
    tab,
    close: async () => {
      const exited = new Promise((resolve) => chromium.once("exit", resolve));
      chromium.kill();
      await exited;
      fs.rmSync(profile, { recursive: true, force: true, maxRetries: 3 });
    },
  };
};

/** True once the editor page has loaded its document and drawn it. */
export const ready = `document.getElementById("connection")?.textContent === "connected" && document.getElementById("status")?.textContent.startsWith("✓") && window.handle?.url`;

/** Wait (for something that should *not* happen to have had its chance). */
export { sleep };
