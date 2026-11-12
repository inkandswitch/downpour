// <rain-diagram>: an interactive, embeddable rain diagram.
//
//   <script type="module" src="./rain-diagram.js"></script>
//   <rain-diagram><script type="text/rain">
//     rain
//     (Doc)-[edit]->(Alice)
//     (Alice)-<revoke>-x[doc-alice]
//   </script></rain-diagram>
//
// The source goes in a `text/rain` script because HTML would otherwise
// parse `<revoke>` as an element. Plain text content works too, for
// diagrams without revocations.
//
// or <rain-diagram src="./example.rain"></rain-diagram>. Add notation="rain" or
// notation="certflow" to override the header's choice, and theme="light" or
// theme="dark" to override the system's colour scheme.
//
// Hover a principal to see everything it can reach (and the certificates
// that get it there). Click a certificate to preview what revoking it would
// void; click again (or press Escape) to clear.
//
// With the `editable` attribute, rows of a rain diagram can be dragged up
// and down, but never above what they rely on or below what relies on them
// (the band shows where). The diagram rearranges live as a row moves; rows
// that would break it (say, by emptying a region) are skipped. Dropping
// rewrites the source and fires `rain-edit` with `{ source, from, to }`;
// Escape cancels. Click any statement to select it; Delete, Backspace, or
// its 🗑 button removes it (a revocation must go before what it revokes).
//
// All logic lives in the Wasm (`RainDiagram`); this file only wires the
// SVG's data-* hooks to it.

import init, {
  RainDiagram,
  deleteStatement,
  insertStatement,
  movePrincipal,
  moveStatement,
  setPosition,
  setRegionNote,
  setRegionRows,
} from "./pkg/rain_diagram_wasm.js";

const ready = init();

const STYLE = `
  :host { display: block; overflow-x: auto; position: relative; }
  svg .now { cursor: row-resize; }
  :host([editable]) svg .region-edge { cursor: row-resize; }
  :host([editable]) svg[data-row-height] .principal .head, :host([editable]) svg[data-row-height] .principal text { cursor: grab; }
  svg .now-handle { cursor: pointer; }
  .snapshot {
    position: fixed;
    z-index: 2;
    width: min(460px, 90%);
    padding: 8px;
    border: 1px solid rgb(127 127 127 / 0.35);
    border-radius: 8px;
    background: Canvas;
    color: CanvasText;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
  }
  .snapshot svg { display: block; width: 100%; height: auto; max-height: 60vh; }
  .snapshot .caption { font: 12px system-ui, sans-serif; opacity: 0.7; margin: 0 0 4px; }
  :host([editable]) svg .region-tab, :host([editable]) svg .region-caption { cursor: text; }
  .region-edit {
    position: absolute;
    z-index: 3;
    height: 22px;
    padding: 0 7px;
    border: 2px solid #3d7dd8;
    border-radius: 6px;
    background: Canvas;
    color: CanvasText;
    font: 600 12px system-ui, sans-serif;
    outline: none;
    box-shadow: 0 4px 12px rgb(0 0 0 / 0.15);
  }
  svg { display: block; max-width: none; }
  .principal { cursor: default; }
  .delegate { cursor: pointer; }
  svg .event, svg .principal, svg .powerline { transition: opacity 120ms; }
  svg.focus .event:not(.lit), svg.focus .principal:not(.lit), svg.focus .powerline:not(.lit) { opacity: 0.15; }
  svg .fallout .pill, svg .fallout .inner { stroke: #d33 !important; stroke-width: 3; }
  svg .struck line, svg .struck .head { stroke: #d33 !important; fill: #d33 !important; }
  :host([editable]) svg[data-row-height] .event { cursor: grab; }
  svg .dragging { filter: drop-shadow(0 4px 6px rgb(0 0 0 / 0.3)); }
  svg .principal.dragging { opacity: 0.9; }
  :host([editable]) svg .lifeline-hit { cursor: crosshair; }
  svg .draft { stroke: currentColor; stroke-width: 2; stroke-dasharray: 6 4; pointer-events: none; }
  :host(:focus) { outline: none; }
  svg .selected { filter: drop-shadow(0 0 3px #3d7dd8) drop-shadow(0 0 1px #3d7dd8); }
  .menu.trash { padding: 3px; }
  svg .aimed .pill, svg .aimed .head { stroke-width: 3; filter: drop-shadow(0 0 4px rgb(61 125 216 / 0.6)); }
  .menu {
    position: absolute;
    z-index: 4;
    display: grid;
    gap: 4px;
    padding: 6px;
    border: 1px solid rgb(127 127 127 / 0.35);
    border-radius: 8px;
    background: Canvas;
    color: CanvasText;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
    font: 12px system-ui, sans-serif;
  }
  .menu-row { display: flex; align-items: center; gap: 4px; }
  .menu-row span { width: 58px; opacity: 0.7; }
  .menu button { padding: 3px 8px; border: 1px solid rgb(127 127 127 / 0.3); border-radius: 6px; background: transparent; color: inherit; font: inherit; cursor: pointer; }
  .menu button:hover, .menu button:focus-visible { background: rgb(61 125 216 / 0.15); outline: none; }
  :host([grabbing]), :host([grabbing]) * { cursor: grabbing !important; }
  svg .drag-range { fill: #3d7dd8; fill-opacity: 0.08; stroke: #3d7dd8; stroke-opacity: 0.4; stroke-dasharray: 4 3; }
  .error { color: #b3261e; font: 13px ui-monospace, monospace; white-space: pre-wrap; }
`;

