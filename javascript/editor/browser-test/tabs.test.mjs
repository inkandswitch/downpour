// Two tabs in one browser: one keeps the offline copy (IndexedDB) under the
// browser's lasting identity, the other syncs in memory as a passing peer;
// edits flow both ways; the owner reloads with everything, as itself.

import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { after, before, test } from "node:test";
import { browser, ready, serve, sync } from "./harness.mjs";

const dist = fileURLToPath(new URL("../dist/", import.meta.url));
let site, chrome, a, b;

before(async () => {
  site = await serve(dist);
  chrome = await browser("tabs");
  a = await chrome.tab("A");
  await a.open(`${site.url}${sync}`);
  const url = await a.until(ready, "tab A");
  b = await chrome.tab("B");
  await b.open(`${site.url}${sync}#${url}`);
  await b.until(ready, "tab B");
});

after(async () => {
  await chrome?.close();
  site?.close();
});

test("the first tab keeps the offline copy; the second syncs in memory", async () => {
  assert.equal(await a.evaluate("window.ownsStorage"), true);
  assert.equal(await b.evaluate("window.ownsStorage"), false);
});

test("edits flow both ways between the tabs", async () => {
  await a.evaluate(`view.dispatch({ changes: { from: view.state.doc.length, insert: "(Bob)-[read(Doc)]->(Carol)\\n" }, userEvent: "input.type" })`);
  await b.until(`window.handle.doc().source.includes("(Bob)-[read(Doc)]->(Carol)")`, "A's edit in B");
  await b.evaluate(`view.dispatch({ changes: { from: 0, insert: "// from B\\n" }, userEvent: "input.type" })`);
  await a.until(`window.handle.doc().source.includes("// from B")`, "B's edit in A");
});

test("each tab is its own peer", async () => {
  assert.notEqual(await a.evaluate("window.peerId"), await b.evaluate("window.peerId"));
});

test("the owner reloads with everything, as the same peer, and keeps the offline copy", async () => {
  const before = await a.evaluate("window.peerId");
  await a.send("Page.reload");
  await a.until(`${ready} && window.handle.doc().source.includes("// from B")`, "A after reload");
  assert.equal(await a.evaluate("window.ownsStorage"), true);
  assert.equal(await a.evaluate("window.peerId"), before);
});

test("an in-memory tab is a new peer each time", async () => {
  const before = await b.evaluate("window.peerId");
  await b.send("Page.reload");
  await b.until(ready, "B after reload");
  assert.equal(await b.evaluate("window.ownsStorage"), false);
  assert.notEqual(await b.evaluate("window.peerId"), before);
});

test("when the owner closes, the next tab to load takes over, as the browser's same peer", async () => {
  const browserPeer = await a.evaluate("window.peerId");
  await a.send("Page.close");
  await b.send("Page.reload");
  await b.until(`${ready} && window.ownsStorage`, "B to take over storage", 20_000);
  assert.equal(await b.evaluate("window.peerId"), browserPeer);
  assert.ok(await b.evaluate(`window.handle.doc().source.includes("// from B")`));
});
