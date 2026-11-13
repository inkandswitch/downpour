// The collaborative editor: the preview's page (../../preview/shell.js), with
// the source in an Automerge document synced over Subduction, edited as text
// in CodeMirror and as a picture in <rain-diagram editable>. Open the same
// URL in another browser to edit together.

// Importing the full packages initializes their Wasm; the repo and the
// Subduction backend import the `slim` builds, which share it
import * as A from "@automerge/automerge";
import { createWebCryptoSigner } from "@automerge/subduction";
import { Repo, isValidAutomergeUrl } from "@automerge/automerge-repo";
import { SubductionBackend } from "@automerge/automerge-repo-subduction";
import { WebSocketSubductionNetworkAdapter } from "@automerge/automerge-repo-subduction-network-websocket";
import { IndexedDBSubductionStorageAdapter } from "@automerge/automerge-repo-subduction-storage-indexeddb";
import { automergeSyncPlugin } from "@automerge/automerge-codemirror";
import { redo, redoDepth, undo, undoDepth } from "@codemirror/commands";
import { EditorState, Transaction } from "@codemirror/state";
import { EditorView, basicSetup } from "codemirror";
import init, { diagnostics, renderSvg, renderText, toSiteJson } from "@inkandswitch/rain-diagram/wasm";
import "@inkandswitch/rain-diagram";
import "../../preview/style.css";
import { shell } from "../../preview/shell.js";

const SYNC_SERVER = new URLSearchParams(location.search).get("sync") ?? "wss://subduction.sync.inkandswitch.com";
const STARTER = `rain
title A shared diagram

(Doc)-[edit(Doc)]->(Alice)     : The Doc grants Alice Edit
(Alice)-[edit(Doc)]->(Bob)     : Alice passes Edit to Bob
(Bob)=[[edit]]=>(Doc)          : Bob edits the Doc
`;

const $ = (id) => document.getElementById(id);
const picture = $("picture");
const connection = $("connection");

// Sync: a Subduction backend with local storage and the sync server
const server = new WebSocketSubductionNetworkAdapter(SYNC_SERVER);
server.subscribe((state) => {
  const name = state.status ?? String(state);
  connection.className = name;
  connection.textContent = state.error ? `${name}: ${state.error}` : name;
});
const backend = new SubductionBackend({
  storage: new IndexedDBSubductionStorageAdapter(),
  network: [server],
  signer: await createWebCryptoSigner(),
});
const repo = new Repo({ backend });

// The document named in the URL's hash, or a new one put there
const hash = location.hash.slice(1);
const handle = isValidAutomergeUrl(hash) ? await repo.find(hash) : await repo.create({ source: STARTER });
if (!isValidAutomergeUrl(hash)) location.hash = handle.url;
const share = $("share");
share.href = share.textContent = location.href;
window.handle = handle; // for poking at in the console

await init();
const text = () => handle.doc()?.source ?? "";
const page = shell({ prefix: "rain-editor", text, wasm: { diagnostics, renderSvg, renderText, toSiteJson } });

// Text: CodeMirror, kept in step with the document by automerge-codemirror,
// in the page's colours (so it follows the theme)
const theme = EditorView.theme({
  "&": { color: "var(--fg)", backgroundColor: "transparent", height: "100%" },
  ".cm-content": { font: "13px/1.6 var(--mono)", caretColor: "var(--fg)" },
  ".cm-cursor": { borderLeftColor: "var(--fg)" },
  ".cm-gutters": { backgroundColor: "var(--panel)", color: "var(--muted)", borderRight: "1px solid var(--border)" },
  ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "rgb(127 127 127 / 0.08)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": { backgroundColor: "rgb(61 125 216 / 0.25)" },
});

// Undo is per person: only what this person typed goes into the history.
// Remote changes (and picture edits, which arrive the same way) carry no
// user event, so they stay out of it, and undo never reverts someone else
const ownEditsOnly = EditorState.transactionExtender.of((tr) =>
  tr.docChanged && !tr.annotation(Transaction.userEvent) ? { annotations: Transaction.addToHistory.of(false) } : null,
);

const syncHistory = (state) => {
  $("undo").disabled = undoDepth(state) === 0;
  $("redo").disabled = redoDepth(state) === 0;
};
const view = new EditorView({
  doc: text(),
  extensions: [
    basicSetup,
    theme,
    ownEditsOnly,
    automergeSyncPlugin({ handle, path: ["source"] }),
    EditorView.updateListener.of((update) => syncHistory(update.state)),
  ],
  parent: $("source"),
});
window.view = view;
$("undo").addEventListener("click", () => undo(view));
$("redo").addEventListener("click", () => redo(view));

// Picture: redrawn on every change, local or remote. Text that doesn't parse
// (someone mid-edit) keeps the last good diagram up, with the error above it
handle.on("change", () => page.render());

// Edits made on the picture come back as a whole new source; updateText
// turns that into the minimal splices, so concurrent typing elsewhere survives
picture.addEventListener("rain-edit", (e) => {
  handle.change((doc) => A.updateText(doc, ["source"], e.detail.source));
});

page.start();