export class RainDiagramElement extends HTMLElement {
  #diagram = null;
  #svg = null;
  #pinned = null;
  #generation = 0;
  #text = "";
  #justDragged = false;
  #moment = null; // events above the "now" line; null means all of them
  #snapshot = false; // whether the certflow popover is open

  static observedAttributes = ["notation", "src", "theme", "zoom"];

  constructor() {
    super();
    this.attachShadow({ mode: "open" });
    this.addEventListener("keydown", (e) => {
      if (e.key === "Escape") this.#clear();
      // Delete the selected statement, unless typing (say, in a caption)
      const typing = e.composedPath()[0]?.matches?.("input, textarea");
      if ((e.key === "Delete" || e.key === "Backspace") && !typing && this.#pinned !== null && this.hasAttribute("editable")) {
        e.preventDefault();
        this.#delete(this.#pinned);
      }
    });
    // The snapshot popover is fixed to the viewport; keep it by its handle
    this.addEventListener("scroll", () => this.#popover());
    window.addEventListener("resize", () => this.#popover());
  }

  connectedCallback() {
    // Focusable, so Delete reaches the selected statement
    if (this.hasAttribute("editable") && !this.hasAttribute("tabindex")) this.tabIndex = 0;
    this.#load();
  }

  attributeChangedCallback(name) {
    if (!this.isConnected) return;
    if (name === "theme" || name === "zoom") this.#theme();
    else if (name === "notation") this.#redraw();
    else this.#load();
  }

  /** Replace the source text and re-render. */
  set source(text) {
    ++this.#generation;
    this.#render(text);
  }

  async #load() {
    const src = this.getAttribute("src");
    if (!src && !this.#inline().trim()) return; // waiting for .source
    const generation = ++this.#generation;
    const text = src ? await (await fetch(src)).text() : dedent(this.#inline());
    if (generation === this.#generation) await this.#render(text);
  }

  /** Inline source: a `text/rain` script if there is one, else the text. */
  #inline() {
    return this.querySelector('script[type="text/rain"]')?.textContent ?? this.textContent;
  }

  async #render(text) {
    const generation = this.#generation;
    await ready;
    if (generation !== this.#generation) return; // superseded
    this.#diagram?.free();
    this.#pinned = null;
    this.#text = text;

    try {
      this.#diagram = new RainDiagram(text);
    } catch (e) {
      this.#diagram = null;
      this.shadowRoot.innerHTML = `<style>${STYLE}</style><div class="error"></div>`;
      // HTML swallows `<revoke>` in plain text content; say so
      const hint = this.querySelector("revoke") ? ' (put the source in a <script type="text/rain"> so HTML keeps "<revoke>")' : "";
      this.shadowRoot.querySelector(".error").textContent = `rain-diagram: ${e.message ?? e}${hint}`;
      return;
    }

    this.#redraw();
  }

  #redraw() {
    if (!this.#diagram) return;
    this.#pinned = null;
    // The SVG is generated by the Wasm with every name and note escaped
    this.shadowRoot.innerHTML = `<style>${STYLE}</style>${this.#picture()}`;
    this.#svg = this.shadowRoot.querySelector("svg");
    this.#theme();
    this.#wire();
    this.#popover();
  }

  /** `zoom="1.5"` scales the picture (by its size, so it stays crisp). */
  #zoom() {
    const zoom = Number(this.getAttribute("zoom")) || 1;
    const box = this.#svg?.viewBox?.baseVal;
    if (!box || !box.width) return;
    this.#svg.setAttribute("width", String(box.width * zoom));
    this.#svg.setAttribute("height", String(box.height * zoom));
  }

  /** The picture's size at zoom 1, for fitting it to a container. */
  get naturalSize() {
    const box = this.#svg?.viewBox?.baseVal;
    return box ? { width: box.width, height: box.height } : null;
  }

  /** `theme="light"` or `theme="dark"` overrides the system's choice. */
  #theme() {
    this.#zoom();
    const theme = this.getAttribute("theme");
    this.#svg?.classList.toggle("light", theme === "light");
    this.#svg?.classList.toggle("dark", theme === "dark");
  }

  /** The SVG, in the `notation` attribute's notation, else the header's. */
  #picture() {
    return this.#pictureOf(this.#diagram);
  }

