// Who's here and where, across two browsers: the list, carets that keep to
// their character, selections, renames, leaving; and the examples, which
// replace the text for everyone as one undoable edit.

import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { after, before, test } from "node:test";
import { browser, ready, serve, sync } from "./harness.mjs";

const dist = fileURLToPath(new URL("../dist/", import.meta.url));
let site, one, two, a, b;
const people = `[...document.querySelectorAll("#people .person")].map((p) => p.textContent)`;
const caret = `(() => { const c = document.querySelector("#source .cm-remote-caret"); return c && { name: c.textContent, at: window.view.posAtDOM(c) }; })()`;

before(async () => {
  site = await serve(dist);
  [one, two] = await Promise.all([browser("A"), browser("B")]);
  [a, b] = await Promise.all([one.tab(), two.tab()]);
  await a.open(`${site.url}${sync}`);
  const url = await a.until(ready, "A to load");
  await b.open(`${site.url}${sync}#${url}`);
  await b.until(ready, "B to load");
});

after(async () => {
  await Promise.all([one?.close(), two?.close()]);
  site?.close();
});

test("each person gets a name like silly-teal-otter, in that colour", async () => {
  const [name, colour] = await a.evaluate(`(() => { const me = document.querySelector("#people button.person"); return [me.textContent, me.style.getPropertyValue("--person")]; })()`);
  assert.match(name, /^[a-z]+-(blue|brown|gold|green|pink|purple|red|teal)-[a-z]+ \(you\)$/);
  assert.match(colour, /^#[0-9a-f]{6}$/);
});

test("both people are listed, on both sides", async () => {
  assert.ok(await a.until(`(${people}).length === 2`, "A to see B", 20_000));
  assert.ok(await b.until(`(${people}).length === 2`, "B to see A", 20_000));
});

test("a caret shows where the other put it, and keeps to its character", async () => {
  const name = (await a.evaluate(`document.querySelector("#people button.person").textContent`)).replace(" (you)", "");
  const line = await a.evaluate(`(() => { const at = window.view.state.doc.line(5).from; window.view.focus(); window.view.dispatch({ selection: { anchor: at } }); return at; })()`);
  const seen = await b.until(`(${caret})?.at === ${line} && ${caret}`, "A's caret on B", 15_000);
  assert.equal(seen.name, name);
  await b.evaluate(`window.view.dispatch({ changes: { from: 0, insert: "// B was here\\n" }, userEvent: "input.type" })`);
  await b.until(`(${caret})?.at === ${line + "// B was here\n".length}`, "the caret to move with its character", 5_000);
});

test("a selection is shaded for the other", async () => {
  await a.until(`window.handle.doc().source.startsWith("// B was here")`, "B's edit on A");
  await a.evaluate(`(() => { const l = window.view.state.doc.line(6); window.view.dispatch({ selection: { anchor: l.from, head: l.to } }); })()`);
  const shaded = await b.until(`[...document.querySelectorAll("#source .cm-line span[style*=background-color]:not(.cm-remote-name)")].map((s) => s.textContent).join("")`, "A's selection on B", 15_000);
  assert.match(shaded, /Alice passes Edit to Bob/);
});

test("what someone sends about themselves can't style or script the other's page", async () => {
  const forged = { name: "<img src=x onerror=window.pwned=1>", colour: "red;position:fixed;inset:0;background:url(https://example.invalid/x);x:" };
  await b.evaluate(`(() => { const l = window.view.state.doc.line(5); window.view.focus(); window.view.dispatch({ selection: { anchor: l.from, head: l.to } }); })()`);
  await b.evaluate(`setTimeout(() => window.handle.broadcast({ __presence: { type: "update", channel: "user", value: ${JSON.stringify(forged)} } }), 300)`);
  await a.until(`(${people}).includes(${JSON.stringify(forged.name)})`, "the forged name on A", 15_000);
  const styles = await a.until(`(() => { const s = [...document.querySelectorAll("#source .cm-line span[style*=background-color]:not(.cm-remote-name)")].map((e) => e.getAttribute("style")); return s.length && s; })()`, "B's selection on A", 15_000);
  assert.ok(styles.every((style) => !style.includes("position") && !style.includes("url(")), JSON.stringify(styles));
  assert.equal(await a.evaluate(`[...document.querySelectorAll("#people .person")].find((p) => p.textContent.startsWith("<img"))?.style.getPropertyValue("--person")`), "#888888");
  assert.equal(await a.evaluate(`document.querySelectorAll("#people img").length + (window.pwned ?? 0)`), 0);
});

test("a rename reaches the other", async () => {
  await a.evaluate(`window.prompt = () => "Ada"; document.querySelector("#people button.person").click()`);
  assert.ok(await b.until(`(${people}).includes("Ada")`, "the rename on B", 15_000));
});

test("an example replaces the text for everyone, and its loader can undo it", async () => {
  const before = await a.evaluate(`window.view.state.doc.toString()`);
  await a.evaluate(`document.getElementById("example-tour").click()`);
  await b.until(`window.view.state.doc.toString().includes("A tour of the notation")`, "the Tour on B", 15_000);
  await a.evaluate(`document.getElementById("undo").click()`);
  await b.until(`window.view.state.doc.toString() === ${JSON.stringify(before)}`, "the undo on B", 15_000);
  await a.evaluate(`document.getElementById("example-basic").click()`);
  await b.until(`window.view.state.doc.toString().startsWith("rain\\ntitle Basic Example")`, "the Basic example on B", 15_000);
});

test("leaving says goodbye: the other drops them at once", async () => {
  await a.open("about:blank");
  assert.ok(await b.until(`(${people}).length === 1 && !document.querySelector("#source .cm-remote-caret")`, "A to go from B's page", 5_000));
});

test("a browser that just vanishes is dropped once it falls silent", async () => {
  await a.open(`${site.url}${sync}#${await b.evaluate("window.handle.url")}`);
  await b.until(`(${people}).length === 2`, "A back on B's list", 20_000);
  await one.close();
  one = null;
  assert.ok(await b.until(`(${people}).length === 1`, "A to go from B's list", 25_000));
});
