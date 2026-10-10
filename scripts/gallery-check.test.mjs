// Tests of the native-extension rules of scripts/gallery-check.mjs. The script
// finds the repository from its own location, so each test builds a tiny
// repository in a temporary folder (the script, a gallery with one native
// extension and no workflows or themes) and runs the script there.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("./gallery-check.mjs", import.meta.url));
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

const LINUX = Buffer.from("linux package bytes");
const MAC = Buffer.from("mac package bytes");

function entry(over = {}) {
  return {
    id: "tool",
    kind: "native",
    name: "Tool",
    description: "A tool for the tests.",
    author: "Ada",
    version: "1.2.3",
    license: "MIT",
    min_sevak: "0.1.0",
    permissions: ["network"],
    repository: "https://github.com/example/tool",
    tags: ["native"],
    platforms: {
      "linux-x86_64": {
        source: "gallery/extensions/tool/tool-1.2.3-linux-x86_64.sevakext",
        sha256: sha256(LINUX),
      },
      "macos-aarch64": {
        source: "gallery/extensions/tool/tool-1.2.3-macos-aarch64.sevakext",
        sha256: sha256(MAC),
      },
    },
    ...over,
  };
}

const vcs = (repo, ...args) =>
  spawnSync("git", ["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", ...args], {
    cwd: repo,
    encoding: "utf8",
  });

/**
 * Builds a repository with `entries`, runs the check there and returns the outcome.
 * With `base`, the repository first commits an index holding those entries and the
 * check then runs with `--base HEAD`; with `reports`, it also writes the report files.
 */
