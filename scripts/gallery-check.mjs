// Checks the opt-in galleries in gallery/ without building anything:
//
//   node scripts/gallery-check.mjs            report problems (exit 1 if there are any)
//   node scripts/gallery-check.mjs --update   first copy the SHA-256 of every committed
//                                             package and theme file into the index
//
// For each entry of gallery/index.json (format 2) and gallery/themes.json
// (version 2) it checks the address (a path relative to the repository root, such
// as gallery/packages/<id>.zip; the app resolves it against the release it was
// built from, see docs/security/gallery-trust.md), that the file exists, that its SHA-256 matches, and that no file in gallery/packages or
// gallery/themes is missing from the index. A native extension (kind "native")
// has one `.sevakext` package per platform under gallery/extensions/<id>/ and
// is checked the same way, platform by platform. It reads the files exactly as they
// are committed (they are stored with LF line endings; see .gitattributes).
//
// The Rust tests (`cargo test -p sevak-plugins -p sevak-core`) go further: they
// install every package, validate every workflow, run the workflows and plugin
// scripts and check every theme's contrast. This script is the quick check a
// contributor can run before that, and the way to refresh hashes after
// re-packing (`cargo run -p sevak-plugins --example gallery_pack -- <folder> <zip>`).
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const gallery = join(root, "gallery");
/** Where an entry points: a path below the repository root, never an address. */
const BASE = "gallery/";
const TREE = "https://github.com/ninad-k/Sevak/tree/main/examples/";
const MAX_PACKAGE_BYTES = 5 * 1024 * 1024; // the app refuses larger downloads
// A native extension carries a compiled program. The app accepts up to 32 MiB;
// the gallery asks for less because every package is committed to the repository.
const MAX_NATIVE_PACKAGE_BYTES = 10 * 1024 * 1024;
const MAX_THEME_BYTES = 64 * 1024;
const MAX_TAGS = 8;
/** The platforms a native extension may have a build for (`<os>-<arch>`). */
const PLATFORMS = [
  "windows-x86_64",
  "windows-aarch64",
  "macos-x86_64",
  "macos-aarch64",
  "linux-x86_64",
  "linux-aarch64",
];

const update = process.argv.includes("--update");
const problems = [];
const problem = (where, message) => problems.push(`${where}: ${message}`);

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const readJson = (name) => JSON.parse(readFileSync(join(gallery, name), "utf8"));
const writeJson = (name, value) =>
  writeFileSync(join(gallery, name), JSON.stringify(value, null, 2) + "\n");

