// The Patchwork tool, in a stand-in host (javascript/patchwork/harness): two
// mounts of one document, each in its own shadow root; Patchwork's theme
// followed; unmounting cleans up. No network.

import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { after, before, test } from "node:test";
import { browser, serve } from "./harness.mjs";

const dist = fileURLToPath(new URL("../../patchwork/harness/dist/", import.meta.url));
let site, chrome, page;
const shadow = (n) => `document.querySelectorAll(".downpour-editor")[${n}]?.shadowRoot`;
const status = (n) => `${shadow(n)}?.querySelector("#status")?.textContent`;

before(async () => {
  site = await serve(dist);
  chrome = await browser("patchwork");
  page = await chrome.tab();
  await page.send("Emulation.setDeviceMetricsOverride", { width: 1300, height: 900, deviceScaleFactor: 1, mobile: false });
  await page.open(site.url);
  await page.until(`${status(0)}?.startsWith("✓") && ${status(1)}?.startsWith("✓")`, "both mounts to draw");
});

after(async () => {
  await chrome?.close();
  site?.close();
});

test("the datatype seeds the document, and titles it from its source", async () => {
  assert.equal(await page.evaluate(`window.datatype.getTitle(window.handle.doc())`), "A shared diagram");
});

test("the editor keeps to its shadow roots", async () => {
  assert.ok(await page.evaluate(`!document.getElementById("source") && !document.getElementById("picture")`));
});

test("typing in one mount reaches the other, highlighted", async () => {
  await page.evaluate(`${shadow(0)}.querySelector(".cm-content").focus()`);
  for (const type of ["keyDown", "keyUp"]) {
    await page.send("Input.dispatchKeyEvent", { type, key: "End", code: "End", windowsVirtualKeyCode: 35, modifiers: 2 });
  }
  await page.send("Input.insertText", { text: "(Bob)-[read(Doc)]->(Carol)    : Bob lets Carol read\n" });
  await page.until(`${shadow(1)}.querySelector(".cm-content").textContent.includes("Bob lets Carol read") && ${status(1)}.includes("4 events")`, "the other mount");
  assert.ok(await page.evaluate(`window.handle.doc().source.includes("Bob lets Carol read")`));
  assert.ok((await page.evaluate(`${shadow(0)}.querySelectorAll(".tok-principal").length`)) > 0);
});

test("Patchwork's theme: its colours, and dark or light to match", async () => {
  const theme = `[document.querySelectorAll(".downpour-editor")[0].dataset.theme, ${shadow(0)}.querySelector("#picture").getAttribute("theme"), getComputedStyle(${shadow(0)}.querySelector("section")).backgroundColor, getComputedStyle(${shadow(0)}.querySelector("#output")).color]`;
  assert.deepEqual((await page.until(`(${theme})[0] && ${theme}`, "a theme")).slice(0, 2), ["light", "light"]);
  await page.evaluate(`document.documentElement.style.cssText = "--editor-fill: #101418; --editor-line: #e8e8e8"; document.documentElement.setAttribute("theme", "gloom")`);
  assert.deepEqual(await page.until(`(${theme})[0] === "dark" && ${theme}`, "the dark theme"), ["dark", "dark", "rgb(16, 20, 24)", "rgb(232, 232, 232)"]);
});

test("unmounting empties its element; the other mount carries on", async () => {
  await page.evaluate(`window.cleanups[0]()`);
  assert.ok(await page.evaluate(`document.getElementById("one").childElementCount === 0`));
  await page.evaluate(`window.handle.change((d) => { d.source = d.source.replace("Bob lets Carol read", "Bob lets Carol look") })`);
  await page.until(`${shadow(0)}?.querySelector(".cm-content").textContent.includes("Carol look")`, "the remaining mount to update");
});

test("no page errors", () => {
  assert.deepEqual(page.errors, []);
});
