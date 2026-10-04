#!/usr/bin/env node
// Generates the marketplace website from the galleries.
//
//   node scripts/build-marketplace.mjs [--out landing/marketplace]
//   npm run marketplace
//
// Reads gallery/index.json and gallery/themes.json and writes static files:
//   <out>/index.html                     searchable, filterable list
//   <out>/<kind>/<id>/index.html         one page per entry (kind: workflow, plugin, native, theme)
//   <out>/catalog.json                   the same data for other tools
//   <out>/assets/marketplace.{css,js}    styles and the filter script
//   <out>/../sitemap.xml                 the product page, the marketplace and every entry
//
// The output is NOT committed (it is in .gitignore): .github/workflows/docs.yml
// runs this before it copies landing/ to the root of the Pages site.
//
// Gallery entries are written by third parties, so every value is escaped
// where it is put into HTML and only https:// addresses become links.
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { CSS, JS } from "./lib/marketplace-assets.mjs";

export const SITE = "https://ninad-k.github.io/Sevak/";
export const DOCS = `${SITE}docs/`;
export const REPO = "https://github.com/ninad-k/Sevak";
export const SUBMIT_URL = `${REPO}/issues/new?template=extension_submission.yml`;
export const PUBLISHING_URL = `${DOCS}marketplace/publishing/`;
const RAW = "https://raw.githubusercontent.com/ninad-k/Sevak/main/";
const MARKER = ".generated-by-build-marketplace";

/** Kinds in display order. `dir` is where the source of an example lives. */
export const KINDS = {
  workflow: { label: "Workflow", plural: "Workflows", dir: "examples/workflows" },
  plugin: { label: "Script plugin", plural: "Script plugins", dir: "examples/plugins" },
  native: { label: "Native extension", plural: "Native extensions", dir: null },
  theme: { label: "Theme", plural: "Themes", dir: null },
};

const REQUIREMENTS = {
  "needs-python": "Python 3 on your command path",
  "needs-node": "Node.js 22 or newer on your command path",
};

// ---------------------------------------------------------------- escaping

const ESC = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;", "`": "&#96;" };
/** Escapes text for HTML content and for quoted attribute values. */
export const esc = (value) => String(value ?? "").replace(/[&<>"'`]/g, (c) => ESC[c]);

/** The normalised address if `value` is an https:// URL without credentials, else null. */
export function httpsUrl(value) {
  if (typeof value !== "string" || value.length > 2000 || /[\u0000- \u007f"'<>\\^`{|}]/.test(value)) return null;
  try {
    const url = new URL(value);
    if (url.protocol !== "https:" || url.username || url.password || !url.hostname) return null;
    return url.href;
  } catch {
    return null;
  }
}

/** A repository-relative path made of safe segments, else null. */
export function repoPath(value) {
  if (typeof value !== "string" || value.length > 300) return null;
  const parts = value.split("/");
  if (!parts.every((p) => /^[\p{L}\p{N}][\p{L}\p{N}._ -]*$/u.test(p) && p !== "..")) return null;
  return parts.join("/");
}

const encodePath = (path) => path.split("/").map(encodeURIComponent).join("/");
const SAFE_ID = /^[a-z0-9][a-z0-9-]{0,63}$/;
const SAFE_TAG = /^[a-z0-9][a-z0-9-]{0,31}$/;
const SAFE_KEYWORD = /^[A-Za-z0-9:_.-]{1,24}$/;
const text = (v, max) => (typeof v === "string" ? v.replace(/[\u0000-\u001f\u007f]/g, " ").trim().slice(0, max) : "");

// ---------------------------------------------------------------- catalog

/** What running the entry means for the person installing it. */
export function trustOf(entry) {
  if (entry.kind === "native") return "native";
  if (entry.kind === "plugin") return "script";
  if (entry.kind === "workflow" && entry.tags.some((t) => t === "needs-python" || t === "needs-node")) return "script";
  return "none";
}

export const TRUST = {
  none: { label: "Runs no code", detail: "It is data only: Sevak interprets it and no program of the author's is started. Workflows that paste into another app still ask for your permission once." },
  script: { label: "Runs a script", detail: "A script on your computer runs when you use the keyword, but only after you allow it. Sevak shows the exact command and asks once; if the files change it asks again." },
  native: { label: "Native program", detail: "A compiled program that runs with your user's rights. Sevak asks for your permission before it runs and shows the permissions its author declares, but it cannot confine the program: install only what you trust." },
};

/** Keyword(s) from the example's manifest, when the example folder is in this checkout. */
function keywordsOf(root, kind, id, folder) {
  const dir = KINDS[kind]?.dir;
  if (!dir) return [];
  const manifest = join(root, dir, folder || id, kind === "plugin" ? "plugin.toml" : "workflow.toml");
  if (!existsSync(manifest)) return [];
  const found = [];
  for (const line of readFileSync(manifest, "utf8").split(/\r?\n/)) {
    const m = /^\s*keyword\s*=\s*"([^"]*)"/.exec(line);
    if (m && SAFE_KEYWORD.test(m[1]) && !found.includes(m[1])) found.push(m[1]);
  }
  return found;
}

