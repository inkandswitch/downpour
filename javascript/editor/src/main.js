// Spike: one Automerge document holding the diagram's source text, synced
// over Subduction, edited as text (CodeMirror) and as a picture
// (<rain-diagram editable>). Open the same URL in two tabs to see it sync.

// Importing the full packages initializes their Wasm; the repo and the
// Subduction backend import the `slim` builds, which share it
import * as A from "@automerge/automerge";
import { createWebCryptoSigner } from "@automerge/subduction";
import { Repo, isValidAutomergeUrl } from "@automerge/automerge-repo";
import { SubductionBackend } from "@automerge/automerge-repo-subduction";
import { WebSocketSubductionNetworkAdapter } from "@automerge/automerge-repo-subduction-network-websocket";
import { IndexedDBSubductionStorageAdapter } from "@automerge/automerge-repo-subduction-storage-indexeddb";
import { automergeSyncPlugin } from "@automerge/automerge-codemirror";
import { EditorView, basicSetup } from "codemirror";
import "@inkandswitch/rain-diagram";

const SYNC_SERVER = new URLSearchParams(location.search).get("sync") ?? "wss://subduction.sync.inkandswitch.com";
const STARTER = `rain
title A shared diagram

(Doc)-[edit(Doc)]->(Alice)     : The Doc grants Alice Edit
(Alice)-[edit(Doc)]->(Bob)     : Alice passes Edit to Bob
(Bob)=[[edit]]=>(Doc)          : Bob edits the Doc
`;

const status = document.getElementById("status");
const picture = document.getElementById("picture");

const server = new WebSocketSubductionNetworkAdapter(SYNC_SERVER);
server.subscribe?.((state) => {
  status.textContent = `${state.status ?? state}${state.error ? `: ${state.error}` : ""}`;
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
window.handle = handle; // for poking at in the console
document.getElementById("share").href = location.href;
document.getElementById("share").dataset.url = handle.url;

// Text: CodeMirror, kept in step with the document by automerge-codemirror
window.view = new EditorView({
  doc: handle.doc().source,
  extensions: [basicSetup, automergeSyncPlugin({ handle, path: ["source"] })],
  parent: document.getElementById("source"),
});

// Picture: redraw on every change, local or remote
const redraw = () => {
  const source = handle.doc()?.source ?? "";
  if (picture.source !== source) picture.source = source;
};
handle.on("change", redraw);
redraw();

// Edits made on the picture come back as a whole new source; updateText
// turns that into the minimal splices, so concurrent typing elsewhere survives
picture.addEventListener("rain-edit", (e) => {
  handle.change((doc) => A.updateText(doc, ["source"], e.detail.source));
});
