// Build identifiable local installers from a source snapshot, without changing
// working-tree versions, Git state, startup registration, or installed Sevak.
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync, realpathSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { next, parse, previousTag, wixVersion } from "./release-version.mjs";

const repositoryRoot = fileURLToPath(new URL("../", import.meta.url));
const git = (cwd, ...args) => execFileSync("git", args, { cwd, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 }).trim();
const buildRoots = new Set(["crates", "src-tauri", "ui", "scripts", "assets", "packaging", "templates", "docs", "gallery", "examples"]);

export function developmentIdentity({ base, commit, dirty, date = new Date(), revision }) {
  if (parse(base).pre.length) throw new Error("--base must be a stable X.Y.Z version");
  if (!/^[0-9a-f]{7,40}$/i.test(commit)) throw new Error("invalid Git commit identity");
  if (!Number.isFinite(date.getTime())) throw new Error("invalid build time");
  const number = revision ?? date.getUTCHours() * 60 + date.getUTCMinutes();
  if (!Number.isInteger(number) || number < 0 || number > 65535) throw new Error("--revision must be an integer from 0 to 65535");
  const timestamp = date.toISOString().replace(/[-:.]/g, "");
  const version = `${base}-dev.${timestamp}.g${commit.slice(0, 9)}${dirty ? ".dirty" : ""}.${number}`;
  return { version, msiVersion: wixVersion(version), commit, dirty, builtAt: date.toISOString(), base };
}

export function sourceFiles(cwd) {
  const files = (...args) => execFileSync("git", ["ls-files", "-z", ...args], { cwd, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 }).split("\0").filter(Boolean);
  const tracked = files("--cached");
  // Include new source files only from known project roots. Exclude common
  // credential filenames; review the --plan inventory before building.
  const sensitive = /(?:^|\/)(?:\.env(?:\..*)?|id_(?:rsa|ed25519|ecdsa|dsa)(?:\.pub)?|credentials(?:\.[^/]*)?|secrets?(?:\.[^/]*)?)$|\.(?:pfx|p12|pem|key)$/i;
  const candidates = files("--others", "--exclude-standard").filter((file) => buildRoots.has(file.split("/")[0]));
  // Report what the filter removed instead of dropping it silently: a new
  // source file with a secret-like name would otherwise vanish from the build.
  const dropped = candidates.filter((file) => sensitive.test(file)).sort();
  const added = candidates.filter((file) => !sensitive.test(file));
  return { paths: [...new Set([...tracked, ...added])].filter((file) => existsSync(join(cwd, file))).sort(), added, dropped };
}

// Directories whose files are compiled into the app. A dropped file here can
// change what gets built (a module named `secrets.rs`, say), so it is an error.
const compiledSourceRoots = ["crates/", "src-tauri/src/", "ui/src/"];

export function checkDroppedFiles(dropped, allow = false) {
  const inSource = dropped.filter((file) => compiledSourceRoots.some((prefix) => file.startsWith(prefix)));
  if (inSource.length && !allow) {
    throw new Error(`untracked files under a source root match the secret-like filename filter and would be left out of the snapshot: ${inSource.join(", ")}. ` +
      "Rename or track them, delete them, or pass --allow-dropped-source-files to build without them.");
  }
  return inSource;
}