function normalise(raw, kind, root) {
  const id = typeof raw.id === "string" ? raw.id : "";
  if (!SAFE_ID.test(id)) throw new Error(`${kind} entry has an id that is not a-z, 0-9 and dashes: ${JSON.stringify(raw.id)}`);
  const tags = (Array.isArray(raw.tags) ? raw.tags : []).filter((t) => typeof t === "string" && SAFE_TAG.test(t)).slice(0, 8);
  const entry = {
    id,
    kind,
    name: text(raw.name, 120) || id,
    description: text(raw.description, 600),
    author: text(raw.author, 80) || "Unknown",
    version: text(raw.version, 32),
    tags,
    license: text(raw.license, 60) || null,
    homepage: httpsUrl(raw.homepage),
    repository: httpsUrl(raw.repository),
    permissions: (Array.isArray(raw.permissions) ? raw.permissions : []).map((p) => text(p, 40)).filter(Boolean).slice(0, 20),
    minSevak: text(raw.min_sevak, 32) || null,
    mode: raw.mode === "light" || raw.mode === "dark" ? raw.mode : null,
    source: repoPath(raw.source ?? raw.url),
    sha256: /^[0-9a-f]{64}$/.test(raw.sha256 ?? "") ? raw.sha256 : null,
    platforms: [],
    keywords: [],
  };
  if (kind === "theme") entry.tags = [...new Set([...(entry.mode ? [entry.mode] : []), ...tags])];
  if (raw.platforms && typeof raw.platforms === "object") {
    for (const [name, p] of Object.entries(raw.platforms)) {
      if (!/^[a-z0-9_-]{1,40}$/.test(name)) continue;
      entry.platforms.push({ platform: name, source: repoPath(p?.source), sha256: /^[0-9a-f]{64}$/.test(p?.sha256 ?? "") ? p.sha256 : null });
    }
    entry.platforms.sort((a, b) => a.platform.localeCompare(b.platform));
  }
  entry.keywords = keywordsOf(root, kind, id, typeof raw.folder === "string" && SAFE_ID.test(raw.folder) ? raw.folder : null);
  entry.trust = trustOf(entry);
  entry.page = `${SITE}marketplace/${kind}/${id}/`;
  return entry;
}

/** The merged, normalised list of every entry in the two galleries. */
export function buildCatalog(index, themes, root = ".") {
  const out = [];
  for (const raw of index?.entries ?? []) {
    const kind = raw?.kind;
    if (kind !== "workflow" && kind !== "plugin" && kind !== "native") continue; // a kind this site does not know
    out.push(normalise(raw, kind, root));
  }
  for (const raw of themes?.themes ?? []) out.push(normalise(raw, "theme", root));
  const seen = new Set();
  for (const e of out) {
    const key = `${e.kind}/${e.id}`;
    if (seen.has(key)) throw new Error(`duplicate marketplace entry ${key}`);
    seen.add(key);
  }
  return out;
}

/** The machine-readable catalog. Contains no HTML; consumers must escape on their own. */
export function catalogJson(entries) {
  return {
    format: 1,
    name: "Sevak marketplace",
    site: `${SITE}marketplace/`,
    submit: SUBMIT_URL,
    entries: entries.map((e) => ({
      id: e.id,
      kind: e.kind,
      name: e.name,
      description: e.description,
      author: e.author,
      version: e.version || null,
      tags: e.tags,
      trust: e.trust,
      license: e.license,
      permissions: e.permissions,
      min_sevak: e.minSevak,
      keywords: e.keywords,
      homepage: e.homepage,
      repository: e.repository,
      page: e.page,
      source: e.source,
      sha256: e.sha256,
      ...(e.platforms.length ? { platforms: Object.fromEntries(e.platforms.map((p) => [p.platform, { source: p.source, sha256: p.sha256 }])) } : {}),
    })),
  };
}

