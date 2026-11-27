// The workflow file as the builder edits it (JSON with the same shape as
// `workflow.toml`; see crates/sevak-plugins/src/workflow/model.rs), the catalog
// of node kinds the builder offers, and the small pieces of graph logic the
// canvas needs. Whether a workflow is *valid* is decided by Rust
// (`check_workflow`); the helpers here only keep the canvas from drawing
// connections that cannot be right.
//
// The catalog's words (labels, summaries, hints, option names) live in the
// locale files under `workflows.*`; the getters below look them up on every
// read, so the builder follows a language change at once.

import { t, type MessageKey } from "../i18n.svelte";

/** A message under `workflows.`, for keys built from node types and field names. */
const w = (key: string) => t(`workflows.${key}` as MessageKey);

export type NodeType =
  | "keyword"
  | "hotkey"
  | "selection"
  | "external"
  | "script_filter"
  | "run_script"
  | "open_url"
  | "open_file"
  | "launch_app"
  | "system_command"
  | "terminal_command"
  | "copy"
  | "paste"
  | "set_variable"
  | "transform"
  | "conditional"
  | "delay"
  | "notification"
  | "large_type"
  | "text_view"
  | "list_filter"
  | "file_filter"
  | "running_apps"
  | "dialog_conditional"
  | "speak"
  | "play_sound";

export type Category = "trigger" | "input" | "action" | "utility" | "output";

export interface WfNode {
  id: string;
  type: NodeType;
  title?: string;
  x: number;
  y: number;
  // The fields of the node's kind (`keyword`, `url`, `command`, ...).
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  [field: string]: any;
}

export interface WfConnection {
  from: string;
  /** `out`, `then` / `else` for a conditional, `button1`..`button3` / `cancel` for a dialog. */
  port?: string;
  to: string;
}

export interface Workflow {
  format: number;
  name: string;
  description: string;
  author: string;
  version: string;
  enabled: boolean;
  variables: Record<string, string>;
  node: WfNode[];
  connection: WfConnection[];
}

export type Severity = "error" | "warning";

export interface Problem {
  severity: Severity;
  node: string | null;
  message: string;
}

export const OUT = "out";
export const THEN = "then";
export const ELSE = "else";
export const CANCEL = "cancel";
/** Most buttons a dialog node shows. */
export const MAX_DIALOG_BUTTONS = 3;

// ---- the catalog ----------------------------------------------------------

export type FieldKind =
  | "text"
  | "textarea"
  | "number"
  | "select"
  | "checkbox"
  | "lines"
  | "env"
  | "accepts"
  | "tristate"
  | "items";

export interface FieldDef {
  key: string;
  label: string;
  kind: FieldKind;
  hint?: string;
  placeholder?: string;
  mono?: boolean;
  options?: { value: string; label: string }[];
  /** The field may be left out of the file when empty. */
  optional?: boolean;
  min?: number;
  max?: number;
}

export interface KindDef {
  type: NodeType;
  label: string;
  category: Category;
  summary: string;
  fields: FieldDef[];
  /** Runs code or commands, so the workflow needs the user's Allow. */
  needsApproval?: boolean;
  /** What the node starts out as. */
  defaults: () => Record<string, unknown>;
}

/** A field as the catalog below writes it; `field()` adds the words. */
type RawField = Omit<FieldDef, "label" | "hint" | "options"> & {
  /** Where the field's words live under `workflows.`; default `kinds.<type>.fields.<key>`. */
  text?: string;
  /** `true`: its own `<text>.hint`; `"placeholders"`: the shared placeholder hint. */
  hint?: true | "placeholders";
  /** The option values; their labels are `workflows.options.<set>.<value>`. */
  options?: { set: string; values: readonly string[] };
};

type RawKind = Omit<KindDef, "label" | "summary" | "fields"> & { fields: RawField[] };

