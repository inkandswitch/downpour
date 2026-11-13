// The preview and its <rain-diagram>, driven in headless Chromium with real
// mouse and keyboard input over the DevTools protocol. No dependencies:
// Node's built-in WebSocket speaks the protocol, and a tiny static server
// serves javascript/ (the Wasm must already be built into
// rain-diagram/pkg; `nix run .#ci-browser` does both).
//
//   CHROMIUM=chromium node --test javascript/preview/browser-test/

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { after, before, describe, test } from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".css": "text/css" };

let server;
let chromium;
let profile;
let page;

/** A minimal DevTools protocol client over one page's WebSocket. */
const connect = async (url) => {
  const socket = new WebSocket(url);
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  let next = 0;
  const pending = new Map();
  socket.addEventListener("message", ({ data }) => {
    const message = JSON.parse(data);
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
  return { send, close: () => socket.close() };
};

/** Run `fn` (a function's source) in the page and return its value. */
const evaluate = async (fn, ...args) => {
  const { result, exceptionDetails } = await page.send("Runtime.evaluate", {
    expression: `(${fn})(...${JSON.stringify(args)})`,
    awaitPromise: true,
    returnByValue: true,
  });
  if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
  return result.value;
};

const until = async (fn, what, ms = 15_000) => {
  const deadline = Date.now() + ms;
  for (;;) {
    if (await evaluate(fn)) return;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 50));
  }
};

/** A fresh preview with the Basic example, ready to use. */
const fresh = async () => {
  await evaluate(() => localStorage.clear());
  await page.send("Page.reload", { ignoreCache: true });
  await until(
    () => document.getElementById("status")?.textContent.startsWith("✓") && !!document.getElementById("picture").shadowRoot?.querySelector("svg"),
    "the preview to render",
  );
};

const source = () => evaluate(() => document.getElementById("source").value);
const status = () => evaluate(() => document.getElementById("status").textContent);

/** Centre of the first element matching `selector` in the picture's shadow DOM, scrolled into view. */
const centre = (selector) =>
  evaluate((selector) => {
    const el = document.getElementById("picture").shadowRoot.querySelector(selector);
    if (!el) return null;
    el.scrollIntoView({ block: "center", inline: "center" });
    const r = el.getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2, left: r.left, right: r.right, top: r.top, bottom: r.bottom };
  }, selector);

/** Like `centre`, without scrolling. */
const rect = (selector) =>
  evaluate((selector) => {
    const r = document.getElementById("picture").shadowRoot.querySelector(selector).getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2, left: r.left, right: r.right, top: r.top, bottom: r.bottom };
  }, selector);

const mouse = (type, x, y, buttons = 0) =>
  page.send("Input.dispatchMouseEvent", { type, x, y, button: type === "mouseMoved" && !buttons ? "none" : "left", buttons, clickCount: 1 });

const click = async ({ x, y }) => {
  await mouse("mousePressed", x, y, 1);
  await mouse("mouseReleased", x, y);
};

const drag = async (from, to, steps = 12) => {
  await mouse("mousePressed", from.x, from.y, 1);
  for (let i = 1; i <= steps; i++) {
    await mouse("mouseMoved", from.x + ((to.x - from.x) * i) / steps, from.y + ((to.y - from.y) * i) / steps, 1);
  }
  await mouse("mouseReleased", to.x, to.y);
};

const key = async (key, code, keyCode, modifiers = 0) => {
  for (const type of ["rawKeyDown", "keyUp"]) {
    await page.send("Input.dispatchKeyEvent", { type, key, code, windowsVirtualKeyCode: keyCode, modifiers });
  }
};

