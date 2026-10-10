// Checks the opt-in galleries in gallery/ without building anything:
//
//   node scripts/gallery-check.mjs            report problems (exit 1 if there are any)
//   node scripts/gallery-check.mjs --update   first copy the SHA-256 of every committed
//                                             package and theme file into the index
//   node scripts/gallery-check.mjs --base origin/main
//                                             also compare with the index at that git
//                                             ref: a changed package needs a higher
//                                             `version`, an id may not change kind
//   node scripts/gallery-check.mjs --summary report.md --json report.json
//                                             also write the automated review report
//                                             (what gallery-review.yml shows on a PR)
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
//
// Problems fail the check. Warnings do not: they are things a human reviewer must
// look at (a new permission, an unusual licence, a removed entry). The script
// only reads files; it never runs anything from the entries it checks.
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const argv = process.argv.slice(2);
/** The value after `--name` (or `--name=value`), or undefined. */
function option(name) {
  const at = argv.findIndex((arg) => arg === `--${name}` || arg.startsWith(`--${name}=`));
  if (at < 0) return undefined;
  return argv[at].includes("=") ? argv[at].slice(name.length + 3) : argv[at + 1];
}
// By default the repository is the one this script lives in. `--root <dir>` checks
// another checkout with this copy of the script: the pull request workflow runs
// the script from the base branch over the contributor's files, so a pull request
// cannot change the rules that judge it.
const root = option("root") ? resolve(option("root")) + sep : fileURLToPath(new URL("../", import.meta.url));
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

const update = argv.includes("--update");
const baseRef = option("base");
const summaryPath = option("summary");
const jsonPath = option("json");
const problems = [];
const warnings = [];
/** What differs from the base ref: { gallery, id, change, ... } */
const changes = [];
const problem = (where, message) => problems.push(`${where}: ${message}`);
const warn = (where, message) => warnings.push(`${where}: ${message}`);

const MAX_NAME_CHARS = 60;
const MAX_DESCRIPTION_CHARS = 300;
const MAX_DEPRECATED_CHARS = 200;
/** Tags that mean "made by the maintainers"; only author "Sevak" may use them. */
const RESERVED_TAGS = ["official", "verified", "featured"];
/** Every field an index entry may have (the app's `Entry` plus the optional deprecation pair). */
const ENTRY_FIELDS = [
  "id", "kind", "name", "description", "author", "version", "tags", "source", "sha256",
  "homepage", "folder", "platforms", "license", "repository", "min_sevak", "permissions",
  "deprecated", "replaced_by",
];
/** Licences a maintainer must look at twice, because the gallery is distributed under Apache-2.0. */
const LICENSE_FLAGGED = /(^|[^A-Za-z])(A?GPL|LGPL|SSPL|BUSL|EUPL|CC-BY-NC|CC-BY-ND|Proprietary|NOASSERTION|NONE|UNLICENSED)/i;
/** An SPDX licence expression: ids joined by AND, OR, WITH (parentheses are stripped first). */
const SPDX = /^[A-Za-z0-9][A-Za-z0-9.+-]*(\s+(AND|OR|WITH)\s+[A-Za-z0-9][A-Za-z0-9.+-]*)*$/;

