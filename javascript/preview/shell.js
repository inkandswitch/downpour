// The page around a diagram: the picture and its status and warnings, the
// Rain / Certflow tabs, the export pane, zoom, the theme toggle, splitters,
// and maximize. Shared by the preview (main.js) and the collaborative editor
// (../editor/), which differ only in how the source is edited.

import { PALETTE, commentStart } from "./highlight.js";
import { splitter } from "./split.js";

/**
 * @param {object} options
 * @param {string} options.prefix       storage key prefix, e.g. "rain-preview"
 * @param {() => string} options.text   the current source
 * @param {object} options.wasm         { diagnostics, renderSvg, renderText, toSiteJson }
 */
export const shell = ({ prefix, text, wasm }) => {
  const $ = (id) => document.getElementById(id);
  const status = $("status");
  const warnings = $("warnings");
  const output = $("output");
  const picture = $("picture");

  // Storage is a per-viewer convenience; the page works without it
  const load = (key, fallback) => {
    try {
      return localStorage.getItem(`${prefix}:${key}`) ?? fallback;
    } catch {
      return fallback;
    }
  };
  const save = (key, value) => {
    try {
      localStorage.setItem(`${prefix}:${key}`, value);
    } catch {}
  };

  const FORMATS = {
    svg: { file: "diagram.svg", type: "image/svg+xml", render: wasm.renderSvg },
    json: { file: "diagram.json", type: "application/json", render: (s) => JSON.stringify(JSON.parse(wasm.toSiteJson(s)), null, 2) },
    text: { file: "diagram.txt", type: "text/plain", render: wasm.renderText },
  };
  let format = FORMATS[load("format", "text")] ? load("format", "text") : "text";

  // Principals in their diagram colours, kept from the last good parse while
  // the text is broken, so colours don't flicker as you type
  let colours = new Map();

  /**
   * Redraw from the current source. Text that doesn't parse leaves the last
   * good diagram up and reports the error instead, so a half-typed line
   * never blanks the picture.
   *
   * @returns {{ errorLine: number | null, colours: Map<string, string> }}
   */
  const render = () => {
    const source = text();
    try {
      const site = JSON.parse(wasm.toSiteJson(source));
      colours = new Map(site.agents.map((agent, i) => [agent.label, PALETTE[i % PALETTE.length]]));
      const warned = wasm.diagnostics(source);
      picture.source = source;
      syncNotation();
      output.textContent = FORMATS[format].render(source);
      status.className = "ok";
      status.textContent = `✓ ${site.agents.length} principals, ${site.events.length} events`;
      warnings.replaceChildren(...warned.map((w) => Object.assign(document.createElement("li"), { textContent: w })));
      return { errorLine: null, colours };
    } catch (e) {
      const message = String(e.message ?? e);
      status.className = "err";
      status.textContent = `✗ ${message}`;
      return { errorLine: Number(message.match(/^(\d+):/)?.[1]) || null, colours };
    }
  };

  const select = (next) => {
    format = next;
    save("format", next);
    for (const tab of document.querySelectorAll("[data-format]")) {
      tab.setAttribute("aria-selected", String(tab.dataset.format === next));
    }
    render();
  };
  for (const tab of document.querySelectorAll("[data-format]")) {
    tab.addEventListener("click", () => select(tab.dataset.format));
  }

  // The notation the header names, unless the other tab was clicked. The
  // override lasts until the header changes.
  const header = () =>
    text()
      .split("\n")
      .map((line) => {
        const comment = commentStart(line);
        return (comment === -1 ? line : line.slice(0, comment)).trim();
      })
      .find(Boolean) === "certflow"
      ? "certflow"
      : "rain";
  let override = null; // { notation, header }
  const syncNotation = () => {
    const written = header();
    const shown = override?.header === written ? override.notation : written;
    picture.setAttribute("notation", shown);
    for (const tab of document.querySelectorAll("[data-notation]")) {
      tab.setAttribute("aria-selected", String(tab.dataset.notation === shown));
    }
  };
  for (const tab of document.querySelectorAll("[data-notation]")) {
    tab.addEventListener("click", () => {
      const written = header();
      override = tab.dataset.notation === written ? null : { notation: tab.dataset.notation, header: written };
      syncNotation();
    });
  }

  const main = document.querySelector("main");
  const pane = $("output-pane");
  splitter($("columns"), {
    container: main,
    property: "--source-width",
    axis: "x",
    origin: () => main.getBoundingClientRect().left + 20 + 8,
    min: 200,
    max: () => main.clientWidth - 40 - 16 - 240,
    key: `${prefix}:source-width`,
  });
  splitter($("rows"), {
    container: pane,
    property: "--picture-height",
    axis: "y",
    origin: () => picture.getBoundingClientRect().top + 6,
    min: 60,
    max: () => pane.getBoundingClientRect().bottom - picture.getBoundingClientRect().top - 120,
    key: `${prefix}:picture-height`,
  });

  picture.addEventListener("rain-edit-refused", ({ detail }) => {
    status.className = "err";
    status.textContent = `✗ ${detail.message}`;
  });

  // System, light, or dark, per viewer. The page and the diagram both follow it.
  const theme = (choice) => {
    save("theme", choice);
    if (choice === "light" || choice === "dark") {
      document.documentElement.dataset.theme = choice;
      picture.setAttribute("theme", choice);
    } else {
      delete document.documentElement.dataset.theme;
      picture.removeAttribute("theme");
    }
    for (const button of document.querySelectorAll("[data-theme-choice]")) {
      button.setAttribute("aria-pressed", String(button.dataset.themeChoice === choice));
    }
  };
  for (const button of document.querySelectorAll("[data-theme-choice]")) {
    button.addEventListener("click", () => theme(button.dataset.themeChoice));
  }
  theme(load("theme", "system"));

  // Zoom, remembered separately for rain and certflow
  const ZOOMS = [0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4];
  const zoomKey = () => `zoom:${picture.getAttribute("notation") ?? "rain"}`;
  const setZoom = (zoom) => {
    const z = Math.min(Math.max(zoom, ZOOMS[0]), ZOOMS.at(-1));
    picture.setAttribute("zoom", String(z));
    $("zoom-level").textContent = `${Math.round(z * 100)}%`;
    save(zoomKey(), String(z));
  };
  const currentZoom = () => Number(picture.getAttribute("zoom")) || 1;
  const step = (direction) => {
    const z = currentZoom();
    const next = direction > 0 ? ZOOMS.find((s) => s > z + 0.001) : ZOOMS.findLast((s) => s < z - 0.001);
    setZoom(next ?? z);
  };
  const fit = () => {
    const size = picture.naturalSize;
    if (!size) return;
    const room = { width: picture.clientWidth - 24, height: picture.clientHeight - 24 };
    setZoom(Math.min(room.width / size.width, room.height / size.height));
  };
  $("zoom-in").addEventListener("click", () => step(1));
  $("zoom-out").addEventListener("click", () => step(-1));
  $("zoom-level").addEventListener("click", () => setZoom(1));
  $("zoom-fit").addEventListener("click", fit);
  picture.addEventListener(
    "wheel",
    (e) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      setZoom(currentZoom() * Math.exp(-e.deltaY * 0.002));
    },
    { passive: false },
  );
  // Each notation keeps its own zoom
  new MutationObserver(() => setZoom(Number(load(zoomKey(), "1")) || 1)).observe(picture, {
    attributes: true,
    attributeFilter: ["notation"],
  });

  // Maximize one pane (source, output, diagram, or export); again to restore
  const maximize = (which) => {
    const next = main.dataset.max === which ? null : which;
    if (next) main.dataset.max = next;
    else delete main.dataset.max;
    save("maximized", next ?? "");
    for (const button of document.querySelectorAll(".maximize")) {
      const on = button.dataset.pane === next;
      button.textContent = on ? "⤡" : "⤢";
      button.title = on ? "Restore" : "Maximize";
      button.setAttribute("aria-pressed", String(on));
    }
  };
  for (const button of document.querySelectorAll(".maximize")) {
    button.addEventListener("click", () => maximize(button.dataset.pane));
  }
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && main.dataset.max && !e.defaultPrevented) maximize(main.dataset.max);
  });
  const remembered = load("maximized", "");
  if (remembered) maximize(remembered);

  $("copy").addEventListener("click", async () => {
    await navigator.clipboard.writeText(output.textContent);
    $("copy").textContent = "Copied";
    setTimeout(() => ($("copy").textContent = "Copy"), 1200);
  });
  $("download").addEventListener("click", () => {
    const { file, type } = FORMATS[format];
    const url = URL.createObjectURL(new Blob([output.textContent], { type }));
    Object.assign(document.createElement("a"), { href: url, download: file }).click();
    URL.revokeObjectURL(url);
  });

  return {
    load,
    save,
    render,
    /** Render for the first time, with the remembered export format. */
    start: () => select(format),
  };
};
