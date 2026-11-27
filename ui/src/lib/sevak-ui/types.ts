// The Sevak UI definition as the renderer receives it. Mirrors
// crates/sevak-plugins/src/workflow/ui.rs: Rust parses, expands templates and
// validates, so everything here is already checked (and `use` is gone).

export type Align = "start" | "center" | "end" | "stretch";
export type LabelStyle = "heading" | "body" | "caption" | "mono";
export type ButtonStyle = "primary" | "secondary" | "danger";

/** One thing an event does; exactly one verb is set (`arg` goes with `output`). */
export interface UiAction {
  set?: Record<string, unknown>;
  close?: boolean;
  output?: string;
  arg?: string;
  run?: string;
  notify?: string;
  copy?: string;
}

export interface UiEvents {
  click?: UiAction[];
  change?: UiAction[];
  submit?: UiAction[];
  select?: UiAction[];
}

export type EventName = keyof UiEvents;

export interface UiListItem {
  title: string;
  subtitle?: string;
  arg?: string;
}

interface Layout {
  gap?: number;
  padding?: number;
  align?: Align;
  columns?: number;
  children: UiComponent[];
}

export type UiComponent =
  | ({ type: "column" } & Layout)
  | ({ type: "row" } & Layout)
  | ({ type: "grid" } & Layout)
  | { type: "label"; text: string; style?: LabelStyle }
  | { type: "markdown"; text: string }
  | {
      type: "button";
      label: string;
      id?: string;
      style?: ButtonStyle;
      disabled?: boolean;
      events?: UiEvents;
    }
  | { type: "checkbox"; id: string; label: string; events?: UiEvents }
  | { type: "textfield"; id: string; placeholder?: string; events?: UiEvents }
  | { type: "passwordfield"; id: string; placeholder?: string; events?: UiEvents }
  | { type: "textarea"; id: string; placeholder?: string; rows?: number; events?: UiEvents }
  | { type: "picker"; id: string; options: { label: string; value: string }[]; events?: UiEvents }
  | { type: "slider"; id: string; min: number; max: number; step?: number; events?: UiEvents }
  | { type: "progress"; value?: number | string; indeterminate?: boolean }
  | { type: "image"; path: string; alt?: string }
  | {
      type: "list";
      id: string;
      items: UiListItem[];
      filter_by?: string;
      multi?: boolean;
      events?: UiEvents;
    }
  | { type: "separator" }
  | { type: "spacer" };

export interface UiDefinition {
  title?: string;
  width?: number;
  height?: number;
  state?: Record<string, unknown>;
  templates?: Record<string, unknown>;
  root: UiComponent;
}

/** What the shell sends to open a UI (`ShowPayload.ui`, `sevak_ui_session`). */
export interface UiPayload {
  id: string;
  target: "launcher" | "window";
  workflow: string;
  title: string;
  width: number;
  height: number;
  definition: UiDefinition;
  /** `data:` URLs by the path written in the definition. */
  images: Record<string, string>;
  vars: Record<string, string>;
  query: string;
  scripts: string[];
}

/** What running a script changes in the open UI. */
export interface UiUpdate {
  set: Record<string, unknown>;
  root?: UiComponent;
  images: Record<string, string>;
  notify?: string;
  close: boolean;
}

/** What the renderer tells the shell. */
export type UiEventMessage =
  | { kind: "output"; port: string; arg: string | null; state: Record<string, unknown> }
  | { kind: "close"; state: Record<string, unknown> | null }
  | { kind: "run"; script: string; input: unknown }
  | { kind: "notify"; text: string }
  | { kind: "copy"; text: string };
