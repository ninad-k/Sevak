import type { ThemeSetting } from "./ipc";

/**
 * Light and dark are pinned with `data-theme`; `system` removes it so the
 * `prefers-color-scheme` media query in app.css decides.
 */
export function applyTheme(theme: ThemeSetting | null | undefined): void {
  const root = document.documentElement;
  if (theme === "light" || theme === "dark") root.dataset.theme = theme;
  else delete root.dataset.theme;
}
