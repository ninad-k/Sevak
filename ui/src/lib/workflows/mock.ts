// Browser-preview data for the workflow pages: an in-memory stand-in for the
// Rust backend, so `npm run dev` shows a populated builder and gallery. It is
// only imported when there is no Tauri runtime (see ipc.ts).

import type {
  Gallery,
  GalleryKind,
  InstalledEntry,
  LoadedWorkflow,
  Result,
  SavedWorkflow,
  TemplateInfo,
  WorkflowList,
  WorkflowSummary,
} from "./ipc";
import catalog from "../../../../gallery/index.json";
import { isStart, kindOf, normalize, outputPorts, portOf, type Problem, type Workflow } from "./model";

function wf(partial: Partial<Workflow> & Pick<Workflow, "name" | "node" | "connection">): Workflow {
  return normalize({
    format: 1,
    description: "",
    author: "",
    version: "1.0",
    enabled: true,
    variables: {},
    ...partial,
  } as Workflow);
}

const duckduckgo = (): Workflow =>
  wf({
    name: "DuckDuckGo search",
    description: "Type ddg and some words to search DuckDuckGo.",
    author: "Sevak",
    node: [
      { id: "keyword", type: "keyword", title: "Search DuckDuckGo for {query}", x: 40, y: 120, keyword: "ddg", argument: "optional", subtitle: "Opens the results in your browser" },
      { id: "typed-something", type: "conditional", title: "Typed something?", x: 320, y: 120, left: "{query}", test: "not_empty", right: "" },
      { id: "search", type: "open_url", title: "Search", x: 600, y: 60, url: "https://duckduckgo.com/?q={query}" },
      { id: "home", type: "open_url", title: "Home page", x: 600, y: 200, url: "https://duckduckgo.com/" },
    ],
    connection: [
      { from: "keyword", to: "typed-something" },
      { from: "typed-something", port: "then", to: "search" },
      { from: "typed-something", port: "else", to: "home" },
    ],
  });

const tidyText = (): Workflow =>
  wf({
    name: "Tidy up whitespace",
    description: "Collapses spaces and line breaks in the selected text.",
    author: "Sevak",
    node: [
      { id: "selection", type: "selection", title: "Tidy up whitespace", x: 40, y: 100, accepts: ["text"] },
      { id: "collapse", type: "transform", title: "Collapse whitespace", x: 300, y: 100, op: "regex_replace", input: "{query}", find: "\\s+", replace: " " },
      { id: "trim", type: "transform", title: "Trim the ends", x: 560, y: 100, op: "trim", input: "{query}" },
      { id: "paste", type: "paste", title: "Replace the selection", x: 820, y: 100, text: "{query}" },
    ],
    connection: [
      { from: "selection", to: "collapse" },
      { from: "collapse", to: "trim" },
      { from: "trim", to: "paste" },
    ],
  });

const fruit = (): Workflow =>
  wf({
    name: "Script filter",
    description: "A keyword whose results come from a script (Python 3 here).",
    node: [
      { id: "filter", type: "script_filter", title: "Pick a fruit", x: 40, y: 100, keyword: "fruit", script: "filter.py", command: [] },
      { id: "notify", type: "notification", title: "Say what was picked", x: 340, y: 100, heading: "You picked", body: "{query}" },
    ],
    connection: [{ from: "filter", to: "notify" }],
  });

const store = new Map<string, Workflow>([
  ["duckduckgo", duckduckgo()],
  ["tidy-text", tidyText()],
  ["script-filter", fruit()],
]);
const approved = new Set<string>(["duckduckgo", "tidy-text"]);

const BROKEN = {
  folder: "loop",
  error: "a: the connections form a loop: a -> b -> a",
};

// ---- validation (a small copy of the Rust rules, for the preview) ---------

/** A few keywords other things answer, to show the clash warning in the preview. */
const TAKEN: Record<string, string> = {
  g: "web search Google",
  f: "the files search",
  b: "bookmarks",
};

/** The warning for a keyword something else answers too, or `null`. */
function clash(keyword: unknown): string | null {
  const word = String(keyword ?? "").trim();
  const owner = TAKEN[word.toLowerCase()];
  return owner ? `Keyword "${word}" is also used by ${owner}; both will show results.` : null;
}

