// IPC of the theme editor and gallery. Like settings-ipc.ts, every wrapper
// tolerates running in a plain browser (`npm run dev`), where a small in-memory
// stand-in plays the backend so the editor can be tried out.

import { invoke } from "@tauri-apps/api/core";
import { hasTauri } from "./ipc";
import type { StoredTheme, ThemeGallery, ThemeSpec, ThemesDto } from "./themes";

const preview = () => import.meta.env.DEV && !hasTauri();

function errorText(err: unknown): string {
  return typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
}

export type Result<T> = { ok: true; value: T } | { ok: false; error: string };

async function call<T>(command: string, args?: Record<string, unknown>): Promise<Result<T>> {
  try {
    return { ok: true, value: await invoke<T>(command, args) };
  } catch (err) {
    return { ok: false, error: errorText(err) };
  }
}

export async function listThemes(): Promise<ThemesDto | null> {
  if (preview()) return (await import("./mock-themes")).mockThemes();
  const result = await call<ThemesDto>("list_themes");
  if (!result.ok) console.warn("[ipc] list_themes failed:", result.error);
  return result.ok ? result.value : null;
}

/** Writes `themes/<name>.toml`. */
export async function saveTheme(theme: ThemeSpec): Promise<Result<StoredTheme>> {
  if (preview()) return (await import("./mock-themes")).mockSaveTheme(theme);
  return call<StoredTheme>("save_theme", { theme });
}

/** Writes a built-in theme to the themes folder (unless it is there) and returns its file. */
export async function useBuiltinTheme(name: string): Promise<Result<StoredTheme>> {
  if (preview()) return (await import("./mock-themes")).mockUseBuiltin(name);
  return call<StoredTheme>("use_builtin_theme", { name });
}

/** Native file picker; `null` when cancelled. */
export async function importTheme(): Promise<Result<StoredTheme | null>> {
  if (preview()) return { ok: false, error: "Importing needs the native file dialog (not available in the browser preview)." };
  return call<StoredTheme | null>("import_theme");
}

/** Native save dialog; `false` when cancelled. */
export async function exportTheme(file: string): Promise<Result<boolean>> {
  if (preview()) return { ok: false, error: "Exporting needs the native file dialog (not available in the browser preview)." };
  return call<boolean>("export_theme", { file });
}

export async function openThemesDir(): Promise<string | null> {
  if (preview()) return null;
  const result = await call<void>("open_themes_dir");
  return result.ok ? null : result.error;
}

/** **The one request the editor makes**: the gallery index. Only call it on a click. */
export async function fetchThemeGallery(): Promise<Result<ThemeGallery>> {
  if (preview()) return (await import("./mock-themes")).mockGallery();
  return call<ThemeGallery>("fetch_theme_gallery");
}

/**
 * Downloads, verifies the SHA-256 and installs gallery theme `id`. A theme of the
 * same name is replaced only when `replace` is true ("Reinstall").
 */
export async function installGalleryTheme(id: string, replace = false): Promise<Result<StoredTheme>> {
  if (preview()) return (await import("./mock-themes")).mockInstallGallery(id, replace);
  return call<StoredTheme>("install_gallery_theme", { id, replace });
}