  #pictureOf(diagram) {
    const notation = this.getAttribute("notation") || diagram.kind;
    if (notation === "certflow") return diagram.certflowSvg();
    return diagram.rainSvgAt(this.#happened(diagram));
  }

  /** How many events are above the "now" line, clamped to the diagram. */
  #happened(diagram = this.#diagram) {
    return Math.min(this.#moment ?? Infinity, diagram.events);
  }

  /** The certflow snapshot of the "now" line's moment, by its handle. */
  #popover() {
    this.shadowRoot.querySelector(".snapshot")?.remove();
    const handle = this.#svg?.querySelector(".now-handle");
    if (!this.#snapshot || !handle) return;

    const happened = this.#happened();
    const card = document.createElement("div");
    card.className = "snapshot";
    // Generated by the Wasm with every name escaped
    card.innerHTML = `<div class="caption">Certflow after ${happened} of ${this.#diagram.events} events</div>${this.#diagram.certflowSvgAt(happened)}`;
    const snapshot = card.querySelector("svg");
    const theme = this.getAttribute("theme");
    snapshot?.classList.toggle("light", theme === "light");
    snapshot?.classList.toggle("dark", theme === "dark");
    // Scale by the viewBox to fit the card
    snapshot?.removeAttribute("width");
    snapshot?.removeAttribute("height");

    // Fixed to the viewport, so a scrolling container can't clip it: below
    // the handle if there's room, otherwise above
    const at = handle.getBoundingClientRect();
    card.style.right = `${Math.max(innerWidth - at.right, 8)}px`;
    this.shadowRoot.append(card);
    const height = card.getBoundingClientRect().height;
    if (at.bottom + 6 + height <= innerHeight) {
      card.style.top = `${at.bottom + 6}px`;
    } else {
      card.style.top = `${Math.max(at.top - 6 - height, 8)}px`;
    }
  }

  #wire() {
    const svg = this.#svg;

