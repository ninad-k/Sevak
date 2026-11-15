// Prepare bounded data for the signed, opt-in extension advisory channel.
// This does not sign or publish anything. The workflow verifies its signature
// with the application's Rust verifier before uploading the two assets.
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const MAX_BYTES = 128 * 1024;
// The gallery id grammar shared by packages and themes (sevak-core theme_store).
const ID = /^[a-z0-9_-]{1,48}$/;
const VERSION = /^[A-Za-z0-9.+-]{1,64}$/;
const RESERVED = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/;
const CONTROLS = /[\x00-\x1f\x7f-\x9f]/;

function fields(value, keys, description) {
  if (!value || typeof value !== "object" || Array.isArray(value) || Object.keys(value).some((key) => !keys.includes(key))) {
    throw new Error(`Invalid ${description} fields.`);
  }
}

export function validateSource(source) {
  fields(source, ["format", "revocations"], "advisory source");
  if (source.format !== 1 || !Array.isArray(source.revocations) || source.revocations.length > 256) throw new Error("Expected format 1 with at most 256 revocations.");
  const seen = new Set();
  for (const entry of source.revocations) {
    fields(entry, ["kind", "id", "versions", "reason", "advisory_url"], "revocation");
    // Omitted means a package (workflow, plugin or native extension).
    if (entry.kind !== undefined && entry.kind !== "theme") throw new Error(`Invalid kind for ${entry.id}; omit it for packages or use "theme".`);
    const key = `${entry.kind ?? "package"}:${entry.id}`;
    if (typeof entry.id !== "string" || !ID.test(entry.id) || RESERVED.test(entry.id) || seen.has(key)) throw new Error("Revocations need unique, valid extension IDs.");
    seen.add(key);
    if (entry.kind === "theme" && (!Array.isArray(entry.versions) || entry.versions.length !== 1 || entry.versions[0] !== "*")) {
      throw new Error(`Theme ${entry.id} has no versions; revoke it with a sole '*'.`);
    }
    if (!Array.isArray(entry.versions) || !entry.versions.length || entry.versions.length > 64 ||
        new Set(entry.versions).size !== entry.versions.length ||
        !entry.versions.every((version) => typeof version === "string" && (VERSION.test(version) || (version === "*" && entry.versions.length === 1)))) {
      throw new Error(`Invalid revoked versions for ${entry.id}; use exact versions or a sole '*'.`);
    }
    if (typeof entry.reason !== "string" || !entry.reason.trim() || entry.reason !== entry.reason.trim() || [...entry.reason].length > 500 || CONTROLS.test(entry.reason)) throw new Error(`Invalid reason for ${entry.id}.`);
    if (entry.advisory_url !== undefined && (typeof entry.advisory_url !== "string" ||
        Buffer.byteLength(entry.advisory_url) > 200 ||
        !/^https:\/\/github\.com\/ninad-k\/Sevak\/security\/advisories\/GHSA-[a-z0-9-]+$/.test(entry.advisory_url))) {
      throw new Error(`Invalid Sevak advisory URL for ${entry.id}.`);
    }
  }
  return source;
}

export function prepareAdvisories(source, { now = Math.floor(Date.now() / 1000), days = 14 } = {}) {
  validateSource(source);
  if (!Number.isSafeInteger(now) || now <= 0) throw new Error("Invalid issue time.");
  if (!Number.isInteger(days) || days < 1 || days > 30) throw new Error("Expiry must be from 1 to 30 days.");
  const payload = { format: 1, issued_at: now, expires_at: now + days * 86400, revocations: source.revocations };
  if (!Number.isSafeInteger(payload.expires_at)) throw new Error("Invalid expiry time.");
  const bytes = JSON.stringify(payload, null, 2) + "\n";
  if (Buffer.byteLength(bytes) > MAX_BYTES) throw new Error("Advisory feed exceeds 128 KiB.");
  return bytes;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [command, source, destination, days = "14"] = process.argv.slice(2);
    if (!["prepare", "check"].includes(command) || !source || (command === "prepare" && !destination)) throw new Error("Usage: extension-advisories.mjs prepare <source.json> <output.json> [days] | check <source.json>");
    if (statSync(source).size > MAX_BYTES) throw new Error("Advisory source exceeds 128 KiB.");
    const bytes = prepareAdvisories(JSON.parse(readFileSync(source, "utf8")), { days: Number(days) });
    if (command === "prepare") {
      mkdirSync(dirname(resolve(destination)), { recursive: true });
      writeFileSync(destination, bytes);
      console.log(`Prepared ${Buffer.byteLength(bytes)} bytes; unsigned until the signing step succeeds.`);
    } else console.log("Extension advisory source is valid.");
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
