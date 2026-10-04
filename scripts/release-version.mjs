// Release versioning for the CD workflows (.github/workflows/release.yml,
// promote.yml, rollback.yml).
//
//   node scripts/release-version.mjs next         prints the next stable version, or
//                                                 nothing if HEAD is already released
//   node scripts/release-version.mjs next-beta    prints the next beta version
//                                                 (X.Y.Z-beta.N), or nothing if HEAD
//                                                 already has a beta tag
//   node scripts/release-version.mjs previous-tag
//                                                 prints the newest release tag (stable or
//                                                 beta) before HEAD, or nothing
//   node scripts/release-version.mjs set <ver>    writes <ver> into every manifest
//   node scripts/release-version.mjs compare <a> <b>
//                                                 prints -1, 0 or 1 (semver order,
//                                                 pre-releases sort before releases)
//
// The next version comes from the newest stable `vX.Y.Z` tag plus the
// Conventional Commit subjects since it: `feat:` bumps the minor version, a
// breaking change (`feat!:` or a `BREAKING CHANGE:` footer) bumps the major
// version (the minor one while still on 0.x), and anything else bumps the patch
// version. With no tag yet, the version in src-tauri/tauri.conf.json is
// released as-is. Raising that version above the latest tag forces it as the
// next release.
//
// A beta is the version the next stable release would get, plus `-beta.N`
// (N counts up from 1 per base version). Beta tags (`v1.3.0-beta.2`) never
// move the stable baseline: `next` only looks at plain `vX.Y.Z` tags.
import { readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const STABLE = /^(\d+)\.(\d+)\.(\d+)$/;
// Full semver subset we ship: X.Y.Z with an optional dot-separated pre-release.
const SEMVER =
  /^(\d+)\.(\d+)\.(\d+)(?:-((?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*))?$/;

const git = (cwd, ...args) =>
  execFileSync("git", args, { cwd, encoding: "utf8" }).trim();

export const isStable = (version) => STABLE.test(version);

export const parse = (version) => {
  const m = SEMVER.exec(version);
  if (!m) throw new Error(`not a valid X.Y.Z[-pre] version: ${version}`);
  return { core: m.slice(1, 4).map(Number), pre: m[4] ? m[4].split(".") : [] };
};

const comparePre = (a, b) => {
  // A release outranks any pre-release of the same X.Y.Z.
  if (!a.length || !b.length) return Math.sign(b.length - a.length);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if (a[i] === undefined) return -1;
    if (b[i] === undefined) return 1;
    const [x, y] = [a[i], b[i]];
    const numeric = [/^\d+$/.test(x), /^\d+$/.test(y)];
    if (numeric[0] && numeric[1]) {
      if (Number(x) !== Number(y)) return Number(x) < Number(y) ? -1 : 1;
    } else if (numeric[0] !== numeric[1]) {
      return numeric[0] ? -1 : 1; // numeric identifiers sort before text
    } else if (x !== y) {
      return x < y ? -1 : 1;
    }
  }
  return 0;
};

export const compare = (a, b) => {
  const [x, y] = [parse(a), parse(b)];
  for (let i = 0; i < 3; i++) if (x.core[i] !== y.core[i]) return x.core[i] < y.core[i] ? -1 : 1;
  return comparePre(x.pre, y.pre);
};

const configVersion = () =>
  JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"))
    .version;

/** The newest plain `vX.Y.Z` tag reachable from HEAD (betas are ignored). */
function latestTag(cwd) {
  const tags = git(cwd, "tag", "--list", "v*", "--merged", "HEAD")
    .split("\n")
    .map((t) => t.slice(1))
    .filter((v) => STABLE.test(v));
  return tags.sort(compare).at(-1);
}

/**
 * The version after `last` for these commit messages; `baseline` (the version
 * in tauri.conf.json) wins when it is higher. Pure, so it can be tested.
 */
export function bump(last, messages, baseline) {
  const breaking = messages.some((m) =>
    /^[a-z]+(\([^)]*\))?!:|^BREAKING[ -]CHANGE:/m.test(m),
  );
  const feature = messages.some((m) => /^feat(\([^)]*\))?:/m.test(m));

  let [major, minor, patch] = parse(last).core;
  if (breaking && major > 0) [major, minor, patch] = [major + 1, 0, 0];
  else if (breaking || feature) [minor, patch] = [minor + 1, 0];
  else patch += 1;

  const bumped = `${major}.${minor}.${patch}`;
  return compare(baseline, bumped) > 0 ? baseline : bumped;
}

