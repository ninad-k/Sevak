// Tests for scripts/release-version.mjs. Run with `npm run test:scripts`.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import {
  betaVersion,
  bump,
  compare,
  isStable,
  next,
  nextBeta,
  parse,
  previousTag,
  wixVersion,
} from "./release-version.mjs";

test("semver order: pre-releases sort before their release, numbers numerically", () => {
  const ordered = [
    "0.9.9",
    "1.0.0-alpha",
    "1.0.0-alpha.1",
    "1.0.0-beta.2",
    "1.0.0-beta.10",
    "1.0.0-rc.1",
    "1.0.0",
    "1.0.1-beta.1",
    "1.0.1",
    "1.1.0",
    "2.0.0",
  ];
  for (let i = 0; i < ordered.length; i++) {
    for (let j = 0; j < ordered.length; j++) {
      assert.equal(compare(ordered[i], ordered[j]), Math.sign(i - j), `${ordered[i]} vs ${ordered[j]}`);
    }
  }
});

test("parse accepts X.Y.Z[-pre] and rejects everything else", () => {
  assert.deepEqual(parse("1.2.3-beta.4"), { core: [1, 2, 3], pre: ["beta", "4"] });
  for (const bad of ["1.2", "v1.2.3", "1.2.3-", "1.2.3+build", "01.2.3x", "1.2.3-beta..1"]) {
    assert.throws(() => parse(bad), /not a valid/, bad);
  }
  assert.ok(isStable("1.2.3"));
  assert.ok(!isStable("1.2.3-beta.1"));
});

test("bump follows the conventional commit types", () => {
  assert.equal(bump("1.2.3", ["fix: a", "docs: b"], "0.1.0"), "1.2.4");
  assert.equal(bump("1.2.3", ["fix: a", "feat(files): b"], "0.1.0"), "1.3.0");
  assert.equal(bump("1.2.3", ["feat!: break"], "0.1.0"), "2.0.0");
  assert.equal(bump("1.2.3", ["fix: x\n\nBREAKING CHANGE: y"], "0.1.0"), "2.0.0");
  assert.equal(bump("0.4.2", ["feat!: break"], "0.1.0"), "0.5.0", "breaking on 0.x bumps the minor");
  assert.equal(bump("0.4.2", ["fix: a"], "1.0.0"), "1.0.0", "a higher version in tauri.conf.json forces the release");
});

test("beta numbering counts up per base version", () => {
  assert.equal(betaVersion("1.3.0", []), "1.3.0-beta.1");
  assert.equal(betaVersion("1.3.0", [""]), "1.3.0-beta.1");
  assert.equal(betaVersion("1.3.0", ["v1.3.0-beta.1", "v1.3.0-beta.2"]), "1.3.0-beta.3");
  assert.equal(betaVersion("1.3.0", ["v1.3.0-beta.9", "v1.3.0-beta.10"]), "1.3.0-beta.11", "numeric, not textual, order");
  assert.equal(betaVersion("1.3.0", ["v1.2.0-beta.7", "v1.3.1-beta.4"]), "1.3.0-beta.1", "other bases do not count");
  assert.equal(betaVersion("1.3.0", ["v1.3.0-beta.x"]), "1.3.0-beta.1");
});

test("the MSI version of a pre-release is numeric", () => {
  assert.equal(wixVersion("1.3.0"), undefined);
  assert.equal(wixVersion("1.3.0-beta.2"), "1.3.0.2");
  assert.equal(wixVersion("0.2.7-rc.12"), "0.2.7.12");
  assert.throws(() => wixVersion("1.3.0-beta"), /numeric/);
  assert.throws(() => wixVersion("1.3.0-beta.70000"), /too large/);
});

// --- against a throwaway git repository --------------------------------------

const env = {
  ...process.env,
  GIT_AUTHOR_NAME: "t",
  GIT_AUTHOR_EMAIL: "t@example.com",
  GIT_COMMITTER_NAME: "t",
  GIT_COMMITTER_EMAIL: "t@example.com",
};

function repo() {
  const dir = mkdtempSync(join(tmpdir(), "sevak-release-"));
  const git = (...args) => execFileSync("git", args, { cwd: dir, env, encoding: "utf8" }).trim();
  git("init", "-q", "-b", "main");
  let n = 0;
  const commit = (message) => {
    writeFileSync(join(dir, "f.txt"), String(++n));
    git("add", "f.txt");
    git("commit", "-q", "-m", message);
  };
  return { dir, git, commit, done: () => rmSync(dir, { recursive: true, force: true }) };
}

test("next: a fresh repository releases the baseline, then bumps from the newest stable tag", () => {
  const r = repo();
  try {
    r.commit("feat: first");
    assert.equal(next({ cwd: r.dir, baseline: "0.1.0" }), "0.1.0");
    r.git("tag", "v0.1.0");
    assert.equal(next({ cwd: r.dir, baseline: "0.1.0" }), "", "HEAD is already released");
    r.commit("fix: small");
    assert.equal(next({ cwd: r.dir, baseline: "0.1.0" }), "0.1.1");
    r.commit("feat: bigger");
    assert.equal(next({ cwd: r.dir, baseline: "0.1.0" }), "0.2.0");
  } finally {
    r.done();
  }
});

test("next-beta: numbers betas of the coming version and ignores them for stable versioning", () => {
  const r = repo();
  try {
    r.commit("feat: first");
    r.git("tag", "v1.2.0");
    r.commit("feat: new thing");
    const opts = { cwd: r.dir, baseline: "0.1.0" };
    assert.equal(next(opts), "1.3.0");
    assert.equal(nextBeta(opts), "1.3.0-beta.1");

    r.git("tag", "v1.3.0-beta.1");
    assert.equal(nextBeta(opts), "", "HEAD already has a beta; nothing new to ship");
    assert.equal(next(opts), "1.3.0", "a beta tag does not move the stable baseline");

    r.commit("fix: after the first beta");
    assert.equal(nextBeta(opts), "1.3.0-beta.2");
    r.git("tag", "v1.3.0-beta.2");

    // A breaking change raises the base the next betas are numbered against.
    r.commit("feat!: rename keys");
    assert.equal(nextBeta(opts), "2.0.0-beta.1");

    // Once the stable release exists, the cycle restarts from it.
    r.git("tag", "v2.0.0");
    assert.equal(nextBeta(opts), "", "HEAD is a stable release");
    r.commit("fix: patch");
    assert.equal(nextBeta(opts), "2.0.1-beta.1");

    // Release notes are written against the newest stable or beta tag, and a
    // tag that is not a version (the channel-beta pointer) is never the base.
    r.git("tag", "channel-beta");
    assert.equal(previousTag({ cwd: r.dir }), "v2.0.0");
    r.git("tag", "v2.0.1-beta.1");
    assert.equal(previousTag({ cwd: r.dir }), "v2.0.1-beta.1");
  } finally {
    r.done();
  }
});
