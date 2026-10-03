// IPC for Settings > Workflows and Gallery. Like the other wrappers, these run
// in a plain browser (`npm run dev`) against the in-memory data of `mock.ts`.

import { invoke } from "@tauri-apps/api/core";
import { hasTauri } from "../ipc";
import type { Problem, Workflow } from "./model";

export type Result<T> = { ok: true; value: T } | { ok: false; error: string };

export interface HotkeyError {
  key: string;
  error: string;
}

export interface WorkflowSummary {
  folder: string;
  name: string;
  description: string;
  author: string;
  version: string;
  enabled: boolean;
  /** It runs code, so it waits for the user's Allow. */
  needs_approval: boolean;
  approved: boolean;
  keywords: string[];
  nodes: number;
  /** All warnings, keyword clashes included. */
  warnings: number;
  /** The keyword clashes among them, as sentences. */
  keyword_warnings: string[];
  /** Why it cannot load. */
  error: string | null;
  hotkey_errors: HotkeyError[];
}

export interface WorkflowList {
  /** The folder workflows live in. */
  folder: string;
  workflows: WorkflowSummary[];
}

export interface LoadedWorkflow {
  folder: string;
  workflow: Workflow;
  problems: Problem[];
}

export interface SavedWorkflow {
  folder: string;
  problems: Problem[];
}

export interface TemplateInfo {
  id: string;
  name: string;
  description: string;
}

export type GalleryKind = "workflow" | "plugin";

export interface GalleryEntry {
  id: string;
  kind: GalleryKind;
  name: string;
  description: string;
  author: string;
  version: string;
  source: string;
  sha256: string;
  homepage?: string | null;
  folder?: string | null;
  installed: boolean;
}

export interface Gallery {
  /** The address the index was fetched from. */
  source: string;
  name: string;
  entries: GalleryEntry[];
  skipped: number;
}

export interface InstalledEntry {
  id: string;
  kind: GalleryKind;
  folder: string;
}

const preview = () => import.meta.env.DEV && !hasTauri();

function errorText(err: unknown): string {
  return typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
}

async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<Result<T>> {
  try {
    return { ok: true, value: await invoke<T>(command, args) };
  } catch (err) {
    console.warn(`[ipc] ${command} failed:`, err);
    return { ok: false, error: errorText(err) };
  }
}

/** The preview data. A production build drops this import, so none of it ships. */
async function mock() {
  if (import.meta.env.DEV) return import("./mock");
  throw new Error("preview data is only available in development");
}

export async function listWorkflows(): Promise<Result<WorkflowList>> {
  if (preview()) return { ok: true, value: (await mock()).list() };
  return call("list_workflows");
}

export async function loadWorkflow(folder: string): Promise<Result<LoadedWorkflow>> {
  if (preview()) return (await mock()).load(folder);
  return call("load_workflow", { folder });
}

/**
 * Live validation for the builder; nothing is saved. `folder` is where the
 * workflow is saved (or `null`), so its own keywords are not reported as clashes.
 */
export async function checkWorkflow(
  workflow: Workflow,
  folder: string | null,
): Promise<Result<Problem[]>> {
  if (preview()) return { ok: true, value: (await mock()).check(workflow) };
  return call("check_workflow", { workflow, folder });
}

/** Saves into `folder`, or into a new folder when it is `null`. */
export async function saveWorkflow(
  folder: string | null,
  workflow: Workflow,
): Promise<Result<SavedWorkflow>> {
  if (preview()) return (await mock()).save(folder, workflow);
  return call("save_workflow", { folder, workflow });
}

export async function workflowTemplates(): Promise<Result<TemplateInfo[]>> {
  if (preview()) return { ok: true, value: (await mock()).templates() };
  return call("workflow_templates");
}

export async function createWorkflow(template: string): Promise<Result<SavedWorkflow>> {
  if (preview()) return (await mock()).create(template);
  return call("create_workflow", { template });
}

/** Resolves to whether it was deleted (the user may decline in the dialog). */
export async function deleteWorkflow(folder: string): Promise<Result<boolean>> {
  if (preview()) return (await mock()).remove(folder);
  return call("delete_workflow", { folder });
}

export async function setWorkflowEnabled(folder: string, enabled: boolean): Promise<Result<null>> {
  if (preview()) return (await mock()).setEnabled(folder, enabled);
  return call("set_workflow_enabled", { folder, enabled });
}

/** Shows the Allow dialog for one workflow; resolves to whether it was allowed. */
export async function reviewWorkflow(folder: string): Promise<Result<boolean>> {
  if (preview()) return (await mock()).review(folder);
  return call("review_workflow", { folder });
}

export async function openWorkflowsFolder(): Promise<Result<null>> {
  if (preview()) return { ok: true, value: null };
  return call("open_workflows_folder");
}

/** Downloads the gallery index: only ever called by the "Load gallery" button. */
export async function loadGallery(): Promise<Result<Gallery>> {
  if (preview()) return (await mock()).gallery();
  return call("gallery_load");
}

/** Installs one entry of the loaded gallery: only ever called by "Install". */
export async function installGalleryEntry(id: string): Promise<Result<InstalledEntry>> {
  if (preview()) return (await mock()).install(id);
  return call("gallery_install", { id });
}
