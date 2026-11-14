// A rain diagram document: its source, and the title Patchwork lists it by
// (the diagram's own `title` line until someone renames it)

import { STARTER } from "../../preview/examples.js";

const titleLine = (source) => source?.match(/^\s*title\s+(.+?)\s*$/m)?.[1];

export const RainDiagram = {
  init(doc) {
    doc.source = STARTER;
  },
  getTitle(doc) {
    return doc.title || titleLine(doc.source) || "Rain diagram";
  },
  setTitle(doc, title) {
    doc.title = title;
  },
  markCopy(doc) {
    doc.title = `Copy of ${this.getTitle(doc)}`;
  },
};
