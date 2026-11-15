// Syntax colouring for the source. It mirrors the grammar line by line (the
// parser itself lives in Rust); principals take their diagram colours.
// `tokens` is shared: the preview renders it as HTML (`highlight`), and the
// collaborative editor as CodeMirror decorations.

// Same order as the SVG renderer's palette, so text and picture match
export const PALETTE = ["#3d7dd8", "#e8892b", "#3f9b5a", "#8a5cc9", "#c2553a", "#2a9d96", "#c94f8a", "#7d8a2e"];

/**
 * A run of source text: a token class (`tok-${kind}` in style.css), or
 * `null` for plain text, and a principal's colour.
 *
 * @typedef {{ kind: string | null, text: string, colour?: string }} Token
 */

/** @returns {Token[]} */
const tok = (kind, text, colour) => (text ? [{ kind, text, ...(colour && { colour }) }] : []);
const plain = (text) => tok(null, text);

/** Where a comment starts: `%%` anywhere, or `//` at the start or after whitespace. */
export const commentStart = (line) => {
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

const principal = (name, colours) => tok("principal", `(${name})`, colours.get(name.trim()));

const LEVEL = /^(admin|edit|read|relay|👑|✏️?|👀|📡)/i;
// A command path, as in UCAN: `/`, or `/`-separated segments
const COMMAND = /^\/[\p{L}\p{N}._\/-]*/u;

/** A statement body and tail: arrows (any length), principals, abilities, `where` caveats, `: note`. */
const statement = (code, colours) => {
  const out = [];
  let rest = code;

  // `label:` before the first principal
  const label = rest.match(/^(\s*)([^\s(:][^(:]*?)(\s*:)(?=\s*\()/);
  if (label) {
    out.push(...plain(label[1]), ...tok("label", label[2]), ...tok("punct", label[3]));
    rest = rest.slice(label[0].length);
  }

  const rules = [
    [/^\(([^()]*)\)/, (m) => principal(m[1], colours)],
    [/^-+<revoke>-+x/, (m) => tok("revoke", m[0])],
    [/^(=+\[\[|\]\]=+>|-+\[|\]-+>|\[|\])/, (m) => tok("punct", m[0])],
    [LEVEL, (m) => tok("level", m[0])],
    [COMMAND, (m) => tok("level", m[0])],
    [/^via\b/, (m) => tok("keyword", m[0])],
    // A caveat runs to the `]` (it can't contain brackets)
    [/^(where)(\s[^\]]*)/, (m) => [...tok("keyword", m[1]), ...tok("caveat", m[2])]],
    [/^:.*/, (m) => [...tok("punct", ":"), ...tok("note", m[0].slice(1))]],
    [/^\s+/, (m) => plain(m[0])],
    [/^[^\s()[\]:=-]+/, (m) => tok("label", m[0])], // ids in [revoke target] and via lists
    [/^./su, (m) => plain(m[0])],
  ];
  while (rest) {
    for (const [pattern, emit] of rules) {
      const m = rest.match(pattern);
      if (m) {
        out.push(...emit(m));
        rest = rest.slice(m[0].length);
        break;
      }
    }
  }
  return out;
};

/** One line's code (no comment). */
const line = (code, isHeader, colours) => {
  const indent = code.match(/^\s*/)[0];
  const body = code.slice(indent.length);
  if (!body) return plain(code);

  if (isHeader) return [...plain(indent), ...tok("keyword", body)];

  let m;
  if ((m = body.match(/^(title)(\s+)(.*)$/))) {
    return [...plain(indent), ...tok("keyword", m[1]), ...plain(m[2]), ...tok("title", m[3])];
  }
  if ((m = body.match(/^(principals|participants|participant)(:?)(.*)$/)) && !/^:?\s*\(/.test(m[2] + m[3])) {
    const names = m[3]
      .split(",")
      .flatMap((piece, i) => [...(i ? tok("punct", ",") : []), ...tok("principal", piece, colours.get(piece.trim()))]);
    return [...plain(indent), ...tok("keyword", m[1] + m[2]), ...names];
  }
  if ((m = body.match(/^(>>>)(\s*)([^\s(:]*)(\s*)(?:\(([^)]*)\))?(\s*)(:?)(.*)$/))) {
    const [, marker, s1, id, s2, who, s3, colon, note] = m;
    return [
      ...plain(indent),
      ...tok("region", marker),
      ...plain(s1),
      ...tok("label", id),
      ...plain(s2),
      ...(who !== undefined ? principal(who, colours) : []),
      ...plain(s3),
      ...tok("punct", colon),
      ...tok("note", note),
    ];
  }
  if ((m = body.match(/^(<<<)(.*)$/))) {
    return [...plain(indent), ...tok("region", m[1]), ...tok("label", m[2])];
  }
  return [...plain(indent), ...statement(body, colours)];
};

/**
 * The source's tokens, one array per line. Each line's tokens spell it out
 * exactly, so their lengths give positions.
 *
 * @param {string} text
 * @param {Map<string, string>} colours principal name → colour
 * @returns {Token[][]}
 */
export const tokens = (text, colours) => {
  let seenHeader = false;
  return text.split("\n").map((raw) => {
    const at = commentStart(raw);
    const code = at === -1 ? raw : raw.slice(0, at);
    const isHeader = !seenHeader && code.trim() !== "";
    if (isHeader) seenHeader = true;
    return [...line(code, isHeader, colours), ...(at === -1 ? [] : tok("comment", raw.slice(at)))];
  });
};

const escape = (text) => text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
const html = ({ kind, text, colour }) =>
  kind ? `<span class="tok-${kind}"${colour ? ` style="color:${colour}"` : ""}>${escape(text)}</span>` : escape(text);

/**
 * The source as highlighted HTML.
 *
 * @param {string} text
 * @param {Map<string, string>} colours principal name → colour
 * @param {number | null} errorLine 1-based line to mark, if any
 */
export const highlight = (text, colours, errorLine = null) =>
  tokens(text, colours)
    .map((line, i) => {
      const out = line.map(html).join("");
      return i + 1 === errorLine ? `<span class="err-line">${out || " "}</span>` : out;
    })
    .join("\n") + "\n"; // a trailing newline keeps the last line's height
