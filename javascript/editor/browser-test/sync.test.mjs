// Whether everything has reached the sync server, said in the header; and a
// document that isn't there yet is looked for until it is.

import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { after, before, test } from "node:test";
import { browser, ready, serve, sync } from "./harness.mjs";

const dist = fileURLToPath(new URL("../dist/", import.meta.url));
// Nothing listens here, so the page can't reach a server
const NOWHERE = "?sync=ws://127.0.0.1:9";
const state = `document.getElementById("sync").className`;
let site;
const browsers = [];
const open = async (name) => {
  const b = await browser(name);
  browsers.push(b);
  return b.tab();
};

before(async () => {
  site = await serve(dist);
});

after(async () => {
  await Promise.all(browsers.map((b) => b.close()));
  site?.close();
});

test("a new document syncs; an edit goes syncing, then synced", async () => {
  const a = await open("A");
  await a.open(`${site.url}${sync}`);
  await a.until(ready, "A to load");
  await a.until(`${state} === "synced"`, "synced", 15_000);
  await a.evaluate(`window.seen = []; new MutationObserver(() => window.seen.push(${state})).observe(document.getElementById("sync"), { attributes: true })`);
  await a.evaluate(`window.view.dispatch({ changes: { from: 0, insert: "// one\\n" }, userEvent: "input.type" })`);
  const seen = await a.until(`${state} === "synced" && window.seen.includes("syncing") && window.seen`, "syncing, then synced", 15_000);
  assert.equal(seen.at(-1), "synced");
});

test("with no server, an edit is not synced, and the page says where it's kept", async () => {
  const c = await open("C");
  await c.open(`${site.url}${NOWHERE}`);
  await c.until("!!window.view", "C to load", 20_000);
  await c.evaluate(`window.view.dispatch({ changes: { from: 0, insert: "// offline\\n" }, userEvent: "input.type" })`);
  const title = await c.until(`${state} === "offline" && document.getElementById("sync").title`, "not synced", 20_000);
  assert.match(title, /kept in this browser/);
});

test("a document only someone else has is looked for, and found once they connect", async () => {
  const d = await open("D");
  const e = await open("E");
  await d.open(`${site.url}${NOWHERE}`);
  const lonely = await d.until("window.handle?.url", "D to make a document", 20_000);
  await e.open(`${site.url}${sync}#${lonely}`);
  await e.until(`document.getElementById("sync").textContent.includes("looking")`, "E to be looking", 40_000);
  // D comes online with the same document (kept in its browser)
  await d.open(`${site.url}${sync}#${lonely}`);
  await e.until(ready, "E to find it", 60_000);
});
