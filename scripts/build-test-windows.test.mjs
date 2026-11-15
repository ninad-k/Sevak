import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { checkDroppedFiles, chooseBase, main, copySourceSnapshot, developmentIdentity, parseOptions, sourceFiles } from "./build-test-windows.mjs";
import { compare } from "./release-version.mjs";

const commit = "123456789" + "a".repeat(31);
test("local build versions are identifiable prereleases with bounded MSI fields", () => {
  const build = developmentIdentity({ base: "0.13.0", commit, dirty: true, date: new Date("2026-10-10T20:30:45.123Z") });
  assert.equal(build.version, "0.13.0-dev.20261010T203045123Z.g123456789.dirty.1230");
  assert.equal(build.msiVersion, "0.13.0.1230");
  assert.ok(compare(build.version, "0.12.0") > 0);
  assert.ok(compare(build.version, "0.13.0") < 0);
  assert.throws(() => developmentIdentity({ base: "0.13.0", commit, revision: 65536 }), /revision/);
  assert.throws(() => developmentIdentity({ base: "0.13.0-beta.1", commit }), /stable/);
  assert.throws(() => developmentIdentity({ base: "0.13.0", commit: "unknown" }), /commit/);
});

test("local packaging options reject ambiguous or incomplete requests", () => {
  assert.deepEqual(parseOptions(["--plan", "--base", "0.13.0", "--revision", "12"]), { plan: true, prepareOnly: false, allowDroppedSourceFiles: false, base: "0.13.0", revision: 12 });
  assert.equal(parseOptions(["--allow-dropped-source-files"]).allowDroppedSourceFiles, true);
  assert.throws(() => parseOptions(["--base"]), /incomplete/);
  assert.throws(() => parseOptions(["--plan", "--prepare-only"]), /choose/);
  assert.throws(() => parseOptions(["--publish"]), /unknown/);
});

function fixture() {
  const dir = mkdtempSync(join(tmpdir(), "sevak-test-build-"));
  const env = { ...process.env, GIT_AUTHOR_NAME: "test", GIT_AUTHOR_EMAIL: "test@example.com", GIT_COMMITTER_NAME: "test", GIT_COMMITTER_EMAIL: "test@example.com" };
  const git = (...args) => execFileSync("git", args, { cwd: dir, env, encoding: "utf8" }).trim();
  git("init", "-q", "-b", "main");
  git("config", "core.autocrlf", "false");
  const put = (path, text) => { mkdirSync(join(dir, path, ".."), { recursive: true }); writeFileSync(join(dir, path), text); };
  put("Cargo.toml", '[workspace.package]\nversion = "0.1.0"\n');
  put("src-tauri/tauri.conf.json", JSON.stringify({ version: "0.1.0", bundle: { windows: { wix: {} } } }));
  put("package.json", JSON.stringify({ version: "0.1.0" }));
  put("package-lock.json", JSON.stringify({ version: "0.1.0", packages: { "": { version: "0.1.0" } } }));
  put("scripts/release-version.mjs", readFileSync(fileURLToPath(new URL("./release-version.mjs", import.meta.url))));
  put("ui/src/app.ts", "export const old = true;\n");
  git("add", "."); git("commit", "-qm", "feat: initial"); git("tag", "v0.12.0");
  return { dir, git, put, done: () => rmSync(dir, { recursive: true, force: true }) };
}

test("an already released HEAD gets the next patch development base", () => {
  const repo = fixture();
  try {
    assert.equal(chooseBase(repo.dir, "0.1.0"), "0.12.1");
    repo.put("ui/src/app.ts", "export const newFeature = true;\n");
    repo.git("add", "."); repo.git("commit", "-qm", "feat: a new feature");
    assert.equal(chooseBase(repo.dir, "0.1.0"), "0.13.0");
  } finally { repo.done(); }
});