before(async () => {
  server = http.createServer((req, res) => {
    const url = decodeURI(req.url.split("?")[0]);
    const file = path.join(root, url.endsWith("/") ? `${url}index.html` : url);
    if (!file.startsWith(root)) return res.writeHead(403).end();
    fs.readFile(file, (error, body) => {
      if (error) return res.writeHead(404).end();
      res.writeHead(200, { "Content-Type": TYPES[path.extname(file)] ?? "application/octet-stream" }).end(body);
    });
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();

  profile = fs.mkdtempSync(path.join(os.tmpdir(), "rain-browser-"));
  const env = { ...process.env };
  delete env.LD_LIBRARY_PATH; // a desktop session's libraries can clash with Nix's Chromium
  chromium = spawn(
    process.env.CHROMIUM ?? "chromium",
    ["--headless=new", "--no-sandbox", "--disable-gpu", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--window-size=1600,1200", "about:blank"],
    { env, stdio: ["ignore", "ignore", "pipe"] },
  );
  const browser = await new Promise((resolve, reject) => {
    let log = "";
    chromium.stderr.on("data", (chunk) => {
      log += chunk;
      const found = log.match(/DevTools listening on (ws:\/\/\S+)/);
      if (found) resolve(found[1]);
    });
    chromium.on("exit", (code) => reject(new Error(`chromium exited (${code}):\n${log}`)));
  });
  const { port: devtools } = new URL(browser);
  const target = await (await fetch(`http://127.0.0.1:${devtools}/json/new?http://127.0.0.1:${port}/preview/`, { method: "PUT" })).json();
  page = await connect(target.webSocketDebuggerUrl);
  await page.send("Page.enable");
  await page.send("Runtime.enable");
  await until(() => document.readyState === "complete", "the first load");
});

after(() => {
  page?.close();
  chromium?.kill();
  server?.close();
  if (profile) fs.rmSync(profile, { recursive: true, force: true });
});

describe("the preview's <rain-diagram>", () => {
  test("dragging a row down moves its statement", async () => {
    await fresh();
    const row = await centre('.event[data-event="2"] .hit');
    const height = await evaluate(() => Number(document.getElementById("picture").shadowRoot.querySelector("svg").dataset.rowHeight));
    const zoom = await evaluate(() => {
      const svg = document.getElementById("picture").shadowRoot.querySelector("svg");
      return svg.getBoundingClientRect().height / svg.viewBox.baseVal.height;
    });
    await drag(row, { x: row.x, y: row.y + height * zoom });
    const lines = (await source()).split("\n");
    const invoke = lines.findIndex((l) => l.startsWith("(Bob)=[[edit]]=>(Doc)"));
    const bc = lines.findIndex((l) => l.startsWith("bc: (Bob)-[edit]->(Carol)"));
    assert.ok(invoke > bc, `the invocation should now follow bc:\n${lines.join("\n")}`);
  });

  test("drawing between lifelines offers a menu and adds the statement", async () => {
    await fresh();
    // At row 2's height Carol's lifeline is clear (that row is Bob→Doc); lower
    // down, the "now" line's grip would take the press instead. Measure
    // after scrolling once, so the coordinates agree
    const { y } = await centre('.event[data-event="2"] .hit');
    const carol = await rect('.lifeline-hit[data-column="3"]');
    const doc = await rect('.lifeline-hit[data-column="0"]');
    const under = await evaluate(
      (x, y) => document.getElementById("picture").shadowRoot.elementFromPoint(x, y)?.getAttribute("class"),
      carol.x,
      y,
    );
    assert.equal(under, "hit lifeline-hit", "Carol's lifeline is what's under the press");
    await drag({ x: carol.x, y }, { x: doc.x, y });
    const options = await evaluate(() =>
      Array.from(document.getElementById("picture").shadowRoot.querySelectorAll(".menu button"), (b) => b.textContent),
    );
    assert.deepEqual(options, ["👑 Admin", "✏️ Edit", "👀 Read", "📡 Relay", "👑 Admin", "✏️ Edit", "👀 Read", "📡 Relay"]);
    // The second row is Invoke
    const invoke = await centre(".menu .menu-row:nth-child(2) button:nth-of-type(2)");
    await click(invoke);
    assert.match(await source(), /\n\(Carol\)=\[\[edit\]\]=>\(Doc\)\n/);
  });

  test("delete refuses what a revocation names, deletes the revocation, and undo/redo work", async () => {
    await fresh();
    const original = await source();

    await click(await centre('.event[data-event="3"] .tint'));
    await key("Backspace", "Backspace", 8);
    assert.match(await status(), /statement 5 revokes statement 3; delete that first/);
    assert.equal(await source(), original);

    await click(await centre('.event[data-event="5"] .hit'));
    await key("Backspace", "Backspace", 8);
    const deleted = await source();
    assert.doesNotMatch(deleted, /<revoke>/);

    await key("z", "KeyZ", 90, 2); // Ctrl+Z
    assert.equal(await source(), original);
    await key("y", "KeyY", 89, 2); // Ctrl+Y
    assert.equal(await source(), deleted);
    await evaluate(() => document.getElementById("undo").click());
    assert.equal(await source(), original);
  });

  test("the certflow popover never covers the clock, and an outside click closes it", async () => {
    await fresh();
    const clock = await centre(".now-handle");
    await click(clock);
    const card = await evaluate(() => {
      const card = document.getElementById("picture").shadowRoot.querySelector(".snapshot");
      if (!card) return null;
      const r = card.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom };
    });
    assert.ok(card, "the popover opens");
    const overlaps = !(card.right <= clock.left || card.left >= clock.right || card.bottom <= clock.top || card.top >= clock.bottom);
    assert.equal(overlaps, false, JSON.stringify({ card, clock }));

    await click({ x: 5, y: 5 });
    assert.equal(await evaluate(() => !!document.getElementById("picture").shadowRoot.querySelector(".snapshot")), false);
  });
});
