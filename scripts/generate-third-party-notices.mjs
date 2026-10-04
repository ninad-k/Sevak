// Regenerates THIRD_PARTY_NOTICES.md: the licence inventory and licence texts of
// everything that ends up in a Sevak installer.
//
//   node scripts/generate-third-party-notices.mjs            write THIRD_PARTY_NOTICES.md
//   node scripts/generate-third-party-notices.mjs --check    fail if the file is out of date
//
// Needs `npm ci` to have run (for the npm packages) and the Cargo registry
// sources (`cargo metadata` downloads them). Nothing else to install.
//
// What it covers
//   Rust  every crate the `sevak` app links for the Windows, macOS (Intel and
//         Apple silicon) and Linux targets. `cargo metadata --filter-platform`
//         resolves each target triple; the results are unioned. Only normal
//         dependency edges are followed, so dev-dependencies and build-only
//         dependencies (tauri-build and what it pulls in) are left out.
//         Procedural-macro crates are normal edges and so are listed, which
//         over-reports slightly and never under-reports.
//   npm   the packages whose files are in the shipped UI bundle. Which ones is
//         read from the source map of a throwaway production `vite build`
//         (not from package.json, because Svelte's runtime is a devDependency
//         yet is bundled into the app). `--npm-packages a,b` skips the build.
//   Data  scripts/notices/data-and-assets.md is hand-written (WordNet, Unicode
//         and CLDR emoji data, colour palettes, icons) and copied in verbatim.
//
// Licence texts are taken from the package sources, de-duplicated (one entry
// per distinct text, listing every package it covers). Licences that need a
// maintainer's eye (copyleft, weak copyleft, unknown) are listed up front.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { bundledNpmNames } from "./lib/ui-bundle.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OUTPUT = join(root, "THIRD_PARTY_NOTICES.md");
const DATA_SECTION = join(root, "scripts", "notices", "data-and-assets.md");
const APP_PACKAGE = "sevak";

// Target triples of the shipped binaries -> the platform label used in the tables.
const TARGETS = [
  ["x86_64-pc-windows-msvc", "Windows"],
  ["x86_64-apple-darwin", "macOS"],
  ["aarch64-apple-darwin", "macOS"],
  ["x86_64-unknown-linux-gnu", "Linux"],
];
const PLATFORM_ORDER = ["Windows", "macOS", "Linux"];

const args = process.argv.slice(2);
const check = args.includes("--check");
const npmPackagesArg = args.find((a) => a.startsWith("--npm-packages="))?.split("=")[1];

// ---------------------------------------------------------------- licences

// SPDX ids by how they bind a redistributor. Anything not listed is "unknown".
const PERMISSIVE = new Set([
  "MIT", "MIT-0", "Apache-2.0", "ISC", "BSD-2-Clause", "BSD-3-Clause", "0BSD", "Zlib",
  "Unicode-3.0", "Unicode-DFS-2016", "CC0-1.0", "Unlicense", "BSL-1.0", "CDLA-Permissive-2.0",
  "Apache-2.0 WITH LLVM-exception", "BlueOak-1.0.0", "Python-2.0", "NCSA",
]);
const WEAK_COPYLEFT = new Set(["MPL-2.0", "MPL-1.1", "EPL-2.0", "EPL-1.0", "CDDL-1.0", "CDDL-1.1", "LGPL-2.1-only", "LGPL-2.1-or-later", "LGPL-3.0-only", "LGPL-3.0-or-later"]);
const STRONG_COPYLEFT = /^(A?GPL|SSPL|OSL|EUPL|CC-BY-(NC|SA)|.*-NC)/i;
const RISK = ["permissive", "weak copyleft", "copyleft", "unknown"];

function singleRisk(id) {
  const trimmed = id.trim();
  if (PERMISSIVE.has(trimmed) || PERMISSIVE.has(trimmed.replace(/\+$/, ""))) return 0;
  if (WEAK_COPYLEFT.has(trimmed)) return 1;
  if (STRONG_COPYLEFT.test(trimmed)) return 2;
  return 3;
}

