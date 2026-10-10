// Read source manifests as TOML data. Unknown or missing workflow content must
// never result in a claim that the workflow runs no code.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { parse } from "smol-toml";

const DIRECTORIES = { workflow: "workflows", plugin: "plugins", native: "native" };
const SAFE_FOLDER = /^[a-z0-9-]{1,64}$/;
const SAFE_KEYWORD = /^[A-Za-z0-9:_.-]{1,24}$/;
const DATA_NODES = new Set([
  "keyword", "hotkey", "selection", "external", "open_url", "copy", "paste",
  "set_variable", "transform", "conditional", "delay", "notification", "large_type", "text_view",
]);
const CODE_NODES = new Set(["run_script", "script_filter", "terminal_command", "system_command", "open_file", "launch_app"]);

export function readManifest(root, entry) {
  const directory = DIRECTORIES[entry.kind];
  const folder = entry.kind === "native" ? entry.id : (entry.folder ?? entry.id);
  if (!directory || typeof folder !== "string" || !SAFE_FOLDER.test(folder)) return null;
  try {
    return parse(readFileSync(join(root, "examples", directory, folder,
      entry.kind === "workflow" ? "workflow.toml" : "plugin.toml"), "utf8"), { unsafeKeyBehaviour: "throw" });
  } catch {
    return null;
  }
}

export function manifestKeywords(root, entry) {
  const manifest = readManifest(root, entry);
  const values = entry.kind === "workflow"
    ? (Array.isArray(manifest?.node) ? manifest.node.filter((n) => n?.type === "keyword").map((n) => n.keyword) : [])
    : [manifest?.keyword];
  return [...new Set(values.filter((k) => typeof k === "string" && SAFE_KEYWORD.test(k)))];
}

export function workflowBehavior(root, entry) {
  const nodes = readManifest(root, entry)?.node;
  if (!Array.isArray(nodes) || nodes.length === 0) return { trust: "unknown", securityReview: true };
  if (nodes.some((node) => CODE_NODES.has(node?.type))) return { trust: "script", securityReview: true };
  if (nodes.some((node) => !DATA_NODES.has(node?.type))) return { trust: "unknown", securityReview: true };
  return { trust: "none", securityReview: nodes.some((node) => node.type === "paste") };
}
