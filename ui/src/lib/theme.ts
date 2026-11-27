import type { SettingsThemeSetting, ThemeSetting } from "./ipc";

/**
 * Light and dark are pinned with `data-theme`; `system` removes it so the
 * `prefers-color-scheme` media query in app.css decides.
 */
export function applyTheme(theme: ThemeSetting | null | undefined): void {
  const root = document.documentElement;
  if (theme === "light" || theme === "dark") root.dataset.theme = theme;
  else delete root.dataset.theme;
}

/**
 * The mode the Settings window uses: `follow` is the launcher's own `theme`,
 * anything else overrides it for the Settings window only.
 */
export function resolveSettingsTheme(
  settingsTheme: SettingsThemeSetting | null | undefined,
  launcherTheme: ThemeSetting,
): ThemeSetting {
  if (settingsTheme === "light" || settingsTheme === "dark" || settingsTheme === "system") {
    return settingsTheme;
  }
  return launcherTheme;
}