export function check(workflow: Workflow): Problem[] {
  const problems: Problem[] = [];
  const error = (node: string | null, message: string) =>
    problems.push({ severity: "error", node, message });
  const warn = (node: string | null, message: string) =>
    problems.push({ severity: "warning", node, message });

  if (workflow.name.trim() === "") error(null, "the workflow needs a name");
  if (!workflow.node.some(isStart)) {
    error(null, "the workflow has no trigger: add a keyword, hotkey, Universal Actions, external trigger or script filter node");
  }
  const ids = new Set<string>();
  for (const node of workflow.node) {
    if (ids.has(node.id)) error(node.id, "this id is used twice");
    ids.add(node.id);
    if ((node.type === "keyword" || node.type === "script_filter") && !String(node.keyword ?? "").trim()) {
      error(node.id, "choose a keyword");
    } else if (node.type === "keyword" || node.type === "script_filter") {
      const message = clash(node.keyword);
      if (message) warn(node.id, message);
    }
    if (node.type === "open_url" && !String(node.url ?? "").trim()) error(node.id, "enter a link");
    if (node.type === "hotkey" && !String(node.key ?? "").trim()) error(node.id, "choose a shortcut");
  }
  const edges: [string, string][] = [];
  for (const conn of workflow.connection) {
    const from = workflow.node.find((n) => n.id === conn.from);
    const to = workflow.node.find((n) => n.id === conn.to);
    if (!from || !to) {
      error(null, `the connection ${conn.from} -> ${conn.to} points at a node that does not exist`);
      continue;
    }
    if (!outputPorts(from).includes(portOf(conn))) {
      error(from.id, `this node has no output "${portOf(conn)}"`);
      continue;
    }
    if (isStart(to)) {
      error(to.id, `a ${kindOf(to.type).label.toLowerCase()} has no input to connect to`);
      continue;
    }
    edges.push([conn.from, conn.to]);
  }
  // A loop, found by repeatedly removing nodes nothing points at.
  const remaining = new Set(workflow.node.map((n) => n.id));
  let changed = true;
  while (changed) {
    changed = false;
    for (const id of [...remaining]) {
      if (!edges.some(([from, to]) => to === id && remaining.has(from))) {
        remaining.delete(id);
        changed = true;
      }
    }
  }
  if (remaining.size > 0) {
    error([...remaining][0], `the connections form a loop through ${[...remaining].join(", ")}`);
  } else {
    const reached = new Set<string>(workflow.node.filter(isStart).map((n) => n.id));
    let grew = true;
    while (grew) {
      grew = false;
      for (const [from, to] of edges) {
        if (reached.has(from) && !reached.has(to)) {
          reached.add(to);
          grew = true;
        }
      }
    }
    for (const node of workflow.node) {
      if (isStart(node) && !edges.some(([from]) => from === node.id)) {
        warn(node.id, "nothing is connected after this trigger, so it does nothing");
      } else if (!isStart(node) && !reached.has(node.id)) {
        warn(node.id, "no trigger leads to this node, so it never runs");
      }
    }
  }
  return problems.sort((a, b) => Number(b.severity === "error") - Number(a.severity === "error"));
}

const hasErrors = (problems: Problem[]) => problems.some((p) => p.severity === "error");

// ---- the operations ----------------------------------------------------------

function summary(folder: string, workflow: Workflow): WorkflowSummary {
  const needs = workflow.node.some((n) => kindOf(n.type).needsApproval);
  return {
    folder,
    name: workflow.name,
    description: workflow.description,
    author: workflow.author,
    version: workflow.version,
    enabled: workflow.enabled,
    needs_approval: needs,
    approved: !needs || approved.has(folder),
    keywords: workflow.node
      .filter((n) => n.type === "keyword" || n.type === "script_filter")
      .map((n) => String(n.keyword)),
    nodes: workflow.node.length,
    warnings: check(workflow).filter((p) => p.severity === "warning").length,
    keyword_warnings: check(workflow)
      .filter((p) => p.severity === "warning" && p.message.startsWith("Keyword "))
      .map((p) => p.message),
    error: null,
    hotkey_errors: [],
  };
}

export function list(): WorkflowList {
  const rows = [...store].map(([folder, workflow]) => summary(folder, workflow));
  rows.push({
    folder: BROKEN.folder,
    name: BROKEN.folder,
    description: "",
    author: "",
    version: "",
    enabled: false,
    needs_approval: false,
    approved: false,
    keywords: [],
    nodes: 0,
    warnings: 0,
    keyword_warnings: [],
    error: BROKEN.error,
    hotkey_errors: [],
  });
  return { folder: "C:\\Users\\you\\AppData\\Roaming\\sevak\\workflows", workflows: rows };
}

export function load(folder: string): Result<LoadedWorkflow> {
  const workflow = store.get(folder);
  if (!workflow) return { ok: false, error: "cannot read the workflow: it does not exist" };
  const copy = JSON.parse(JSON.stringify(workflow)) as Workflow;
  return { ok: true, value: { folder, workflow: copy, problems: check(copy) } };
}

export function save(folder: string | null, workflow: Workflow): Result<SavedWorkflow> {
  const problems = check(workflow);
  const firstError = problems.find((p) => p.severity === "error");
  if (firstError) {
    return { ok: false, error: `The workflow has problems: ${firstError.message}` };
  }
  let target = folder;
  if (target === null) {
    const base = workflow.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "workflow";
    target = base;
    for (let n = 2; store.has(target); n++) target = `${base}-${n}`;
  }
  store.set(target, JSON.parse(JSON.stringify(workflow)) as Workflow);
  return { ok: true, value: { folder: target, problems } };
}

