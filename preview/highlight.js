// Syntax colouring for the source panel. It mirrors the grammar line by line
// (the parser itself lives in Rust); principals take their diagram colours.

// Same order as the SVG renderer's palette, so text and picture match
export const PALETTE = ["#3d7dd8", "#e8892b", "#3f9b5a", "#8a5cc9", "#c2553a", "#2a9d96", "#c94f8a", "#7d8a2e"];

const escape = (text) => text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
const span = (cls, text, style = "") => (text ? `<span class="tok-${cls}"${style}>${escape(text)}</span>` : "");

/** Where a comment starts: `%%` anywhere, or `//` at the start or after whitespace. */
const commentStart = (line) => {
  const percent = line.indexOf("%%");
  let slashes = -1;
  for (let i = line.indexOf("//"); i !== -1; i = line.indexOf("//", i + 1)) {
    if (i === 0 || /\s/.test(line[i - 1])) {
      slashes = i;
      break;
    }
  }
  const starts = [percent, slashes].filter((i) => i !== -1);
  return starts.length ? Math.min(...starts) : -1;
};

const principal = (name, colours) => {
  const colour = colours.get(name.trim());
  return span("principal", `(${name})`, colour ? ` style="color:${colour}"` : "");
};

const LEVEL = /^(admin|edit|read|relay|👑|✏️?|👀|📡)/i;

/** A statement body and tail: arrows, principals, levels, `@N%`, `: note`. */
const statement = (code, colours) => {
  let out = "";
  let rest = code;

  // `label:` before the first principal
  const label = rest.match(/^(\s*)([^\s(:][^(:]*?)(\s*:)(?=\s*\()/);
  if (label) {
    out += escape(label[1]) + span("label", label[2]) + span("punct", label[3]);
    rest = rest.slice(label[0].length);
  }

  const rules = [
    [/^\(([^()]*)\)/, (m) => principal(m[1], colours)],
    [/^-<revoke>-x/, (m) => span("revoke", m[0])],
    [/^(=\[\[|\]\]=>|-\[|\]->|\[|\])/, (m) => span("punct", m[0])],
    [LEVEL, (m) => span("level", m[0])],
    [/^via\b/, (m) => span("keyword", m[0])],
    [/^@\d*%?/, (m) => span("position", m[0])],
    [/^:.*/, (m) => span("punct", ":") + span("note", m[0].slice(1))],
    [/^\s+/, (m) => escape(m[0])],
    [/^[^\s()[\]:@=-]+/, (m) => span("label", m[0])], // ids in [revoke target] and via lists
    [/^./su, (m) => escape(m[0])],
  ];
  while (rest) {
    for (const [pattern, emit] of rules) {
      const m = rest.match(pattern);
      if (m) {
        out += emit(m);
        rest = rest.slice(m[0].length);
        break;
      }
    }
  }
  return out;
};

/** Colour one line's code (no comment). */
const line = (code, isHeader, colours) => {
  const indent = code.match(/^\s*/)[0];
  const body = code.slice(indent.length);
  if (!body) return escape(code);

  if (isHeader) return escape(indent) + span("keyword", body);

  let m;
  if ((m = body.match(/^(title)(\s+)(.*)$/))) {
    return escape(indent) + span("keyword", m[1]) + escape(m[2]) + span("title", m[3]);
  }
  if ((m = body.match(/^(principals|participants|participant)(:?)(.*)$/)) && !/^:?\s*\(/.test(m[2] + m[3])) {
    const names = m[3]
      .split(",")
      .map((piece) => {
        const colour = colours.get(piece.trim());
        return span("principal", piece, colour ? ` style="color:${colour}"` : "");
      })
      .join(span("punct", ","));
    return escape(indent) + span("keyword", m[1] + m[2]) + names;
  }
  if ((m = body.match(/^(>>>)(\s*)([^\s(:]*)(\s*)(?:\(([^)]*)\))?(\s*)(:?)(.*)$/))) {
    const [, marker, s1, id, s2, who, s3, colon, note] = m;
    return (
      escape(indent) +
      span("region", marker) +
      escape(s1) +
      span("label", id) +
      escape(s2) +
      (who !== undefined ? principal(who, colours) : "") +
      escape(s3) +
      span("punct", colon) +
      span("note", note)
    );
  }
  if ((m = body.match(/^(<<<)(.*)$/))) {
    return escape(indent) + span("region", m[1]) + span("label", m[2]);
  }
  return escape(indent) + statement(body, colours);
};

/**
 * The source as highlighted HTML.
 *
 * @param {string} text
 * @param {Map<string, string>} colours principal name → colour
 * @param {number | null} errorLine 1-based line to mark, if any
 */
export const highlight = (text, colours, errorLine = null) => {
  let seenHeader = false;
  return (
    text
      .split("\n")
      .map((raw, i) => {
        const at = commentStart(raw);
        const code = at === -1 ? raw : raw.slice(0, at);
        const comment = at === -1 ? "" : raw.slice(at);
        const isHeader = !seenHeader && code.trim() !== "";
        if (isHeader) seenHeader = true;
        const html = line(code, isHeader, colours) + span("comment", comment);
        return i + 1 === errorLine ? `<span class="err-line">${html || " "}</span>` : html;
      })
      .join("\n") + "\n" // a trailing newline keeps the last line's height
  );
};
