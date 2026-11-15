# `@inkandswitch/downpour-patchwork`

> [!NOTE]
> Built and tested in a stand-in host; not yet published to Patchwork.

A [Patchwork](https://patchwork.inkandswitch.com) module for rain and certflow diagrams. It has two plugins:

| Plugin | `id` | What it is |
|---|---|---|
| `patchwork:datatype` | `rain-diagram` | A document with a `source` string, seeded with a small diagram. Its title is the diagram's `title` line until someone renames it. |
| `patchwork:tool` | `rain-diagram` | The [collaborative editor](../editor/), mounted with `mountEditor` on the document Patchwork opens |

Patchwork brings the repo, sync, and Automerge itself, so the tool is just the editor. It is about 30 KB of JS (gzipped) plus the diagram's Wasm. The standalone page is 2 MB, because there the editor brings Automerge and Subduction itself.

## How it fits Patchwork

- **[`index.js`](index.js)** exports `plugins`, with metadata only. Patchwork reads it in a worker, so each plugin's code comes in through its `load()`.
- **[`src/tool.js`](src/tool.js)** is the render function, `(handle, element) => cleanup`. It mounts the editor in its own shadow root, because Patchwork tools share the light DOM, and the editor's ids and styles would otherwise clash. Patchwork's `--editor-*` theme variables flow into the editor's own colours. The picture goes light or dark to match. Presence uses the account's name.
- **[`vite.config.js`](vite.config.js)** leaves the packages Patchwork's importmap provides (Automerge, automerge-repo, `@codemirror/{commands,language,state,view}`) external, and bundles the rest.

## Build and publish

```console
$ pnpm install            # from the repo root
$ cd javascript/patchwork
$ pnpm build              # dist/index.js and its chunks
```

Publishing uses [`pushwork`](https://github.com/inkandswitch/pushwork). It syncs this folder to an Automerge URL, and Patchwork loads the module from that URL.

```console
$ npx pushwork@latest init .     # once: makes .pushwork/, and the module's automerge: URL
$ pnpm push                      # build, then sync
$ npx pushwork@latest url        # the URL to register
```

Then, in Patchwork, open _My tools_ → _Register a Module_ and give it that URL. You can also register it with `pw-modules add "$MODULE_SETTINGS_DOC_URL" "$(npx pushwork@latest url)"`. After that, _Create new_ offers _Rain diagram_.

> [!NOTE]
> `.pushwork/` holds the module's URL and local sync state. Before the first sync, check that `node_modules/` and `harness/` stay out of what it pushes.

## Testing without Patchwork

[`harness/`](harness/) is a stand-in host. It provides the globals a tool reads (`repo`, `accountDocHandle`), creates a document with the datatype, and mounts the tool twice through the render contract. [`../editor/browser-test/patchwork.test.mjs`](../editor/browser-test/patchwork.test.mjs) drives it, and runs in `nix run .#ci-editor`.

```console
$ node node_modules/vite/bin/vite.js build harness --emptyOutDir   # then serve harness/dist
```

The harness uses this repo's automerge-repo (3.0 alpha). Patchwork currently ships 2.6 (`2.6.0-subduction.49`). The tool uses only the API the two share: `DocHandle` events, `broadcast`, and `Presence`. Still, the first run in Patchwork is the real test.
