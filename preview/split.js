// Draggable splitters. Each one sets a CSS custom property (in pixels) on a
// container. Sizes are remembered per viewer, arrow keys nudge a focused
// splitter (shift for bigger steps), and a double-click resets it.

/**
 * @param {HTMLElement} handle   the role="separator" element
 * @param {object} options
 * @param {HTMLElement} options.container  where the property is set
 * @param {string} options.property        e.g. "--source-width"
 * @param {"x" | "y"} options.axis
 * @param {() => number} options.origin    container edge the size is measured from
 * @param {number} options.min
 * @param {() => number} options.max
 * @param {string} options.key             storage key
 */
export const splitter = (handle, { container, property, axis, origin, min, max, key }) => {
  const apply = (px) => {
    const size = Math.round(Math.min(Math.max(px, min), max()));
    container.style.setProperty(property, `${size}px`);
    handle.setAttribute("aria-valuenow", String(size));
    return size;
  };

  const current = () => parseFloat(getComputedStyle(container).getPropertyValue(property)) || null;

  const save = (size) => {
    try {
      localStorage.setItem(key, String(size));
    } catch {}
  };

  const reset = () => {
    container.style.removeProperty(property);
    handle.removeAttribute("aria-valuenow");
    try {
      localStorage.removeItem(key);
    } catch {}
  };

  try {
    const saved = Number(localStorage.getItem(key));
    if (saved > 0) apply(saved);
  } catch {}

  handle.addEventListener("pointerdown", (down) => {
    down.preventDefault();
    // Moves are tracked on the window, so capture is a nicety
    try {
      handle.setPointerCapture(down.pointerId);
    } catch {}
    handle.classList.add("dragging");
    document.body.classList.add("resizing");

    let size = null;
    const move = (e) => {
      size = apply((axis === "x" ? e.clientX : e.clientY) - origin());
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      handle.classList.remove("dragging");
      document.body.classList.remove("resizing");
      if (size !== null) save(size);
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  });

  handle.addEventListener("dblclick", reset);

  handle.addEventListener("keydown", (e) => {
    const step = e.shiftKey ? 64 : 16;
    const delta = { ArrowLeft: -step, ArrowUp: -step, ArrowRight: step, ArrowDown: step }[e.key];
    if (delta === undefined) return;
    e.preventDefault();
    const fallback = axis === "x" ? handle.getBoundingClientRect().left - origin() : handle.getBoundingClientRect().top - origin();
    save(apply((current() ?? fallback) + delta));
  });
};
