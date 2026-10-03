// Client-side checks for the settings form. They mirror `settings::validate` in
// the Rust shell, which re-checks everything on save; the shortcut itself is
// parsed by Rust only (see `validateHotkey`).

import type { Config, WebSearchEngine } from "./settings-ipc";

export type SectionId = "general" | "appearance" | "search" | "plugins" | "web" | "files" | "linux";

export interface EngineErrors {
  keyword?: string;
  name?: string;
  url?: string;
}

export interface Problems {
  engines: EngineErrors[];
  fallback?: string;
  filesKeyword?: string;
  filesDepth?: string;
  /** Number of problems per section (hotkey problems are added by the form). */
  count: Record<SectionId, number>;
}

export const MIN_WIDTH = 400;
export const MAX_WIDTH = 1600;
export const MAX_DEPTH = 32;

export function engineErrors(engine: WebSearchEngine, all: WebSearchEngine[]): EngineErrors {
  const errors: EngineErrors = {};
  const keyword = engine.keyword.trim();
  if (keyword === "") errors.keyword = "Required";
  else if (/\s/.test(keyword)) errors.keyword = "No spaces";
  else if (all.filter((other) => other.keyword.trim().toLowerCase() === keyword.toLowerCase()).length > 1) {
    errors.keyword = "Already used";
  }
  if (engine.name.trim() === "") errors.name = "Required";
  const url = engine.url.trim();
  if (!/^https?:\/\//i.test(url)) errors.url = "Must start with http:// or https://";
  else if (!engine.url.includes("{query}")) errors.url = "Must contain {query}";
  return errors;
}

export function validate(config: Config): Problems {
  const count: Record<SectionId, number> = {
    general: 0,
    appearance: 0,
    search: 0,
    plugins: 0,
    web: 0,
    files: 0,
    linux: 0,
  };

  const engines = config.web_search.map((engine) => engineErrors(engine, config.web_search));
  for (const errors of engines) count.web += Object.keys(errors).length;

  const keywords = new Set(config.web_search.map((engine) => engine.keyword.trim().toLowerCase()));
  const problems: Problems = { engines, count };

  const fallback = config.search.fallback_web_search.trim();
  if (fallback !== "" && !keywords.has(fallback.toLowerCase())) {
    problems.fallback = `The engine "${fallback}" no longer exists.`;
    count.search++;
  }

  const filesKeyword = config.files.keyword.trim();
  if (/\s/.test(filesKeyword)) problems.filesKeyword = "No spaces";
  else if (filesKeyword !== "" && keywords.has(filesKeyword.toLowerCase())) {
    problems.filesKeyword = "Already a web search keyword";
  }
  if (problems.filesKeyword) count.files++;

  const depth = config.files.max_depth;
  if (!Number.isInteger(depth) || depth < 0 || depth > MAX_DEPTH) {
    problems.filesDepth = `Enter a whole number from 0 to ${MAX_DEPTH}`;
    count.files++;
  }
  return problems;
}

export function totalProblems(problems: Problems): number {
  return Object.values(problems.count).reduce((sum, n) => sum + n, 0);
}
