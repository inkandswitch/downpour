# `@inkandswitch/rain-diagram-wasm`

The rain and certflow diagram core (parser, authority analysis, renderers, and source edits), compiled to Wasm and packaged to load anywhere JavaScript runs: Node, bundlers, plain browser pages, workers, and `<script>` tags. For the interactive `<rain-diagram>` element, see [`../rain-diagram/`](../rain-diagram/).

> [!NOTE]
> Not published yet (`"private": true`). Build it with `nix run .#bodge`, which writes `dist/` here. `nix run .#ci-package` builds it and runs the smoke tests in `package-test/`.

```js
import { renderSvg, renderText, RainDiagram } from "@inkandswitch/rain-diagram-wasm";

const source = `rain
da: (Doc)-[edit(Doc)]->(Alice)
(Alice)-[edit(Doc)]->(Bob)
(Doc)-<revoke>-x[da]`;

console.log(renderText(source));          // Unicode text art
document.body.innerHTML = renderSvg(source);

const diagram = new RainDiagram(source);  // parsed once, for repeated queries
diagram.reach("Bob");                     // what Bob can act on, as JSON
diagram.free();                           // or `using diagram = …`
```

Problems are thrown as `Error`s. A source that doesn't parse gives a message starting with `line:column:`. Numbers (statement indices, columns, percentages) must be whole numbers ≥ 0.

## API

| Export | Does |
|---|---|
| `renderSvg(source)`, `renderText(source)`, `toSiteJson(source)` | One-shot renders |
| `validate(source)`, `diagnostics(source)` | Parse errors (thrown) and warnings (`["line:col: message", …]`) |
| `insertStatement(source, at, kind, from, to, ability, subject?)` | Add a statement as row `at`. `kind` is `"delegate"`, `"invoke"`, or `"revoke"`; `from`/`to` are columns (for a revocation, `to` is the revoked certificate's row). `ability` is a level keyword or a command path like `/crud/read`. `subject` is the column a delegation is over; leave it out to use what the issuer holds |
| `deleteStatement`, `moveStatement`, `movePrincipal`, `setPosition`, `setRegionRows`, `setRegionNote` | Other source edits; each returns the new source |
| `new RainDiagram(source)` | Parse once for repeated queries (below); `free()` it when done |

`RainDiagram` methods:

| Method | Returns |
|---|---|
| `svg()`, `rainSvg()`, `certflowSvg()`, `rainSvgAt(n)`, `certflowSvgAt(n)`, `text()` | Renders, optionally after only `n` events |
| `kind`, `events` | `"rain"` or `"certflow"`, and the event count |
| `principals()` | Principal names, by column |
| `reach(name)` | JSON `{ subjects: [{ key, level, caveats }], events }`: one entry per broadest ability held over each subject (`level` is the ability, so a subject can appear twice, as with `/crud/read` and `/msg`), the `where` caveats along the way, and the certificates on the routes |
| `heldBefore(column, at)` | JSON `[{ subject, level }]`: what that principal holds just before row `at`, where a dragged statement would land. Its authority over itself isn't listed |
| `movable(row)`, `dependents(row)` | Rows a statement can move to, as `[first, last]`, and the rows a revocation of it would void |

## Using it

| Where | How | Checked |
|---|---|---|
| Vite 8+ | Nothing to set up: the wasm is emitted as its own asset | ✓ Vite 8.3.8: [`examples/vite/`](examples/vite/) is built and loaded in CI (`nix run .#ci-vite`) |
| Vite 7 and older | Add the package to `optimizeDeps.exclude` | from wasm-bodge's docs |
| webpack 5.83+ | Nothing to set up | from wasm-bodge's docs |
| webpack before 5.83 | Enable `experiments.topLevelAwait` | from wasm-bodge's docs |
| Rollup | Needs node-resolve and an asset plugin such as `@web/rollup-plugin-import-meta-assets` | from wasm-bodge's docs |
| Node | `import` or `require`; ready to use, no init | ✓ |
| Plain page, module | `import … from "…/dist/esm/web.js"`: the wasm is inlined (base64), so no fetch | ✓ |
| Plain page, `<script>` | `<script src="…/dist/iife/index.js">` defines a global `RainDiagramWasm` | ✓ |
| Cloudflare Workers | the `workerd` export condition | not yet |
| Your own loading | `/slim` exports `initSync`; pass it the bytes from `/wasm` | ✓ |

With a Content Security Policy, allow the wasm asset in `connect-src` and Wasm compilation in `script-src` (`'wasm-unsafe-eval'`).

The plain-page builds inline the wasm as base64, so they're about a third larger than the separate `.wasm` file a bundler emits. The main README's [Size table](../../README.md#size) has the current figures, checked by CI.

## How it's built

[wasm-bodge](https://github.com/alexjg/wasm-bodge), pinned in the repo's Nix flake, turns the `wasm-bindgen` output of `rust/rain_diagram_wasm` into this package. It writes `dist/` and adds the generated fields (`exports`, `main`, `types`, `files`, and so on) to this `package.json`. The build is release-mode with `wasm-opt`, and `--panic abort` so it runs on the stable toolchain.