function field(type: NodeType, raw: RawField): FieldDef {
  const { text, hint, options, ...rest } = raw;
  const base = text ?? `kinds.${type}.fields.${raw.key}`;
  const def = { ...rest } as FieldDef;
  Object.defineProperty(def, "label", { enumerable: true, get: () => w(`${base}.label`) });
  if (hint) {
    Object.defineProperty(def, "hint", {
      enumerable: true,
      get: () => w(hint === "placeholders" ? "placeholders" : `${base}.hint`),
    });
  }
  if (options) {
    Object.defineProperty(def, "options", {
      enumerable: true,
      get: () => options.values.map((value) => ({ value, label: w(`options.${options.set}.${value}`) })),
    });
  }
  return def;
}

function localize(raw: RawKind): KindDef {
  const def = { ...raw, fields: raw.fields.map((f) => field(raw.type, f)) } as KindDef;
  Object.defineProperty(def, "label", { enumerable: true, get: () => w(`kinds.${raw.type}.label`) });
  Object.defineProperty(def, "summary", { enumerable: true, get: () => w(`kinds.${raw.type}.summary`) });
  return def;
}

const TRANSFORM_OPS = [
  "set",
  "upper",
  "lower",
  "title",
  "trim",
  "url_encode",
  "url_decode",
  "base64_encode",
  "base64_decode",
  "replace",
  "regex_replace",
  "first_line",
  "split",
] as const;

const TESTS = [
  "equals",
  "not_equals",
  "contains",
  "not_contains",
  "starts_with",
  "ends_with",
  "matches",
  "is_empty",
  "not_empty",
] as const;

const SYSTEM_COMMANDS = [
  "lock",
  "sleep",
  "hibernate",
  "restart",
  "shutdown",
  "logout",
  "empty_trash",
] as const;

const FILE_SCOPES = ["any", "files", "folders"] as const;

const APP_OUTPUTS = ["name", "path", "pid"] as const;

const PROGRAM_FIELDS: RawField[] = [
  {
    key: "command",
    text: "program.command",
    kind: "lines",
    mono: true,
    optional: true,
    hint: true,
  },
  {
    key: "script",
    text: "program.script",
    kind: "text",
    mono: true,
    optional: true,
    placeholder: "main.py",
    hint: true,
  },
];