/** Windows reserves these names; mirrors `sevak_core::safe_names::is_reserved_device_name`. */
function isReservedDeviceName(name) {
  const base = name.split(/[.:]/)[0].trim().toUpperCase();
  if (["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "CLOCK$"].includes(base)) return true;
  return /^(COM|LPT)[1-9¹²³]$/.test(base);
}

/** "1", "1.2" or "1.2.3" to [1, 2, 3]; null for anything else. */
function parseVersion(text) {
  const match = /^(\d+)(?:\.(\d+))?(?:\.(\d+))?$/.exec(String(text ?? ""));
  return match ? [match[1], match[2] ?? 0, match[3] ?? 0].map(Number) : null;
}
/** Negative, zero or positive like a comparator; null when either side is not a version. */
function compareVersions(a, b) {
  const [x, y] = [parseVersion(a), parseVersion(b)];
  if (!x || !y) return null;
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] - y[i];
  return 0;
}

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
  for (const item of readdirSync(join(gallery, folder), { withFileTypes: true })) {
    if (item.isSymbolicLink()) problem(where, `${folder}/${item.name} is a symbolic link`);
    else if (!files.has(item.name)) problem(where, `${folder}/${item.name} is not listed`);
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
  if (
    !Array.isArray(permissions) ||
    permissions.length > 8 ||
    new Set(permissions).size !== permissions.length ||
    !permissions.every((p) => /^[a-z][a-z0-9-]{0,23}$/.test(p))
  ) {
    problem(where, "permissions must be at most 8 unique lower case words (network, filesystem, ...)");
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

/** An id is valid for the app and safe as a folder name on every platform. */
function checkId(where, id) {
  if (!/^[a-z0-9-]{1,48}$/.test(id ?? "")) problem(where, "the id may only use a-z, 0-9 and -");
  else if (isReservedDeviceName(id)) problem(where, `the id ${id} is a Windows device name`);
}

/** Rules for every kind of index entry (the kind-specific ones follow in the loop). */
function checkCommon(where, entry) {
  for (const field of Object.keys(entry)) {
    if (!ENTRY_FIELDS.includes(field)) warn(where, `unknown field "${field}" (a typo? the app ignores it)`);
  }
  if (typeof entry.name === "string" && entry.name.length > MAX_NAME_CHARS) {
    problem(where, `name is longer than ${MAX_NAME_CHARS} characters`);
  }
  if (typeof entry.description === "string" && entry.description.length > MAX_DESCRIPTION_CHARS) {
    problem(where, `description is longer than ${MAX_DESCRIPTION_CHARS} characters`);
  }
  if (typeof entry.version === "string" && entry.version.trim() && !parseVersion(entry.version)) {
    problem(where, "version must be numeric like 1.2 or 1.2.3");
  }
  if (entry.homepage !== undefined && !String(entry.homepage).startsWith("https://")) {
    problem(where, "homepage must be an https:// address");
  }

  // The licence is an SPDX expression. It is required where the entry is not
  // written by the project itself, and always for a native extension.
  const license = entry.license;
  if (license === undefined) {
    if (entry.author !== "Sevak" && entry.kind !== "native") problem(where, "license is required (an SPDX id such as MIT or Apache-2.0)");
  } else if (typeof license !== "string" || !SPDX.test(license.replace(/[()]/g, " ").trim())) {
    if (entry.kind !== "native" || (typeof license === "string" && license.trim())) {
      problem(where, `license "${license}" is not an SPDX expression (MIT, Apache-2.0, MIT OR Apache-2.0, ...)`);
    }
  } else if (LICENSE_FLAGGED.test(license)) {
    warn(where, `licence ${license} needs a maintainer's decision: the gallery is distributed under Apache-2.0`);
  }

  // Tags: the generic rules are in the loop. These are about meaning.
  const tags = Array.isArray(entry.tags) ? entry.tags : [];
  for (const tag of tags) {
    if (RESERVED_TAGS.includes(tag) && entry.author !== "Sevak") {
      problem(where, `the tag ${tag} is reserved for entries made by the maintainers`);
    }
  }
  if (tags.includes("no-code") && entry.kind !== "workflow") problem(where, "only a workflow can be tagged no-code");

  // Deprecation is optional and ignored by builds that do not know it.
  if (entry.deprecated !== undefined) {
    if (typeof entry.deprecated !== "string" || !entry.deprecated.trim() || entry.deprecated.length > MAX_DEPRECATED_CHARS) {
      problem(where, `deprecated must be a short reason (1 to ${MAX_DEPRECATED_CHARS} characters)`);
    }
  }
  if (entry.replaced_by !== undefined) {
    if (entry.deprecated === undefined) problem(where, "replaced_by needs deprecated");
    if (entry.replaced_by === entry.id) problem(where, "replaced_by points at the entry itself");
  }
}

/** The `keyword = "..."` values in an entry's manifest (a plugin's, or each trigger node's of a workflow). */
function manifestKeywords(entry) {
  const folderName = entry.kind === "plugin" ? "plugins" : "workflows";
  const manifest = entry.kind === "plugin" ? "plugin.toml" : "workflow.toml";
  const path = join(root, "examples", folderName, entry.folder ?? entry.id, manifest);
  if (!existsSync(path)) return [];
  const text = readFileSync(path, "utf8");
  return [...text.matchAll(/^\s*keyword\s*=\s*"([^"]+)"/gm)].map((match) => match[1].toLowerCase());
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
    checkId(where, entry.id);
    if (ids.has(entry.id)) problem(where, "the id is used twice");
    ids.add(entry.id);
    if (!["workflow", "plugin", "native"].includes(entry.kind)) problem(where, `unknown kind ${entry.kind}`);
    checkCommon(where, entry);
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

  // Keywords are how people reach an entry, so two entries may not share one.
  // (The Rust tests also check them against the built-in plugins and engines;
  // a native extension's keyword is inside its package and is checked there.)
  const keywordOwners = new Map();
  for (const entry of index.entries ?? []) {
    if (entry.kind === "native") continue;
    for (const keyword of new Set(manifestKeywords(entry))) {
      if (!keywordOwners.has(keyword)) keywordOwners.set(keyword, []);
      keywordOwners.get(keyword).push(entry.id);
    }
  }
  for (const [keyword, owners] of keywordOwners) {
    if (owners.length > 1) problem("index.json", `the keyword "${keyword}" is used by ${owners.join(" and ")}`);
  }

  // A replacement must exist, and a chain of replacements must end.
  const byId = new Map((index.entries ?? []).map((entry) => [entry.id, entry]));
  for (const entry of index.entries ?? []) {
    if (entry.replaced_by === undefined || entry.replaced_by === entry.id) continue;
    const where = `index.json ${entry.id}`;
    if (!byId.has(entry.replaced_by)) {
      problem(where, `replaced_by names ${entry.replaced_by}, which is not in the index`);
    } else if (byId.get(entry.replaced_by).deprecated !== undefined) {
      warn(where, `its replacement ${entry.replaced_by} is deprecated too`);
    }
  }
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
    checkId(where, entry.id);
    if (ids.has(entry.id)) problem(where, "the id is used twice");
    ids.add(entry.id);
    if ((entry.name ?? "").length > MAX_NAME_CHARS) problem(where, `name is longer than ${MAX_NAME_CHARS} characters`);
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

// ---- comparison with the base ref ----------------------------------------------
/** The file at `gallery/<name>` in `baseRef`, parsed; `{}` if the ref has no such file. */
function readBase(name) {
  try {
    const text = execFileSync("git", ["show", `${baseRef}:gallery/${name}`], {
      cwd: root,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
    return JSON.parse(text);
  } catch {
    try {
      execFileSync("git", ["rev-parse", "--verify", "--quiet", `${baseRef}^{commit}`], { cwd: root, stdio: "ignore" });
      return {};
    } catch {
      problem("--base", `cannot read ${baseRef}: not a ref of this repository (is the history fetched?)`);
      return null;
    }
  }
}

/** Does adding or changing this entry need the security review (it runs code)? */
function runsCode(entry) {
  if (entry.kind !== "workflow") return true;
  const path = join(root, "examples", "workflows", entry.folder ?? entry.id, "workflow.toml");
  if (!existsSync(path)) return false;
  return /^\s*type\s*=\s*"(run_script|paste)"/m.test(readFileSync(path, "utf8"));
}

/** A string that changes exactly when the downloaded bytes of an entry change. */
const fingerprint = (entry) =>
  entry.kind === "native"
    ? JSON.stringify(Object.entries(entry.platforms ?? {}).map(([platform, a]) => [platform, a?.sha256]).sort())
    : String(entry.sha256 ?? "");

function compareEntries(base) {
  const before = new Map((base.entries ?? []).map((entry) => [entry.id, entry]));
  const now = new Map((index.entries ?? []).map((entry) => [entry.id, entry]));
  for (const entry of index.entries ?? []) {
    const where = `index.json ${entry.id}`;
    const old = before.get(entry.id);
    if (!old) {
      changes.push({ gallery: "index", id: entry.id, change: "new", kind: entry.kind, version: entry.version, security_review: runsCode(entry) });
      continue;
    }
    if (old.kind !== entry.kind) problem(where, `the kind changed from ${old.kind} to ${entry.kind}; remove the entry and submit a new id`);
    if (old.author !== entry.author) warn(where, `the author changed from "${old.author}" to "${entry.author}": confirm the transfer with both`);
    if ((old.license ?? "") !== (entry.license ?? "")) warn(where, `the licence changed from "${old.license ?? ""}" to "${entry.license ?? ""}"`);
    const added = (entry.permissions ?? []).filter((p) => !(old.permissions ?? []).includes(p));
    if (added.length > 0) warn(where, `new declared permissions: ${added.join(", ")}`);

    const order = compareVersions(entry.version, old.version);
    const changed = fingerprint(entry) !== fingerprint(old);
    if (changed) {
      if (order === null) problem(where, "the package changed and the version is not comparable with the base's");
      else if (order === 0) problem(where, `the package changed but version is still ${entry.version}: bump it`);
      else if (order < 0) problem(where, `version went down from ${old.version} to ${entry.version}`);
      changes.push({ gallery: "index", id: entry.id, change: "updated", kind: entry.kind, from: old.version, version: entry.version, security_review: runsCode(entry) });
    } else if (order !== null && order < 0) {
      problem(where, `version went down from ${old.version} to ${entry.version}`);
    } else if (order !== null && order > 0) {
      warn(where, `version went from ${old.version} to ${entry.version} but the package is unchanged`);
    } else if (entry.deprecated !== old.deprecated || entry.replaced_by !== old.replaced_by) {
      changes.push({ gallery: "index", id: entry.id, change: entry.deprecated ? "deprecated" : "undeprecated", kind: entry.kind, version: entry.version, security_review: false });
    }
  }
  for (const [id, old] of before) {
    if (now.has(id)) continue;
    changes.push({ gallery: "index", id, change: "removed", kind: old.kind, version: old.version, security_review: false });
    warn(`index.json ${id}`, "removed from the index (a maintainer decision: see docs/marketplace/updating-and-removal.md)");
  }
}

function compareThemes(base) {
  const before = new Map((base.themes ?? []).map((entry) => [entry.id, entry]));
  const now = new Set();
  for (const entry of themes.themes ?? []) {
    now.add(entry.id);
    const old = before.get(entry.id);
    if (!old) changes.push({ gallery: "themes", id: entry.id, change: "new", kind: "theme", security_review: false });
    else if (old.sha256 !== entry.sha256) changes.push({ gallery: "themes", id: entry.id, change: "updated", kind: "theme", security_review: false });
  }
  for (const id of before.keys()) {
    if (now.has(id)) continue;
    changes.push({ gallery: "themes", id, change: "removed", kind: "theme", security_review: false });
    warn(`themes.json ${id}`, "removed from the gallery (a maintainer decision)");
  }
}

if (argv.some((arg) => arg === "--base" || arg.startsWith("--base="))) {
  if (!baseRef || baseRef.startsWith("-")) {
    problem("--base", "needs a ref, for example --base origin/main");
  } else {
    const baseIndex = readBase("index.json");
    const baseThemes = readBase("themes.json");
    if (baseIndex && baseThemes) {
      compareEntries(baseIndex);
      compareThemes(baseThemes);
    }
  }
}

// ---- the report --------------------------------------------------------------
const summary = `${index.entries.length} workflows, plugins and native extensions, ${themes.themes.length} themes`;

function markdownReport() {
  const lines = ["## Automated gallery review", ""];
  lines.push(problems.length === 0 ? "**Result: passed.**" : `**Result: ${problems.length} problem(s) must be fixed.**`, "");
  lines.push(`Checked ${summary}.${baseRef ? ` Compared with \`${baseRef}\`.` : ""}`, "");
  if (changes.length > 0) {
    lines.push("### What this change touches", "", "| Gallery | Id | Change | Version | Needs security review |", "|---|---|---|---|---|");
    for (const c of changes) {
      const version = c.from ? `${c.from} → ${c.version}` : (c.version ?? "");
      lines.push(`| ${c.gallery} | \`${c.id}\` | ${c.change} (${c.kind}) | ${version} | ${c.security_review ? "yes" : "no"} |`);
    }
    lines.push("");
  } else if (baseRef) {
    lines.push("This change does not add, update or remove any index entry.", "");
  }
  const list = (title, items) => {
    if (items.length === 0) return;
    lines.push(`### ${title}`, "", ...items.map((item) => `- ${item}`), "");
  };
  list("Problems (fail the check)", problems);
  list("Warnings (for the human reviewer)", warnings);
  lines.push("These checks read files only; they say nothing about whether the content is safe. See `docs/marketplace/review-process.md`.");
  return lines.join("\n") + "\n";
}

if (update && problems.length === 0) {
  writeJson("index.json", index);
  writeJson("themes.json", themes);
}
if (summaryPath) writeFileSync(summaryPath, markdownReport());
if (jsonPath) {
  const report = {
    ok: problems.length === 0,
    problems,
    warnings,
    changes,
    needs_security_review: changes.some((c) => c.security_review),
  };
  writeFileSync(jsonPath, JSON.stringify(report, null, 2) + "\n");
}
if (warnings.length > 0) console.error(warnings.map((line) => `  warning: ${line}`).join("\n"));
if (problems.length > 0) {
  console.error(problems.map((line) => `  ${line}`).join("\n"));
  console.error(`\n${problems.length} problem(s) in gallery/. Hashes can be refreshed with --update.`);
  process.exit(1);
}
console.log(`gallery ok: ${summary}` + (update ? " (hashes refreshed)" : ""));