function check(entries, { extra = () => {}, args = [], base, reports = false } = {}) {
  const repo = mkdtempSync(join(tmpdir(), "gallery-check-"));
  try {
    mkdirSync(join(repo, "scripts"));
    cpSync(script, join(repo, "scripts", "gallery-check.mjs"));
    const gallery = join(repo, "gallery");
    mkdirSync(join(gallery, "packages"), { recursive: true });
    mkdirSync(join(gallery, "themes"));
    mkdirSync(join(gallery, "extensions", "tool"), { recursive: true });
    writeFileSync(join(gallery, "extensions", "tool", "tool-1.2.3-linux-x86_64.sevakext"), LINUX);
    writeFileSync(join(gallery, "extensions", "tool", "tool-1.2.3-macos-aarch64.sevakext"), MAC);
    writeFileSync(join(gallery, "index.json"), JSON.stringify({ format: 2, entries }, null, 2));
    writeFileSync(join(gallery, "themes.json"), JSON.stringify({ version: 2, themes: [] }, null, 2));
    extra(gallery, repo);
    const extraArgs = [...args];
    if (base) {
      writeFileSync(join(gallery, "index.json"), JSON.stringify({ format: 2, entries: base }, null, 2));
      vcs(repo, "init", "-q");
      vcs(repo, "add", "-A");
      vcs(repo, "commit", "-q", "-m", "base");
      writeFileSync(join(gallery, "index.json"), JSON.stringify({ format: 2, entries }, null, 2));
      extraArgs.push("--base", "HEAD");
    }
    if (reports) extraArgs.push("--summary", join(repo, "report.md"), "--json", join(repo, "report.json"));
    const run = spawnSync(process.execPath, [join(repo, "scripts", "gallery-check.mjs"), ...extraArgs], {
      encoding: "utf8",
    });
    const read = (name) => (reports ? readFileSync(join(repo, name), "utf8") : undefined);
    return {
      ...run,
      index: () => JSON.parse(readFileSync(join(gallery, "index.json"), "utf8")),
      markdown: read("report.md"),
      json: reports ? JSON.parse(read("report.json")) : undefined,
    };
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
}

test("a native entry whose packages match is accepted", () => {
  const run = check([entry()]);
  assert.equal(run.status, 0, run.stderr);
  assert.match(run.stdout, /gallery ok: 1 workflows, plugins and native extensions/);
});

test("a wrong hash is reported, and --update corrects it", () => {
  const bad = entry();
  bad.platforms["linux-x86_64"].sha256 = "0".repeat(64);
  const run = check([bad]);
  assert.equal(run.status, 1);
  assert.match(run.stderr, /linux-x86_64: sha256 is 0{64} but extensions\/tool\/.* hashes to /);
  const fixed = check([bad], { args: ["--update"] });
  assert.equal(fixed.status, 0, fixed.stderr);
});

test("a missing package, an unknown platform and an address instead of a path are reported", () => {
  const missing = entry();
  missing.platforms["linux-x86_64"].source = "gallery/extensions/tool/not-there.sevakext";
  assert.match(check([missing]).stderr, /does not exist/);

  const unknown = entry();
  unknown.platforms["beos-x86_64"] = unknown.platforms["linux-x86_64"];
  assert.match(check([unknown]).stderr, /unknown platform beos-x86_64/);

  const address = entry();
  address.platforms["linux-x86_64"].source = "https://example.com/tool.sevakext";
  assert.match(check([address]).stderr, /must be gallery\/extensions\/tool\/<file>\.sevakext/);

  const outside = entry();
  outside.platforms["linux-x86_64"].source = "gallery/packages/tool.sevakext";
  assert.match(check([outside]).stderr, /a path, not an address/);
});

test("the fields a native extension must have are required", () => {
  for (const [field, pattern] of [
    ["license", /license is empty/],
    ["repository", /repository must be an https/],
    ["version", /version must be like 1\.2\.3/],
  ]) {
    const e = entry();
    delete e[field];
    assert.match(check([e]).stderr, pattern, field);
  }
  assert.match(check([entry({ version: "latest" })]).stderr, /version must be like/);
  assert.match(check([entry({ permissions: ["Network"] })]).stderr, /permissions must be at most 8 unique lower case words/);
  assert.match(check([entry({ source: "gallery/packages/x.zip" })]).stderr, /not `source` and `sha256`/);
  assert.match(check([entry({ platforms: {} })]).stderr, /at least one platform/);
  assert.match(check([entry({ folder: "other" })]).stderr, /no folder/);
});

test("a package nobody lists, and a folder without an entry, are reported", () => {
  const stray = check([entry()], {
    extra: (gallery) =>
      writeFileSync(join(gallery, "extensions", "tool", "tool-0.0.1-linux-x86_64.sevakext"), "old"),
  });
  assert.match(stray.stderr, /extensions\/tool\/tool-0\.0\.1-linux-x86_64\.sevakext is not listed/);

  const orphan = check([], {
    extra: (gallery) => writeFileSync(join(gallery, "extensions", "tool", "x.sevakext"), "x"),
  });
  assert.match(orphan.stderr, /extensions\/tool has no native entry in the index/);
});

test("platforms are for native extensions only", () => {
  const run = check([{ ...entry(), kind: "workflow" }]);
  assert.match(run.stderr, /only native extensions have platforms/);
});

/** A workflow entry with a one-file package, as gallery_pack would have made (the check only hashes it). */
function workflow(id, keyword, over = {}, keepNative = false) {
  const bytes = Buffer.from(`zip of ${id}`);
  return {
    entry: {
      id,
      kind: "workflow",
      name: id,
      description: "Runs no code.",
      author: "Sevak",
      version: "1.0",
      tags: ["no-code"],
      source: `gallery/packages/${id}.zip`,
      sha256: sha256(bytes),
      ...over,
    },
    files: (gallery, repo) => {
      // These tests list no native extension (the fixture's folder would be an orphan).
      if (!keepNative) rmSync(join(gallery, "extensions"), { recursive: true, force: true });
      writeFileSync(join(gallery, "packages", `${id}.zip`), bytes);
      const folder = join(repo, "examples", "workflows", id);
      mkdirSync(folder, { recursive: true });
      writeFileSync(join(folder, "workflow.toml"), `[[node]]\ntype = "keyword"\nkeyword = "${keyword}"\n`);
    },
  };
}

test("an id may not be a Windows device name", () => {
  for (const id of ["con", "nul", "com1", "lpt9"]) {
    assert.match(check([entry({ id })]).stderr, /is a Windows device name/, id);
  }
  assert.match(check([entry({ id: "Tool" })]).stderr, /the id may only use a-z, 0-9 and -/);
});

test("licences must be SPDX expressions, and risky ones are flagged for a human", () => {
  assert.equal(check([entry({ license: "MIT OR Apache-2.0" })]).status, 0);
  assert.equal(check([entry({ license: "(MIT AND BSD-3-Clause)" })]).status, 0);
  assert.match(check([entry({ license: "see the file" })]).stderr, /not an SPDX expression/);
  const copyleft = check([entry({ license: "GPL-3.0-only" })]);
  assert.equal(copyleft.status, 0, "a warning does not fail the check");
  assert.match(copyleft.stderr, /warning: .*GPL-3\.0-only needs a maintainer's decision/);
  assert.equal(check([entry({ license: "MIT" })]).stderr, "");
});

test("a licence is required of anyone but the project itself", () => {
  const { entry: mine, files } = workflow("mine", "mine");
  const outsider = { ...mine, author: "Ada" };
  assert.match(check([outsider], { extra: files }).stderr, /license is required/);
  assert.equal(check([mine], { extra: files }).status, 0);
  assert.equal(check([{ ...outsider, license: "MIT" }], { extra: files }).status, 0);
});

test("reserved tags, no-code on code, and long texts are refused", () => {
  assert.match(check([entry({ tags: ["native", "verified"] })]).stderr, /tag verified is reserved/);
  assert.equal(check([entry({ tags: ["native", "official"], author: "Sevak" })]).status, 0);
  assert.match(check([entry({ tags: ["no-code"] })]).stderr, /only a workflow can be tagged no-code/);
  assert.match(check([entry({ description: "x".repeat(301) })]).stderr, /description is longer than 300/);
  assert.match(check([entry({ name: "x".repeat(61) })]).stderr, /name is longer than 60/);
  assert.match(check([entry({ version: "1.2.3.4" })]).stderr, /version must be/);
  assert.match(check([entry({ homepage: "http://example.com" })]).stderr, /homepage must be an https/);
});

test("unknown fields are a warning, not a failure", () => {
  const run = check([entry({ licence: "MIT" })]);
  assert.equal(run.status, 0);
  assert.match(run.stderr, /warning: .*unknown field "licence"/);
});

test("deprecated and replaced_by are optional and checked when present", () => {
  assert.equal(check([entry({ deprecated: "Superseded by a better tool." })]).status, 0);
  assert.match(check([entry({ deprecated: "" })]).stderr, /deprecated must be a short reason/);
  assert.match(check([entry({ deprecated: true })]).stderr, /deprecated must be a short reason/);
  assert.match(check([entry({ replaced_by: "other" })]).stderr, /replaced_by needs deprecated/);
  assert.match(check([entry({ deprecated: "old", replaced_by: "tool" })]).stderr, /points at the entry itself/);
  assert.match(check([entry({ deprecated: "old", replaced_by: "ghost" })]).stderr, /ghost, which is not in the index/);

  const { entry: newer, files } = workflow("newer", "newer", {}, true);
  const ok = check([entry({ deprecated: "old", replaced_by: "newer" }), newer], { extra: files });
  assert.equal(ok.status, 0, ok.stderr);
});

test("two entries may not share a keyword", () => {
  const a = workflow("first", "dup");
  const b = workflow("second", "DUP");
  const c = workflow("third", "other");
  const both = (x, y) => (gallery, repo) => (x.files(gallery, repo), y.files(gallery, repo));
  const clash = check([a.entry, b.entry], { extra: both(a, b) });
  assert.match(clash.stderr, /the keyword "dup" is used by first and second/);
  const fine = check([a.entry, c.entry], { extra: both(a, c) });
  assert.equal(fine.status, 0, fine.stderr);
});

test("against a base ref, changed bytes need a higher version", () => {
  const changed = entry();
  changed.platforms["linux-x86_64"].sha256 = "a".repeat(64);
  // The new hash does not match the file either; only the version complaints matter here.
  assert.match(check([changed], { base: [entry()] }).stderr, /package changed but version is still 1\.2\.3: bump it/);
  const lower = check([{ ...changed, version: "1.2.2" }], { base: [entry()] });
  assert.match(lower.stderr, /version went down from 1\.2\.3 to 1\.2\.2/);
  const higher = check([{ ...changed, version: "1.3.0" }], { base: [entry()] });
  assert.doesNotMatch(higher.stderr, /bump it|went down/);
});

test("against a base ref, an unchanged entry passes and a kind change does not", () => {
  const same = check([entry()], { base: [entry()] });
  assert.equal(same.status, 0, same.stderr);
  const flipped = check([{ ...entry(), kind: "workflow" }], { base: [entry()] });
  assert.match(flipped.stderr, /the kind changed from native to workflow/);
  assert.match(check([entry({ version: "1.3.0" })], { base: [entry()] }).stderr, /but the package is unchanged/);
});

test("the report lists new, updated and removed entries and what needs a security review", () => {
  const { entry: added, files } = workflow("added", "added", {}, true);
  const run = check([entry(), added], { base: [entry(), entry({ id: "gone" })], extra: files, reports: true });
  assert.match(run.stderr, /warning: index\.json gone: removed from the index/);
  const byId = Object.fromEntries(run.json.changes.map((c) => [c.id, c]));
  assert.equal(byId.added.change, "new");
  assert.equal(byId.added.security_review, false, "a workflow that runs no code");
  assert.equal(byId.gone.change, "removed");
  assert.equal(run.json.needs_security_review, false);
  assert.match(run.markdown, /\| index \| `added` \| new \(workflow\) \| 1\.0 \| no \|/);
  assert.match(run.markdown, /Result: passed/);
});

test("new native extensions are marked for the security review", () => {
  const run = check([entry(), entry({ id: "tool2" })], { base: [entry()], reports: true });
  // tool2 has no package files, so the check fails, but the report still says a human is needed.
  assert.equal(run.status, 1);
  assert.equal(run.json.ok, false);
  assert.equal(run.json.changes.find((c) => c.id === "tool2").security_review, true);
  assert.equal(run.json.needs_security_review, true);
  assert.match(run.markdown, /problem\(s\) must be fixed/);
});

test("an unknown base ref is a problem", () => {
  const run = check([entry()], { args: ["--base", "no-such-ref"] });
  assert.equal(run.status, 1);
  assert.match(run.stderr, /--base: cannot read no-such-ref/);
});

test("a new declared permission is flagged for the reviewer", () => {
  const run = check([entry({ permissions: ["network", "filesystem"] })], { base: [entry()] });
  assert.match(run.stderr, /warning: .*new declared permissions: filesystem/);
});

test("--root checks another copy of the repository with this copy of the script", () => {
  const repo = mkdtempSync(join(tmpdir(), "gallery-check-root-"));
  try {
    const gallery = join(repo, "gallery");
    mkdirSync(join(gallery, "extensions", "tool"), { recursive: true });
    mkdirSync(join(gallery, "packages"));
    mkdirSync(join(gallery, "themes"));
    writeFileSync(join(gallery, "extensions", "tool", "tool-1.2.3-linux-x86_64.sevakext"), LINUX);
    writeFileSync(join(gallery, "extensions", "tool", "tool-1.2.3-macos-aarch64.sevakext"), MAC);
    writeFileSync(join(gallery, "index.json"), JSON.stringify({ format: 2, entries: [entry()] }));
    writeFileSync(join(gallery, "themes.json"), JSON.stringify({ version: 2, themes: [] }));
    // A script inside the checked tree that says everything is fine must not be what judges it.
    mkdirSync(join(repo, "scripts"));
    writeFileSync(join(repo, "scripts", "gallery-check.mjs"), "console.log('trust me');\n");
    const run = spawnSync(process.execPath, [script, "--root", repo], { encoding: "utf8" });
    assert.equal(run.status, 0, run.stderr);
    assert.match(run.stdout, /gallery ok: 1 workflows/);
    writeFileSync(join(gallery, "extensions", "tool", "tool-1.2.3-linux-x86_64.sevakext"), "tampered");
    const bad = spawnSync(process.execPath, [script, "--root", repo], { encoding: "utf8" });
    assert.equal(bad.status, 1);
    assert.match(bad.stderr, /linux-x86_64: sha256 is/);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