// ---------------------------------------------------------------- html

const link = (url, label, extra = "") => {
  const safe = httpsUrl(url);
  return safe ? `<a href="${esc(safe)}" rel="noopener noreferrer"${extra}>${esc(label)}</a>` : "";
};

function shell({ title, description, canonical, depth, body, scripts = "", ogType = "website", current = false }) {
  const up = "../".repeat(depth);
  const site = `${up}../`;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(title)}</title>
<meta name="description" content="${esc(description)}">
<link rel="canonical" href="${esc(canonical)}">
<meta name="color-scheme" content="dark light">
<meta name="theme-color" content="#111020">
<link rel="icon" type="image/png" href="${site}assets/sevak-icon.png">
<meta property="og:type" content="${ogType}">
<meta property="og:site_name" content="Sevak">
<meta property="og:url" content="${esc(canonical)}">
<meta property="og:title" content="${esc(title)}">
<meta property="og:description" content="${esc(description)}">
<meta property="og:image" content="${SITE}assets/og-image.png">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="627">
<meta property="og:image:alt" content="Sevak's search bar with app results, next to the tagline Your desktop. At your service.">
<meta name="twitter:card" content="summary_large_image">
<link rel="stylesheet" href="${up}assets/marketplace.css">
</head>
<body>
<a class="skip" href="#main">Skip to content</a>
<header class="nav">
  <div class="wrap">
    <a class="brand" href="${site}"><img src="${site}assets/sevak-icon.png" alt="" width="30" height="30">Sevak</a>
    <nav class="nav-links" aria-label="Main">
      <a href="${site}">Home</a>
      <a href="${up || "./"}"${current ? ' aria-current="page"' : ""}>Marketplace</a>
      <a href="${site}docs/">Docs</a>
      <a href="${esc(REPO)}">GitHub</a>
      <a class="btn btn-primary" href="${esc(SUBMIT_URL)}" rel="noopener">Submit your extension</a>
    </nav>
  </div>
</header>
<main id="main">
${body}
</main>
<footer>
  <div class="wrap">
    <span>Sevak · Apache License 2.0. Listings are reviewed, not endorsed: read what a package does before you install it.</span>
    <nav aria-label="Footer">
      <a href="${site}docs/marketplace/">About the marketplace</a>
      <a href="${esc(PUBLISHING_URL)}">Publish an extension</a>
      <a href="${site}docs/security/gallery-trust/">Gallery trust</a>
      <a href="${up}catalog.json">catalog.json</a>
    </nav>
  </div>
</footer>
${scripts}
</body>
</html>
`;
}

function submitBlock() {
  return `<aside class="submit" aria-labelledby="submit-title">
  <div>
    <h2 id="submit-title">Submit your extension</h2>
    <p>Built a workflow, script plugin, native extension or theme? Open a submission and a maintainer reviews it against the gallery rules. Read the publishing guide first.</p>
  </div>
  <div class="cta">
    <a class="btn btn-primary" href="${esc(SUBMIT_URL)}" rel="noopener">Submit your extension</a>
    <a class="btn" href="${esc(PUBLISHING_URL)}">Publishing guide</a>
  </div>
</aside>`;
}

function badges(e) {
  return `<span class="badge kind">${esc(KINDS[e.kind].label)}</span><span class="badge trust-${e.trust}">${esc(TRUST[e.trust].label)}</span>`;
}

function card(e, i) {
  const search = [e.name, e.description, e.author, e.id, ...e.tags, ...e.keywords, KINDS[e.kind].label].join(" ").toLowerCase();
  const source = e.homepage ?? e.repository;
  return `<li class="card" data-card data-kind="${esc(e.kind)}" data-name="${esc(e.name)}" data-tags="${esc(e.tags.join(" "))}" data-index="${i}" data-text="${esc(search)}">
  <div class="badges">${badges(e)}</div>
  <h3><a href="${esc(e.kind)}/${esc(e.id)}/">${esc(e.name)}</a></h3>
  <p class="desc">${esc(e.description)}</p>
  <p class="meta">by ${esc(e.author)}${e.version ? ` · v${esc(e.version)}` : ""}${e.license ? ` · ${esc(e.license)}` : ""}${e.permissions.length ? ` · permissions: ${esc(e.permissions.join(", "))}` : ""}</p>
  ${e.tags.length ? `<ul class="tags" aria-label="Tags">${e.tags.map((t) => `<li>${esc(t)}</li>`).join("")}</ul>` : ""}
  <p class="links"><a href="${esc(e.kind)}/${esc(e.id)}/">Details<span class="sr-only"> of ${esc(e.name)}</span></a>${source ? ` ${link(source, "Source")}` : ""}</p>
