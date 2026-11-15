// Two people (separate browsers) on one document: the page, the theme, a
// half-typed line elsewhere, and undo that is per person, for typing and
// for picture edits. Needs a built editor (javascript/editor/dist) and the
// network; `nix run .#ci-editor` does both.

import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { after, before, test } from "node:test";
import { browser, ready, serve, sleep, sync } from "./harness.mjs";

const dist = fileURLToPath(new URL("../dist/", import.meta.url));
let site, one, two, a, b, base, url;

before(async () => {
  site = await serve(dist);
  base = `${site.url}${sync}`;
  [one, two] = await Promise.all([browser("A"), browser("B")]);
  [a, b] = await Promise.all([one.tab(), two.tab()]);
  // A's system is dark, before loading
  await a.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });
  await a.open(base);
  url = await a.until(ready, "A to load");
  await b.open(`${base}#${url}`);
  await b.until(ready, "B to load");
});

after(async () => {
  await Promise.all([one?.close(), two?.close()]);
  site?.close();
});

test("the preview's panes, with CodeMirror for the text", async () => {
  assert.ok(await a.evaluate(`["source-pane", "output-pane", "notation-bar", "export-bar", "warnings", "output"].every((id) => document.getElementById(id))`));
  assert.ok(await a.evaluate(`!!document.querySelector("#source .cm-editor")`));
});

test("the share link is just the link", async () => {
  assert.equal(await a.evaluate(`document.getElementById("share").textContent`), `${base}#${url}`);
});

test("the text is highlighted, principals in their diagram colours", async () => {
  const principals = await a.evaluate(`[...document.querySelectorAll("#source .tok-principal")].map((p) => p.style.color)`);
  assert.ok(principals.length > 0);
  assert.ok(principals.every(Boolean), "every principal has a colour");
});

test("the theme follows the system, then the toggle, for page and nodes alike", async () => {
  const colours = `(() => { const head = document.querySelector("rain-diagram").shadowRoot.querySelector(".head"); return [getComputedStyle(document.body).backgroundColor, head && getComputedStyle(head).fill]; })()`;
  assert.deepEqual(await a.evaluate(colours), ["rgb(22, 21, 20)", "rgb(31, 30, 28)"]);
  await a.evaluate(`document.querySelector("[data-theme-choice=light]").click()`);
  assert.deepEqual(await a.evaluate(colours), ["rgb(251, 250, 247)", "rgb(255, 255, 255)"]);
  await a.evaluate(`document.querySelector("[data-theme-choice=dark]").click()`);
  assert.deepEqual(await a.evaluate(colours), ["rgb(22, 21, 20)", "rgb(31, 30, 28)"]);
});

test("the theme is each person's own", async () => {
  assert.equal(await b.evaluate(`document.documentElement.dataset.theme ?? "system"`), "system");
});

test("a half-typed line elsewhere keeps the diagram up, with the error and the line marked", async () => {
  const events = `document.querySelector("rain-diagram").shadowRoot.querySelectorAll(".event").length`;
  const drawn = await b.evaluate(events);
  assert.ok(drawn > 0);
  await a.evaluate(`view.dispatch({ changes: { from: view.state.doc.length, insert: "(Bob)-[rea" }, userEvent: "input.type" })`);
  await b.until(`window.handle.doc().source.endsWith("(Bob)-[rea")`, "the broken text to reach B");
  await b.until(`document.getElementById("status").textContent.startsWith("✗")`, "B to show the error");
  assert.equal(await b.evaluate(events), drawn);
  assert.ok(await b.until(`document.querySelector("#source .cm-line.err-line")?.textContent.includes("(Bob)-[rea")`, "B to mark the line"));
  await a.evaluate(`view.dispatch({ changes: { from: view.state.doc.length - "(Bob)-[rea".length, to: view.state.doc.length }, userEvent: "delete" })`);
  await b.until(`document.getElementById("status").textContent.startsWith("✓")`, "B to parse again");
});

const pictureEdit = (from, to) =>
  `document.querySelector("rain-diagram").dispatchEvent(new CustomEvent("rain-edit", { detail: { source: window.handle.doc().source.replace(${JSON.stringify(from)}, ${JSON.stringify(to)}) } }))`;

