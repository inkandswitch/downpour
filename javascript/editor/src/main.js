// The collaborative editor's own page: the editor (mount.js) on a document
// synced over Subduction, named in the URL, with who's here, the connection,
// and whether everything has reached the sync server. Open the same URL in
// another browser to edit together.

// Importing the full packages initializes their Wasm; the repo and the
// Subduction backend import the `slim` builds, which share it
import "@automerge/automerge";
import { createMemorySigner, createWebCryptoSigner } from "@automerge/subduction";
import { Repo, isValidAutomergeUrl } from "@automerge/automerge-repo";
import { SubductionBackend } from "@automerge/automerge-repo-subduction";
import { DummySubductionStorageAdapter } from "@automerge/automerge-repo-subduction/helpers/DummySubductionStorageAdapter.js";
import { WebSocketSubductionNetworkAdapter } from "@automerge/automerge-repo-subduction-network-websocket";
import { IndexedDBSubductionStorageAdapter } from "@automerge/automerge-repo-subduction-storage-indexeddb";
import "../../preview/style.css";
import "./editor.css";
import { STARTER } from "../../preview/examples.js";
import { mountEditor } from "./mount.js";

const SYNC_SERVER = new URLSearchParams(location.search).get("sync") ?? "wss://subduction.sync.inkandswitch.com";

const $ = (id) => document.getElementById(id);
const connection = $("connection");

// One tab per browser owns local storage: the IndexedDB adapter allows one
// live backend per database, across tabs too. That tab keeps documents for
// offline use, under the browser's persistent identity (the WebCrypto key
// is kept in IndexedDB). Other tabs sync in memory, each as its own peer, so
// the server never sees one identity connected twice.
const ownsStorage = await new Promise((resolve) => {
  if (!navigator.locks) return resolve(true);
  navigator.locks.request("downpour-editor:storage", { ifAvailable: true }, (lock) => {
    resolve(lock !== null);
    // Held until the tab closes
    return lock ? new Promise(() => {}) : undefined;
  });
});

// Sync: a Subduction backend with storage and the sync server
const server = new WebSocketSubductionNetworkAdapter(SYNC_SERVER);
server.subscribe((state) => {
  const name = state.status ?? String(state);
  connection.className = name;
  connection.textContent = state.error ? `${name}: ${state.error}` : name;
});
const signer = ownsStorage ? await createWebCryptoSigner() : createMemorySigner();
const backend = new SubductionBackend({
  storage: ownsStorage ? new IndexedDBSubductionStorageAdapter() : new DummySubductionStorageAdapter(),
  network: [server],
  signer,
});
const repo = new Repo({ backend });

// The document named in the URL's hash, or a new one put there
const hash = location.hash.slice(1);
// A document nobody has sent yet (just made elsewhere, or the server is
// unreachable) is unavailable for now, not forever: keep looking, and say so
const find = async (url) => {
  for (let wait = 500; ; wait = Math.min(wait * 2, 10_000)) {
    try {
      return await repo.find(url);
    } catch (e) {
      $("sync").className = "offline";
      $("sync").textContent = "… looking for this document";
      $("sync").title = `${e.message ?? e}; trying again`;
      await new Promise((resolve) => setTimeout(resolve, wait));
    }
  }
};
const handle = isValidAutomergeUrl(hash) ? await find(hash) : await repo.create({ source: STARTER });
if (!isValidAutomergeUrl(hash)) location.hash = handle.url;
const share = $("share");
share.href = share.textContent = location.href;
window.handle = handle; // for poking at in the console
connection.title = ownsStorage
  ? "This tab keeps a copy for offline use"
  : "Another tab keeps the offline copy; this one syncs live only";
window.ownsStorage = ownsStorage;
window.peerId = signer.peerId(); // this tab's identity on the network

// Whether the sync server has everything this tab has: its latest heads
// (sent as it takes changes in) match this copy's
const syncState = $("sync");
let serverHeads = null;
const showSync = () => {
  const heads = handle.heads();
  const synced = serverHeads?.length === heads.length && heads.every((h) => serverHeads.includes(h));
  const [state, text, title] = synced
    ? ["synced", "✓ synced", "Every change here has reached the sync server"]
    : ["connected", "connecting"].includes(connection.className)
      ? ["syncing", "↻ syncing…", "Changes are on their way to or from the sync server"]
      : ownsStorage
        ? ["offline", "● not synced", "Changes are kept in this browser, and sync when the server is back"]
        : ["offline", "● not synced", "Changes are kept in this tab only (another tab keeps the offline copy) until the server is back"];
  syncState.className = state;
  syncState.textContent = text;
  syncState.title = title;
};
handle.on("remote-heads", ({ heads }) => {
  serverHeads = heads;
  showSync();
});
handle.on("change", showSync);
server.subscribe(showSync);
showSync();

const { view } = await mountEditor($("app"), handle, { people: $("people") });
window.view = view; // for poking at in the console
