import init, { diagnostics, renderSvg, renderText, toSiteJson } from "../rain-diagram/pkg/rain_diagram_wasm.js";
import "../rain-diagram/rain-diagram.js";
import { highlight } from "./highlight.js";
import { History } from "./history.js";
import { shell } from "./shell.js";

const BASIC = `rain
title Basic Example

(Doc)-[admin(Doc)]->(Alice)    : The Doc grants Alice Admin
(Alice)-[edit(Doc)]->(Bob)     : Alice passes Edit to Bob
(Bob)=[[edit]]=>(Doc)          : Bob edits the Doc
>>> C (Carol): Carol joins in
bc: (Bob)-[edit(Doc)]->(Carol) : Bob gives Carol Edit
(Carol)=[[edit]]=>(Doc)        : Carol edits the Doc
<<< C
(Alice)-<revoke>-x[bc]         : Alice revokes Bob→Carol
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
(Doc)-[admin(Doc)]->(Alice) : The Doc grants Alice Admin
(Alice)-[edit(Doc)]->(Bob)  : Alice passes Edit to Bob

// Regions group rows: \`>>> name (Principal): caption\` … \`<<< name\`. The
// (Principal) is optional and gives the region that principal's colour.
// In the preview, double-click a caption to edit it; drag an edge to resize.
>>> A (Bob): Bob does stuff
// An invocation: someone using their authority, (actor)=[[level]]=>(subject).
(Bob)=[[edit]]=>(Doc)          : Bob edits the Doc
// \`label:\` names a certificate so revocations and \`via\` can refer to it.
// Only labelled certificates can be referred to; unlabelled ones need none.
bc: (Bob)-[edit(Doc)]->(Carol) : Bob gives Carol Edit
(Carol)=[[edit]]=>(Doc)        : Carol edits the Doc
<<< A

(Carol)-[read(Doc)]->(Dan) : Carol shares Read with Dan
// More dashes on one side slide a row's label along its arrow: here one
// before and two after put it a third of the way. One each is automatic.
// In the preview, drag a label sideways to set it.
(Dan)=[[read]]==>(Doc)     : Dan reads the Doc

// Groups: authority flows through membership. Eve reaches the Doc through
// Team; the small circle on Team→Eve is the transitive handle.
(Doc)-[read(Doc)]->(Team)                 : The Doc lets Team read it
team-eve: (Team)-[admin(Team)]->(Eve)     : Eve joins Team
// \`via\` names the route explicitly (it's normally worked out for you).
(Eve)-[relay(Doc) via team-eve]->(Server) : Eve lets a sync server relay it

// Command paths, as in UCAN, work wherever a level does: \`/crud\` covers
// \`/crud/read\`. \`where\` adds a caveat, opaque text that everything relying
// on the certificate is bound by, so caveats pile up down a chain. Hover a
// label in the picture to see every caveat it's bound by.
(Doc)-[/crud(Doc) where path = "/docs"]->(Dan)     : The Doc grants Dan CRUD under /docs
(Dan)-[/crud/read(Doc) where size < 1MB]->(Server) : Dan narrows it to reads, adds a limit
(Server)=[[/crud/read]]=>(Doc)                     : The Server reads, bound by both caveats

// A revocation: (revoker)-<revoke>-x[label]. It's retroactive: everything
// that relied on Bob→Carol goes void, even rows above this one. Drag the
// dotted "now" line (or click its clock) to see any moment as certflow.
(Alice)-<revoke>-x[bc]           : Alice revokes Bob→Carol, and all that relied on it
`;

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
