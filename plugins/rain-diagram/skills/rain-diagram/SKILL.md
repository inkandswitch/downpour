---
name: rain-diagram
description: Write, check, and render rain and certflow diagrams, the plain-text notation for how authority flows through capability systems (Keyhive, UCAN, Biscuit, object capabilities). Use when asked to draw, diagram, visualize, or explain who can access what through delegation, attenuation, group membership, or revocation; when editing `.rain` or `.certflow` files or ` ```rain ` / ` ```certflow ` code blocks; or when the user mentions rain diagrams, certflow diagrams, or powerlines.
---

# Rain diagrams

A rain diagram is a sequence diagram of capability certificates: principals are columns, events go down the page in time order, and the tool _derives_ who holds what after each event. You write the certificates. You never draw powerlines, attenuation, or revocation fallout by hand: the tool computes them, so they can't be wrong. A certflow diagram is the same source drawn as a snapshot graph without time.

The notation is new, so don't guess it from Mermaid or PlantUML. Use the forms below, and let the checker correct you.

## Get the tool

```console
$ nix run github:inkandswitch/rain -- check doc.rain                      # no install
$ cargo install --git https://github.com/inkandswitch/rain rain_diagram_cli  # installs `rain-diagram`
```

If neither Nix nor Cargo is available, say so. Don't hand back an unchecked diagram as if it were checked.

## The loop

