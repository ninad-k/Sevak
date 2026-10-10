import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const workflow = readFileSync(new URL("../.github/workflows/gallery-review-comment.yml", import.meta.url), "utf8");
const source = workflow.split("          script: |")[1].split(/\r?\n/).map((line) => line.replace(/^            /, "")).join("\n");
const execute = new (Object.getPrototypeOf(async function () {}).constructor)("require", "context", "github", "core", source);

async function comment(report, conclusion = "success") {
  const writes = [];
  const encoded = JSON.stringify(report);
  const fs = { existsSync: () => true, statSync: () => ({ size: encoded.length }), readFileSync: () => encoded };
  const github = {
    paginate: async () => [],
    rest: {
      repos: { listPullRequestsAssociatedWithCommit: async () => ({ data: [{ state: "open", number: 1, head: { sha: "head" }, base: { repo: { full_name: "owner/repo" } } }] }) },
      issues: { listComments: () => {}, createComment: async (value) => writes.push(value), addLabels: async () => {} },
    },
  };
  await execute(() => fs, { repo: { owner: "owner", repo: "repo" }, payload: { workflow_run: { head_sha: "head", conclusion, html_url: "https://example.com/run" } } }, github, { notice: () => {} });
  return writes[0]?.body;
}

test("an unavailable checker never posts a passing result", async () => {
  const body = await comment({ ok: true, unavailable: true, changes: [] });
  assert.match(body, /automated review unavailable/);
  assert.doesNotMatch(body, /Result: passed/);
});

test("a passing checker cannot hide a failed or cancelled review workflow", async () => {
  for (const conclusion of ["failure", "cancelled", "timed_out"]) {
    const body = await comment({ ok: true, changes: [] }, conclusion);
    assert.doesNotMatch(body, /Result: passed/);
    assert.match(body, /did not complete successfully/);
  }
  assert.match(await comment({ ok: true, changes: [] }), /Result: passed/);
});

test("malformed report shapes are ignored and hostile strings stay inside code spans", async () => {
  assert.equal(await comment(null), undefined);
  assert.equal(await comment([]), undefined);
  const body = await comment({ ok: false, problems: ["`\n@everyone [click](https://example.com)"], changes: [] });
  assert.match(body, /- `  @everyone \[click\]\(https:\/\/example.com\)`/);
});
