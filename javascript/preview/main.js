import init, { diagnostics, renderSvg, renderText, toSiteJson } from "../rain-diagram/pkg/rain_diagram_wasm.js";
import "../rain-diagram/rain-diagram.js";
import { BASIC, TOUR } from "./examples.js";
import { highlight } from "./highlight.js";
import { History } from "./history.js";
import { shell } from "./shell.js";

const $ = (id) => document.getElementById(id);
const source = $("source");
const highlightLayer = $("highlight");
const picture = $("picture");

const page = shell({
  prefix: "rain-preview",
  text: () => source.value,
  wasm: { diagnostics, renderSvg, renderText, toSiteJson },
});

const render = () => {
  page.save("source", source.value);
  const { errorLine, colours } = page.render();
  paintSource(errorLine, colours);
};

// Syntax colouring under the (transparent) textarea, scrolled with it
let lastErrorLine = null;
let lastColours = new Map();
const paintSource = (errorLine = lastErrorLine, colours = lastColours) => {
  lastErrorLine = errorLine;
  lastColours = colours;
  highlightLayer.innerHTML = highlight(source.value, colours, errorLine);
  syncScroll();
};
const syncScroll = () => {
  highlightLayer.scrollTop = source.scrollTop;
  highlightLayer.scrollLeft = source.scrollLeft;
};

// One history for typing, picture edits, and loading examples. It owns
// undo everywhere (the textarea's own would lose track of picture edits).
const history = new History();
let recorded = ""; // the source as of the last recorded edit

const syncHistory = () => {
  $("undo").disabled = !history.canUndo;
  $("redo").disabled = !history.canRedo;
};

/** Replace the source as one undoable step. */
const edit = (text) => {
  history.record(recorded, text);
  recorded = text;
  source.value = text;
  syncHistory();
  render();
};

const travel = (step) => {
  const to = step(source.value);
  if (!to) return;
  recorded = source.value = to.text;
  source.setSelectionRange(to.caret, to.caret);
  syncHistory();
  render();
};
const undo = () => travel((text) => history.undo(text));
const redo = () => travel((text) => history.redo(text));

const mac = /Mac|iP(hone|ad)/.test(navigator.platform);
$("undo").title = `Undo (${mac ? "⌘Z" : "Ctrl+Z"})`;
$("redo").title = `Redo (${mac ? "⇧⌘Z" : "Ctrl+Y"})`;
$("undo").addEventListener("click", undo);
$("redo").addEventListener("click", redo);

window.addEventListener("keydown", (e) => {
  if (!(mac ? e.metaKey : e.ctrlKey) || e.altKey) return;
  // Leave other text fields (say, a region caption being edited) alone
  const target = e.composedPath()[0];
  if (target !== source && target?.matches?.("input, textarea, [contenteditable]")) return;
  const key = e.key.toLowerCase();
  if (key === "z" || key === "y") {
    e.preventDefault();
    key === "z" && !e.shiftKey ? undo() : redo();
  }
});

// Undo from the browser's Edit menu reaches the textarea this way
source.addEventListener("beforeinput", (e) => {
  if (e.inputType === "historyUndo" || e.inputType === "historyRedo") {
    e.preventDefault();
    e.inputType === "historyUndo" ? undo() : redo();
  }
});

source.addEventListener("input", () => {
  history.record(recorded, source.value, { typing: true });
  recorded = source.value;
  syncHistory();
});

// Editing the diagram rewrites the source
picture.addEventListener("rain-edit", ({ detail }) => edit(detail.source));

for (const [id, text] of [
  ["example-basic", BASIC],
  ["example-tour", TOUR],
]) {
  $(id).addEventListener("click", () => {
    edit(text);
    source.scrollTop = 0;
  });
}

// Sent by the dev server (serve.mjs) when the Wasm rebuild fails; the page
// keeps running the last good build
window.addEventListener("rain:build-error", ({ detail }) => {
  $("status").className = "err";
  $("status").textContent = "✗ Wasm build failed; still running the last good build";
  $("output").textContent = `${detail}\n\n${$("output").textContent}`;
});

await init();
source.value = recorded = page.load("source", BASIC);
source.addEventListener("input", render);
source.addEventListener("scroll", syncScroll);
page.start();