</li>`;
}

export function renderIndex(entries) {
  const counts = Object.fromEntries(Object.keys(KINDS).map((k) => [k, entries.filter((e) => e.kind === k).length]));
  const tags = [...new Set(entries.flatMap((e) => e.tags))].sort();
  const chips = [
    `<button type="button" class="chip" data-kind-chip="all" aria-pressed="true">All (${entries.length})</button>`,
    ...Object.entries(KINDS)
      .filter(([k]) => counts[k] > 0)
      .map(([k, v]) => `<button type="button" class="chip" data-kind-chip="${k}" aria-pressed="false">${esc(v.plural)} (${counts[k]})</button>`),
  ].join("\n        ");
  const body = `<section class="hero">
  <div class="wrap">
    <p class="kicker">Marketplace</p>
    <h1>Extend Sevak</h1>
    <p class="lede">Workflows, script plugins, native extensions and themes, reviewed in the open and installed from inside Sevak. Every listing says whether it runs code, and every download is checked against a published SHA-256.</p>
    <div class="cta">
      <a class="btn btn-primary" href="${esc(SUBMIT_URL)}" rel="noopener">Submit your extension</a>
      <a class="btn" href="${esc(PUBLISHING_URL)}">How publishing works</a>
    </div>
  </div>
</section>
<div class="wrap" data-market>
  <div class="controls" data-controls hidden>
    <div class="row">
      <div class="field grow">
        <label for="q">Search</label>
        <input id="q" type="search" data-search autocomplete="off" placeholder="Name, keyword, author or what you want to do">
      </div>
      <div class="field">
        <label for="tag">Tag</label>
        <select id="tag" data-tag>
          <option value="all">All tags</option>
          ${tags.map((t) => `<option value="${esc(t)}">${esc(t)}</option>`).join("\n          ")}
        </select>
      </div>
      <div class="field">
        <label for="sort">Sort by</label>
        <select id="sort" data-sort>
          <option value="recommended">Recommended</option>
          <option value="name">Name A to Z</option>
          <option value="name-desc">Name Z to A</option>
        </select>
      </div>
    </div>
    <div>
      <div class="label" id="kind-label">Kind</div>
      <div class="chips" role="group" aria-labelledby="kind-label">
        ${chips}
      </div>
    </div>
  </div>
  <p class="status" role="status" aria-live="polite" data-status>${entries.length} listed</p>
  <ul class="grid" data-list aria-label="Marketplace listings">
${entries.map(card).join("\n")}
  </ul>
  <div class="empty" data-empty hidden>
    <p>Nothing matches those filters.</p>
    <button type="button" class="btn" data-reset>Clear the filters</button>
  </div>
  ${submitBlock()}
  <p class="note">The machine-readable list is <a href="catalog.json">catalog.json</a>. Released builds of Sevak read the gallery pinned to their own release, so a new listing reaches the app with the next release.</p>
