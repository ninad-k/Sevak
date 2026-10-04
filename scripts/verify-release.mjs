// Checks a downloaded release before the promote and rollback workflows
// re-point the update channel at it.
//
//   node scripts/verify-release.mjs <assets-dir> <version> [manifest.json ...]
//
// <assets-dir> holds every asset of the release. Fails (exit 1, one problem per
// line) unless:
//   - SHA256SUMS.txt exists, lists every other asset, and each hash matches;
//   - every installer the updater can use has a non-empty `.sig` next to it;
//   - each named manifest (latest.json, latest-beta.json) is for <version>, and
//     every platform in it points at an asset of this release whose signature
//     is the `.sig` file's content.
// This proves the files are the ones the build uploaded and that the manifest
// matches them. It cannot check the minisign signature itself (that needs the
// public key and is done by the installed app when it applies an update).
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { TARGETS } from "./updater-manifest.mjs";

const SUMS = "SHA256SUMS.txt";

/** "<hash>  <name>" lines (sha256sum output; a leading "*" marks binary mode). */
export function parseSums(text) {
  return new Map(
    text
      .split("\n")
      .map((line) => line.trim().match(/^([0-9a-f]{64})\s+\*?(.+)$/i))
      .filter(Boolean)
      .map(([, hash, name]) => [name, hash.toLowerCase()]),
  );
}

const sha256 = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");

export function verify(dir, version, manifests = []) {
  const problems = [];
  const files = readdirSync(dir);

  if (!files.includes(SUMS)) {
    problems.push(`${SUMS} is missing`);
  } else {
    const sums = parseSums(readFileSync(join(dir, SUMS), "utf8"));
    if (!sums.size) problems.push(`${SUMS} lists no files`);
    for (const [name, hash] of sums) {
      if (!files.includes(name)) problems.push(`${name} is listed in ${SUMS} but is not an asset`);
      else if (sha256(join(dir, name)) !== hash) problems.push(`${name}: checksum does not match ${SUMS}`);
    }
    for (const name of files) {
      if (name !== SUMS && !sums.has(name)) problems.push(`${name} is not listed in ${SUMS}`);
    }
  }

  for (const name of files) {
    if (TARGETS.some(([pattern]) => pattern.test(name))) {
      if (!files.includes(`${name}.sig`)) problems.push(`${name} has no ${name}.sig`);
      else if (!readFileSync(join(dir, `${name}.sig`), "utf8").trim()) problems.push(`${name}.sig is empty`);
    }
  }

  for (const manifest of manifests) {
    if (!existsSync(join(dir, manifest))) {
      problems.push(`${manifest} is missing`);
      continue;
    }
    let data;
    try {
      data = JSON.parse(readFileSync(join(dir, manifest), "utf8"));
    } catch (err) {
      problems.push(`${manifest} is not valid JSON: ${err.message}`);
      continue;
    }
    if (data.version !== version) {
      problems.push(`${manifest} is for version ${data.version}, expected ${version}`);
    }
    for (const [target, entry] of Object.entries(data.platforms ?? {})) {
      let asset;
      try {
        asset = decodeURIComponent(new URL(entry.url).pathname.split("/").at(-1));
      } catch {
        problems.push(`${manifest} ${target}: bad url ${entry.url}`);
        continue;
      }
      if (!files.includes(asset)) {
        problems.push(`${manifest} ${target}: ${asset} is not an asset of this release`);
      } else if (!entry.url.includes(`/v${version}/`)) {
        problems.push(`${manifest} ${target}: url is not under the v${version} release`);
      } else if (
        !files.includes(`${asset}.sig`) ||
        readFileSync(join(dir, `${asset}.sig`), "utf8").trim() !== String(entry.signature).trim()
      ) {
        problems.push(`${manifest} ${target}: signature differs from ${asset}.sig`);
      }
    }
    if (!Object.keys(data.platforms ?? {}).length) problems.push(`${manifest} lists no platforms`);
  }
  return problems;
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  const [dir, version, ...manifests] = process.argv.slice(2);
  if (!dir || !version) {
    console.error("usage: verify-release.mjs <assets-dir> <version> [manifest.json ...]");
    process.exit(2);
  }
  const problems = verify(dir, version, manifests);
  if (problems.length) {
    for (const problem of problems) console.error(`error: ${problem}`);
    process.exit(1);
  }
  console.log(`release ${version}: checksums, signatures${manifests.length ? " and " + manifests.join(", ") : ""} verified`);
}