export function copySourceSnapshot(cwd, destination, paths) {
  const sourceRoot = resolve(cwd);
  const snapshotRoot = resolve(destination);
  const sourceRelative = relative(snapshotRoot, sourceRoot);
  if (!sourceRelative || (!sourceRelative.startsWith(`..${sep}`) && sourceRelative !== ".." && !isAbsolute(sourceRelative))) {
    throw new Error("snapshot cannot contain or replace the source checkout");
  }
  mkdirSync(snapshotRoot, { recursive: false });
  const hash = createHash("sha256");
  for (const name of paths) {
    if (isAbsolute(name) || name.split(/[\\/]/).includes("..")) throw new Error(`unsafe snapshot path: ${name}`);
    const source = join(sourceRoot, name);
    if (!lstatSync(source).isFile() || lstatSync(source).isSymbolicLink()) throw new Error(`snapshot requires a regular source file: ${name}`);
    // Check ancestors too: a directory junction must not import outside files.
    for (let dir = dirname(source); dir !== sourceRoot; dir = dirname(dir)) {
      if (lstatSync(dir).isSymbolicLink()) throw new Error(`snapshot cannot follow a linked directory: ${name}`);
    }
    const bytes = readFileSync(source);
    hash.update(name); hash.update("\0"); hash.update(bytes); hash.update("\0");
    const target = join(snapshotRoot, name);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, bytes);
  }
  return { fileCount: paths.length, sha256: hash.digest("hex") };
}

export function chooseBase(cwd, baseline) {
  const planned = next({ cwd, baseline });
  if (planned) return planned;
  // HEAD already has a stable tag: a dev prerelease of that version would be a
  // downgrade. Move to the next patch while remaining below its future release.
  const current = previousTag({ cwd }).replace(/^v/, "") || baseline;
  const [major, minor, patch] = parse(current).core;
  return `${major}.${minor}.${patch + 1}`;
}

export function parseOptions(args) {
  const options = { plan: false, prepareOnly: false, allowDroppedSourceFiles: false };
  for (let i = 0; i < args.length; i++) {
    const option = args[i];
    if (option === "--plan") options.plan = true;
    else if (option === "--allow-dropped-source-files") options.allowDroppedSourceFiles = true;
    else if (option === "--prepare-only") options.prepareOnly = true;
    else if (["--base", "--revision", "--output"].includes(option) && args[i + 1] && !args[i + 1].startsWith("--")) {
      options[option.slice(2)] = option === "--revision" ? Number(args[++i]) : args[++i];
    } else throw new Error(`unknown or incomplete option: ${option}`);
  }
  if (options.plan && options.prepareOnly) throw new Error("choose --plan or --prepare-only");
  return options;
}

