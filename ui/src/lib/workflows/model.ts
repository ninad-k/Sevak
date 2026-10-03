// The workflow file as the builder edits it (JSON with the same shape as
// `workflow.toml`; see crates/sevak-plugins/src/workflow/model.rs), the catalog
// of node kinds the builder offers, and the small pieces of graph logic the
// canvas needs. Whether a workflow is *valid* is decided by Rust
// (`check_workflow`); the helpers here only keep the canvas from drawing
// connections that cannot be right.

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
  | "text_view";

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
  /** `out`, or `then` / `else` for a conditional. */
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
  | "tristate";

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

const PLACEHOLDERS = "Placeholders: {query} and {var:name}.";

const TRANSFORM_OPS = [
  ["set", "Set to the text"],
  ["upper", "UPPER CASE"],
  ["lower", "lower case"],
  ["title", "Title Case"],
  ["trim", "Trim spaces"],
  ["url_encode", "URL-encode"],
  ["url_decode", "URL-decode"],
  ["base64_encode", "Base64 encode"],
  ["base64_decode", "Base64 decode"],
  ["replace", "Replace text"],
  ["regex_replace", "Replace with a regular expression"],
  ["first_line", "First line"],
  ["split", "Split and take one part"],
] as const;

const TESTS = [
  ["equals", "equals"],
  ["not_equals", "does not equal"],
  ["contains", "contains"],
  ["not_contains", "does not contain"],
  ["starts_with", "starts with"],
  ["ends_with", "ends with"],
  ["matches", "matches the regular expression"],
  ["is_empty", "is empty"],
  ["not_empty", "is not empty"],
] as const;

const SYSTEM_COMMANDS = [
  ["lock", "Lock"],
  ["sleep", "Sleep"],
  ["hibernate", "Hibernate"],
  ["restart", "Restart (asks first)"],
  ["shutdown", "Shut down (asks first)"],
  ["logout", "Log out (asks first)"],
  ["empty_trash", "Empty the trash (asks first)"],
] as const;

const opts = (pairs: readonly (readonly [string, string])[]) =>
  pairs.map(([value, label]) => ({ value, label }));

const PROGRAM_FIELDS: FieldDef[] = [
  {
    key: "command",
    label: "Program and arguments",
    kind: "lines",
    mono: true,
    optional: true,
    hint: "One per line, the program first. Use this or the script below. No shell is used.",
  },
  {
    key: "script",
    label: "Script file",
    kind: "text",
    mono: true,
    optional: true,
    placeholder: "main.py",
    hint: "A file in the workflow's folder: .py, .ps1, .js, .sh or an executable.",
  },
];

