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

/** Builds a repository with `entries`, runs the check there and returns the outcome. */
function check(entries, { extra = () => {}, args = [] } = {}) {
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
    extra(gallery);
    const run = spawnSync(process.execPath, [join(repo, "scripts", "gallery-check.mjs"), ...args], {
      encoding: "utf8",
    });
    return { ...run, index: () => JSON.parse(readFileSync(join(gallery, "index.json"), "utf8")) };
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