</div>`;
  return shell({
    title: "Sevak Marketplace: workflows, plugins, native extensions and themes",
    description: `Browse ${entries.length} reviewed extensions for the Sevak launcher: workflows, script plugins, native extensions and themes. Each one states whether it runs code.`,
    canonical: `${SITE}marketplace/`,
    depth: 0,
    body,
    scripts: `<script src="assets/marketplace.js" defer></script>`,
    current: true,
  });
}

function installSteps(e) {
  const keyword = e.keywords.length
    ? `Open the launcher and type ${e.keywords.map((k) => `<code>${esc(k)}</code>`).join(" or ")} followed by a space.`
    : "Open the launcher and type the keyword from the description, followed by a space.";
  const allow = e.trust === "script" ? "Allow it when Sevak asks (it shows the exact command)." : "";
  const find = `find <strong>${esc(e.name)}</strong>`;
  switch (e.kind) {
    case "theme":
      return [
        "Open <strong>Settings → Appearance → Theme editor</strong>.",
        "Choose <strong>Browse online themes</strong>. This is the only request Sevak makes, and only when you click.",
        `${find[0].toUpperCase()}${find.slice(1)} and choose <strong>Install</strong>. Sevak checks the SHA-256 before saving the file to your themes folder.`,
        "Select the theme in the editor to apply it.",
      ];
    case "native":
      return [
        "Open <strong>Settings → Extensions</strong> (or type <code>ext</code> in the launcher).",
        "Choose <strong>Load the list</strong>, then " + find + ".",
        "Choose <strong>Install native extension</strong>. Sevak downloads the package for your platform and checks its SHA-256.",
        "Read the permissions it declares and allow it when Sevak asks.",
        keyword,
      ];
    default:
      return [
        "Open <strong>Settings → Gallery</strong>.",
        "Choose <strong>Load gallery</strong>, then " + find + ".",
        "Choose <strong>Install</strong>. Sevak checks the SHA-256 before it saves anything.",
        ...(allow ? [allow] : []),
        keyword,
      ].filter(Boolean);
  }
}

function verifyBlock(e) {
  const files = e.platforms.length
    ? e.platforms.map((p) => ({ label: p.platform, source: p.source, sha256: p.sha256 }))
    : [{ label: null, source: e.source, sha256: e.sha256 }];
  const usable = files.filter((f) => f.source && f.sha256);
  if (!usable.length) return "<p>No checksum is published for this listing.</p>";
  const rows = usable
    .map((f) => {
      const url = `${RAW}${encodePath(f.source)}`;
      return `<tr>${e.platforms.length ? `<td>${esc(f.label)}</td>` : ""}<td><a href="${esc(url)}" rel="noopener noreferrer">${esc(f.source.split("/").pop())}</a></td><td class="hash">${esc(f.sha256)}</td></tr>`;
    })
    .join("");
  const name = esc(usable[0].source.split("/").pop());
  return `<p>Sevak does this for you on every install and refuses a file that does not match. To check it yourself, download the file and compare its SHA-256 with the value below.</p>
<table><thead><tr>${e.platforms.length ? "<th>Platform</th>" : ""}<th>File</th><th>SHA-256</th></tr></thead><tbody>${rows}</tbody></table>
<pre class="code">Windows (PowerShell)   Get-FileHash -Algorithm SHA256 ${name}
macOS                  shasum -a 256 ${name}
Linux                  sha256sum ${name}</pre>
<p class="note">The hash is of the file as committed to the repository. Released builds of Sevak read the list pinned to their own release.</p>`;
}

function issueUrl(e, what) {
  return `${REPO}/issues/new?title=${encodeURIComponent(`${what}: ${e.name} (${e.kind}/${e.id})`)}`;
}

export function renderDetail(e) {
  const reqs = e.tags.filter((t) => REQUIREMENTS[t]).map((t) => REQUIREMENTS[t]);
  const sourceLink = e.homepage ?? e.repository;
  const sourceFile = e.source ? `${REPO}/blob/main/${encodePath(e.source)}` : null;
  const meta = [
    ["Kind", esc(KINDS[e.kind].label)],
    ["Author", esc(e.author)],
    e.version ? ["Version", esc(e.version)] : null,
    e.license ? ["Licence", esc(e.license)] : null,
    e.minSevak ? ["Needs Sevak", esc(e.minSevak) + " or newer"] : null,
    e.mode ? ["Mode", esc(e.mode)] : null,
    e.platforms.length ? ["Platforms", e.platforms.map((p) => esc(p.platform)).join(", ")] : null,
    e.permissions.length ? ["Declared permissions", e.permissions.map((p) => `<code>${esc(p)}</code>`).join(" ")] : null,
  ].filter(Boolean);
  const body = `<div class="wrap">
  <nav class="crumbs" aria-label="Breadcrumb"><a href="../../">Marketplace</a> › ${esc(KINDS[e.kind].plural)} › ${esc(e.name)}</nav>
  <div class="detail">
    <article>
      <div class="badges">${badges(e)}</div>
      <h1>${esc(e.name)}</h1>
      <p class="lede">${esc(e.description)}</p>
      ${e.tags.length ? `<ul class="tags" aria-label="Tags">${e.tags.map((t) => `<li>${esc(t)}</li>`).join("")}</ul>` : ""}

      <h2>Install in Sevak</h2>
      <ol class="steps">
