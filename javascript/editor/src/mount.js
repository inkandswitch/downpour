// The collaborative editor on any Automerge document with a `source` string:
// the preview's panes (../../preview/shell.js), the text in CodeMirror and
// the picture in <rain-diagram editable>, both editing the document. The
// standalone page (main.js) and the Patchwork tool (../patchwork/) mount it.

import { automergeSyncPlugin } from "@automerge/automerge-codemirror";
import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, redo, redoDepth, undo, undoDepth } from "@codemirror/commands";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { EditorState, Transaction } from "@codemirror/state";
import {
  EditorView,
  crosshairCursor,
  drawSelection,
  dropCursor,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  rectangularSelection,
} from "@codemirror/view";
import init, { diagnostics, renderSvg, renderText, toSiteJson } from "@inkandswitch/rain-diagram/wasm";
import "@inkandswitch/rain-diagram";
import { BASIC, TOUR } from "../../preview/examples.js";
import { diff } from "../../preview/history.js";
import { shell } from "../../preview/shell.js";
import pageCss from "../../preview/style.css?inline";
import editorCss from "./editor.css?inline";
import markup from "./editor.html?raw";
import { highlighting, paint } from "./highlighting.js";
import { presence, remoteSelections } from "./presence.js";

// CodeMirror's basic setup, less what needs a language (folding,
// indenting, its highlighting: ours is highlighting.js) or has nothing to do
// here (autocomplete, lint): a fifth of the editor's JS
const setup = [
  lineNumbers(),
  highlightActiveLineGutter(),
  highlightSpecialChars(),
  history(),
  drawSelection(),
  dropCursor(),
  EditorState.allowMultipleSelections.of(true),
  closeBrackets(),
  rectangularSelection(),
  crosshairCursor(),
  highlightActiveLine(),
  highlightSelectionMatches(),
  keymap.of([...closeBracketsKeymap, ...defaultKeymap, ...searchKeymap, ...historyKeymap]),
];

// The text in the page's colours, so it follows the theme
const theme = EditorView.theme({
  "&": { color: "var(--fg)", backgroundColor: "transparent", height: "100%" },
  ".cm-content": { font: "13px/1.6 var(--mono)", caretColor: "var(--fg)" },
  ".cm-cursor": { borderLeftColor: "var(--fg)" },
  ".cm-gutters": { backgroundColor: "var(--panel)", color: "var(--muted)", borderRight: "1px solid var(--border)" },
  ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "rgb(127 127 127 / 0.08)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": { backgroundColor: "rgb(61 125 216 / 0.25)" },
});

// Undo is per person: only this person's edits (typed, made on the picture,
// or an example loaded) go into the history. Remote changes carry no user
// event, so they stay out of it, and undo never reverts someone else
const ownEditsOnly = EditorState.transactionExtender.of((tr) =>
  tr.docChanged && !tr.annotation(Transaction.userEvent) ? { annotations: Transaction.addToHistory.of(false) } : null,
);

// The page's styles inside a shadow root: `:root` there is the host, and
// the host carries the theme
const SHADOW_CSS = `${pageCss}\n${editorCss}`
  .replace(/:root:not\(\[([^\]]+)\]\)/g, ":host(:not([$1]))")
  .replace(/:root\[([^\]]+)\]/g, ":host([$1])")
  .replace(/:root/g, ":host")
  .replace(/(^|[}\s,])body(?=\s*\{)/g, "$1:host")
  .concat("\n:host { display: block; height: 100%; }\nmain { height: 100%; padding: 8px; }\n");