export const KINDS: KindDef[] = (
  [
  {
    type: "keyword",
    category: "trigger",
    fields: [
      { key: "keyword", kind: "text", mono: true, placeholder: "docs" },
      {
        key: "argument",
        kind: "select",
        options: { set: "argument", values: ["optional", "required", "none"] },
      },
      {
        key: "subtitle",
        kind: "text",
        optional: true,
        hint: "placeholders",
      },
    ],
    defaults: () => ({ keyword: "", argument: "optional", subtitle: "" }),
  },
  {
    type: "hotkey",
    category: "trigger",
    fields: [
      {
        key: "key",
        kind: "text",
        mono: true,
        placeholder: "Ctrl+Alt+K",
        hint: true,
      },
    ],
    defaults: () => ({ key: "" }),
  },
  {
    type: "selection",
    category: "trigger",
    fields: [
      {
        key: "accepts",
        kind: "accepts",
        hint: true,
      },
    ],
    defaults: () => ({ accepts: ["text", "url", "file"] }),
  },
  {
    type: "external",
    category: "trigger",
    fields: [],
    defaults: () => ({}),
  },
  {
    type: "script_filter",
    category: "input",
    needsApproval: true,
    fields: [
      { key: "keyword", kind: "text", mono: true, placeholder: "fruit" },
      ...PROGRAM_FIELDS,
      {
        key: "args",
        kind: "lines",
        mono: true,
        optional: true,
      },
      {
        key: "timeout_ms",
        kind: "number",
        optional: true,
        min: 10,
        max: 1000,
        placeholder: "50",
      },
      {
        key: "hard_timeout_ms",
        kind: "number",
        optional: true,
        min: 500,
        max: 60000,
        placeholder: "3000",
      },
    ],
    defaults: () => ({ keyword: "", command: [], script: "script.py" }),
  },
  {
    type: "run_script",
    category: "action",
    needsApproval: true,
    fields: [
      ...PROGRAM_FIELDS,
      {
        key: "args",
        kind: "lines",
        mono: true,
        optional: true,
        hint: true,
      },
      {
        key: "stdin",
        kind: "textarea",
        optional: true,
        hint: "placeholders",
      },
      {
        key: "env",
        kind: "env",
        optional: true,
        hint: true,
      },
      {
        key: "timeout_ms",
        kind: "number",
        optional: true,
        min: 100,
        max: 300000,
        placeholder: "10000",
      },
      {
        key: "log_stderr",
        kind: "checkbox",
        optional: true,
        hint: true,
      },
    ],
    defaults: () => ({ command: [], script: "script.py" }),
  },
  {
    type: "open_url",
    category: "action",
    fields: [
      {
        key: "url",
        kind: "text",
        mono: true,
        placeholder: "https://example.com/?q={query}",
        hint: true,
      },
    ],
    defaults: () => ({ url: "https://" }),
  },
  {
    type: "open_file",
    category: "action",
    needsApproval: true,
    fields: [
      {
        key: "path",
        kind: "text",
        mono: true,
        hint: true,
      },
    ],
    defaults: () => ({ path: "{query}" }),
  },
  {
    type: "launch_app",
    category: "action",
    needsApproval: true,
    fields: [
      { key: "app", kind: "text", placeholder: "Firefox" },
      {
        key: "args",
        kind: "lines",
        mono: true,
        optional: true,
        hint: true,
      },
    ],
    defaults: () => ({ app: "" }),
  },
  {
    type: "system_command",
    category: "action",
    needsApproval: true,
    fields: [
      { key: "command", kind: "select", options: { set: "system", values: SYSTEM_COMMANDS } },
    ],
    defaults: () => ({ command: "lock" }),
  },
  {
    type: "terminal_command",
    category: "action",
    needsApproval: true,
    fields: [
      {
        key: "command",
        kind: "textarea",
        mono: true,
        hint: true,
      },
    ],
    defaults: () => ({ command: "" }),
  },
  {
    type: "copy",
    category: "action",
    fields: [{ key: "text", kind: "textarea", hint: "placeholders" }],
    defaults: () => ({ text: "{query}" }),
  },
  {
    type: "paste",
    category: "action",
    fields: [
      { key: "text", kind: "textarea", hint: "placeholders" },
      {
        key: "restore_clipboard",
        kind: "tristate",
        optional: true,
      },
    ],
    defaults: () => ({ text: "{query}" }),
  },
  {
    type: "set_variable",
    category: "utility",
    fields: [
      { key: "name", kind: "text", mono: true, placeholder: "site" },
      { key: "value", kind: "text", hint: "placeholders" },
    ],
    defaults: () => ({ name: "", value: "" }),
  },
  {
    type: "transform",
    category: "utility",
    fields: [
      { key: "op", kind: "select", options: { set: "transform", values: TRANSFORM_OPS } },
      { key: "input", kind: "text", hint: "placeholders" },
      {
        key: "find",
        kind: "text",
        mono: true,
        optional: true,
        hint: true,
      },
      {
        key: "replace",
        kind: "text",
        mono: true,
        optional: true,
        hint: true,
      },
      {
        key: "index",
        kind: "number",
        optional: true,
        hint: true,
      },
      {
        key: "into",
        kind: "text",
        mono: true,
        optional: true,
        hint: true,
      },
    ],
    defaults: () => ({ op: "upper", input: "{query}" }),
  },
  {
    type: "conditional",
    category: "utility",
    fields: [
      { key: "left", kind: "text", hint: "placeholders" },
      { key: "test", kind: "select", options: { set: "test", values: TESTS } },
      { key: "right", kind: "text", mono: true, optional: true },
      { key: "ignore_case", kind: "checkbox", optional: true },
    ],
    defaults: () => ({ left: "{query}", test: "equals", right: "" }),
  },
  {
    type: "delay",
    category: "utility",
    fields: [
      { key: "ms", kind: "number", min: 0, max: 60000 },
    ],
    defaults: () => ({ ms: 500 }),
  },
  {
    type: "notification",
    category: "output",
    fields: [
      { key: "heading", kind: "text", optional: true },
      { key: "body", kind: "textarea", hint: "placeholders" },
    ],
    defaults: () => ({ heading: "", body: "{query}" }),
  },
  {
    type: "large_type",
    category: "output",
    fields: [{ key: "text", kind: "textarea", hint: "placeholders" }],
    defaults: () => ({ text: "{query}" }),
  },
  {
    type: "text_view",
    category: "output",
    fields: [
      { key: "heading", kind: "text", optional: true },
      { key: "text", kind: "textarea", hint: "placeholders" },
    ],
    defaults: () => ({ text: "{query}" }),
  },
  {
    type: "list_filter",
    category: "input",
    fields: [
      { key: "keyword", kind: "text", mono: true, placeholder: "pick" },
      { key: "items", kind: "items", hint: true },
    ],
    defaults: () => ({ keyword: "", items: [{ title: "", subtitle: "", arg: "" }] }),
  },
  {
    type: "file_filter",
    category: "input",
    fields: [
      { key: "keyword", kind: "text", mono: true, placeholder: "docs" },
      {
        key: "folders",
        kind: "lines",
        mono: true,
        optional: true,
        placeholder: "~/Documents",
        hint: true,
      },
      {
        key: "extensions",
        kind: "lines",
        mono: true,
        optional: true,
        placeholder: "pdf",
        hint: true,
      },
      {
        key: "scope",
        kind: "select",
        optional: true,
        options: { set: "fileScope", values: FILE_SCOPES },
      },
    ],
    defaults: () => ({ keyword: "", folders: [], extensions: [], scope: "any" }),
  },
  {
    type: "running_apps",
    category: "input",
    fields: [
      { key: "keyword", kind: "text", mono: true, placeholder: "apps" },
      {
        key: "output",
        kind: "select",
        optional: true,
        options: { set: "appOutput", values: APP_OUTPUTS },
        hint: true,
      },
    ],
    defaults: () => ({ keyword: "", output: "name" }),
  },
  {
    type: "dialog_conditional",
    category: "utility",
    fields: [
      { key: "heading", kind: "text", optional: true },
      { key: "message", kind: "textarea", hint: "placeholders" },
      { key: "buttons", kind: "lines", hint: true },
    ],
    defaults: () => ({ heading: "", message: "{query}", buttons: ["OK", "Cancel"] }),
  },
  {
    type: "speak",
    category: "output",
    fields: [
      { key: "text", kind: "textarea", hint: "placeholders" },
      { key: "voice", kind: "text", optional: true, hint: true },
    ],
    defaults: () => ({ text: "{query}" }),
  },
  {
    type: "play_sound",
    category: "output",
    fields: [
      {
        key: "path",
        kind: "text",
        mono: true,
        placeholder: "~/Sounds/ding.wav",
        hint: true,
      },
    ],
    defaults: () => ({ path: "" }),
  },
  ] as RawKind[]
).map(localize);