${installSteps(e).map((s) => `        <li>${s}</li>`).join("\n")}
      </ol>
      <p class="note">Nothing is requested until you click, and no account is needed. A new listing reaches released builds with the next release.</p>

      ${e.keywords.length ? `<h2>Keyword to type</h2>\n      <p>${e.keywords.map((k) => `<code>${esc(k)}</code>`).join(" ")}</p>` : ""}

      ${reqs.length ? `<h2>Requirements</h2>\n      <ul>${reqs.map((r) => `<li>${esc(r)}</li>`).join("")}</ul>` : ""}

      <h2>What to expect: ${esc(TRUST[e.trust].label.toLowerCase())}</h2>
      <p>${esc(e.kind === "theme" ? "It is a plain color file: it holds no code and nothing in it can run. Sevak checks that it parses and shows contrast ratings in the editor." : TRUST[e.trust].detail)} Read <a href="../../../docs/security/gallery-trust/">Gallery trust</a> for how listings are checked.</p>

      <h2>Verify the download</h2>
      ${verifyBlock(e)}

      <h2>Source</h2>
      <p>${[sourceLink && link(sourceLink, "Browse the source folder"), sourceFile && link(sourceFile, "View the package file"), e.repository && e.repository !== sourceLink && link(e.repository, "Project repository")].filter(Boolean).join(" · ") || "No public source address is listed."}</p>

      <h2>Problems and changes</h2>
      <div class="actions">
        ${link(issueUrl(e, "Problem"), "Report a problem", ' class="btn"')}
        ${link(issueUrl(e, "Change request"), "Request a change", ' class="btn"')}
      </div>
    </article>
    <aside class="side" aria-label="Details">
      <dl>
${meta.map(([k, v]) => `        <div><dt>${k}</dt><dd>${v}</dd></div>`).join("\n")}
      </dl>
    </aside>
  </div>
  ${submitBlock()}
</div>`;
  return shell({
    title: `${e.name} · Sevak Marketplace`,
    description: `${e.description} ${TRUST[e.trust].label}. By ${e.author}.`.slice(0, 300),
    canonical: e.page,
    depth: 2,
    body,
    ogType: "article",
  });
}

export function renderSitemap(entries) {
  const urls = [SITE, `${SITE}marketplace/`, DOCS, ...entries.map((e) => e.page)];
  return `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls.map((u) => `  <url><loc>${esc(u)}</loc></url>`).join("\n")}\n</urlset>\n`;
}

// ---------------------------------------------------------------- build

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const put = (path, data) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, data);
};

/** Writes the whole site into `out` (replacing a previous build) and returns the catalog. */
export function build({ root, out }) {
  const entries = buildCatalog(readJson(join(root, "gallery", "index.json")), readJson(join(root, "gallery", "themes.json")), root);
  if (existsSync(out)) {
    // Only ever delete what a previous run wrote.
    if (!existsSync(join(out, MARKER)) && readdirSync(out).length > 0) {
      throw new Error(`${out} exists and was not generated by this script; refusing to replace it`);
    }
    rmSync(out, { recursive: true, force: true });
  }
  put(join(out, MARKER), "Generated by scripts/build-marketplace.mjs. Safe to delete.\n");
  put(join(out, "index.html"), renderIndex(entries));
  for (const e of entries) put(join(out, e.kind, e.id, "index.html"), renderDetail(e));
  put(join(out, "catalog.json"), JSON.stringify(catalogJson(entries), null, 2) + "\n");
  put(join(out, "assets", "marketplace.css"), CSS + "\n");
  put(join(out, "assets", "marketplace.js"), JS + "\n");
  put(join(dirname(out), "sitemap.xml"), renderSitemap(entries));
  return entries;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const at = process.argv.indexOf("--out");
  const out = resolve(root, at > 0 ? process.argv[at + 1] : "landing/marketplace");
  try {
    const entries = build({ root, out });
    console.log(`marketplace: ${entries.length} entries written to ${out}`);
  } catch (err) {
    console.error(`marketplace: ${err.message}`);
    process.exit(1);
  }
}
