import init, { diagnostics, renderSvg, renderText, toSiteJson } from "./pkg/rain_diagram_wasm.js";
import "./rain-diagram.js";
import { splitter } from "./split.js";

const EXAMPLE = `rain
title Revocation is retroactive

(Doc)-[admin]->(Alice)           : The Doc grants Alice Admin
(Alice)-[edit(Doc)]->(Bob)       : Alice passes Edit to Bob
>>> A (Bob): Bob does stuff
(Bob)=[[edit]]=>(Doc)            : Bob edits the Doc
bc: (Bob)-[edit]->(Carol)        : Bob gives Carol Edit
(Carol)=[[edit]]=>(Doc)          : Carol edits the Doc
<<< A
(Carol)-[read]->(Dan)            : Carol shares Read with Dan
(Dan)=[[read]]=>(Doc) @30%       : Dan reads the Doc
(Alice)-<revoke>-x[bc]           : Alice revokes Bob→Carol, and all that relied on it
`;

const STORAGE = { format: "rain-preview:format", source: "rain-preview:source" };

const FORMATS = {
  svg: { file: "diagram.svg", type: "image/svg+xml", render: renderSvg },
  json: { file: "diagram.json", type: "application/json", render: (s) => JSON.stringify(JSON.parse(toSiteJson(s)), null, 2) },
  text: { file: "diagram.txt", type: "text/plain", render: renderText },
};

const $ = (id) => document.getElementById(id);
const source = $("source");
const status = $("status");
const warnings = $("warnings");
const output = $("output");
const picture = $("picture");

// Storage is a per-viewer convenience; the preview works without it
const load = (key, fallback) => {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
};

const save = (key, value) => {
  try {
    localStorage.setItem(key, value);
  } catch {}
};

let format = FORMATS[load(STORAGE.format, "text")] ? load(STORAGE.format, "text") : "text";

const render = () => {
  save(STORAGE.source, source.value);

  try {
    const site = JSON.parse(toSiteJson(source.value));
    const warned = diagnostics(source.value);
    picture.source = source.value;
    syncNotation();
    output.textContent = FORMATS[format].render(source.value);
    status.className = "ok";
    status.textContent = `✓ ${site.agents.length} principals, ${site.events.length} events`;
    warnings.replaceChildren(...warned.map((w) => Object.assign(document.createElement("li"), { textContent: w })));
  } catch (e) {
    status.className = "err";
    status.textContent = `✗ ${e.message ?? e}`;
  }
};

const select = (next) => {
  format = next;
  save(STORAGE.format, next);
  for (const tab of document.querySelectorAll("[data-format]")) {
    tab.setAttribute("aria-selected", String(tab.dataset.format === next));
  }
  render();
};

for (const tab of document.querySelectorAll("[data-format]")) {
  tab.addEventListener("click", () => select(tab.dataset.format));
}

// The notation the header names, unless the other tab was clicked. The
// override lasts until the header changes.
const header = () =>
  source.value
    .split("\n")
    .map((line) => line.split("%%")[0].trim())
    .find(Boolean) === "certflow"
    ? "certflow"
    : "rain";

let override = null; // { notation, header }

const syncNotation = () => {
  const written = header();
  const shown = override?.header === written ? override.notation : written;
  picture.setAttribute("notation", shown);
  for (const tab of document.querySelectorAll("[data-notation]")) {
    tab.setAttribute("aria-selected", String(tab.dataset.notation === shown));
  }
};

for (const tab of document.querySelectorAll("[data-notation]")) {
  tab.addEventListener("click", () => {
    const written = header();
    override = tab.dataset.notation === written ? null : { notation: tab.dataset.notation, header: written };
    syncNotation();
  });
}

const main = document.querySelector("main");
const pane = $("output-pane");

splitter($("columns"), {
  container: main,
  property: "--source-width",
  axis: "x",
  origin: () => main.getBoundingClientRect().left + 20 + 8,
  min: 200,
  max: () => main.clientWidth - 40 - 16 - 240,
  key: "rain-preview:source-width",
});

splitter($("rows"), {
  container: pane,
  property: "--picture-height",
  axis: "y",
  origin: () => picture.getBoundingClientRect().top + 6,
  min: 60,
  max: () => pane.getBoundingClientRect().bottom - picture.getBoundingClientRect().top - 120,
  key: "rain-preview:picture-height",
});

// Dragging a row in the diagram rewrites the source
picture.addEventListener("rain-edit", ({ detail }) => {
  source.value = detail.source;
  render();
});

picture.addEventListener("rain-edit-refused", ({ detail }) => {
  status.className = "err";
  status.textContent = `✗ ${detail.message}`;
});

// System, light, or dark. The page and the diagram both follow it.
const theme = (choice) => {
  save("rain-preview:theme", choice);
  if (choice === "light" || choice === "dark") {
    document.documentElement.dataset.theme = choice;
    picture.setAttribute("theme", choice);
  } else {
    delete document.documentElement.dataset.theme;
    picture.removeAttribute("theme");
  }
  for (const button of document.querySelectorAll("[data-theme-choice]")) {
    button.setAttribute("aria-pressed", String(button.dataset.themeChoice === choice));
  }
};

for (const button of document.querySelectorAll("[data-theme-choice]")) {
  button.addEventListener("click", () => theme(button.dataset.themeChoice));
}
theme(load("rain-preview:theme", "system"));

$("example").addEventListener("click", () => {
  source.value = EXAMPLE;
  render();
});

$("copy").addEventListener("click", async () => {
  await navigator.clipboard.writeText(output.textContent);
  $("copy").textContent = "Copied";
  setTimeout(() => ($("copy").textContent = "Copy"), 1200);
});

$("download").addEventListener("click", () => {
  const { file, type } = FORMATS[format];
  const url = URL.createObjectURL(new Blob([output.textContent], { type }));
  Object.assign(document.createElement("a"), { href: url, download: file }).click();
  URL.revokeObjectURL(url);
});

// Sent by the dev server (serve.mjs) when the Wasm rebuild fails; the page
// keeps running the last good build
window.addEventListener("rain:build-error", ({ detail }) => {
  status.className = "err";
  status.textContent = "✗ Wasm build failed; still running the last good build";
  output.textContent = `${detail}\n\n${output.textContent}`;
});

await init();
source.value = load(STORAGE.source, EXAMPLE);
source.addEventListener("input", render);
select(format);
