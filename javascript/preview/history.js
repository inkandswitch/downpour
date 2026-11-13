// Undo history for the source text, as a stack of edits rather than
// snapshots.
//
// Each edit is a splice, `{ at, removed, inserted }`, and undoing applies its
// inverse. Storing edits instead of whole sources is what lets this carry
// over to collaboration: there, an undo must revert only _your_ edit,
// leaving everyone else's alone, by applying the inverse splice as a new
// change at a position that follows concurrent edits (an Automerge cursor).
//
//   before ─── edit ───▶ after
//          ◀── inverse ──
//
// Typing coalesces: keystrokes that follow on from the last within a
// second become one step, like an editor.

const LIMIT = 500;
const PAUSE = 1000;

/** The smallest splice turning `before` into `after`. */
export const diff = (before, after) => {
  let start = 0;
  const shortest = Math.min(before.length, after.length);
  while (start < shortest && before[start] === after[start]) start++;
  let end = 0;
  while (end < shortest - start && before[before.length - 1 - end] === after[after.length - 1 - end]) end++;
  return {
    at: start,
    removed: before.slice(start, before.length - end),
    inserted: after.slice(start, after.length - end),
  };
};

export const apply = (text, { at, removed, inserted }) => text.slice(0, at) + inserted + text.slice(at + removed.length);

export const invert = ({ at, removed, inserted }) => ({ at, removed: inserted, inserted: removed });

export class History {
  #undo = [];
  #redo = [];
  #typedAt = 0;

  /** Record going from `before` to `after`; `typing` edits may coalesce. */
  record(before, after, { typing = false } = {}) {
    if (before === after) return;
    const now = Date.now();
    const last = this.#undo.at(-1);
    let edit = diff(before, after);

    if (typing && last?.typing && now - this.#typedAt < PAUSE && follows(last.edit, edit)) {
      // One step from before the run of typing to now
      this.#undo.pop();
      edit = diff(apply(before, invert(last.edit)), after);
    }
    this.#undo.push({ edit, typing });
    if (this.#undo.length > LIMIT) this.#undo.shift();
    this.#redo = [];
    this.#typedAt = typing ? now : 0;
  }

  /** `text` with the last edit undone, and where the caret goes, or null. */
  undo(text) {
    return this.#step(text, this.#undo, this.#redo, invert);
  }

  /** `text` with the last undone edit redone, and where the caret goes, or null. */
  redo(text) {
    return this.#step(text, this.#redo, this.#undo, (edit) => edit);
  }

  get canUndo() {
    return this.#undo.length > 0;
  }

  get canRedo() {
    return this.#redo.length > 0;
  }

  #step(text, from, to, direction) {
    const entry = from.pop();
    if (!entry) return null;
    to.push(entry);
    this.#typedAt = 0;
    const edit = direction(entry.edit);
    return { text: apply(text, edit), caret: edit.at + edit.inserted.length };
  }
}

/** Whether `next` carries on where `last` left off (typing on, or backspacing). */
const follows = (last, next) =>
  next.at === last.at + last.inserted.length || next.at + next.removed.length === last.at + last.inserted.length;
