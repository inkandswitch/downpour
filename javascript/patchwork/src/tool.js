// The editor as a Patchwork tool: `(handle, element) => cleanup`. Patchwork
// brings the repo and its sync, so this is the editor alone, in a shadow
// root (Patchwork tools share the light DOM), in Patchwork's theme, under
// the account's name.

import { mountEditor } from "@inkandswitch/rain-editor/mount";

// Patchwork's theme variables in for the editor's own (a dark or light
// theme still sets `data-theme`, for the colours Patchwork doesn't name)
const THEME = `
:host([data-theme]) {
  --bg: var(--editor-fill, #fbfaf7);
  --panel: var(--editor-fill, #ffffff);
  --fg: var(--editor-line, #1d1c1a);
  --muted: var(--editor-line-offset-50, color-mix(in oklch, var(--editor-line, #1d1c1a), var(--editor-fill, #fbfaf7) 45%));
  --border: var(--editor-fill-offset-20, color-mix(in oklch, var(--editor-fill, #fbfaf7), var(--editor-line, #1d1c1a) 15%));
  --mono: var(--editor-family-code, ui-monospace, monospace);
}
`;

/** Whether Patchwork's editor surface is dark, by its fill's lightness. */
const isDark = (element) => {
  const probe = element.appendChild(document.createElement("span"));
  probe.style.color = "var(--editor-fill, white)";
  const [r, g, b] = getComputedStyle(probe).color.match(/\d+(\.\d+)?/g).map(Number);
  probe.remove();
  return 0.2126 * r + 0.7152 * g + 0.0722 * b < 128;
};

const accountName = async () => {
  try {
    const contact = await window.repo.find(window.accountDocHandle.doc().contactUrl);
    return contact.doc()?.name || undefined;
  } catch {
    return undefined;
  }
};

export default function RainTool(handle, element) {
  let editor = null;
  let gone = false;

  const follow = () => {
    if (!editor) return;
    const theme = isDark(element) ? "dark" : "light";
    editor.host.dataset.theme = theme;
    editor.host.shadowRoot.querySelector("#picture")?.setAttribute("theme", theme);
  };
  // Patchwork switches themes with `theme` on <html>; the system with its scheme
  const themes = new MutationObserver(follow);
  themes.observe(document.documentElement, { attributes: true, attributeFilter: ["theme", "class", "style"] });
  const scheme = matchMedia("(prefers-color-scheme: dark)");
  scheme.addEventListener("change", follow);

  (async () => {
    const mounted = await mountEditor(element, handle, {
      shadow: true,
      styles: THEME,
      name: await accountName(),
      prefix: "rain-patchwork",
    });
    if (gone) return mounted.unmount();
    editor = mounted;
    follow();
  })();

  return () => {
    gone = true;
    themes.disconnect();
    scheme.removeEventListener("change", follow);
    editor?.unmount();
  };
}
