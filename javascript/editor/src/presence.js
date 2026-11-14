// Who's here, and where: each tab shares a name, a colour, and its selection
// over the document's ephemeral messages (automerge-repo's Presence, which
// Subduction carries but never stores). Selections travel as Automerge
// cursors, so a caret stays on its character as edits arrive from anyone.

import { getCursor, getCursorPosition } from "@automerge/automerge";
import { Presence } from "@automerge/automerge-repo";
import { StateEffect, StateField } from "@codemirror/state";
import { Decoration, EditorView, WidgetType } from "@codemirror/view";

// People's colours, by the name they go by: apart from the principal
// palette, so nobody reads a caret as a principal
const COLOURS = {
  blue: "#1f78b4",
  brown: "#8b4513",
  gold: "#b8860b",
  green: "#2e8b57",
  pink: "#c71585",
  purple: "#6a3d9a",
  red: "#d0473d",
  teal: "#008b8b",
};
const MOODS = ["brave", "calm", "cheeky", "clever", "cosy", "curious", "dapper", "eager", "fuzzy", "gentle", "giddy", "jolly", "lucky", "merry", "nimble", "plucky", "quiet", "silly", "sleepy", "sunny", "swift", "witty", "zesty"];
const ANIMALS = ["badger", "beaver", "crane", "duck", "ferret", "fox", "frog", "goose", "hare", "hedgehog", "heron", "koala", "lemur", "lynx", "moose", "newt", "otter", "owl", "panda", "puffin", "quokka", "raven", "seal", "stoat", "toad", "walrus", "wombat", "yak"];
const PATH = ["source"];

const others = StateEffect.define();

class Caret extends WidgetType {
  constructor(name, colour) {
    super();
    this.name = name;
    this.colour = colour;
  }

  eq(other) {
    return other.name === this.name && other.colour === this.colour;
  }

  toDOM() {
    const caret = Object.assign(document.createElement("span"), { className: "cm-remote-caret" });
    caret.style.borderLeftColor = this.colour;
    const label = Object.assign(document.createElement("span"), { className: "cm-remote-name", textContent: this.name });
    label.style.backgroundColor = this.colour;
    caret.append(label);
    return caret;
  }

  ignoreEvent() {
    return true;
  }
}

const decorations = (people) =>
  Decoration.set(
    people.flatMap(({ name, colour, from, to, head }) => [
      ...(from < to ? [Decoration.mark({ attributes: { style: `background-color:${colour}33` } }).range(from, to)] : []),
      Decoration.widget({ widget: new Caret(name, colour), side: 1 }).range(head),
    ]),
    true,
  );

const field = StateField.define({
  create: () => Decoration.none,
  update: (value, tr) => {
    const next = tr.effects.findLast((e) => e.is(others));
    return next ? decorations(next.value) : value.map(tr.changes);
  },
  provide: (f) => EditorView.decorations.from(f),
});

const style = EditorView.baseTheme({
  ".cm-remote-caret": { position: "relative", borderLeft: "2px solid", marginLeft: "-1px", marginRight: "-1px" },
  ".cm-remote-name": {
    position: "absolute",
    bottom: "100%",
    left: "-2px",
    padding: "0 4px",
    borderRadius: "3px 3px 3px 0",
    color: "white",
    font: "600 10px/1.5 system-ui, sans-serif",
    whiteSpace: "nowrap",
    pointerEvents: "none",
    opacity: "0.9",
  },
});

/** CodeMirror extensions that draw other people's selections. */
export const remoteSelections = [field, style];

/**
 * Share this tab's presence on `handle`, draw everyone else's in `view`, and
 * list who's here in `list`, if given.
 *
 * @param {object} options
 * @param {import("@automerge/automerge-repo").DocHandle} options.handle
 * @param {EditorView} options.view
 * @param {HTMLElement} [options.list]
 * @param {string} [options.name] this person's name, when the host knows it
 * @param {(key: string, fallback: string) => string} options.load
 * @param {(key: string, value: string) => void} options.save
 */
export const presence = ({ handle, view, list, name, load, save }) => {
  // A name like silly-teal-otter, in that colour, kept per browser
  const random = (items) => items[Math.floor(Math.random() * items.length)];
  const hue = random(Object.keys(COLOURS));
  const saved = load("name", "");
  const user = saved && COLOURS[load("colour", "")]
    ? { name: saved, colour: COLOURS[load("colour", "")], hue: load("colour", "") }
    : { name: `${random(MOODS)}-${hue}-${random(ANIMALS)}`, colour: COLOURS[hue], hue };
  save("name", user.name);
  save("colour", user.hue);
  if (name) user.name = name;

  // Cursors at the selection, or none while the document hasn't caught up
  // with the view
  const selection = () => {
    const doc = handle.doc();
    const { anchor, head } = view.state.selection.main;
    try {
      return { anchor: getCursor(doc, PATH, anchor), head: getCursor(doc, PATH, head) };
    } catch {
      return null;
    }
  };

  const shared = new Presence({ handle });
  shared.start({ initialState: { user, selection: null }, heartbeatMs: 5_000, peerTtlMs: 15_000 });

  // Everyone else, with their selection resolved against this tab's copy (a
  // cursor this copy hasn't seen yet just leaves the caret out for now)
  const people = () =>
    Object.values(shared.getPeerStates().value).map(({ value }) => {
      const person = { name: "Someone", colour: "#888888", ...value.user };
      if (!value.selection) return person;
      try {
        const length = view.state.doc.length;
        const at = (cursor) => Math.min(getCursorPosition(handle.doc(), PATH, cursor), length);
        const anchor = at(value.selection.anchor);
        const head = at(value.selection.head);
        return { ...person, from: Math.min(anchor, head), to: Math.max(anchor, head), head };
      } catch {
        return person;
      }
    });

  const chip = ({ name, colour }, you = false) => {
    const element = document.createElement(you ? "button" : "span");
    element.className = "person";
    element.style.setProperty("--person", colour);
    element.textContent = you ? `${name} (you)` : name;
    if (you) {
      element.type = "button";
      element.title = "Change your name";
      element.addEventListener("click", () => {
        const name = prompt("Your name, as others see it", user.name)?.trim();
        if (!name) return;
        user.name = name;
        save("name", name);
        shared.broadcast("user", { ...user });
        draw();
      });
    }
    return element;
  };

  const draw = () => {
    const here = people();
    list?.replaceChildren(chip(user, true), ...here.map((person) => chip(person)));
    // Deferred: draw can run from inside a CodeMirror update
    queueMicrotask(() => live && view.dispatch({ effects: others.of(here.filter((p) => p.head !== undefined)) }));
  };

  let live = true;
  const EVENTS = ["update", "snapshot", "goodbye", "pruning"];
  for (const event of EVENTS) shared.on(event, draw);
  handle.on("change", draw);

  // Share the selection once the view and the document agree, at most once
  // a frame
  let pending = false;
  const share = () => {
    if (pending) return;
    pending = true;
    requestAnimationFrame(() => {
      pending = false;
      if (!live) return;
      const next = selection();
      if (next) shared.broadcast("selection", next);
    });
  };

  // Leaving says goodbye, so others drop this caret at once
  const leave = () => shared.stop();
  addEventListener("pagehide", leave);
  draw();
  return {
    /** For an EditorView update listener: share selection moves and edits. */
    update: (update) => {
      if (update.selectionSet || update.docChanged) share();
    },
    stop: () => {
      live = false;
      removeEventListener("pagehide", leave);
      for (const event of EVENTS) shared.off(event, draw);
      handle.off("change", draw);
      if (shared.running) shared.stop();
    },
  };
};