function run(file, args, cwd, env) {
  const result = spawnSync(file, args, { cwd, env, stdio: "inherit", windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${file} exited ${result.status ?? result.signal}`);
}

export function main(args = process.argv.slice(2), root = repositoryRoot) {
  const options = parseOptions(args);
  const files = sourceFiles(root);
  const baseline = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8")).version;
  const identity = developmentIdentity({
    base: options.base ?? chooseBase(root, baseline),
    commit: git(root, "rev-parse", "HEAD"),
    dirty: Boolean(git(root, "diff", "--name-only", "HEAD")) || files.added.length > 0,
    revision: options.revision,
  });
  const output = resolve(options.output ?? join(root, "target/local-builds", identity.version));
  const plan = {
    schemaVersion: 1, channel: "local-development", ...identity, output,
    source: { fileCount: files.paths.length, newSourceFiles: files.added, droppedSecretLikeFiles: files.dropped },
    notes: ["Uses local Git tags; fetch tags before planning when current release ancestry matters.",
      "MSI upgrade comparison ignores its fourth numeric field; use distinct --base values for a real upgrade test.",
      "No installation, startup registration, Git tag, commit, or publish is performed."],
  };
  // Print the plan first so a refused build still shows what was dropped.
  if (options.plan) console.log(JSON.stringify(plan, null, 2));
  checkDroppedFiles(files.dropped, options.allowDroppedSourceFiles);
  if (options.plan) return plan;
  if (process.platform !== "win32" && !options.prepareOnly) throw new Error("Windows is required to build Windows installers");
  if (existsSync(output)) throw new Error(`output already exists; choose a new folder: ${output}`);
  // Output can be an external directory or under target, never within source
  // trees where it could be included in the next snapshot.
  const inRepo = relative(root, output);
  if (inRepo === "" || (!inRepo.startsWith(`..${sep}`) && !isAbsolute(inRepo) && !inRepo.startsWith(`target${sep}`))) {
    throw new Error("in-repository output must be under target/");
  }
  mkdirSync(output, { recursive: true });
  const manifestFile = join(output, "BUILD-INFO.json");
  const manifest = { ...plan, status: "preparing", artifacts: [], validation: { status: "not_run", installationExecuted: false } };
  const save = () => writeFileSync(manifestFile, JSON.stringify(manifest, null, 2) + "\n");
  save();
  try {
    const snapshot = join(output, "source");
    manifest.source = { ...manifest.source, ...copySourceSnapshot(root, snapshot, files.paths) };
    run(process.execPath, [join(snapshot, "scripts/release-version.mjs"), "set", identity.version], snapshot, process.env);
    manifest.status = "prepared";
    save();
    if (options.prepareOnly) { console.log(`Prepared ${identity.version}: ${manifestFile}`); return manifest; }
    const cargoTarget = join(root, "target/local-build-cache");
    const env = { ...process.env, CARGO_TARGET_DIR: cargoTarget, SEVAK_GIT_COMMIT: `${identity.commit}${identity.dirty ? "-dirty" : ""}` };
    manifest.status = "building"; save();
    // Fixed commands only; no user-supplied text is interpolated into a shell.
    run(process.env.ComSpec || "cmd.exe", ["/d", "/s", "/c", "npm ci"], snapshot, env);
    run(process.env.ComSpec || "cmd.exe", ["/d", "/s", "/c", "npm run tauri -- build --bundles nsis,msi"], snapshot, env);
    for (const [format, extension] of [["nsis", ".exe"], ["msi", ".msi"]]) {
      const dir = join(cargoTarget, "release/bundle", format);
      const matches = readdirSync(dir).filter((name) => name.startsWith(`Sevak_${identity.version}_`) && name.endsWith(extension));
      if (matches.length !== 1) throw new Error(`expected one ${format} artifact for ${identity.version}, found ${matches.length}`);
      const path = join(output, matches[0]);
      copyFileSync(join(dir, matches[0]), path);
      const bytes = readFileSync(path);
      manifest.artifacts.push({ format, path, sha256: createHash("sha256").update(bytes).digest("hex"), bytes: bytes.length });
    }
    const binary = join(output, "sevak.exe");
    copyFileSync(join(cargoTarget, "release/sevak.exe"), binary);
    manifest.artifacts.push({ format: "binary", path: binary, sha256: createHash("sha256").update(readFileSync(binary)).digest("hex") });
    writeFileSync(join(output, "SHA256SUMS.txt"), manifest.artifacts.map((artifact) => `${artifact.sha256}  ${relative(output, artifact.path)}`).join("\n") + "\n");
    const reportPath = join(output, "artifact-validation.json");
    run("pwsh", ["-NoProfile", "-File", join(snapshot, "scripts/test-windows-installer.ps1"),
      "-MsiPath", manifest.artifacts.find((a) => a.format === "msi").path,
      "-NsisPath", manifest.artifacts.find((a) => a.format === "nsis").path,
      "-BinaryPath", binary, "-ExpectedVersion", identity.version,
      "-ExpectedMsiVersion", identity.msiVersion, "-ReportPath", reportPath], snapshot, env);
    manifest.validation = { status: "passed-read-only-checks", reportPath, installationExecuted: false };
    manifest.status = "built"; save();
    console.log(`Built ${identity.version}: ${output}\nArtifact inspection passed. Installation and restart tests were not run.`);
    return manifest;
  } catch (error) {
    manifest.status = "failed"; manifest.error = error.message; save();
    throw error;
  }
}

if (process.argv[1] && realpathSync(resolve(process.argv[1])) === realpathSync(fileURLToPath(import.meta.url))) {
  try { main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