/** A category's name, as the "Add a node" menu groups the kinds. */
export const categoryLabel = (category: Category): string => w(`categories.${category}`);

export const CATEGORIES: Category[] = ["trigger", "input", "action", "utility", "output"];

export function kindOf(type: NodeType): KindDef {
  return KINDS.find((kind) => kind.type === type) ?? KINDS[0];
}

/** Triggers and inputs have no input port: nothing feeds them. */
export const isStart = (node: WfNode) => {
  const category = kindOf(node.type).category;
  return category === "trigger" || category === "input";
};

/** A dialog has a port per button (as many as it has labels, at most three) and `cancel`. */
export const outputPorts = (node: WfNode): string[] => {
  if (node.type === "conditional") return [THEN, ELSE];
  if (node.type === "dialog_conditional") {
    const count = Array.isArray(node.buttons) ? Math.min(node.buttons.length, MAX_DIALOG_BUTTONS) : 0;
    return [...Array.from({ length: Math.max(count, 1) }, (_, i) => `button${i + 1}`), CANCEL];
  }
  return [OUT];
};

export const portOf = (conn: WfConnection) => conn.port ?? OUT;

// ---- building ---------------------------------------------------------------

export function newWorkflow(): Workflow {
  return {
    format: 1,
    name: t("workflows.defaultName"),
    description: "",
    author: "",
    version: "",
    enabled: true,
    variables: {},
    node: [
      {
        id: "keyword",
        type: "keyword",
        title: "",
        x: 40,
        y: 80,
        ...(kindOf("keyword").defaults() as object),
      },
    ],
    connection: [],
  };
}

