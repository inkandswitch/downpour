// The preview's syntax colouring (../../preview/highlight.js) as CodeMirror
// decorations: the same tokens and `tok-*` classes, principals in their
// diagram colours, and the line a parse error points at marked.

import { RangeSetBuilder, StateEffect, StateField } from "@codemirror/state";
import { Decoration, EditorView } from "@codemirror/view";
import { tokens } from "../../preview/highlight.js";

/** New principal colours and error line, from the page's latest render. */
export const paint = StateEffect.define();

const ERROR_LINE = Decoration.line({ class: "err-line" });

const decorate = (doc, { colours, errorLine }) => {
  const builder = new RangeSetBuilder();
  tokens(doc.toString(), colours).forEach((line, i) => {
    let at = doc.line(i + 1).from;
    if (i + 1 === errorLine) builder.add(at, at, ERROR_LINE);
    for (const { kind, text, colour } of line) {
      if (kind) {
        const mark = Decoration.mark({ class: `tok-${kind}`, ...(colour && { attributes: { style: `color:${colour}` } }) });
        builder.add(at, at + text.length, mark);
      }
      at += text.length;
    }
  });
  return builder.finish();
};

const field = StateField.define({
  create: (state) => {
    const style = { colours: new Map(), errorLine: null };
    return { style, decorations: decorate(state.doc, style) };
  },
  update: (value, tr) => {
    const style = tr.effects.findLast((e) => e.is(paint))?.value ?? value.style;
    return style === value.style && !tr.docChanged ? value : { style, decorations: decorate(tr.state.doc, style) };
  },
  provide: (f) => EditorView.decorations.from(f, (value) => value.decorations),
});

export const highlighting = [field];