const TEMPLATES: (TemplateInfo & { workflow: () => Workflow })[] = [
  {
    id: "search-a-site",
    name: "Search a site",
    description: "A keyword that opens a website's search for what you type.",
    workflow: () =>
      wf({
        name: "Search a site",
        description: "Type the keyword and some text to search a website.",
        node: [
          { id: "keyword", type: "keyword", title: "Search Wikipedia for {query}", x: 40, y: 80, keyword: "wiki", argument: "required", subtitle: "Opens the search in your browser" },
          { id: "open", type: "open_url", title: "Open the search", x: 340, y: 80, url: "https://en.wikipedia.org/w/index.php?search={query}" },
        ],
        connection: [{ from: "keyword", to: "open" }],
      }),
  },
  {
    id: "open-url-with-query",
    name: "Open a URL with the query",
    description: "Branches on what you type: a number opens an issue, words search.",
    workflow: () =>
      wf({
        name: "Open a URL with the query",
        description: "A number opens that issue; any other text searches the issues.",
        node: [
          { id: "keyword", type: "keyword", title: "Open issue {query}", x: 40, y: 140, keyword: "issue", argument: "required", subtitle: "A number opens it, words search" },
          { id: "is-number", type: "conditional", title: "Is it a number?", x: 300, y: 140, left: "{query}", test: "matches", right: "^#?[0-9]+$" },
          { id: "strip", type: "transform", title: "Drop a leading #", x: 580, y: 60, op: "regex_replace", input: "{query}", find: "^#", replace: "" },
          { id: "open-issue", type: "open_url", title: "Open the issue", x: 860, y: 60, url: "https://github.com/ninad-k/Sevak/issues/{query}" },
          { id: "search", type: "open_url", title: "Search the issues", x: 580, y: 240, url: "https://github.com/ninad-k/Sevak/issues?q={query}" },
        ],
        connection: [
          { from: "keyword", to: "is-number" },
          { from: "is-number", port: "then", to: "strip" },
          { from: "strip", to: "open-issue" },
          { from: "is-number", port: "else", to: "search" },
        ],
      }),
  },
  {
    id: "script-filter",
    name: "Script filter",
    description: "Results from your own script; picking one runs the next steps.",
    workflow: fruit,
  },
];

export function templates(): TemplateInfo[] {
  return TEMPLATES.map(({ id, name, description }) => ({ id, name, description }));
}

export function create(template: string): Result<SavedWorkflow> {
  const found = TEMPLATES.find((t) => t.id === template);
  if (!found) return { ok: false, error: "unknown template" };
  return save(null, found.workflow());
}

export function remove(folder: string): Result<boolean> {
  return { ok: true, value: store.delete(folder) };
}

export function setEnabled(folder: string, enabled: boolean): Result<null> {
  const workflow = store.get(folder);
  if (workflow) workflow.enabled = enabled;
  return { ok: true, value: null };
}

export function review(folder: string): Result<boolean> {
  approved.add(folder);
  return { ok: true, value: true };
}

// ---- the gallery ----------------------------------------------------------------
// Use the shipped catalog so the browser preview stays in sync with packages.
const installed = new Set<string>(["duckduckgo"]);
type CatalogEntry = (typeof catalog.entries)[number];

// JSON imports widen `kind` to string, so filtering by it alone does not
// narrow away native entries, which use platforms instead of source/sha256.
function isZipEntry(entry: CatalogEntry): entry is CatalogEntry & {
  kind: GalleryKind;
  source: string;
  sha256: string;
} {
  return (entry.kind === "workflow" || entry.kind === "plugin")
    && typeof entry.source === "string" && typeof entry.sha256 === "string";
}

export function gallery(): Result<Gallery> {
  return {
    ok: true,
    value: {
      source: "https://raw.githubusercontent.com/ninad-k/Sevak/main/gallery/index.json",
      note: "Browser preview of the checkout catalog; released apps use their release catalog.",
      name: catalog.name,
      skipped: 0,
      entries: catalog.entries.filter(isZipEntry).map(entry => ({ ...entry, source: `https://raw.githubusercontent.com/ninad-k/Sevak/main/${entry.source}`, installed: installed.has(entry.id) })),
    },
  };
}

export function install(id: string): Result<InstalledEntry> {
  const entry = catalog.entries.filter(isZipEntry).find(entry => entry.id === id);
  if (!entry) return { ok: false, error: "Reload the gallery; that package is no longer listed." };
  installed.add(id);
  const folder = "folder" in entry && typeof entry.folder === "string" ? entry.folder : id;
  return { ok: true, value: { id, kind: entry.kind, folder } };
}