test("snapshot includes working changes and new source/docs, excludes local data, and stamps only the snapshot", () => {
  const repo = fixture();
  try {
    repo.put("ui/src/app.ts", "export const changed = true;\n");
    repo.put("crates/new/src/lib.rs", "pub fn added() {}\n");
    repo.put("docs/new.md", "New help\n");
    repo.put("gallery/revocations.json", "{}\n");
    repo.put("examples/new/src/main.rs", "fn main() {}\n");
    for (const path of [".claude/private.md", "npm-local.log", "target-native/debug/file", "credentials.txt", "ui/.env.local", "scripts/publisher.pfx", "assets/signing.key", "docs/credentials.json"]) repo.put(path, "private\n");
    const files = sourceFiles(repo.dir);
    for (const name of ["crates/new/src/lib.rs", "docs/new.md", "gallery/revocations.json", "examples/new/src/main.rs"]) assert.ok(files.paths.includes(name));
    assert.ok(!files.paths.some((path) => /private|npm-local|target-native|credentials|\.env|\.pfx$|signing\.key/.test(path)));
    // Dropped files in build roots are recorded, not silently ignored; files
    // outside the build roots were never candidates.
    assert.deepEqual(files.dropped, ["assets/signing.key", "docs/credentials.json", "scripts/publisher.pfx", "ui/.env.local"]);
    assert.deepEqual(checkDroppedFiles(files.dropped), []);
    const original = readFileSync(join(repo.dir, "Cargo.toml"), "utf8");
    const snapshot = join(repo.dir, "snapshot");
    const manifest = copySourceSnapshot(repo.dir, snapshot, files.paths);
    assert.match(manifest.sha256, /^[a-f0-9]{64}$/);
    assert.equal(readFileSync(join(snapshot, "ui/src/app.ts"), "utf8"), "export const changed = true;\n");
    execFileSync(process.execPath, [join(snapshot, "scripts/release-version.mjs"), "set", "0.13.0-dev.test.42"]);
    assert.equal(readFileSync(join(repo.dir, "Cargo.toml"), "utf8"), original);
    assert.match(readFileSync(join(snapshot, "Cargo.toml"), "utf8"), /0\.13\.0-dev\.test\.42/);
    assert.equal(JSON.parse(readFileSync(join(snapshot, "src-tauri/tauri.conf.json"))).bundle.windows.wix.version, "0.13.0.42");
    assert.ok(!existsSync(join(snapshot, ".claude")));
  } finally { repo.done(); }
});

test("a secret-like untracked file under a compiled source root fails unless explicitly allowed", () => {
  const repo = fixture();
  try {
    repo.put("crates/new/src/secrets.rs", "pub fn redact() {}\n");
    repo.put("src-tauri/src/credentials.rs", "pub fn store() {}\n");
    repo.put("ui/src/.env", "VITE_X=1\n");
    repo.put("src-tauri/icons/dev.pem", "not source\n");
    const files = sourceFiles(repo.dir);
    for (const name of ["crates/new/src/secrets.rs", "src-tauri/src/credentials.rs", "ui/src/.env", "src-tauri/icons/dev.pem"]) {
      assert.ok(files.dropped.includes(name), name);
      assert.ok(!files.paths.includes(name), name);
    }
    assert.throws(() => checkDroppedFiles(files.dropped), (error) =>
      /crates\/new\/src\/secrets\.rs/.test(error.message) && /src-tauri\/src\/credentials\.rs/.test(error.message) &&
      /ui\/src\/\.env/.test(error.message) && !/dev\.pem/.test(error.message) && /--allow-dropped-source-files/.test(error.message));
    assert.deepEqual(checkDroppedFiles(files.dropped, true).sort(), ["crates/new/src/secrets.rs", "src-tauri/src/credentials.rs", "ui/src/.env"]);
  } finally { repo.done(); }
});

test("the plan and BUILD-INFO.json record dropped files, and a blocked build writes nothing", () => {
  const repo = fixture();
  const out = mkdtempSync(join(tmpdir(), "sevak-test-build-output-"));
  try {
    repo.put("docs/secrets.md", "notes\n");
    const plan = main(["--plan", "--base", "0.13.0", "--revision", "1"], repo.dir);
    assert.deepEqual(plan.source.droppedSecretLikeFiles, ["docs/secrets.md"]);
    const output = join(out, "build");
    const manifest = main(["--prepare-only", "--base", "0.13.0", "--revision", "1", "--output", output], repo.dir);
    assert.deepEqual(manifest.source.droppedSecretLikeFiles, ["docs/secrets.md"]);
    assert.deepEqual(JSON.parse(readFileSync(join(output, "BUILD-INFO.json"), "utf8")).source.droppedSecretLikeFiles, ["docs/secrets.md"]);
    repo.put("crates/new/src/secret.rs", "pub fn x() {}\n");
    const blocked = join(out, "blocked");
    assert.throws(() => main(["--plan", "--base", "0.13.0"], repo.dir), /--allow-dropped-source-files/);
    assert.throws(() => main(["--prepare-only", "--base", "0.13.0", "--output", blocked], repo.dir), /crates\/new\/src\/secret\.rs/);
    assert.ok(!existsSync(blocked));
    const allowed = main(["--prepare-only", "--allow-dropped-source-files", "--base", "0.13.0", "--output", blocked], repo.dir);
    assert.deepEqual(allowed.source.droppedSecretLikeFiles, ["crates/new/src/secret.rs", "docs/secrets.md"]);
    assert.ok(!existsSync(join(blocked, "source/crates/new/src/secret.rs")));
  } finally { repo.done(); rmSync(out, { recursive: true, force: true }); }
});

test("snapshot rejects source replacement and escaping paths", () => {
  const repo = fixture();
  try {
    assert.throws(() => copySourceSnapshot(repo.dir, repo.dir, []), /cannot contain/);
    assert.throws(() => copySourceSnapshot(repo.dir, join(repo.dir, "bad-snapshot"), ["../secret"]), /unsafe snapshot/);
  } finally { repo.done(); }
});