export const KINDS: KindDef[] = [
  {
    type: "keyword",
    label: "Keyword",
    category: "trigger",
    summary: "Type a keyword (and some text) to start the workflow.",
    fields: [
      { key: "keyword", label: "Keyword", kind: "text", mono: true, placeholder: "docs" },
      {
        key: "argument",
        label: "Text after the keyword",
        kind: "select",
        options: [
          { value: "optional", label: "Optional" },
          { value: "required", label: "Required" },
          { value: "none", label: "None" },
        ],
      },
      {
        key: "subtitle",
        label: "Second line of the result",
        kind: "text",
        optional: true,
        hint: PLACEHOLDERS,
      },
    ],
    defaults: () => ({ keyword: "", argument: "optional", subtitle: "" }),
  },
  {
    type: "hotkey",
    label: "Hotkey",
    category: "trigger",
    summary: "A global shortcut starts the workflow.",
    fields: [
      {
        key: "key",
        label: "Shortcut",
        kind: "text",
        mono: true,
        placeholder: "Ctrl+Alt+K",
        hint: "Like the shortcuts in Settings. On Wayland use an external trigger instead.",
      },
    ],
    defaults: () => ({ key: "" }),
  },
  {
    type: "selection",
    label: "Universal Actions",
    category: "trigger",
    summary: "Appears in Universal Actions for the text, links or files you select.",
    fields: [
      {
        key: "accepts",
        label: "Offer it for",
        kind: "accepts",
        hint: "The selection becomes the argument (one link or path per line).",
      },
    ],
    defaults: () => ({ accepts: ["text", "url", "file"] }),
  },
  {
    type: "external",
    label: "External trigger",
    category: "trigger",
    summary: "Started from the command line or a desktop shortcut.",
    fields: [],
    defaults: () => ({}),
  },
  {
    type: "script_filter",
    label: "Script filter",
    category: "input",
    summary: "A keyword whose results come from a script (Alfred Script Filter JSON).",
    needsApproval: true,
    fields: [
      { key: "keyword", label: "Keyword", kind: "text", mono: true, placeholder: "fruit" },
      ...PROGRAM_FIELDS,
      {
        key: "args",
        label: "Arguments before the typed text",
        kind: "lines",
        mono: true,
        optional: true,
      },
      {
        key: "timeout_ms",
        label: "Wait for results (ms)",
        kind: "number",
        optional: true,
        min: 10,
        max: 1000,
        placeholder: "50",
      },
      {
        key: "hard_timeout_ms",
        label: "Stop the script after (ms)",
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
    label: "Run script",
    category: "action",
    summary: "Runs a program; what it prints becomes the argument.",
    needsApproval: true,
    fields: [
      ...PROGRAM_FIELDS,
      {
        key: "args",
        label: "More arguments",
        kind: "lines",
        mono: true,
        optional: true,
        hint: `One per line. ${PLACEHOLDERS}`,
      },
      {
        key: "stdin",
        label: "Standard input",
        kind: "textarea",
        optional: true,
        hint: PLACEHOLDERS,
      },
      {
        key: "env",
        label: "Environment variables",
        kind: "env",
        optional: true,
        hint: "NAME=value, one per line.",
      },
      {
        key: "timeout_ms",
        label: "Stop it after (ms)",
        kind: "number",
        optional: true,
        min: 100,
        max: 300000,
        placeholder: "10000",
      },
      {
        key: "log_stderr",
        label: "Log its error output",
        kind: "checkbox",
        optional: true,
        hint: "Off by default: it can contain your data.",
      },
    ],
    defaults: () => ({ command: [], script: "script.py" }),
  },
  {
    type: "open_url",
    label: "Open URL",
    category: "action",
    summary: "Opens a web or mail link. Placeholders are URL-encoded.",
    fields: [
      {
        key: "url",
        label: "Link",
        kind: "text",
        mono: true,
        placeholder: "https://example.com/?q={query}",
        hint: "Add |raw to a placeholder to leave it as it is: {var:site|raw}.",
      },
    ],
    defaults: () => ({ url: "https://" }),
  },
  {
    type: "open_file",
    label: "Open file",
    category: "action",
    summary: "Opens a file or folder with its default program.",
    fields: [
      {
        key: "path",
        label: "Path",
        kind: "text",
        mono: true,
        hint: `~ is your home folder; a relative path is inside the workflow's folder. ${PLACEHOLDERS}`,
      },
    ],
    defaults: () => ({ path: "{query}" }),
  },
  {
    type: "launch_app",
    label: "Launch app",
    category: "action",
    summary: "Starts an application by name or path.",
    needsApproval: true,
    fields: [
      { key: "app", label: "Application", kind: "text", placeholder: "Firefox" },
      {
        key: "args",
        label: "Arguments",
        kind: "lines",
        mono: true,
        optional: true,
        hint: "Used when the application is a path to a program.",
      },
    ],
    defaults: () => ({ app: "" }),
  },
  {
    type: "system_command",
    label: "System command",
    category: "action",
    summary: "Lock, sleep, restart and so on.",
    needsApproval: true,
    fields: [
      { key: "command", label: "Command", kind: "select", options: opts(SYSTEM_COMMANDS) },
    ],
    defaults: () => ({ command: "lock" }),
  },
  {
    type: "terminal_command",
    label: "Terminal command",
    category: "action",
    summary: "Runs a command line in a terminal window.",
    needsApproval: true,
    fields: [
      {
        key: "command",
        label: "Command line",
        kind: "textarea",
        mono: true,
        hint: `${PLACEHOLDERS} Quote text you insert: {query|sh} (macOS, Linux) or {query|ps} (PowerShell).`,
      },
    ],
    defaults: () => ({ command: "" }),
  },
  {
    type: "copy",
    label: "Copy",
    category: "action",
    summary: "Copies text to the clipboard.",
    fields: [{ key: "text", label: "Text", kind: "textarea", hint: PLACEHOLDERS }],
    defaults: () => ({ text: "{query}" }),
  },
  {
    type: "paste",
    label: "Paste",
    category: "action",
    summary: "Pastes text into the app you were using.",
    fields: [
      { key: "text", label: "Text", kind: "textarea", hint: PLACEHOLDERS },
      {
        key: "restore_clipboard",
        label: "Put the earlier clipboard back",
        kind: "tristate",
        optional: true,
      },
    ],
    defaults: () => ({ text: "{query}" }),
  },
  {
    type: "set_variable",
    label: "Set variable",
    category: "utility",
    summary: "Stores a value for the nodes after it.",
    fields: [
      { key: "name", label: "Name", kind: "text", mono: true, placeholder: "site" },
      { key: "value", label: "Value", kind: "text", hint: PLACEHOLDERS },
    ],
    defaults: () => ({ name: "", value: "" }),
  },
  {
    type: "transform",
    label: "Transform",
    category: "utility",
    summary: "Changes the argument, or stores the result in a variable.",
    fields: [
      { key: "op", label: "What to do", kind: "select", options: opts(TRANSFORM_OPS) },
      { key: "input", label: "Input", kind: "text", hint: PLACEHOLDERS },
      {
        key: "find",
        label: "Find / split at",
        kind: "text",
        mono: true,
        optional: true,
        hint: "For replace, regular-expression replace and split.",
      },
      {
        key: "replace",
        label: "Replace with",
        kind: "text",
        mono: true,
        optional: true,
        hint: "A regular expression may use $1 for a group.",
      },
      {
        key: "index",
        label: "Part to keep (split)",
        kind: "number",
        optional: true,
        hint: "0 is the first part; -1 the last.",
      },
      {
        key: "into",
        label: "Store in variable",
        kind: "text",
        mono: true,
        optional: true,
        hint: "Leave empty to replace the argument.",
      },
    ],
    defaults: () => ({ op: "upper", input: "{query}" }),
  },
  {
    type: "conditional",
    label: "Conditional",
    category: "utility",
    summary: "Continues along then or else.",
    fields: [
      { key: "left", label: "Text", kind: "text", hint: PLACEHOLDERS },
      { key: "test", label: "Test", kind: "select", options: opts(TESTS) },
      { key: "right", label: "Compared with", kind: "text", mono: true, optional: true },
      { key: "ignore_case", label: "Ignore case", kind: "checkbox", optional: true },
    ],
    defaults: () => ({ left: "{query}", test: "equals", right: "" }),
  },
  {
    type: "delay",
    label: "Delay",
    category: "utility",
    summary: "Waits a moment (at most a minute).",
    fields: [
      { key: "ms", label: "Wait (ms)", kind: "number", min: 0, max: 60000 },
    ],
    defaults: () => ({ ms: 500 }),
  },
  {
    type: "notification",
    label: "Notification",
    category: "output",
    summary: "Shows a system notification.",
    fields: [
      { key: "heading", label: "Heading", kind: "text", optional: true },
      { key: "body", label: "Text", kind: "textarea", hint: PLACEHOLDERS },
    ],
    defaults: () => ({ heading: "", body: "{query}" }),
  },
  {
    type: "large_type",
    label: "Large Type",
    category: "output",
    summary: "Shows text huge on screen.",
    fields: [{ key: "text", label: "Text", kind: "textarea", hint: PLACEHOLDERS }],
    defaults: () => ({ text: "{query}" }),
  },
  {
    type: "text_view",
    label: "Text view",
    category: "output",
    summary: "Shows text in the launcher window.",
    fields: [
      { key: "heading", label: "Heading", kind: "text", optional: true },
      { key: "text", label: "Text", kind: "textarea", hint: PLACEHOLDERS },
    ],
    defaults: () => ({ text: "{query}" }),
  },
];

export const CATEGORY_LABELS: Record<Category, string> = {
  trigger: "Triggers",
  input: "Inputs",
  action: "Actions",
  utility: "Utilities",
  output: "Outputs",
};

export const CATEGORIES: Category[] = ["trigger", "input", "action", "utility", "output"];

export function kindOf(type: NodeType): KindDef {
  return KINDS.find((kind) => kind.type === type) ?? KINDS[0];
}

/** Triggers and inputs have no input port: nothing feeds them. */
export const isStart = (node: WfNode) => {
  const category = kindOf(node.type).category;
  return category === "trigger" || category === "input";
};

export const outputPorts = (node: WfNode): string[] =>
  node.type === "conditional" ? [THEN, ELSE] : [OUT];

export const portOf = (conn: WfConnection) => conn.port ?? OUT;

// ---- building ---------------------------------------------------------------

export function newWorkflow(): Workflow {
  return {
    format: 1,
    name: "New workflow",
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
  if (!source || !target) return { ok: false, reason: "That node does not exist." };
  if (isStart(target)) {
    return { ok: false, reason: `A ${kindOf(target.type).label.toLowerCase()} has no input.` };
  }
  if (!outputPorts(source).includes(port)) return { ok: false, reason: "No such output." };
  if (from === to) return { ok: false, reason: "A node cannot connect to itself." };
  if (workflow.connection.some((c) => c.from === from && portOf(c) === port && c.to === to)) {
    return { ok: false, reason: "Already connected." };
  }
  if (wouldLoop(workflow, from, to)) {
    return { ok: false, reason: "That would make a loop." };
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
  return { x: node.x + NODE_W, y: node.y + 22 + index * 24 };
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
