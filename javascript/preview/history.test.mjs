// Undo history properties: `node --test javascript/preview/*.test.mjs` (or `nix run .#ci-js`).

import assert from "node:assert/strict";
import { test } from "node:test";
import { History, apply, diff, invert } from "./history.js";

const random = (length) =>
  Array.from({ length: Math.floor(Math.random() * length) }, () => "ab\n("[Math.floor(Math.random() * 4)]).join("");

test("an edit and its inverse take you there and back", () => {
  for (let i = 0; i < 20_000; i++) {
    const [before, after] = [random(12), random(12)];
    const edit = diff(before, after);
    assert.equal(apply(before, edit), after);
    assert.equal(apply(after, invert(edit)), before);
  }
});

test("undoing everything returns to the start, and redoing everything to the end", () => {
  for (let i = 0; i < 2_000; i++) {
    const history = new History();
    const states = [random(10)];
    for (let j = 0; j < 8; j++) {
      const next = random(10);
      history.record(states.at(-1), next, { typing: Math.random() < 0.5 });
      states.push(next);
    }
    let text = states.at(-1);
    for (let step; (step = history.undo(text)); ) text = step.text;
    assert.equal(text, states[0]);
    for (let step; (step = history.redo(text)); ) text = step.text;
    assert.equal(text, states.at(-1));
  }
});

test("a run of typing is one step; a picture edit is its own", () => {
  const history = new History();
  let text = "rain\n";
  for (const ch of "(A)") {
    history.record(text, text + ch, { typing: true });
    text += ch;
  }
  history.record(text, text + "\n(B)");
  text += "\n(B)";

  text = history.undo(text).text;
  assert.equal(text, "rain\n(A)");
  text = history.undo(text).text;
  assert.equal(text, "rain\n");
  assert.equal(history.undo(text), null);
});

test("a new edit clears redo", () => {
  const history = new History();
  history.record("a", "ab");
  const undone = history.undo("ab");
  assert.equal(undone.text, "a");
  history.record("a", "ac");
  assert.equal(history.canRedo, false);
});