/** `base`, or `base-2`, `base-3`... whichever is not an id yet. */
export function uniqueId(workflow: Workflow, base: string): string {
  const taken = new Set(workflow.node.map((node) => node.id));
  if (!taken.has(base)) return base;
  let n = 2;
  while (taken.has(`${base}-${n}`)) n++;
  return `${base}-${n}`;
}

export function makeNode(workflow: Workflow, type: NodeType, x: number, y: number): WfNode {
  const kind = kindOf(type);
  return {
    id: uniqueId(workflow, type.replace(/_/g, "-")),
    type,
    title: "",
    x,
    y,
    ...(kind.defaults() as object),
  };
}

/** Fills in what a workflow loaded from the backend may leave out. */
export function normalize(workflow: Workflow): Workflow {
  return {
    format: workflow.format ?? 1,
    name: workflow.name ?? "",
    description: workflow.description ?? "",
    author: workflow.author ?? "",
    version: workflow.version ?? "",
    enabled: workflow.enabled ?? true,
    variables: { ...(workflow.variables ?? {}) },
    node: (workflow.node ?? []).map((node) => ({ ...node, x: node.x ?? 0, y: node.y ?? 0 })),
    connection: (workflow.connection ?? []).map((conn) => ({ ...conn, port: conn.port ?? OUT })),
  };
}

/**
 * What goes to the backend: empty optional fields are left out, numbers are
 * whole, positions are rounded.
 */
export function toSave(workflow: Workflow): Workflow {
  const copy: Workflow = JSON.parse(JSON.stringify(workflow));
  for (const node of copy.node) {
    node.x = Math.round(node.x);
    node.y = Math.round(node.y);
    const defs = kindOf(node.type).fields;
    for (const field of defs) {
      const value = node[field.key];
      const empty =
        value === undefined ||
        value === null ||
        value === "" ||
        (Array.isArray(value) && value.length === 0 && field.kind !== "accepts") ||
        (field.kind === "env" && Object.keys(value ?? {}).length === 0) ||
        (field.kind === "checkbox" && field.optional && value === false);
      if (field.optional && empty) {
        delete node[field.key];
      } else if (field.kind === "number" && typeof value === "number") {
        node[field.key] = Math.trunc(value);
      }
    }
  }
  copy.connection = copy.connection.map((conn) => ({
    from: conn.from,
    port: conn.port ?? OUT,
    to: conn.to,
  }));
  return copy;
}

// ---- graph logic ------------------------------------------------------------------

/** Whether a connection from `from` to `to` would close a loop. */
export function wouldLoop(workflow: Workflow, from: string, to: string): boolean {
  if (from === to) return true;
  const next = new Map<string, string[]>();
  for (const conn of workflow.connection) {
    next.set(conn.from, [...(next.get(conn.from) ?? []), conn.to]);
  }
  const seen = new Set<string>();
  const stack = [to];
  while (stack.length > 0) {
    const id = stack.pop()!;
    if (id === from) return true;
    if (seen.has(id)) continue;
    seen.add(id);
    stack.push(...(next.get(id) ?? []));
  }
  return false;
}

export type ConnectResult = { ok: true } | { ok: false; reason: string };

