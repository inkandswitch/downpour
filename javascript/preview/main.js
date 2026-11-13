import init, { diagnostics, renderSvg, renderText, toSiteJson } from "../rain-diagram/pkg/rain_diagram_wasm.js";
import "../rain-diagram/rain-diagram.js";
import { PALETTE, commentStart, highlight } from "./highlight.js";
import { History } from "./history.js";
import { splitter } from "./split.js";

const BASIC = `rain
title Basic Example

(Doc)-[admin]->(Alice)           : The Doc grants Alice Admin
(Alice)-[edit(Doc)]->(Bob)       : Alice passes Edit to Bob
(Bob)=[[edit]]=>(Doc)            : Bob edits the Doc
>>> C (Carol): Carol joins in
bc: (Bob)-[edit]->(Carol)        : Bob gives Carol Edit
(Carol)=[[edit]]=>(Doc)          : Carol edits the Doc
<<< C
(Alice)-<revoke>-x[bc]           : Alice revokes Bob→Carol
`;

const TOUR = `rain
// A tour of the notation. Lines (or line ends) starting \`//\` are comments;
// \`%%\` works too. In the preview you can also drag rows up and down, slide
// labels sideways, hover a principal to see its reach, and click a
// certificate to see what revoking it would void.
//
// The first line picks the default view: \`rain\` (time runs down the page)
// or \`certflow\` (a snapshot). The Rain / Certflow tabs switch either way.
title A Tour of Features

// Optional: the order of the lifelines, left to right. Without it,
// principals appear in the order they're first mentioned.
principals: Doc, Alice, Bob, Carol, Dan, Team, Eve, Server

// A delegation: (issuer)-[level(subject)]->(audience), then an optional
// \`: note\` shown beside the row. Levels, most to least: admin 👑, edit ✏️,
// read 👀, relay 📡. With no (subject), admin means "over myself" and any
// other level passes on what the issuer already holds.
(Doc)-[admin]->(Alice)           : The Doc grants Alice Admin
(Alice)-[edit(Doc)]->(Bob)       : Alice passes Edit to Bob

// Regions group rows: \`>>> name (Principal): caption\` … \`<<< name\`. The
// (Principal) is optional and gives the region that principal's colour.
// In the preview, double-click a caption to edit it; drag an edge to resize.
>>> A (Bob): Bob does stuff
// An invocation: someone using their authority, (actor)=[[level]]=>(subject).
(Bob)=[[edit]]=>(Doc)            : Bob edits the Doc
// \`label:\` names a certificate so revocations and \`via\` can refer to it.
// Only labelled certificates can be referred to; unlabelled ones need none.
bc: (Bob)-[edit]->(Carol)        : Bob gives Carol Edit
(Carol)=[[edit]]=>(Doc)          : Carol edits the Doc
<<< A

(Carol)-[read]->(Dan)            : Carol shares Read with Dan
// More dashes on one side slide a row's label along its arrow: here one
// before and two after put it a third of the way. One each is automatic.
// In the preview, drag a label sideways to set it.
(Dan)=[[read]]==>(Doc)           : Dan reads the Doc

// Groups: authority flows through membership. Eve reaches the Doc through
// Team; the small circle on Team→Eve is the transitive handle.
(Doc)-[read]->(Team)             : The Doc lets Team read it
team-eve: (Team)-[admin]->(Eve)  : Eve joins Team
// \`via\` names the route explicitly (it's normally worked out for you).
(Eve)-[relay(Doc) via team-eve]->(Server) : Eve lets a sync server relay it

// Command paths, as in UCAN, work wherever a level does: \`/crud\` covers
// \`/crud/read\`. \`where\` adds a caveat, opaque text that everything relying
// on the certificate is bound by, so caveats pile up down a chain. Hover a
// label in the picture to see every caveat it's bound by.
(Doc)-[/crud(Doc) where path = "/docs"]->(Dan)     : The Doc grants Dan CRUD under /docs
(Dan)-[/crud/read(Doc) where size < 1MB]->(Server) : Dan narrows it to reads, adds a limit
(Server)=[[/crud/read]]=>(Doc)                : The Server reads, bound by both caveats

// A revocation: (revoker)-<revoke>-x[label]. It's retroactive: everything
// that relied on Bob→Carol goes void, even rows above this one. Drag the
// dotted "now" line (or click its clock) to see any moment as certflow.
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
const highlightLayer = $("highlight");
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

  let errorLine = null;
  try {
    const site = JSON.parse(toSiteJson(source.value));
    // Principals in their diagram colours (kept from the last good parse
    // while the text is broken, so colours don't flicker as you type)
    colours = new Map(site.agents.map((agent, i) => [agent.label, PALETTE[i % PALETTE.length]]));
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
    errorLine = Number(String(e.message ?? e).match(/^(\d+):/)?.[1]) || null;
  }
  paintSource(errorLine);
};

// Syntax colouring under the (transparent) textarea, scrolled with it
let colours = new Map();
let lastErrorLine = null;
const paintSource = (errorLine = lastErrorLine) => {
  lastErrorLine = errorLine;
  highlightLayer.innerHTML = highlight(source.value, colours, errorLine);
  syncScroll();
};
const syncScroll = () => {
  highlightLayer.scrollTop = source.scrollTop;
  highlightLayer.scrollLeft = source.scrollLeft;
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
    .map((line) => {
      const comment = commentStart(line);
      return (comment === -1 ? line : line.slice(0, comment)).trim();
    })
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

// Zoom, remembered separately for rain and certflow
const ZOOMS = [0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4];
const zoomKey = () => `rain-preview:zoom:${picture.getAttribute("notation") ?? "rain"}`;
const setZoom = (zoom) => {
  const z = Math.min(Math.max(zoom, ZOOMS[0]), ZOOMS.at(-1));
  picture.setAttribute("zoom", String(z));
  $("zoom-level").textContent = `${Math.round(z * 100)}%`;
  save(zoomKey(), String(z));
};
const currentZoom = () => Number(picture.getAttribute("zoom")) || 1;
const step = (direction) => {
  const z = currentZoom();
  const next = direction > 0 ? ZOOMS.find((s) => s > z + 0.001) : ZOOMS.findLast((s) => s < z - 0.001);
  setZoom(next ?? z);
};
const fit = () => {
  const size = picture.naturalSize;
  if (!size) return;
  const room = { width: picture.clientWidth - 24, height: picture.clientHeight - 24 };
  setZoom(Math.min(room.width / size.width, room.height / size.height));
};
$("zoom-in").addEventListener("click", () => step(1));
$("zoom-out").addEventListener("click", () => step(-1));
$("zoom-level").addEventListener("click", () => setZoom(1));
$("zoom-fit").addEventListener("click", fit);
picture.addEventListener(
  "wheel",
  (e) => {
    if (!e.ctrlKey && !e.metaKey) return;
    e.preventDefault();
    setZoom(currentZoom() * Math.exp(-e.deltaY * 0.002));
  },
  { passive: false },
);
// Each notation keeps its own zoom
new MutationObserver(() => setZoom(Number(load(zoomKey(), "1")) || 1)).observe(picture, {
  attributes: true,
  attributeFilter: ["notation"],
});

// Maximize one pane (source, output, diagram, or export); again to restore
const maximize = (pane) => {
  const main = document.querySelector("main");
  const next = main.dataset.max === pane ? null : pane;
  if (next) main.dataset.max = next;
  else delete main.dataset.max;
  save("rain-preview:maximized", next ?? "");
  for (const button of document.querySelectorAll(".maximize")) {
    const on = button.dataset.pane === next;
    button.textContent = on ? "⤡" : "⤢";
    button.title = on ? "Restore" : "Maximize";
    button.setAttribute("aria-pressed", String(on));
  }
};
for (const button of document.querySelectorAll(".maximize")) {
  button.addEventListener("click", () => maximize(button.dataset.pane));
}
window.addEventListener("keydown", (e) => {
  const main = document.querySelector("main");
  if (e.key === "Escape" && main.dataset.max && !e.defaultPrevented) maximize(main.dataset.max);
});
const remembered = load("rain-preview:maximized", "");
if (remembered) maximize(remembered);

for (const [id, text] of [
  ["example-basic", BASIC],
  ["example-tour", TOUR],
]) {
  $(id).addEventListener("click", () => {
    edit(text);
    source.scrollTop = 0;
  });
}

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
source.value = recorded = load(STORAGE.source, BASIC);
source.addEventListener("input", render);
source.addEventListener("scroll", syncScroll);
select(format);
