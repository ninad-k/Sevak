// IPC for Settings > Help: the diagnostics report. Like the other wrappers it
// runs in a plain browser (`npm run dev`), with a made-up report.

import { invoke } from "@tauri-apps/api/core";
import { hasTauri } from "./ipc";

export type Result<T> = { ok: true; value: T } | { ok: false; error: string };

const preview = () => import.meta.env.DEV && !hasTauri();

function errorText(err: unknown): string {
  return typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
}

const PREVIEW_REPORT = `# Sevak diagnostics

Made 2026-10-04 12:00:00 UTC by Settings → Help → Copy diagnostics.

> Made on this computer. **Nothing was sent anywhere**: this text exists only here until you copy it or save it. Read it before you share it.

## Overview

- **Sevak:** 0.1.0, commit 9f3e2d1ab (release build for x86_64-pc-windows-msvc)
- **Operating system:** Windows 11 Pro 24H2 (build 26100.2314), x86_64
- **Shortcut:** Super+Space (taken over with the Windows keyboard hook)

## Health checks

- **OK** The config file parses.
- **OK** The global shortcut is registered. Super+Space (taken over with the Windows keyboard hook)
- **OK** The tray icon was created.

## Logs

1 log file (sevak.2026-10-04.log 3.9 KB). In the recent logs: **0 errors, 1 warning, 0 panics**.
`;

/** The report for the running app, as text. */
export async function getDiagnostics(): Promise<Result<string>> {
  if (preview()) return { ok: true, value: PREVIEW_REPORT };
  try {
    return { ok: true, value: await invoke<string>("get_diagnostics") };
  } catch (err) {
    return { ok: false, error: errorText(err) };
  }
}

/** Puts the text on the clipboard. */
export async function copyDiagnostics(text: string): Promise<Result<null>> {
  if (preview()) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      /* the preview has no clipboard permission; pretend it worked */
    }
    return { ok: true, value: null };
  }
  try {
    await invoke("copy_diagnostics", { text });
    return { ok: true, value: null };
  } catch (err) {
    return { ok: false, error: errorText(err) };
  }
}

/** Asks where to save the text; resolves to where it went, or `null` if cancelled. */
export async function saveDiagnostics(text: string): Promise<Result<string | null>> {
  if (preview()) return { ok: true, value: "~/sevak-diagnostics.md" };
  try {
    return { ok: true, value: await invoke<string | null>("save_diagnostics", { text }) };
  } catch (err) {
    return { ok: false, error: errorText(err) };
  }
}
