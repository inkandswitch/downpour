# `@inkandswitch/downpour-editor`

> [!NOTE]
> Planned; nothing here yet.

A collaborative editor for rain and certflow diagrams: the source text in an Automerge document, synced with automerge-repo v3 over Subduction, with the highlighted text editor and an editable [`<rain-diagram>`](../rain-diagram/) side by side.

Starting points in this repo:

- [`../preview/`](../preview/) is the single-user version of this editor.
- [`../preview/history.js`](../preview/history.js) keeps undo as splices rather than snapshots, so each step can become an Automerge change and undo can revert only the local user's edits.