export function next({ cwd = root, baseline = configVersion() } = {}) {
  const last = latestTag(cwd);
  if (!last) return baseline;

  const log = git(cwd, "log", "--format=%s%n%b%x00", `v${last}..HEAD`);
  if (!log) return ""; // HEAD is the latest release
  return bump(last, log.split("\0"), baseline);
}

/** `X.Y.Z-beta.N` where N follows the highest existing beta of that base. */
export function betaVersion(base, tags) {
  const prefix = `v${base}-beta.`;
  const highest = tags
    .filter((t) => t.startsWith(prefix) && /^\d+$/.test(t.slice(prefix.length)))
    .map((t) => Number(t.slice(prefix.length)))
    .reduce((a, b) => Math.max(a, b), 0);
  return `${base}-beta.${highest + 1}`;
}

export function nextBeta({ cwd = root, baseline = configVersion() } = {}) {
  const base = next({ cwd, baseline });
  if (!base) return ""; // HEAD is already a stable release
  // Re-running on a commit that already has a beta must not mint another.
  if (git(cwd, "tag", "--points-at", "HEAD", "--list", "v*-beta.*")) return "";
  return betaVersion(base, git(cwd, "tag", "--list", "v*-beta.*").split("\n"));
}

/**
 * The MSI version for a pre-release: `1.3.0-beta.2` becomes `1.3.0.2`. The
 * Windows Installer only takes numeric fields (at most 255.255.65535.65535),
 * and Tauri refuses a pre-release it cannot map itself. Releases return
 * undefined and keep Tauri's own mapping.
 */
export function wixVersion(version) {
  const { core, pre } = parse(version);
  if (!pre.length) return undefined;
  const build = pre.findLast((id) => /^\d+$/.test(id));
  if (build === undefined) throw new Error(`pre-release needs a numeric part: ${version}`);
  if (core[0] > 255 || core[1] > 255 || core[2] > 65535 || Number(build) > 65535) {
    throw new Error(`version too large for an MSI: ${version}`);
  }
  return `${core.join(".")}.${Number(build)}`;
}

/** The newest `vX.Y.Z` or `vX.Y.Z-beta.N` tag reachable from HEAD: the base for release notes. */
export function previousTag({ cwd = root } = {}) {
  const tags = git(cwd, "tag", "--list", "v*", "--merged", "HEAD")
    .split("\n")
    .filter((t) => SEMVER.test(t.slice(1)));
  return tags.sort((a, b) => compare(a.slice(1), b.slice(1))).at(-1) ?? "";
}

function set(version) {
  parse(version);

  const editJson = (path, apply) => {
    const file = join(root, path);
    const data = JSON.parse(readFileSync(file, "utf8"));
    apply(data);
    writeFileSync(file, JSON.stringify(data, null, 2) + "\n");
  };
  const wix = wixVersion(version);
  editJson("src-tauri/tauri.conf.json", (c) => {
    c.version = version;
    // The MSI wants a numeric X.Y.Z.N version; a pre-release needs it spelled out.
    if (wix) {
      c.bundle.windows.wix = { ...c.bundle.windows.wix, version: wix };
    } else if (c.bundle?.windows?.wix) {
      delete c.bundle.windows.wix.version;
    }
  });
  editJson("package.json", (p) => (p.version = version));
  editJson("package-lock.json", (l) => {
    l.version = version;
    if (l.packages?.[""]) l.packages[""].version = version;
  });

  // [workspace.package] version, inherited by every crate.
  const cargo = join(root, "Cargo.toml");
  const toml = readFileSync(cargo, "utf8");
  const updated = toml.replace(
    /(\[workspace\.package\][^[]*?\nversion\s*=\s*)"[^"]*"/,
    `$1"${version}"`,
  );
  if (updated === toml && !toml.includes(`version = "${version}"`)) {
    throw new Error("could not find [workspace.package] version in Cargo.toml");
  }
  writeFileSync(cargo, updated);
}

const isMain =
  process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url;
if (isMain) {
  const [command, arg, arg2] = process.argv.slice(2);
  if (command === "next") {
    console.log(next());
  } else if (command === "next-beta") {
    console.log(nextBeta());
  } else if (command === "previous-tag") {
    console.log(previousTag());
  } else if (command === "set" && arg) {
    set(arg);
    console.log(`version set to ${arg}`);
  } else if (command === "compare" && arg && arg2) {
    console.log(compare(arg, arg2));
  } else {
    console.error("usage: release-version.mjs next | next-beta | previous-tag | set <version> | compare <a> <b>");
    process.exit(2);
  }
}