/** `gallery/<folder>/<file>` to `<file>`, or null when it points elsewhere. */
function fileIn(url, folder) {
  const prefix = `${BASE}${folder}/`;
  if (typeof url !== "string" || !url.startsWith(prefix)) return null;
  const file = url.slice(prefix.length);
  return file && !/[/?#\\\s]/.test(file) ? file : null;
}

function listed(where, folder, files) {
  const names = readdirSync(join(gallery, folder));
  for (const name of names) {
    if (!files.has(name)) problem(where, `${folder}/${name} is not listed`);
  }
}

/**
 * A native extension: one `.sevakext` package per platform in
 * gallery/extensions/<id>/, each listed with its SHA-256. (The Rust tests open
 * the packages and compare their manifests with the entry; this only needs
 * the files.)
 */
function checkNative(where, entry, dirs) {
  if (entry.source !== undefined || entry.sha256 !== undefined) {
    problem(where, "a native extension lists `platforms`, not `source` and `sha256`");
  }
  if (!/^\d+\.\d+\.\d+$/.test(entry.version ?? "")) problem(where, "version must be like 1.2.3");
  if (typeof entry.license !== "string" || !entry.license.trim()) problem(where, "license is empty");
  if (!String(entry.repository ?? "").startsWith("https://")) problem(where, "repository must be an https:// address");
  if (entry.min_sevak !== undefined && !/^\d+\.\d+\.\d+$/.test(entry.min_sevak)) {
    problem(where, "min_sevak must be like 1.2.3");
  }
  const permissions = entry.permissions ?? [];
  if (!Array.isArray(permissions) || !permissions.every((p) => /^[a-z][a-z0-9-]{0,23}$/.test(p))) {
    problem(where, "permissions must be a list of lower case words (network, filesystem, ...)");
  }
  if (entry.folder !== undefined && entry.folder !== entry.id) problem(where, "a native extension has no folder");
  const platforms = entry.platforms;
  if (typeof platforms !== "object" || platforms === null || Object.keys(platforms).length === 0) {
    problem(where, "platforms must name at least one platform");
    return;
  }
  dirs.set(entry.id, new Set());
  for (const [platform, artifact] of Object.entries(platforms)) {
    if (!PLATFORMS.includes(platform)) {
      problem(where, `unknown platform ${platform} (known: ${PLATFORMS.join(", ")})`);
      continue;
    }
    const prefix = `${BASE}extensions/${entry.id}/`;
    const source = artifact?.source;
    const file = typeof source === "string" && source.startsWith(prefix) ? source.slice(prefix.length) : null;
    if (!file || !file.endsWith(".sevakext") || /[/?#\\\s]/.test(file)) {
      problem(where, `${platform}: source must be ${prefix}<file>.sevakext (a path, not an address)`);
      continue;
    }
    dirs.get(entry.id).add(file);
    const path = join(gallery, "extensions", entry.id, file);
    if (!existsSync(path)) {
      problem(where, `${platform}: extensions/${entry.id}/${file} does not exist`);
      continue;
    }
    const bytes = readFileSync(path);
    if (bytes.length > MAX_NATIVE_PACKAGE_BYTES) problem(where, `${platform}: the package is larger than 10 MiB`);
    const actual = sha256(bytes);
    if (artifact.sha256 !== actual) {
      if (update) artifact.sha256 = actual;
      else problem(where, `${platform}: sha256 is ${artifact.sha256} but extensions/${entry.id}/${file} hashes to ${actual}`);
    }
  }
}

// ---- gallery/index.json: workflows, script plugins and native extensions ------
const index = readJson("index.json");
if (index.format !== 2) problem("index.json", `format is ${index.format}, expected 2`);
/** native extension id -> the package files its entry lists */
const nativeDirs = new Map();
{
  const ids = new Set();
  const zips = new Set();
  for (const [position, entry] of (index.entries ?? []).entries()) {
    const where = `index.json ${entry.id ?? `entry ${position + 1}`}`;
    if (!/^[a-z0-9-]{1,48}$/.test(entry.id ?? "")) problem(where, "the id may only use a-z, 0-9 and -");
    if (ids.has(entry.id)) problem(where, "the id is used twice");
    ids.add(entry.id);
    if (!["workflow", "plugin", "native"].includes(entry.kind)) problem(where, `unknown kind ${entry.kind}`);
    for (const field of ["name", "description", "author", "version"]) {
      if (typeof entry[field] !== "string" || !entry[field].trim()) problem(where, `${field} is empty`);
    }
    const tags = entry.tags ?? [];
    if (!Array.isArray(tags) || tags.length === 0 || tags.length > MAX_TAGS) {
      problem(where, `tags must be a list of 1 to ${MAX_TAGS} labels`);
    } else if (!tags.every((tag) => /^[a-z0-9-]{1,24}$/.test(tag)) || new Set(tags).size !== tags.length) {
      problem(where, "tags must be unique, lower case (a-z, 0-9, -) and at most 24 characters");
    }

    if (entry.kind === "native") {
      checkNative(where, entry, nativeDirs);
      continue;
    }
    if (entry.platforms !== undefined) problem(where, "only native extensions have platforms");

    const folderName = entry.kind === "plugin" ? "plugins" : "workflows";
    const source = join(root, "examples", folderName, entry.folder ?? entry.id);
    const manifest = entry.kind === "plugin" ? "plugin.toml" : "workflow.toml";
    if (!existsSync(join(source, manifest))) problem(where, `examples/${folderName}/${entry.folder ?? entry.id}/${manifest} is missing`);
    if (entry.homepage !== undefined && !String(entry.homepage).startsWith(TREE)) {
      problem(where, `homepage should be under ${TREE}`);
    }

    const file = fileIn(entry.source, "packages");
    if (!file || !file.endsWith(".zip")) {
      problem(where, `source must be ${BASE}packages/<file>.zip (a path, not an address)`);
      continue;
    }
    zips.add(file);
    const path = join(gallery, "packages", file);
    if (!existsSync(path)) {
      problem(where, `packages/${file} does not exist`);
      continue;
    }
    const bytes = readFileSync(path);
    if (bytes.length > MAX_PACKAGE_BYTES) problem(where, "the package is larger than 5 MiB");
    const actual = sha256(bytes);
    if (entry.sha256 !== actual) {
      if (update) entry.sha256 = actual;
      else problem(where, `sha256 is ${entry.sha256} but packages/${file} hashes to ${actual}`);
    }
  }
  listed("index.json", "packages", zips);
  // gallery/extensions/<id>/ exists only for a listed native extension, and
  // holds only the packages its entry lists.
  const extensionsRoot = join(gallery, "extensions");
  if (existsSync(extensionsRoot)) {
    for (const id of readdirSync(extensionsRoot)) {
      if (!nativeDirs.has(id)) {
        problem("index.json", `extensions/${id} has no native entry in the index`);
        continue;
      }
      listed(`index.json ${id}`, `extensions/${id}`, nativeDirs.get(id));
    }
  }
}

// ---- gallery/themes.json: themes -----------------------------------------------
const themes = readJson("themes.json");
if (themes.version !== 2) problem("themes.json", `version is ${themes.version}, expected 2`);
{
  const ids = new Set();
  const files = new Set();
  for (const [position, entry] of (themes.themes ?? []).entries()) {
    const where = `themes.json ${entry.id ?? `entry ${position + 1}`}`;
    if (!/^[a-z0-9-]{1,48}$/.test(entry.id ?? "")) problem(where, "the id may only use a-z, 0-9 and -");
    if (ids.has(entry.id)) problem(where, "the id is used twice");
    ids.add(entry.id);
    for (const field of ["name", "author", "description"]) {
      if (typeof entry[field] !== "string" || !entry[field].trim()) problem(where, `${field} is empty`);
    }
    if ((entry.description ?? "").length > 200) problem(where, "description is longer than 200 characters");
    if (!["light", "dark"].includes(entry.mode)) problem(where, `mode must be light or dark, not ${entry.mode}`);

    const file = fileIn(entry.url, "themes");
    if (!file || !file.endsWith(".toml")) {
      problem(where, `url must be ${BASE}themes/<file>.toml (a path, not an address)`);
      continue;
    }
    files.add(file);
    const path = join(gallery, "themes", file);
    if (!existsSync(path)) {
      problem(where, `themes/${file} does not exist`);
      continue;
    }
    const bytes = readFileSync(path);
    const text = bytes.toString("utf8");
    if (bytes.length > MAX_THEME_BYTES) problem(where, "the theme file is larger than 64 KiB");
    if (text.includes("\r")) problem(where, `themes/${file} must use LF line endings`);
    const palettes = ["light", "dark"].filter((mode) => new RegExp(`^\\[${mode}\\]$`, "m").test(text));
    if (palettes.length !== 1 || palettes[0] !== entry.mode) {
      problem(where, `mode ${entry.mode} does not match the palettes in the file (${palettes.join(", ") || "none"})`);
    }
    const actual = sha256(bytes);
    if (entry.sha256 !== actual) {
      if (update) entry.sha256 = actual;
      else problem(where, `sha256 is ${entry.sha256} but themes/${file} hashes to ${actual}`);
    }
  }
  listed("themes.json", "themes", files);
}

if (update && problems.length === 0) {
  writeJson("index.json", index);
  writeJson("themes.json", themes);
}
if (problems.length > 0) {
  console.error(problems.map((line) => `  ${line}`).join("\n"));
  console.error(`\n${problems.length} problem(s) in gallery/. Hashes can be refreshed with --update.`);
  process.exit(1);
}
console.log(
  `gallery ok: ${index.entries.length} workflows, plugins and native extensions, ${themes.themes.length} themes` +
    (update ? " (hashes refreshed)" : ""),
);