/** Why `from.port -> to` cannot be added, or `ok`. */
export function canConnect(
  workflow: Workflow,
  from: string,
  port: string,
  to: string,
): ConnectResult {
  const source = workflow.node.find((node) => node.id === from);
  const target = workflow.node.find((node) => node.id === to);
  if (!source || !target) return { ok: false, reason: t("workflows.connect.missing") };
  if (isStart(target)) {
    return { ok: false, reason: t("workflows.connect.noInput", { kind: kindOf(target.type).label }) };
  }
  if (!outputPorts(source).includes(port)) return { ok: false, reason: t("workflows.connect.noOutput") };
  if (from === to) return { ok: false, reason: t("workflows.connect.self") };
  if (workflow.connection.some((c) => c.from === from && portOf(c) === port && c.to === to)) {
    return { ok: false, reason: t("workflows.connect.already") };
  }
  if (wouldLoop(workflow, from, to)) {
    return { ok: false, reason: t("workflows.connect.loop") };
  }
  return { ok: true };
}

/** A removed node takes its connections with it. */
export function removeNode(workflow: Workflow, id: string): void {
  workflow.node = workflow.node.filter((node) => node.id !== id);
  workflow.connection = workflow.connection.filter((c) => c.from !== id && c.to !== id);
}

/** Renames a node and every connection that names it. */
export function renameNode(workflow: Workflow, oldId: string, newId: string): void {
  for (const node of workflow.node) if (node.id === oldId) node.id = newId;
  for (const conn of workflow.connection) {
    if (conn.from === oldId) conn.from = newId;
    if (conn.to === oldId) conn.to = newId;
  }
}

// ---- geometry ---------------------------------------------------------------------

export const NODE_W = 200;
export const NODE_H = 64;
export const PORT_R = 7;

export interface Point {
  x: number;
  y: number;
}

/** Where the connector of an output port leaves the node. */
export function outPoint(node: WfNode, port: string): Point {
  const ports = outputPorts(node);
  if (ports.length === 1) return { x: node.x + NODE_W, y: node.y + NODE_H / 2 };
  const index = Math.max(0, ports.indexOf(port));
  // Three or four ports are packed closer to fit the box.
  const [top, step] = ports.length > 2 ? [10, 15] : [22, 24];
  return { x: node.x + NODE_W, y: node.y + top + index * step };
}

export function inPoint(node: WfNode): Point {
  return { x: node.x, y: node.y + NODE_H / 2 };
}

/** A smooth curve between two points, as an SVG path. */
export function curve(a: Point, b: Point): string {
  const dx = Math.max(40, Math.abs(b.x - a.x) / 2);
  return `M ${a.x} ${a.y} C ${a.x + dx} ${a.y}, ${b.x - dx} ${b.y}, ${b.x} ${b.y}`;
}

/** Spreads nodes that sit on top of each other (a workflow written by hand has no layout). */
export function autoLayout(workflow: Workflow): void {
  const depth = new Map<string, number>();
  const starts = workflow.node.filter(isStart).map((node) => node.id);
  const queue: [string, number][] = starts.map((id) => [id, 0]);
  while (queue.length > 0) {
    const [id, level] = queue.shift()!;
    if ((depth.get(id) ?? -1) >= level) continue;
    depth.set(id, level);
    for (const conn of workflow.connection) if (conn.from === id) queue.push([conn.to, level + 1]);
    if (level > workflow.node.length) break;
  }
  const rows = new Map<number, number>();
  for (const node of workflow.node) {
    const level = depth.get(node.id) ?? 0;
    const row = rows.get(level) ?? 0;
    rows.set(level, row + 1);
    node.x = 40 + level * (NODE_W + 70);
    node.y = 40 + row * (NODE_H + 40);
  }
}

/** True when most nodes share a position, i.e. nobody has laid the graph out. */
export function needsLayout(workflow: Workflow): boolean {
  if (workflow.node.length < 2) return false;
  const places = new Set(workflow.node.map((node) => `${node.x},${node.y}`));
  return places.size < workflow.node.length;
}