// Risk of an SPDX expression: "A OR B" takes the better choice, "A AND B" the worse.
function expressionRisk(expression) {
  const tokens = expression
    .replace(/\//g, " OR ") // legacy "MIT/Apache-2.0"
    .replace(/\(/g, " ( ")
    .replace(/\)/g, " ) ")
    .split(/\s+/)
    .filter(Boolean);
  let i = 0;
  const atom = () => {
    if (tokens[i] === "(") {
      i++;
      const value = orExpr();
      i++; // ")"
      return value;
    }
    let id = tokens[i++];
    if (tokens[i] === "WITH") {
      id += ` WITH ${tokens[i + 1]}`;
      i += 2;
    }
    return singleRisk(id);
  };
  const andExpr = () => {
    let value = atom();
    while (tokens[i] === "AND") {
      i++;
      value = Math.max(value, atom());
    }
    return value;
  };
  const orExpr = () => {
    let value = andExpr();
    while (tokens[i] === "OR") {
      i++;
      value = Math.min(value, andExpr());
    }
    return value;
  };
  return tokens.length === 0 ? 3 : orExpr();
}

const LICENSE_FILE = /^(licen[cs]e|licen[cs]es|copying|unlicen[cs]e|notice|copyright|patents)([-_. ].*)?$/i;

function licenseFiles(dir) {
  const found = [];
  const visit = (current, depth) => {
    let entries;
    try {
      entries = readdirSync(current, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
      const path = join(current, entry.name);
      if (entry.isFile() && LICENSE_FILE.test(entry.name)) {
        // Skip build scripts and sources that merely look like licence files.
        if (/\.(rs|js|mjs|cjs|ts|json|toml|yml|yaml|sh|py|c|h)$/i.test(entry.name)) continue;
        if (statSync(path).size > 200_000) continue;
        found.push(path);
      } else if (entry.isDirectory() && depth === 0 && /^licen[cs]es?$/i.test(entry.name)) {
        visit(path, 1);
      }
    }
  };
  visit(dir, 0);
  return found;
}

function normalise(text) {
  return text.replace(/\r\n?/g, "\n").replace(/[ \t]+$/gm, "").trim();
}

// Standard texts for the few packages that ship no licence file at all. The
// copyright holders come from the package manifest and are listed in the table
// at the end of the file.
const STANDARD_TEXTS = {
  MIT: `MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`,
  "BSD-3-Clause": `BSD 3-Clause License

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its contributors
   may be used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.`,
  "BSL-1.0": `Boost Software License - Version 1.0

Permission is hereby granted, free of charge, to any person or organization
obtaining a copy of the software and accompanying documentation covered by
this license (the "Software") to use, reproduce, display, distribute,
execute, and transmit the Software, and to prepare derivative works of the
Software, and to permit third-parties to whom the Software is furnished to
do so, all subject to the following:

The copyright notices in the Software and this entire statement, including
the above license grant, this restriction and the following disclaimer,
must be included in all copies of the Software, in whole or in part, and
all derivative works of the Software, unless such copies or derivative
works are solely in the form of machine-executable object code generated by
a source language processor.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE, TITLE AND NON-INFRINGEMENT. IN NO EVENT
SHALL THE COPYRIGHT HOLDERS OR ANYONE DISTRIBUTING THE SOFTWARE BE LIABLE
FOR ANY DAMAGES OR OTHER LIABILITY, WHETHER IN CONTRACT, TORT OR OTHERWISE,
ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.`,
};
// Texts too long to keep here are borrowed from another package that ships them.
const BORROWED = {
  "Apache-2.0": /^\s*Apache License\s+Version 2\.0, January 2004/,
  "MPL-2.0": /^\s*Mozilla Public License Version 2\.0/,
};
const PREFERENCE = ["MIT", "Apache-2.0", "BSD-3-Clause", "BSL-1.0", "MPL-2.0"];

function standardLicense(pkg, pool) {
  const ids = pkg.license.replace(/[()]/g, " ").split(/\s+|\//).filter((t) => t && !["OR", "AND", "WITH"].includes(t));
  const id = PREFERENCE.find((candidate) => ids.includes(candidate));
  if (!id) throw new Error(`${pkg.name} ${pkg.version} ships no licence file and its licence (${pkg.license}) has no standard text here`);
  if (STANDARD_TEXTS[id]) return { id, text: STANDARD_TEXTS[id] };
  for (const { text } of pool.values()) if (BORROWED[id]?.test(text)) return { id, text };
  throw new Error(`no ${id} text found to reuse for ${pkg.name}`);
}

// ----------------------------------------------------------------- Rust

function run(command, commandArgs, options = {}) {
  const result = spawnSync(command, commandArgs, { cwd: root, encoding: "utf8", maxBuffer: 1 << 30, ...options });
  if (result.status !== 0) {
    throw new Error(`${command} ${commandArgs.join(" ")} failed:\n${result.stderr || result.error}`);
  }
  return result.stdout;
}

function rustPackages() {
  const byId = new Map(); // "name version" -> { pkg, platforms:Set }
  const workspace = new Set();
  for (const [triple, platform] of TARGETS) {
    const meta = JSON.parse(run("cargo", ["metadata", "--locked", "--format-version", "1", "--filter-platform", triple]));
    const packages = new Map(meta.packages.map((p) => [p.id, p]));
    for (const id of meta.workspace_members) workspace.add(packages.get(id).name);
    const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
    const start = meta.packages.find((p) => p.name === APP_PACKAGE && meta.workspace_members.includes(p.id));
    if (!start) throw new Error(`workspace package "${APP_PACKAGE}" not found`);
    const seen = new Set([start.id]);
    const queue = [start.id];
    while (queue.length) {
      for (const dep of nodes.get(queue.pop()).deps) {
        // Normal edges only: dev- and build-dependencies are not in the binary.
        if (!dep.dep_kinds.some((k) => k.kind === null)) continue;
        if (seen.has(dep.pkg)) continue;
        seen.add(dep.pkg);
        queue.push(dep.pkg);
      }
    }
    for (const id of seen) {
      const pkg = packages.get(id);
      if (meta.workspace_members.includes(id)) continue; // Sevak itself
      const key = `${pkg.name} ${pkg.version}`;
      if (!byId.has(key)) byId.set(key, { pkg, platforms: new Set() });
      byId.get(key).platforms.add(platform);
    }
  }
  return [...byId.values()]
    .map(({ pkg, platforms }) => ({
      ecosystem: "rust",
      name: pkg.name,
      version: pkg.version,
      license: pkg.license ?? (pkg.license_file ? `(see ${pkg.license_file})` : ""),
      authors: pkg.authors ?? [],
      repository: pkg.repository ?? pkg.homepage ?? "",
      dir: dirname(pkg.manifest_path),
      platforms: PLATFORM_ORDER.filter((p) => platforms.has(p)),
    }))
    .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
}

// ------------------------------------------------------------------ npm

function bundledNames() {
  return npmPackagesArg ? npmPackagesArg.split(",").filter(Boolean) : bundledNpmNames(root);
}

function npmPackages() {
  const lock = JSON.parse(readFileSync(join(root, "package-lock.json"), "utf8"));
  return bundledNames()
    .sort()
    .map((name) => {
      const entry = lock.packages[`node_modules/${name}`];
      if (!entry) throw new Error(`${name} is bundled but not in package-lock.json`);
      const dir = join(root, "node_modules", name);
      const manifest = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
      const repository = typeof manifest.repository === "string" ? manifest.repository : manifest.repository?.url ?? manifest.homepage ?? "";
      return {
        ecosystem: "npm",
        name,
        version: entry.version,
        license: typeof manifest.license === "string" ? manifest.license : entry.license ?? "",
        authors: [typeof manifest.author === "string" ? manifest.author : manifest.author?.name].filter(Boolean),
        repository: repository.replace(/^git\+/, "").replace(/\.git$/, ""),
        dir,
        platforms: PLATFORM_ORDER,
      };
    });
}

// --------------------------------------------------------------- output

function fence(text) {
  const longest = Math.max(2, ...[...text.matchAll(/`+/g)].map((m) => m[0].length));
  const bar = "`".repeat(longest + 1);
  return `${bar}text\n${text}\n${bar}`;
}

// A readable heading for a licence text: what kind of licence it is, then its
// first copyright line, which is what tells texts of the same kind apart.
function heading(text) {
  const kind =
    /Apache License/.test(text) && /Version 2\.0/.test(text) ? "Apache License 2.0"
    : /Mozilla Public License/.test(text) ? "Mozilla Public License 2.0"
    : /Boost Software License/.test(text) ? "Boost Software License 1.0"
    : /Unicode/i.test(text) && /Data Files|DFS/.test(text) ? "Unicode licence"
    : /Redistribution and use in source and binary forms/.test(text) ? "BSD-style licence"
    : /Permission to use, copy, modify, and\/or distribute this software for any/.test(text) ? "ISC licence"
    : /Permission is hereby granted, free of charge/.test(text) ? "MIT-style licence"
    : /This is free and unencumbered software/.test(text) ? "Unlicense"
    : /zlib/i.test(text) && /altered source versions/.test(text) ? "zlib licence"
    : null;
  const copyright = text
    .split("\n")
    .map((l) => l.trim())
    .find((l) => /^(copyright|©)/i.test(l) && !/notice|patent|retained|owner|holder|\{/i.test(l) && (kind !== "Apache License 2.0" || /\d{4}/.test(l)));
  const first = text.split("\n").find((l) => l.trim())?.trim() ?? "";
  const clean = (s) => s.slice(0, 80).replace(/[#`|<>]/g, "");
  if (kind) return copyright ? `${kind}, ${clean(copyright)}` : kind;
  return clean(copyright ?? first);
}

const escapeCell =(s) => String(s).replace(/\|/g, "\\|");

function generate() {
  const packages = [...rustPackages(), ...npmPackages()];

  // One entry per distinct licence text.
  const texts = new Map(); // hash -> { text, users: [] }
  const withoutText = [];
  for (const pkg of packages) {
    const files = licenseFiles(pkg.dir);
    if (files.length === 0) withoutText.push(pkg);
    for (const file of files) {
      const text = normalise(readFileSync(file, "utf8"));
      if (!text) continue;
      const hash = createHash("sha256").update(text).digest("hex");
      if (!texts.has(hash)) texts.set(hash, { text, users: new Set() });
      texts.get(hash).users.add(`${pkg.name} ${pkg.version}`);
    }
    pkg.risk = expressionRisk(pkg.license);
  }

  // Packages with no licence file get the standard text of their declared licence.
  for (const pkg of withoutText) {
    const { id, text } = standardLicense(pkg, texts);
    pkg.applied = id;
    const norm = normalise(text);
    const hash = createHash("sha256").update(norm).digest("hex");
    if (!texts.has(hash)) texts.set(hash, { text: norm, users: new Set() });
    texts.get(hash).users.add(`${pkg.name} ${pkg.version}`);
    texts.get(hash).standard = true;
  }

  const rust = packages.filter((p) => p.ecosystem === "rust");
  const npm = packages.filter((p) => p.ecosystem === "npm");
  const attention = packages.filter((p) => p.risk > 0);

  const counts = new Map();
  for (const pkg of packages) counts.set(pkg.license || "(none declared)", (counts.get(pkg.license || "(none declared)") ?? 0) + 1);

  const lines = [];
  const emit = (...l) => lines.push(...l);

  emit(
    "# Third-party notices",
    "",
    "<!-- Generated by scripts/generate-third-party-notices.mjs. Do not edit by hand:",
    "     change scripts/notices/data-and-assets.md for the data sections, then regenerate. -->",
    "",
    "Sevak is licensed under the [Apache License 2.0](LICENSE). It includes the",
    "third-party material listed here, each under its own licence.",
    "",
    "**How this file is made.** Everything after the data sections is generated by",
    "`node scripts/generate-third-party-notices.mjs` from `Cargo.lock`,",
    "`package-lock.json` and the packages' own licence files. The Rust inventory is",
    "the union of what the `sevak` app links on Windows, macOS (Intel and Apple",
    "silicon) and Linux, resolved with `cargo metadata --filter-platform` for each",
    "target; build-only and dev-only dependencies are left out. The npm inventory is",
    "the set of packages whose code is inside the shipped UI bundle, read from a",
    "production build's source map. Regenerate it after any dependency change; see",
    "[Supply chain](docs/security/supply-chain.md#regenerating-the-third-party-notices).",
    "",
  );

  emit(readFileSync(DATA_SECTION, "utf8").trim(), "");

  emit("## Licences that need attention", "");
  if (attention.length === 0) {
    emit("None: every dependency is available under a permissive licence (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode and similar).", "");
  } else {
    emit("These dependencies are not purely permissive, or declare no recognisable licence.", "", "| Package | Version | Licence | Class |", "|---|---|---|---|");
    for (const pkg of attention) emit(`| ${escapeCell(pkg.name)} | ${pkg.version} | ${escapeCell(pkg.license || "(none declared)")} | ${RISK[pkg.risk]} |`);
    emit("");
    if (attention.some((p) => /MPL/.test(p.license))) {
      emit(
        "MPL-2.0 is a file-level (weak) copyleft licence: changes to the MPL-licensed files themselves must be",
        "published under the MPL, but it does not reach Sevak's own code. Sevak uses these crates unmodified, as",
        "published on crates.io, and their source code is available there and from each package's repository; the",
        "full licence text is under \"Licence texts\" below.",
        "",
      );
    }
  }

  emit("## Licence summary", "", "| Declared licence | Packages |", "|---|---|");
  for (const [license, count] of [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))) emit(`| ${escapeCell(license)} | ${count} |`);
  emit("");

  const table = (list) => {
    emit("| Package | Version | Licence | Platforms |", "|---|---|---|---|");
    for (const pkg of list) emit(`| ${escapeCell(pkg.name)} | ${pkg.version} | ${escapeCell(pkg.license || "(none declared)")} | ${pkg.platforms.length === PLATFORM_ORDER.length ? "all" : pkg.platforms.join(", ")} |`);
    emit("");
  };
  emit(`## npm packages in the UI bundle (${npm.length})`, "");
  table(npm);
  emit(`## Rust crates (${rust.length})`, "");
  table(rust);

  emit("## Licence texts", "", "Each distinct licence or notice text appears once, followed by the packages it comes from.", "");
  const sorted = [...texts.entries()].sort(([ha, a], [hb, b]) => b.users.size - a.users.size || a.text.localeCompare(b.text) || ha.localeCompare(hb));
  sorted.forEach(([, { text, users, standard }], index) => {
    emit(`### ${index + 1}. ${heading(text)}`, "", `Used by: ${[...users].sort().join(", ")}`, "");
    if (standard) emit('Some of these packages ship no licence file, so this is the standard text of their declared licence; their copyright holders are listed under "Packages that ship no licence file".', "");
    emit(fence(text), "");
  });

  if (withoutText.length > 0) {
    emit(
      "## Packages that ship no licence file",
      "",
      "These packages declare a licence in their manifest but include no licence text in their published sources.",
      'The standard text of the licence named under "Applied" appears above, and the authors named in the manifest are the copyright holders.',
      "",
      "| Package | Version | Declared licence | Applied | Authors | Source |",
      "|---|---|---|---|---|---|",
    );
    for (const pkg of withoutText) emit(`| ${escapeCell(pkg.name)} | ${pkg.version} | ${escapeCell(pkg.license || "(none declared)")} | ${pkg.applied} | ${escapeCell(pkg.authors.join(", ") || "(none listed)")} | ${escapeCell(pkg.repository)} |`);
    emit("");
  }

  return { markdown: `${lines.join("\n").replace(/\n{3,}/g, "\n\n").trimEnd()}\n`, rust: rust.length, npm: npm.length, texts: texts.size, attention, withoutText };
}

const result = generate();
if (check) {
  const current = existsSync(OUTPUT) ? readFileSync(OUTPUT, "utf8").replace(/\r\n/g, "\n") : "";
  if (current !== result.markdown) {
    console.error("THIRD_PARTY_NOTICES.md is out of date. Run: node scripts/generate-third-party-notices.mjs");
    process.exit(1);
  }
  console.log("THIRD_PARTY_NOTICES.md is up to date.");
} else {
  writeFileSync(OUTPUT, result.markdown);
  console.log(`Wrote THIRD_PARTY_NOTICES.md: ${result.rust} Rust crates, ${result.npm} npm packages, ${result.texts} distinct licence texts.`);
  for (const pkg of result.attention) console.log(`  attention: ${pkg.ecosystem} ${pkg.name} ${pkg.version}: ${pkg.license || "(none declared)"} [${RISK[pkg.risk]}]`);
  for (const pkg of result.withoutText) console.log(`  no licence file: ${pkg.ecosystem} ${pkg.name} ${pkg.version} (${pkg.license})`);
}
