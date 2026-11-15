import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import test from "node:test";
import { prepareAdvisories, validateSource } from "./extension-advisories.mjs";

const entry = { id: "example-tool", versions: ["1.2.3"], reason: "This build executes an unexpected command.", advisory_url: "https://github.com/ninad-k/Sevak/security/advisories/GHSA-abcd-1234-abcd" };
const source = (record = entry) => ({ format: 1, revocations: [record] });

test("advisories produce a fresh bounded envelope and preserve exact version scope", () => {
  const result = JSON.parse(prepareAdvisories(source(), { now: 1000, days: 14 }));
  assert.deepEqual(result, { format: 1, issued_at: 1000, expires_at: 1210600, revocations: [entry] });
  assert.equal(JSON.parse(prepareAdvisories(source({ ...entry, versions: ["*"] }))).revocations[0].versions[0], "*");
});

test("invalid or ambiguous identifiers, versions, reasons and URLs are rejected", () => {
  for (const change of [
    { id: "../tool" }, { id: "con" }, { id: "x".repeat(49) }, { id: "" }, { id: "Tool" }, { id: "a.b" },
    { kind: "package" }, { kind: "workflow" }, { kind: "theme", versions: ["1.0"] },
    { versions: [] }, { versions: ["*", "1"] }, { versions: ["1", "1"] }, { versions: ["1/2"] }, { versions: [null] },
    { reason: " " }, { reason: " padded " }, { reason: "a\nb" }, { reason: "a".repeat(501) },
    { advisory_url: "https://example.com/advisory" }, { advisory_url: entry.advisory_url + "?query=1" }, { advisory_url: entry.advisory_url + "#fragment" },
    { advisory_url: entry.advisory_url + "a".repeat(200) },
    { arbitrary: true },
  ]) assert.throws(() => prepareAdvisories(source({ ...entry, ...change })), JSON.stringify(change));
  assert.throws(() => validateSource({ ...source(), extra: true }), /fields/);
  assert.throws(() => validateSource({ format: 1, revocations: [entry, entry] }), /unique/);
});

test("theme notices use their own namespace, the shared id grammar and a sole '*'", () => {
  const theme = { kind: "theme", id: "solar_dark", versions: ["*"], reason: "Test revoked theme." };
  assert.deepEqual(JSON.parse(prepareAdvisories(source(theme))).revocations, [theme]);
  // One id may be revoked once as a package and once as a theme.
  const both = { format: 1, revocations: [{ ...theme, id: "nord" }, { id: "nord", versions: ["*"], reason: "Package." }] };
  assert.doesNotThrow(() => validateSource(both));
  assert.throws(() => validateSource({ format: 1, revocations: [theme, theme] }), /unique/);
});

test("lifetime, entry counts and byte limits match the client bounds", () => {
  for (const days of [0, -1, 31, 1.5, NaN]) assert.throws(() => prepareAdvisories(source(), { days }), /Expiry/);
  assert.throws(() => prepareAdvisories(source(), { now: Number.MAX_SAFE_INTEGER }), /expiry/);
  assert.throws(() => prepareAdvisories(source(), { now: 0 }), /issue time/);
  assert.throws(() => prepareAdvisories({ format: 1, revocations: Array(257).fill(entry) }), /256/);
  assert.throws(() => prepareAdvisories(source({ ...entry, versions: Array.from({ length: 65 }, (_, i) => `1.${i}`) })), /versions/);
  assert.doesNotThrow(() => prepareAdvisories(source({ ...entry, versions: Array.from({ length: 64 }, (_, i) => `1.${i}`) })));
  const many = Array.from({ length: 256 }, (_, i) => ({ id: `extension-${i}`, versions: ["1"], reason: "界".repeat(500) }));
  assert.throws(() => prepareAdvisories({ format: 1, revocations: many }), /128 KiB/);
});

test("the committed source is valid and the CLI validates it without signing", () => {
  const path = new URL("../gallery/revocations.json", import.meta.url);
  validateSource(JSON.parse(readFileSync(path, "utf8")));
  const output = execFileSync(process.execPath, ["scripts/extension-advisories.mjs", "check", "gallery/revocations.json"], { encoding: "utf8" });
  assert.match(output, /source is valid/);
});
