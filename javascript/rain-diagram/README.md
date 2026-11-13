# `@inkandswitch/rain-diagram`

`<rain-diagram>`, a web component that draws a rain or certflow diagram from the plain-text notation and makes it interactive. All the logic (parsing, authority analysis, drawing, and source edits) is the Rust core compiled to Wasm; `rain-diagram.js` only wires the SVG's `data-*` hooks to it.

> [!NOTE]
> Not published yet (`"private": true`). The [preview](../preview/) (`nix run .#preview`) imports it from here, so it always runs this exact component.

## Build

```sh
pnpm run build  # wasm-pack → pkg/ (needs wasm-pack and the pinned wasm-bindgen-cli; both in the Nix dev shell)
```

## Use

```html
<script type="module" src="/path/to/rain-diagram.js"></script>

<rain-diagram><script type="text/rain">
  rain
  doc-alice: (Doc)-[edit(Doc)]->(Alice)
  (Alice)-[edit(Doc)]->(Bob)
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

| Event               | When                     | Detail |
|---------------------|--------------------------|--------|
| `rain-edit`         | an edit was made         | `{ source }`, the rewritten source; a dragged row adds `from` and `to`, its old and new statement index |
| `rain-edit-refused` | an edit would break it   | `{ message }`, why |
| `rain-fallout`      | a certificate is clicked | `{ event, voids }`: its event index, and the indices of everything revoking it would void |
| `rain-moment`       | the "now" line is moved  | `{ happened }`: how many events have happened |
| `rain-reach`        | a principal is hovered   | `{ principal, subjects }`: the principal's key, and what it can act on |

Each of `rain-reach`'s `subjects` is `{ key, level, caveats }`: a subject's key, an ability held over it (`level` is a Keyhive level like `"edit"`, or a command path like `"/crud/read"`), and the `where` caveats on the certificates it came through, earliest first. A subject appears once for each broadest ability held over it, so holding `/crud/read` and `/msg` over the same document lists it twice, while holding `edit` lists it once (`edit` already covers `read`).

```js
diagram.addEventListener("rain-reach", ({ detail }) => {
  for (const { key, level, caveats } of detail.subjects) {
    console.log(`${detail.principal} → ${key}: ${level}`, caveats);
  }
});
```

Set `.source` to replace the text. With `editable`, the host owns the source: listen for `rain-edit` and keep the text (and any undo history) yourself.
