import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("./gallery-add-native.mjs", import.meta.url));
const native = (id, version = "1.0.0") => ({
  id, kind: "native", name: id, version, tags: ["native"],
  platforms: { "linux-x86_64": { source: `gallery/extensions/${id}/${id}-${version}-linux-x86_64.sevakext`, sha256: "a".repeat(64) } },
});

function update(index, entries) {
  const repo = mkdtempSync(join(tmpdir(), "gallery-add-native-"));
  try {
    mkdirSync(join(repo, "scripts"));
    mkdirSync(join(repo, "gallery"));
    cpSync(script, join(repo, "scripts", "gallery-add-native.mjs"));
    const path = join(repo, "gallery", "index.json");
    const before = JSON.stringify(index, null, 2) + "\n";
    writeFileSync(path, before);
    const files = entries.map((entry, i) => {
      const file = join(repo, `entry-${i}.json`);
      writeFileSync(file, JSON.stringify(entry));
      return file;
    });
    const run = spawnSync(process.execPath, [join(repo, "scripts", "gallery-add-native.mjs"), ...files], { encoding: "utf8" });
    const after = readFileSync(path, "utf8");
    return { ...run, before, after, index: JSON.parse(after) };
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
}

test("updates replace platform packages, preserve chosen tags and position, and append new ids", () => {
  const old = native("tool");
  old.tags = ["native", "developer", "offline"];
  old.platforms["windows-x86_64"] = { source: "old.exe", sha256: "b".repeat(64) };
  const workflow = { id: "search", kind: "workflow" };
  const newer = native("tool", "2.0.0");
  const another = native("other");
  const run = update({ format: 2, name: "Gallery", entries: [workflow, old] }, [newer, another]);
  assert.equal(run.status, 0, run.stderr);
  assert.deepEqual(run.index, { format: 2, name: "Gallery", entries: [workflow, { ...newer, tags: old.tags }, another] });
  assert.equal(run.after, JSON.stringify(run.index, null, 2) + "\n");
});

test("a conflicting kind rejects the whole batch without writing earlier additions", () => {
  const run = update({ format: 2, entries: [{ id: "search", kind: "workflow" }] }, [native("new"), native("search")]);
  assert.equal(run.status, 1);
  assert.match(run.stderr, /already a workflow/);
  assert.equal(run.after, run.before);
});

test("ambiguous duplicate input ids reject the whole batch", () => {
  const run = update({ format: 2, entries: [] }, [native("tool"), native("tool", "2.0.0")]);
  assert.equal(run.status, 1);
  assert.match(run.stderr, /supplied more than once/);
  assert.equal(run.after, run.before);
});

test("invalid kinds, unsafe ids and numeric ids leave the catalog unchanged", () => {
  for (const entry of [null, { ...native("tool"), kind: "plugin" }, ...["../tool", "nul", "com1", "", 123].map(id => native(id))]) {
    const run = update({ format: 2, entries: [] }, [entry]);
    assert.equal(run.status, 1);
    assert.match(run.stderr, /not a native extension entry/);
    assert.equal(run.after, run.before);
  }
});

test("unsupported catalog formats are refused before adding entries", () => {
  const run = update({ format: 1, entries: [] }, [native("tool")]);
  assert.equal(run.status, 1);
  assert.match(run.stderr, /expected a format 2 catalog/);
  assert.equal(run.after, run.before);
});
