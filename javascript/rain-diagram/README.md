# `@inkandswitch/rain-diagram`

`<rain-diagram>`, a web component that draws a rain or certflow diagram from the plain-text notation and makes it interactive. All the logic (parsing, authority analysis, drawing, and source edits) is the Rust core compiled to Wasm; `rain-diagram.js` only wires the SVG's `data-*` hooks to it.

> [!NOTE]
> Not published yet (`"private": true`). The [preview](../preview/) (`nix run .#preview`) imports it from here, so it always runs this exact component.

## Build

```sh
npm run build   # wasm-pack → pkg/ (needs wasm-pack and the pinned wasm-bindgen-cli; both in the Nix dev shell)
```

## Use

```html
<script type="module" src="/path/to/rain-diagram.js"></script>

<rain-diagram><script type="text/rain">
  rain
  (Doc)-[edit]->(Alice)
  (Alice)-[edit]->(Bob)
  (Doc)-<revoke>-x[doc-alice]
</script></rain-diagram>

<rain-diagram src="./revocation.rain" editable></rain-diagram>
```

Inline source goes in a `text/rain` script so that HTML doesn't swallow `<revoke>` as a tag.

| Attribute  | Effect                                                                 |
|------------|------------------------------------------------------------------------|
| `editable` | Drag rows, labels, region edges, and principals; draw between lifelines to delegate, invoke, or revoke; select and delete |
| `notation` | `rain` or `certflow`, overriding the header                            |
| `src`      | Load the source from a URL                                             |
| `theme`    | `light` or `dark`, overriding the system colour scheme                 |
| `zoom`     | Scale factor, such as `1.5`                                            |

| Event               | Detail                                     |
|---------------------|--------------------------------------------|
| `rain-edit`         | `{ source }`: the rewritten source         |
| `rain-edit-refused` | `{ message }`: why an edit was refused     |
| `rain-fallout`      | `{ event, voids }`: a certificate clicked  |
| `rain-reach`        | `{ principal, subjects }`: a principal hovered |

Set `.source` to replace the text. With `editable`, the host owns the source: listen for `rain-edit` and keep the text (and any undo history) yourself.
