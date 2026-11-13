# `rain_diagram_cli`

Two command-line tools for [certflow and rain diagrams](../../README.md).

## `rain-diagram`

```console
$ rain-diagram render doc.rain                  # text art to stdout
$ rain-diagram render doc.rain -o doc.svg       # format from the extension (.svg, .json, .txt)
$ rain-diagram render -f certflow-svg --at 3 < doc.rain
$ rain-diagram check diagrams/*.rain --deny-warnings
```

| Format         | Output                                                  |
|----------------|---------------------------------------------------------|
| `text`         | Unicode text art (the default)                          |
| `svg`          | SVG in the notation the header names                    |
| `rain-svg`     | SVG as a rain diagram, whatever the header says         |
| `certflow-svg` | SVG as a certflow diagram, whatever the header says     |
| `json`         | The JSON the Keyhive Lab Notes' renderer reads          |

`--at N` draws an SVG as of the first N events. Problems go to stderr as `path:line:column: error: message` (or `warning:`), the form editors and CI annotate. The exit status is 0 on success; 1 if a diagram has errors (or warnings, with `check --deny-warnings`) or a file can't be read or written; and 2 for bad arguments.

## `mdbook-rain-diagram`

An [mdBook](https://rust-lang.github.io/mdBook/) preprocessor: code blocks fenced as `rain` or `certflow` become diagrams.

````markdown
```rain
(Doc)-[edit(Doc)]->(Alice)
(Alice)-[read(Doc)]->(Bob)
```
````

Put `mdbook-rain-diagram` on the `PATH` and add it to `book.toml`:

```toml
[preprocessor.rain-diagram]
# on-error = "warn"   # show broken diagrams' errors in the page instead of failing the build
```

The fence names the notation, so a block can leave out the `rain` or `certflow` header. The HTML renderer gets inline SVG (in a `div.rain-diagram` that scrolls sideways when wider than the page); other renderers get text art. A diagram that doesn't parse fails the build with its chapter, line, and column. Warnings are printed and the diagram still renders. [`book/`](book/) is a working example, built by `nix run .#ci-mdbook`. Tested with mdBook 0.5.2. It reads the book as plain JSON, so mdBook 0.4's format works too.

## Installing

```console
$ nix run github:inkandswitch/downpour -- render doc.rain     # the CLI, without installing
$ nix profile install github:inkandswitch/downpour            # both binaries
$ cargo install --path rust/rain_diagram_cli                  # from a checkout
```
