// Pure helpers of the Sevak UI renderer: placeholders, initial state,
// filtering. No DOM, so they are unit-tested directly.

import type { UiComponent, UiDefinition, UiListItem } from "./types";

export type UiState = Record<string, unknown>;

export interface Scope {
  state: UiState;
  vars: Record<string, string>;
  query: string;
}

/** A state value as text: arrays join with ", ", missing is empty. */
export function show(value: unknown): string {
  if (value === undefined || value === null) return "";
  if (Array.isArray(value)) return value.map(show).join(", ");
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

/** Fills `{state:id}`, `{var:name}` and `{query}`; anything else stays as written. */
export function substitute(text: string, scope: Scope): string {
  return text.replace(/\{(state|var):([^{}]*)\}|\{query\}/g, (match, kind?: string, name?: string) => {
    if (match === "{query}") return scope.query;
    if (kind === "state") return show(scope.state[name ?? ""]);
    return Object.hasOwn(scope.vars, name ?? "") ? scope.vars[name ?? ""] : "";
  });
}

/** The value of a `set` action: text values are templates, others stay as they are. */
export function resolveValue(value: unknown, scope: Scope): unknown {
  return typeof value === "string" ? substitute(value, scope) : value;
}

/** A progress bar's fraction (0..1) or `null` when it is not a number yet. */
export function progressValue(value: number | string | undefined, scope: Scope): number | null {
  const n = typeof value === "number" ? value : value === undefined ? NaN : Number(substitute(value, scope));
  return Number.isFinite(n) ? Math.min(1, Math.max(0, n)) : null;
}

function walk(component: UiComponent, visit: (c: UiComponent) => void) {
  visit(component);
  if (component.type === "column" || component.type === "row" || component.type === "grid") {
    for (const child of component.children) walk(child, visit);
  }
}

/** The state the UI starts with: the definition's, plus a default for each input. */
export function initialState(definition: UiDefinition): UiState {
  const state: UiState = { ...(definition.state ?? {}) };
  walk(definition.root, (c) => {
    if (!("id" in c) || typeof c.id !== "string" || c.id in state) return;
    switch (c.type) {
      case "checkbox":
        state[c.id] = false;
        break;
      case "slider":
        state[c.id] = c.min;
        break;
      case "picker":
        state[c.id] = c.options[0]?.value ?? "";
        break;
      case "list":
        state[c.id] = c.multi ? [] : "";
        break;
      case "textfield":
      case "passwordfield":
      case "textarea":
        state[c.id] = "";
        break;
    }
  });
  return state;
}

/** Fuzzy: every letter of `needle` appears in `hay`, in order (case-insensitive). */
export function fuzzy(needle: string, hay: string): boolean {
  const n = needle.toLowerCase().replace(/\s+/g, "");
  const h = hay.toLowerCase();
  let at = 0;
  for (const ch of n) {
    at = h.indexOf(ch, at);
    if (at < 0) return false;
    at += 1;
  }
  return true;
}

export function filterItems(items: UiListItem[], needle: string): UiListItem[] {
  if (needle.trim() === "") return items;
  return items.filter((item) => fuzzy(needle, `${item.title} ${item.subtitle ?? ""}`));
}

/** What a list row selects: its `arg`, or its title. */
export const itemValue = (item: UiListItem): string => item.arg || item.title;

/** Output ports a definition names, then `closed` (the same rule as the Rust side). */
export function definitionPorts(root: UiComponent): string[] {
  const ports: string[] = [];
  walk(root, (c) => {
    const events = "events" in c ? c.events : undefined;
    for (const actions of Object.values(events ?? {})) {
      for (const action of actions ?? []) {
        if (action.output && action.output !== "closed" && !ports.includes(action.output)) {
          ports.push(action.output);
        }
      }
    }
  });
  return [...ports, "closed"];
}