/**
 * Mount the editor in `element`, editing `handle`'s `source`.
 *
 * @param {HTMLElement} element
 * @param {import("@automerge/automerge-repo").DocHandle<{ source: string }>} handle
 * @param {object} [options]
 * @param {boolean} [options.shadow]              mount in a shadow root of its own, with its own styles: for
 *                                                someone else's page, whose ids and CSS it then can't touch
 * @param {string} [options.styles]               more CSS for that shadow root (e.g. mapping the host's theme)
 * @param {Document} [options.root]               the document, when not in a shadow root
 * @param {HTMLElement} [options.themeRoot]       where the theme toggle, if any, sets the theme
 * @param {HTMLElement} [options.people]          where to list who's here
 * @param {string} [options.name]                 this person's name, if known
 * @param {string} [options.prefix]               storage key prefix for per-viewer settings
 * @returns {Promise<{ view: EditorView, host: HTMLElement, unmount: () => void }>} `host` is the
 *   shadow root's host (set its `data-theme` to "light" or "dark"), or `element`
 */
export const mountEditor = async (element, handle, options = {}) => {
  const { people, name, prefix = "rain-editor" } = options;
  let { root = document, themeRoot } = options;
  let host = element;
  let app = element;
  if (options.shadow) {
    host = element.appendChild(Object.assign(document.createElement("div"), { className: "downpour-editor" }));
    host.style.height = "100%";
    root = host.attachShadow({ mode: "open" });
    const style = Object.assign(document.createElement("style"), { textContent: SHADOW_CSS + (options.styles ?? "") });
    app = Object.assign(document.createElement("div"), { style: "height: 100%" });
    root.append(style, app);
    themeRoot = host;
  }
  app.innerHTML = markup;
  const $ = (id) => root.querySelector(`#${id}`);
  const picture = $("picture");

  await init();
  const text = () => handle.doc()?.source ?? "";
  const page = shell({ prefix, text, root, themeRoot, wasm: { diagnostics, renderSvg, renderText, toSiteJson } });

  const syncHistory = (state) => {
    $("undo").disabled = undoDepth(state) === 0;
    $("redo").disabled = redoDepth(state) === 0;
  };
  let here; // presence, once the view exists
  const view = new EditorView({
    doc: text(),
    root,
    extensions: [
      setup,
      theme,
      highlighting,
      ownEditsOnly,
      remoteSelections,
      automergeSyncPlugin({ handle, path: ["source"] }),
      EditorView.updateListener.of((update) => {
        syncHistory(update.state);
        here?.update(update);
      }),
    ],
    parent: $("source"),
  });
  $("undo").addEventListener("click", () => undo(view));
  $("redo").addEventListener("click", () => redo(view));

  // Picture: redrawn on every change, local or remote. Text that doesn't
  // parse (someone mid-edit) keeps the last good diagram up, with the error
  // above it. The text takes the render's principal colours and error line.
  // (Deferred: a local change arrives from inside CodeMirror's own update.)
  let live = true;
  const repaint = () => {
    const style = page.render();
    queueMicrotask(() => live && view.dispatch({ effects: paint.of(style) }));
  };
  handle.on("change", repaint);

  // Edits made on the picture come back as a whole new source. Applied to
  // CodeMirror as the smallest splice, as if typed: they join this person's
  // undo history, CodeMirror keeps undo right as remote edits arrive, and
  // automerge-codemirror carries the splice to the document, leaving
  // concurrent edits elsewhere alone
  const replace = (source, userEvent) => {
    const { at, removed, inserted } = diff(view.state.doc.toString(), source);
    view.dispatch({ changes: { from: at, to: at + removed.length, insert: inserted }, userEvent });
  };
  picture.addEventListener("rain-edit", ({ detail }) => replace(detail.source, "input.diagram"));

  // The examples replace the source for everyone, as one undoable edit
  for (const [id, example] of [
    ["example-basic", BASIC],
    ["example-tour", TOUR],
  ]) {
    $(id).addEventListener("click", () => {
      replace(example, "input.example");
      view.scrollDOM.scrollTop = 0;
    });
  }

  here = presence({ handle, view, list: people, name, load: page.load, save: page.save });

  page.start();
  repaint();

  const unmount = () => {
    live = false;
    handle.off("change", repaint);
    here.stop();
    view.destroy();
    page.destroy();
    if (host === element) element.replaceChildren();
    else host.remove();
  };
  return { view, host, unmount };
};
