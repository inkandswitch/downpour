# Rain Diagrams

> [!WARNING]
> This project is pretty vibey. It started as a sketch to see if the plain-text notation from the Keyhive lab notebook could be parsed and rendered, and much of it was written with an AI pair. Expect sharp edges, churn, and opinions that haven't been tested on anyone yet.

A plain-text notation, parser, and (soon) renderers for _certflow_ and _rain_ diagrams: pictures of how authority flows through certificate capability systems like [Keyhive].

- [Keyhive notebook 07: Certflow Diagrams][certflow] explains the snapshot view
- [Keyhive notebook 08: Rain Diagrams][rain] adds time, and the plain-text form this project parses

## The Notation

```
rain                                    %% or `certflow`
title Revoking two hops downstream

(Doc)-[edit]->(Alice)                   : The Doc grants Alice Edit
(Alice)-[edit(Doc)]->(Bob)              : Alice passes Edit to Bob
(Bob)=[[edit]]=>(Doc)                   : Bob edits the Doc
(Bob)-[read]->(Carol)                   : Bob gives Carol Read
cd: (Carol)-[read]->(Dan)               : Carol shares Read with Dan
(Alice)-<revoke>-x[cd]                  : Alice revokes Carol→Dan
```

Principals get round brackets, certificates get square ones (like the shapes in the diagrams), and invocations get double lines.

| Form                              | Meaning                                                     |
|-----------------------------------|-------------------------------------------------------------|
| `(A)-[edit(S)]->(B)`              | A delegates Edit over S to B                                |
| `(A)-[edit]->(B)`                 | Same, with S inferred from what A holds                     |
| `(A)-[edit(S) via ta]->(B)`       | A's authority over S arrives through certificate `ta`       |
| `label: (A)-[…]->(B)`             | Label a certificate (otherwise it's implicitly `a-b`)       |
| `(B)=[[edit]]=>(S)`               | B uses its Edit authority on S                              |
| `(A)-<revoke>-x[label]`           | A revokes the certificate `label`                           |
| `participant A, B`                | Fix column order                                            |
| `title …`, `: note`, `%% comment` | Annotations                                                 |

Levels are `admin`, `edit`, `read`, and `relay` (or 👑 ✏️ 👀 📡).

Powerlines, attenuation, and transitive handles are _derived_ from the certificates, not drawn by hand, so they can't be drawn wrong.

## Architecture

```
                    ┌──────────────────────────────────────┐
  source text ────▶ │ rain_diagram_core  (no_std, Rust)    │
                    │  parser ─▶ ast ─▶ diagram (resolved) │
                    │  derive: powerlines, reach, fallout  │
                    └───┬──────────┬───────────┬───────────┘
                        │          │           │
                        ▼          ▼           ▼
                   Unicode art   SVG        site JSON
                   (comments,    │          (Ink & Switch
                   READMEs)      │          notebook renderer)
                                 │
                 ┌───────────────┼──────────────────────┐
                 ▼               ▼                      ▼
          Mermaid plugin   <rain-diagram>        collaborative editor
          (external        web component         (automerge-repo v3
           diagram)        (interactive embed)    over Subduction)
```

| Crate / package       | Status   | Purpose                                                     |
|-----------------------|----------|-------------------------------------------------------------|
| `rain_diagram_core`   | Started  | Parser, resolver, site JSON export                          |
| `rain_diagram_wasm`   | Started  | JS bindings (`validate`, `toSiteJson`)                      |
| Text renderer         | Planned  | Unicode rain diagrams for code comments                     |
| SVG renderer          | Planned  | Static export; same SVG powers the interactive embed        |
| Web component         | Planned  | Embeddable, interactive (hover for reach, click to revoke)  |
| Mermaid plugin        | Planned  | `registerExternalDiagrams` adapter                          |
| Collaborative editor  | Planned  | Shared source via automerge-repo v3, synced over Subduction |

## Development

```console
$ nix develop        # toolchain, wasm-pack, and a command menu
$ preview            # (in the shell) hot-reloading preview at http://127.0.0.1:8080
$ nix run .#preview  # same, without entering the shell (PORT=… to change)
$ cargo test --workspace --exclude rain_diagram_wasm
$ nix run .#ci-clippy   # also: ci-fmt, ci-test, ci-doc, ci-no-std, ci-bolero, ci-deny, ci-msrv, ci-typos
```

## License

Licensed under either of [Apache License, Version 2.0](./LICENSE-APACHE) or [MIT license](./LICENSE-MIT) at your option.

[certflow]: https://www.inkandswitch.com/keyhive/notebook/07/
[Keyhive]: https://github.com/inkandswitch/keyhive
[rain]: https://www.inkandswitch.com/keyhive/notebook/08/
