// Release versioning for the CD workflow (.github/workflows/release.yml).
//
//   node scripts/release-version.mjs next        prints the next version, or nothing
//                                                if HEAD is already released
//   node scripts/release-version.mjs set <ver>   writes <ver> into every manifest
//
// The next version comes from the newest `vX.Y.Z` tag plus the Conventional
// Commit subjects since it: `feat:` bumps the minor version, a breaking change
// (`feat!:` or a `BREAKING CHANGE:` footer) bumps the major version (the minor
// one while still on 0.x), and anything else bumps the patch version. With no
// tag yet, the version in src-tauri/tauri.conf.json is released as-is. Raising
// that version above the latest tag forces it as the next release.
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const SEMVER = /^(\d+)\.(\d+)\.(\d+)$/;

const git = (...args) =>
  execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();

const parse = (version) => {
  const m = SEMVER.exec(version);
  if (!m) throw new Error(`not a plain X.Y.Z version: ${version}`);
  return m.slice(1).map(Number);
};

const compare = (a, b) => {
  const [x, y] = [parse(a), parse(b)];
  return x[0] - y[0] || x[1] - y[1] || x[2] - y[2];
};

const configVersion = () =>
  JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"))
    .version;

function latestTag() {
  const tags = git("tag", "--list", "v*", "--merged", "HEAD")
    .split("\n")
    .map((t) => t.slice(1))
    .filter((v) => SEMVER.test(v));
  return tags.sort(compare).at(-1);
}

function next() {
  const baseline = configVersion();
  const last = latestTag();
  if (!last) return baseline;

  const log = git("log", "--format=%s%n%b%x00", `v${last}..HEAD`);
  if (!log) return ""; // HEAD is the latest release

  const messages = log.split("\0");
  const breaking = messages.some((m) =>
    /^[a-z]+(\([^)]*\))?!:|^BREAKING[ -]CHANGE:/m.test(m),
  );
  const feature = messages.some((m) => /^feat(\([^)]*\))?:/m.test(m));

  let [major, minor, patch] = parse(last);
  if (breaking && major > 0) [major, minor, patch] = [major + 1, 0, 0];
  else if (breaking || feature) [minor, patch] = [minor + 1, 0];
  else patch += 1;

  const bumped = `${major}.${minor}.${patch}`;
  return compare(baseline, bumped) > 0 ? baseline : bumped;
}

function set(version) {
  parse(version);

  const editJson = (path, apply) => {
    const file = join(root, path);
    const data = JSON.parse(readFileSync(file, "utf8"));
    apply(data);
    writeFileSync(file, JSON.stringify(data, null, 2) + "\n");
  };
  editJson("src-tauri/tauri.conf.json", (c) => (c.version = version));
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

const [command, arg] = process.argv.slice(2);
if (command === "next") {
  console.log(next());
} else if (command === "set" && arg) {
  set(arg);
  console.log(`version set to ${arg}`);
} else {
  console.error("usage: release-version.mjs next | set <X.Y.Z>");
  process.exit(2);
}