    for (const principal of svg.querySelectorAll(".principal[data-principal]")) {
      principal.addEventListener("mouseenter", () => {
        if (!this.#pinned) this.#showReach(principal.dataset.principal);
      });
      principal.addEventListener("mouseleave", () => {
        if (!this.#pinned) this.#clear();
      });
    }

    // Click a certificate to see its fallout; when editable, clicking any
    // statement also selects it for deleting
    const editable = this.hasAttribute("editable") && svg.dataset.rowHeight;
    for (const event of svg.querySelectorAll(editable ? ".event[data-event]" : ".delegate[data-event]")) {
      event.addEventListener("click", (e) => {
        e.stopPropagation();
        if (this.#justDragged) {
          this.#justDragged = false;
          return;
        }
        const ix = Number(event.dataset.event);
        if (this.#pinned === ix) {
          this.#clear();
          return;
        }
        if (event.matches(".delegate")) this.#showFallout(ix);
        else {
          this.#clear();
          this.#pinned = ix;
        }
        if (editable) this.#select(event, ix, e);
      });
    }

    svg.addEventListener("click", () => this.#clear());

    if (this.hasAttribute("editable") && svg.dataset.rowHeight) {
      for (const row of svg.querySelectorAll(".event[data-event]")) {
        row.addEventListener("pointerdown", (e) => this.#drag(row, e));
      }
    }

    // Region captions: double-click to edit in place. Region edges: drag.
    // Principal heads (rain): drag sideways to reorder the columns
    if (this.hasAttribute("editable")) {
      if (svg.dataset.rowHeight) {
        for (const hit of svg.querySelectorAll(".lifeline-hit")) {
          hit.addEventListener("pointerdown", (e) => this.#drawFrom(hit, e));
        }
        for (const head of svg.querySelectorAll(".principal[data-x] .head, .principal[data-x] text")) {
          head.addEventListener("pointerdown", (e) => this.#dragPrincipal(head.closest(".principal"), e));
        }
      }
      for (const edge of svg.querySelectorAll(".region-edge")) {
        edge.addEventListener("pointerdown", (e) => this.#dragEdge(edge, e));
      }
      for (const region of svg.querySelectorAll(".region[data-region]")) {
        region.addEventListener("dblclick", (e) => {
          e.stopPropagation();
          this.#editRegion(region);
        });
      }
    }

    // The "now" line: drag it between rows; its handle toggles a certflow
    // snapshot of that moment
    const now = svg.querySelector(".now");
    now?.addEventListener("pointerdown", (e) => this.#dragNow(e));
    svg.querySelector(".now-handle")?.addEventListener("click", (e) => {
      e.stopPropagation();
      if (this.#justDragged) return;
      this.#snapshot = !this.#snapshot;
      this.#popover();
    });
  }

  /**
   * Edit a region's caption over its tab. Enter or clicking away accepts,
   * Escape cancels; the new note is written back into the source.
   */
  #editRegion(group) {
    const tab = group.querySelector(".region-tab") ?? group;
    const id = group.dataset.region;
    const host = this.getBoundingClientRect();
    const at = tab.getBoundingClientRect();

    const input = document.createElement("input");
    input.className = "region-edit";
    input.value = group.dataset.note ?? "";
    input.placeholder = id;
    input.setAttribute("aria-label", `Caption of region ${id}`);
    input.style.left = `${at.left - host.left + this.scrollLeft}px`;
    input.style.top = `${at.top - host.top + this.scrollTop}px`;
    input.style.width = `${Math.max(at.width + 80, 180)}px`;
    this.shadowRoot.append(input);
    input.focus();
    input.select();

    let done = false;
    const finish = (accept) => {
      if (done) return;
      done = true;
      const note = input.value;
      input.remove();
      if (!accept || note === (group.dataset.note ?? "")) return;
      try {
        const source = setRegionNote(this.#text, id, note);
        this.source = source;
        this.dispatchEvent(new CustomEvent("rain-edit", { bubbles: true, detail: { source } }));
      } catch (e) {
        this.dispatchEvent(new CustomEvent("rain-edit-refused", { bubbles: true, detail: { message: e.message ?? String(e) } }));
      }
    };
    input.addEventListener("keydown", (e) => {
      e.stopPropagation(); // keep Escape and friends to ourselves
      if (e.key === "Enter") finish(true);
      else if (e.key === "Escape") finish(false);
    });
    input.addEventListener("blur", () => finish(true));
  }

  /** Re-render a preview of `text` mid-drag (no listeners until it ends). */
  #paint(text) {
    const preview = new RainDiagram(text);
    this.shadowRoot.innerHTML = `<style>${STYLE}</style>${this.#pictureOf(preview)}`;
    preview.free();
    this.#svg = this.shadowRoot.querySelector("svg");
    this.#theme();
  }

  /**
   * Drag from one lifeline to another to delegate or invoke at that moment,
   * or onto a certificate to revoke it: a menu offers what fits on release.
   */
  #drawFrom(hit, down) {
    down.preventDefault();
    down.stopPropagation();
    const svg = this.#svg;
    const from = Number(hit.dataset.column);
    const xs = [...svg.querySelectorAll(".principal[data-x]")].map((h) => Number(h.dataset.x));
    const height = Number(svg.dataset.rowHeight);
    const top = Number(svg.dataset.rowsTop);
    const events = Number(svg.dataset.events);
    const box = svg.getBoundingClientRect();
    const sx = svg.viewBox.baseVal.width / box.width;
    const sy = svg.viewBox.baseVal.height / box.height;
    const point = (e) => ({ x: (e.clientX - box.left) * sx, y: (e.clientY - box.top) * sy });

    // "At that moment": the row boundary nearest where the drag started
    const at = Math.min(Math.max(Math.round((point(down).y - top) / height), 0), events);
    const y = top + at * height;
    const ns = "http://www.w3.org/2000/svg";
    const draft = document.createElementNS(ns, "line");
    draft.setAttribute("class", "draft");
    draft.setAttribute("x1", String(xs[from]));
    draft.setAttribute("y1", String(y));
    draft.setAttribute("x2", String(xs[from]));
    draft.setAttribute("y2", String(y));
    svg.append(draft);

    let target = null; // { kind: "lifeline", to } or { kind: "cert", to }
    const aim = (e) => {
      for (const el of svg.querySelectorAll(".aimed")) el.classList.remove("aimed");
      // Revoke only when dropped on the certificate's box, not just its row
      const under = this.shadowRoot.elementFromPoint(e.clientX, e.clientY);
      const onBox = under?.matches(".pill, .tint, .event.delegate text");
      const cert = onBox ? under.closest(".event.delegate[data-event]") : null;
      if (cert) {
        cert.classList.add("aimed");
        return { kind: "cert", to: Number(cert.dataset.event) };
      }
      const x = point(e).x;
      const nearest = xs.reduce((best, cx, i) => (Math.abs(cx - x) < Math.abs(xs[best] - x) ? i : best), 0);
      if (nearest === from || Math.abs(xs[nearest] - x) > 30) return null;
      svg.querySelectorAll(".principal[data-x]")[nearest]?.classList.add("aimed");
      return { kind: "lifeline", to: nearest };
    };

    this.setAttribute("grabbing", "");
    const move = (e) => {
      target = aim(e);
      const x = target?.kind === "lifeline" ? xs[target.to] : point(e).x;
      draft.setAttribute("x2", String(x));
      draft.setAttribute("y2", String(target?.kind === "cert" ? point(e).y : y));
    };
    const up = (e) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      this.removeAttribute("grabbing");
      draft.remove();
      for (const el of svg.querySelectorAll(".aimed")) el.classList.remove("aimed");
      if (!target) return;
      this.#justDragged = true;
      setTimeout(() => (this.#justDragged = false));
      this.#menu(e, at, from, target);
    };
    const cancel = () => {
      target = null;
      up(new PointerEvent("pointerup"));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
  }

  /** What dragging between lifelines can do, as a little menu at the pointer. */
  #menu(e, at, from, target) {
    this.shadowRoot.querySelector(".menu")?.remove();
    const LEVELS = [
      ["admin", "👑 Admin"],
      ["edit", "✏️ Edit"],
      ["read", "👀 Read"],
      ["relay", "📡 Relay"],
    ];
    const menu = document.createElement("div");
    menu.className = "menu";
    menu.setAttribute("role", "menu");
    const row = (title, options) => {
      const line = document.createElement("div");
      line.className = "menu-row";
      line.append(Object.assign(document.createElement("span"), { textContent: title }));
      for (const [label, run] of options) {
        const button = Object.assign(document.createElement("button"), { textContent: label });
        button.setAttribute("role", "menuitem");
        button.addEventListener("click", () => {
          close();
          try {
            const source = run();
            this.source = source;
            this.dispatchEvent(new CustomEvent("rain-edit", { bubbles: true, detail: { source } }));
          } catch (err) {
            this.dispatchEvent(new CustomEvent("rain-edit-refused", { bubbles: true, detail: { message: err.message ?? String(err) } }));
          }
        });
        line.append(button);
      }
      menu.append(line);
    };

    if (target.kind === "cert") {
      row("Revoke", [["✂️ Revoke", () => insertStatement(this.#text, at, "revoke", from, target.to, "")]]);
    } else {
      row("Delegate", LEVELS.map(([level, label]) => [label, () => insertStatement(this.#text, at, "delegate", from, target.to, level)]));
      row("Invoke", LEVELS.map(([level, label]) => [label, () => insertStatement(this.#text, at, "invoke", from, target.to, level)]));
    }

    const host = this.getBoundingClientRect();
    menu.style.left = `${e.clientX - host.left + this.scrollLeft + 8}px`;
    menu.style.top = `${e.clientY - host.top + this.scrollTop + 8}px`;
    this.shadowRoot.append(menu);
    menu.querySelector("button")?.focus();

    const close = () => {
      menu.remove();
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("keydown", escape, true);
    };
    const outside = (ev) => {
      if (!ev.composedPath().includes(menu)) close();
    };
    const escape = (ev) => {
      if (ev.key === "Escape") {
        ev.preventDefault();
        close();
      }
    };
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("keydown", escape, true);
  }

  /** Drag a principal's head sideways to another column; columns re-render live. */
  #dragPrincipal(group, down) {
    down.preventDefault();
    down.stopPropagation();
    const heads = [...this.#svg.querySelectorAll(".principal[data-x]")];
    const xs = heads.map((h) => Number(h.dataset.x));
    const from = heads.indexOf(group);
    if (from === -1 || xs.length < 2) return;
    const spacing = xs[1] - xs[0];
    const box = this.#svg.getBoundingClientRect();
    const scale = this.#svg.viewBox.baseVal.width / box.width;
    let moved = false;
    let to = from;
    let candidate = null;
    let dx = 0;

    // Keep the held principal (head and lifeline) under the pointer,
    // wherever its column now is; the others have already moved over
    const hold = () => {
      const held = this.#svg.querySelectorAll(".principal[data-x]")[to];
      held?.classList.add("dragging");
      held?.setAttribute("transform", `translate(${dx - (to - from) * spacing} 0)`);
    };

    this.setAttribute("grabbing", "");
    const move = (e) => {
      if (!moved && Math.abs(e.clientX - down.clientX) < 5) return; // a click
      moved = true;
      // Follow the pointer, but no further than the first or last column
      dx = Math.min(Math.max((e.clientX - down.clientX) * scale, -from * spacing), (xs.length - 1 - from) * spacing);
      const at = Math.round(from + dx / spacing);
      if (at !== to) {
        try {
          candidate = at === from ? null : movePrincipal(this.#text, from, at);
        } catch {
          hold();
          return;
        }
        to = at;
        this.#paint(candidate ?? this.#text);
      }
      hold();
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      this.removeAttribute("grabbing");
      if (!moved) return;
      this.#justDragged = true;
      setTimeout(() => (this.#justDragged = false));
      if (candidate === null) {
        this.#redraw();
        return;
      }
      this.source = candidate;
      this.dispatchEvent(new CustomEvent("rain-edit", { bubbles: true, detail: { source: candidate } }));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  }

  /** Drag a region's top or bottom edge to another row boundary. */
  #dragEdge(edge, down) {
    down.preventDefault();
    down.stopPropagation();
    const svg = this.#svg;
    const id = edge.dataset.region;
    const which = edge.dataset.edge;
    const [first0, last0] = [Number(edge.dataset.first), Number(edge.dataset.last)];
    const height = Number(svg.dataset.rowHeight);
    const top = Number(svg.dataset.rowsTop);
    const events = Number(svg.dataset.events);
    const scale = svg.viewBox.baseVal.height / svg.getBoundingClientRect().height;
    const origin = svg.getBoundingClientRect().top;
    let shown = `${first0}-${last0}`;
    let candidate = null;

    this.setAttribute("grabbing", "");
    const move = (e) => {
      const boundary = Math.min(Math.max(Math.round(((e.clientY - origin) * scale - top) / height), 0), events);
      const [first, last] = which === "top" ? [Math.min(boundary, last0), last0] : [first0, Math.max(boundary - 1, first0)];
      if (`${first}-${last}` === shown) return;
      try {
        candidate = setRegionRows(this.#text, id, first, last);
      } catch {
        return; // would break the diagram: keep the last good one
      }
      shown = `${first}-${last}`;
      this.#paint(candidate);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      this.removeAttribute("grabbing");
      this.#justDragged = true;
      setTimeout(() => (this.#justDragged = false));
      if (candidate === null) {
        this.#redraw();
        return;
      }
      this.source = candidate;
      this.dispatchEvent(new CustomEvent("rain-edit", { bubbles: true, detail: { source: candidate } }));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  }

  /** Drag the "now" line, snapping between rows, re-rendering as it goes. */
  #dragNow(down) {
    down.preventDefault();
    down.stopPropagation();
    const svg = this.#svg;
    const height = Number(svg.dataset.rowHeight);
    const top = Number(svg.dataset.rowsTop);
    const events = Number(svg.dataset.events);
    const scale = svg.viewBox.baseVal.height / svg.getBoundingClientRect().height;
    const origin = svg.getBoundingClientRect().top;
    let moved = false;

    this.setAttribute("grabbing", "");
    const move = (e) => {
      if (!moved && Math.abs(e.clientY - down.clientY) < 3) return; // a click
      moved = true;
      const y = (e.clientY - origin) * scale;
      const happened = Math.min(Math.max(Math.round((y - top) / height), 0), events);
      if (happened === this.#happened()) return;
      this.#moment = happened === events ? null : happened;
      this.#redraw();
      this.dispatchEvent(new CustomEvent("rain-moment", { bubbles: true, detail: { happened } }));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      this.removeAttribute("grabbing");
      if (moved) {
        this.#justDragged = true;
        setTimeout(() => (this.#justDragged = false));
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  }

  /**
   * Drag a row. It follows the pointer within the rows it may move to, and
   * each time it crosses into another one, the whole diagram re-renders as
   * it would be after the drop: rows make room, powerlines re-route, and
   * statuses update live. Escape cancels.
   */
  #drag(row, down) {
    down.preventDefault(); // no text selection while dragging
    const from = Number(row.dataset.event);
    const [first, last] = this.#diagram.movable(from);
    const height = Number(this.#svg.dataset.rowHeight);
    const top = Number(this.#svg.dataset.rowsTop);
    const scale = this.#svg.viewBox.baseVal.height / this.#svg.getBoundingClientRect().height;

    let dragging = false;
    let axis = null; // "y" reorders the row; "x" slides its label along the arrow
    let to = from;
    let candidate = this.#text; // the source as it would be after the drop
    let dy = 0;
    let percent = null;

    const paint = (text) => this.#paint(text);

    // Sideways: where along the arrow the pointer is, as a whole percentage
    const [spanFrom, spanTo] = (row.dataset.span ?? "0 0").split(" ").map(Number);
    const slide = (e) => {
      const box = this.#svg.getBoundingClientRect();
      const x = (e.clientX - box.left) * (this.#svg.viewBox.baseVal.width / box.width);
      const along = spanTo === spanFrom ? 0 : (x - spanFrom) / (spanTo - spanFrom);
      const next = Math.round(Math.min(Math.max(along, 0), 1) * 100);
      if (next === percent) return;
      try {
        candidate = setPosition(this.#text, from, next);
      } catch {
        return;
      }
      percent = next;
      paint(candidate);
      this.#svg.querySelector(`.event[data-event="${from}"]`)?.classList.add("dragging");
    };

    // A closed hand from the press until the release, even off the diagram
    this.setAttribute("grabbing", "");
    const bodyCursor = document.body.style.cursor;
    document.body.style.cursor = "grabbing";

    // Keep the held row under the pointer, wherever it now sits
    const hold = () => {
      const held = this.#svg.querySelector(`.event[data-event="${to}"]`);
      held?.classList.add("dragging");
      held?.setAttribute("transform", `translate(0 ${dy - (to - from) * height})`);
    };

    // Re-render with the row moved to `at` (no listeners; it's a preview)
    const show = (at) => {
      let text = this.#text;
      if (at !== from) {
        try {
          text = moveStatement(this.#text, from, at);
        } catch {
          return; // would break the diagram (say, empty a region): skip it
        }
      }
      to = at;
      candidate = text;
      paint(text);

      const band = document.createElementNS("http://www.w3.org/2000/svg", "rect");
      band.setAttribute("class", "drag-range");
      band.setAttribute("x", "4");
      band.setAttribute("width", String(this.#svg.viewBox.baseVal.width - 8));
      band.setAttribute("y", String(top + first * height));
      band.setAttribute("height", String((last - first + 1) * height));
      this.#svg.querySelector(".powerlines")?.before(band);
      hold();
    };

    const move = (e) => {
      const raw = (e.clientY - down.clientY) * scale;
      const across = e.clientX - down.clientX;
      if (!dragging && Math.hypot(across, e.clientY - down.clientY) < 5) return; // a click
      if (!dragging) {
        dragging = true;
        // Whichever way it first moves decides what the drag does
        axis = Math.abs(across) > Math.abs(e.clientY - down.clientY) ? "x" : "y";
        if (axis === "y") show(from);
      }
      if (axis === "x") {
        slide(e);
        return;
      }
      dy = Math.min(Math.max(raw, (first - from) * height), (last - from) * height);
      const at = Math.round(from + dy / height);
      if (at === to) hold();
      else show(at);
    };

    const finish = (commit) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", escape);
      this.removeAttribute("grabbing");
      document.body.style.cursor = bodyCursor;
      if (!dragging) return; // a click, handled elsewhere
      // Swallow the click that follows this pointerup, and only that one
      this.#justDragged = true;
      setTimeout(() => (this.#justDragged = false));

      const changed = axis === "x" ? percent !== null : to !== from;
      if (!commit || !changed) {
        this.#redraw();
        return;
      }
      this.source = candidate;
      this.dispatchEvent(new CustomEvent("rain-edit", { bubbles: true, detail: { source: candidate, from, to } }));
    };
    const up = () => finish(true);
    const cancel = () => finish(false);
    const escape = (e) => e.key === "Escape" && cancel();

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", escape);
  }

  #showReach(key) {
    const { subjects, events } = JSON.parse(this.#diagram.reach(key));
    const lit = new Set(events.map(String));
    const keys = new Set([key, ...subjects.map((s) => s.key)]);

    this.#focus((el) => {
      if (el.matches(".principal")) return keys.has(el.dataset.principal);
      if (el.matches(".powerline")) return lit.has(el.dataset.to) && (!el.dataset.from || lit.has(el.dataset.from));
      return lit.has(el.dataset.event);
    });
    this.dispatchEvent(new CustomEvent("rain-reach", { detail: { principal: key, subjects } }));
  }

  #showFallout(ix) {
    this.#clear();
    this.#pinned = ix;
    // Array.from: a Uint32Array.map(String) would coerce back to numbers
    const fallout = new Set(Array.from(this.#diagram.dependents(ix), String));

    for (const el of this.#svg.querySelectorAll(".event[data-event]")) {
      el.classList.toggle("fallout", fallout.has(el.dataset.event));
      el.classList.toggle("struck", el.dataset.event === String(ix));
    }
    this.dispatchEvent(new CustomEvent("rain-fallout", { detail: { event: ix, voids: [...fallout].map(Number) } }));
  }

  #focus(isLit) {
    this.#svg.classList.add("focus");
    for (const el of this.#svg.querySelectorAll(".principal, .event, .powerline")) {
      el.classList.toggle("lit", isLit(el));
    }
  }

  /** Mark a statement selected, with a 🗑 button by the pointer. */
  #select(event, ix, e) {
    event.classList.add("selected");
    const trash = Object.assign(document.createElement("button"), { textContent: "🗑 Delete", title: "Delete (⌫)" });
    trash.addEventListener("click", () => this.#delete(ix));
    const menu = Object.assign(document.createElement("div"), { className: "menu trash" });
    menu.append(trash);
    const host = this.getBoundingClientRect();
    menu.style.left = `${e.clientX - host.left + this.scrollLeft + 8}px`;
    menu.style.top = `${e.clientY - host.top + this.scrollTop + 8}px`;
    this.shadowRoot.append(menu);
    this.focus({ preventScroll: true });
  }

  /** Delete statement `ix` from the source, or say why not. */
  #delete(ix) {
    try {
      const source = deleteStatement(this.#text, ix);
      this.source = source;
      this.dispatchEvent(new CustomEvent("rain-edit", { bubbles: true, detail: { source } }));
    } catch (err) {
      this.dispatchEvent(new CustomEvent("rain-edit-refused", { bubbles: true, detail: { message: err.message ?? String(err) } }));
    }
  }

  #clear() {
    this.#pinned = null;
    this.shadowRoot.querySelector(".trash")?.remove();
    for (const el of this.#svg?.querySelectorAll(".selected") ?? []) el.classList.remove("selected");
    this.#svg?.classList.remove("focus");
    for (const el of this.#svg?.querySelectorAll(".lit, .fallout, .struck") ?? []) {
      el.classList.remove("lit", "fallout", "struck");
    }
  }
}

/** Strip the common leading indentation from inline source. */
const dedent = (text) => {
  const lines = text.replace(/^\n+|\s+$/g, "").split("\n");
  const indent = Math.min(...lines.filter((l) => l.trim()).map((l) => l.match(/^\s*/)[0].length));
  return lines.map((l) => l.slice(indent)).join("\n");
};

customElements.define("rain-diagram", RainDiagramElement);