test("undo is per person: A's undo takes back A's typing, not B's picture edit", async () => {
  await a.evaluate(`view.dispatch({ changes: { from: 0, insert: "// A typed this\\n" }, userEvent: "input.type" })`);
  await b.until(`window.handle.doc().source.includes("A typed this")`, "A's line to reach B");
  await b.evaluate(pictureEdit("Bob edits the Doc", "B edited the picture"));
  await a.until(`window.handle.doc().source.includes("B edited the picture")`, "B's edit to reach A");
  await a.evaluate(`document.getElementById("undo").click()`);
  const source = await b.until(`!window.handle.doc().source.includes("A typed this") && window.handle.doc().source`, "A's undo to reach B");
  assert.ok(source.includes("B edited the picture"));
});

test("picture edits are undoable, per person: A's undo takes back A's picture edit, not B's typing", async () => {
  await a.evaluate(pictureEdit("Alice passes Edit to Bob", "A moved this in the picture"));
  await b.until(`window.handle.doc().source.includes("A moved this in the picture")`, "A's picture edit to reach B");
  await b.evaluate(`view.dispatch({ changes: { from: 0, insert: "// B typed this\\n" }, userEvent: "input.type" })`);
  await a.until(`window.handle.doc().source.includes("B typed this")`, "B's typing to reach A");
  await a.evaluate(`document.getElementById("undo").click()`);
  const source = await b.until(`!window.handle.doc().source.includes("A moved this in the picture") && window.handle.doc().source`, "A's undo to reach B");
  assert.ok(source.includes("Alice passes Edit to Bob"));
  assert.ok(source.includes("B typed this"));
});

test("a picture edit made on a stale picture keeps what was typed since", async () => {
  // B starts a line that doesn't parse: A's picture keeps the last good text,
  // and builds A's next edit from it
  await b.until(`document.getElementById("status").textContent.startsWith("✓")`, "B to be parsing");
  const pictured = await a.evaluate(`window.handle.doc().source`);
  await b.evaluate(`view.dispatch({ changes: { from: view.state.doc.length, insert: "(Bob)-[rea" }, userEvent: "input.type" })`);
  await a.until(`document.getElementById("status").textContent.startsWith("✗") && view.state.doc.toString().endsWith("(Bob)-[rea")`, "B's broken line on A");
  const candidate = pictured.replace("The Doc grants Alice Edit", "A edited a stale picture");
  await a.evaluate(`document.querySelector("rain-diagram").dispatchEvent(new CustomEvent("rain-edit", { detail: { source: ${JSON.stringify(candidate)} } }))`);
  const source = await b.until(`window.handle.doc().source.includes("A edited a stale picture") && window.handle.doc().source`, "A's edit on B");
  assert.ok(source.endsWith("(Bob)-[rea"), `B's half-typed line survives:\n${source}`);
  await b.evaluate(`view.dispatch({ changes: { from: view.state.doc.length - "(Bob)-[rea".length, to: view.state.doc.length }, userEvent: "delete" })`);
  await a.until(`document.getElementById("status").textContent.startsWith("✓")`, "A to parse again");
});

test("CRLF line endings: positions in the text are the document's", async () => {
  const crlf = "rain\r\ntitle Windows\r\n\r\n(Alice)-[edit(Doc)]->(Bob)\r\n";
  await a.evaluate(`window.handle.change((d) => { d.source = ${JSON.stringify(crlf)} })`);
  await a.until(`view.state.doc.toString() === ${JSON.stringify(crlf)}`, "the CRLF text in A's editor");
  await a.evaluate(`view.dispatch({ changes: { from: view.state.doc.line(4).from, insert: "X" }, userEvent: "input.type" })`);
  const expected = crlf.replace("(Alice)", "X(Alice)");
  assert.equal(await b.until(`window.handle.doc().source.includes("X") && window.handle.doc().source`, "A's keystroke on B"), expected);
  assert.equal(await b.until(`view.state.doc.toString() === ${JSON.stringify(expected)} && view.state.doc.toString()`, "B's editor"), expected);
});

test("pasting another document's link into the tab opens that document", async () => {
  const other = await two.tab("B2");
  await other.open(base);
  const url = await other.until(ready, "a second document");
  await a.evaluate(`location.hash = ${JSON.stringify(url)}`);
  await a.until(`window.handle?.url === ${JSON.stringify(url)} && document.getElementById("share").textContent.endsWith(${JSON.stringify(url)})`, "A on the second document");
  other.close();
});

test("no page errors", async () => {
  await sleep(200);
  assert.deepEqual([...a.errors, ...b.errors], []);
});