1. _Model the story._ List the principals: people, devices, groups, _and the resources themselves_. List the events in the order they happen.
2. _Write the source_ to a `.rain` (or `.certflow`) file, one statement per line, with a `: note` saying what each event means.
3. _Check it._ `rain-diagram check --deny-warnings doc.rain`. Errors are `path:line:col: error: …` and mean the source doesn't parse or resolve. Warnings mean it parses but doesn't do what it looks like (see [Warnings](#warnings)).
4. _Look at it._ `rain-diagram render doc.rain` prints Unicode text art. Read it and confirm the powerlines (`┄`) come from where you expect, and that revoked certificates are crossed out (`✗`).
5. _Deliver it_ in the form the user needs (see [Output](#output)).

Repeat steps 3 and 4 until the diagram is clean, or until every remaining warning is one you mean.

## Notation

```rain
title Alice cuts off a middleman
(Doc)-[admin(Doc)]->(Alice)    : Alice creates the Doc
ab: (Alice)-[edit(Doc)]->(Bob) : Alice shares Edit with Bob
(Bob)-[read(Doc)]->(Carol)     : Bob passes Read to Carol
(Carol)=[[read]]=>(Doc)        : Carol reads the Doc
(Alice)-<revoke>-x[ab]         : Alice revokes Bob, and Carol with him
```

| Form                              | Meaning                                                    |
|-----------------------------------|------------------------------------------------------------|
| `rain` / `certflow`               | Header line; optional when the file extension says it      |
| `(A)-[edit(S)]->(B)`              | A delegates Edit over subject S to B                       |
| `lbl: (A)-[…]->(B)`               | Label a delegation so later lines can name it              |
| `(B)=[[edit]]=>(S)`               | B uses Edit on S (an invocation; no subject in the brackets) |
| `(A)-<revoke>-x[lbl]`             | A revokes the certificate labelled `lbl`                   |
| `(A)-[edit(S) via lbl]->(B)`      | Force the route through `lbl` (otherwise derived)          |
| `(A)-[/crud/read(S)]->(B)`        | A command path instead of a level (UCAN-style)             |
| `(A)-[edit(S) where x < 3]->(B)`  | A caveat; always last inside the brackets                  |
| `(A)---[edit(S)]->(B)`            | Extra dashes slide the label along (here ¾ of the way)     |
| `>>> R (P): caption` … `<<< R`    | A captioned region, tinted in principal P's colour         |
| `principals: A, B, C`             | Fix the column order                                       |
| `title …`, `: note`, `// comment` | Annotations                                                |

- _Levels_ are `admin` > `edit` > `read` > `relay`. Each implies the ones after it.
- _Command paths_ (`/`, `/crud`, `/crud/read`) cover themselves and the paths under them. Levels and paths never cover each other.
- _Caveats_ are opaque text (UCAN policy, Biscuit Datalog, or plain words) and are never evaluated. They pile up down a chain.
- _Names_ may contain spaces: `(Thread 1)`. Labels use letters, digits, `-`, `_`, and `.`.

## Modelling

- _Every delegation names its subject._ `(A)-[edit]->(B)` is a parse error. The subject is the resource the authority is over.
- _Resources are principals._ A resource grants authority over itself: `(Doc)-[admin(Doc)]->(Alice)` means "Alice creates (owns) the Doc". Start a chain there, or the first delegation grants nothing.
- _Groups are principals too._ Share with the group, then add members by delegating over the group. Members reach what the group holds:

  ```rain
  title Sharing through a group
  (Doc)-[admin(Doc)]->(Alice)      : Alice creates the Doc
  (Team)-[admin(Team)]->(Alice)    : Alice creates Team
  (Alice)-[edit(Doc)]->(Team)      : Alice shares the Doc with Team
  bob: (Alice)-[edit(Team)]->(Bob) : Bob joins Team
  (Bob)=[[edit]]=>(Doc)            : Bob edits through Team
  (Alice)-<revoke>-x[bob]          : Alice removes Bob from Team
  ```

- _Label anything you will revoke or route `via`,_ and put the label on a line _before_ the one that uses it. Only delegations take labels.
- _Who may revoke._ The issuer of the certificate, the issuer of anything it relies on, an `admin` of its subject, or the subject itself. `/` is not an admin.
- _Order is time._ Each event's authority is decided from the events above it only. Put the grant above the use.
- _Columns follow first appearance._ Put the root resource first, or add a `principals:` line.
- _Pick the notation._ Use `rain` when order matters: revocation, or "before and after". Use `certflow` for "who can reach what right now". Both take the same statements.
- _For UCAN or Biscuit,_ use command paths and `where` caveats. Each Biscuit attenuation block becomes one delegation with a `where` check:

  ```rain
  title A UCAN chain, attenuated
  (Service)-[/crud(Service) where path = "/docs"]->(Alice) : the root grant
  (Alice)-[/crud/read(Service) where size < 1MB]->(Bob)  : Alice narrows to read
  (Bob)=[[/crud/read]]=>(Service)                         : bound by both caveats
  ```

## Warnings

A warning means the diagram resolves but an event doesn't do what it looks like. Sometimes that's the point of the diagram: "after the revocation, Carol's read is refused". So for each warning, either fix the source, or keep it on purpose and say in the event's note that it fails, and tell the user.

| Warning starts with…                                   | It means                                   | Usual fix                                              |
|--------------------------------------------------------|--------------------------------------------|--------------------------------------------------------|
| `X holds no authority over S here`                     | X delegates something it doesn't have      | Add the grant to X _above_ this line, or start the chain at S |
| `X only holds L over S, so this grants L, not M`       | Claims more than X holds; L flows          | Claim L, or give X more first                          |
| `X holds … none of which covers or falls under P`      | Path unrelated to what X holds             | Use a path under one X holds                           |
| `X can't use A on S`                                   | Invocation without enough authority        | Grant first, or keep it to show a refusal              |
| `X didn't issue anything … relies on`                  | Revocation from outside the chain; no effect | Revoke from an issuer upstream, or an admin of the subject |

A refusal kept on purpose:

```rain warns
(Doc)-[admin(Doc)]->(Alice)    : Alice creates the Doc
ab: (Alice)-[edit(Doc)]->(Bob) : Alice shares Edit with Bob
(Bob)-[read(Doc)]->(Carol)     : Bob passes Read to Carol
(Alice)-<revoke>-x[ab]         : Alice revokes Bob
(Carol)=[[read]]=>(Doc)        : refused: her Read came through Bob
```

## Output

```console
$ rain-diagram render doc.rain                  # text art (code comments, READMEs, terminals)
$ rain-diagram render doc.rain -o doc.svg       # SVG, as the header says
$ rain-diagram render -f certflow-svg doc.rain  # the other notation's picture
$ rain-diagram render --at 3 doc.rain -o 3.svg  # SVG as of the first 3 events
```

- _Keep the source._ Commit the `.rain` file next to anything rendered from it, so the picture can be regenerated. Don't hand-edit text art or SVG.
- _In an mdBook,_ use a ` ```rain ` or ` ```certflow ` fence and the `mdbook-rain-diagram` preprocessor (`[preprocessor.rain-diagram]` in `book.toml`).
- _In HTML,_ use the `<rain-diagram>` element, with inline source in `<script type="text/rain">`. A bare `<revoke>` would be read as an HTML tag.
- _Show your result._ When you deliver a diagram, include the text-art render (or say where the SVG is), plus any warnings you kept and why.

## More

- [Notation reference](https://github.com/inkandswitch/rain#the-notation)
- [Worked examples](https://github.com/inkandswitch/rain/tree/main/rust/rain_diagram_core/tests/golden)
- [Keyhive Lab Note #8: Rain Diagrams](https://www.inkandswitch.com/keyhive/notebook/08/), which explains what the picture means
