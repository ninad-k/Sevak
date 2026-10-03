import type { AppearanceCss } from "./ipc";

const APPEARANCE_ID = "sevak-appearance";
const CUSTOM_ID = "sevak-custom-css";

/** Sets (or removes, when empty) the `<style>` element `id`, appended last in `<head>`. */
function setSheet(id: string, css: string): void {
  let el = document.getElementById(id) as HTMLStyleElement | null;
  if (!css) {
    el?.remove();
    return;
  }
  if (!el) {
    el = document.createElement("style");
    el.id = id;
    document.head.appendChild(el);
  }
  if (el.textContent !== css) el.textContent = css;
}

/**
 * Applies the accent, font, radius and opacity settings, then the user's own
 * stylesheet. Both are injected after the built-in theme (app.css) and in this
 * order, so a plain `:root { ... }` in the custom stylesheet overrides both.
 * Rust has already validated and sanitized everything.
 */
export function applyAppearance(appearance: AppearanceCss | null | undefined): void {
  if (!appearance) return;
  setSheet(APPEARANCE_ID, appearance.css);
  setSheet(CUSTOM_ID, appearance.custom_css);
}
