// Generates the CycloneDX 1.5 software bills of materials of a Sevak release.
//
//   node scripts/generate-sbom.mjs [--out <dir>]     default: ./sbom
//   node scripts/generate-sbom.mjs --tool-version cargo-cyclonedx
//
// Writes, into the output folder:
//   sevak-sbom-rust.cdx.json   the Rust crates of the `sevak` app, from Cargo.lock
//   sevak-sbom-npm.cdx.json    the npm packages inside the shipped UI bundle
//   SBOM-SHA256SUMS.txt        sha256 of the two files above
//
// Tools (versions pinned here; .github/workflows/sbom.yml reads them from here):
//   cargo-cyclonedx   `cargo install cargo-cyclonedx --version <v> --locked`
//   @cyclonedx/cyclonedx-npm   run through npx, needs `npm ci` first
//
// The Rust SBOM is built for all targets (`--target all`), so it lists the
// dependencies of every platform's build, build-only crates left out. It is a
// superset of what one installer contains; THIRD_PARTY_NOTICES.md lists the
// per-platform union. The npm SBOM is filtered to the packages that are
// actually bundled (see scripts/lib/ui-bundle.mjs): the full tool output also
// lists Vite, TypeScript and the other build tooling, which is not shipped.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { bundledNpmNames } from "./lib/ui-bundle.mjs";

const TOOLS = { "cargo-cyclonedx": "0.5.9", "@cyclonedx/cyclonedx-npm": "6.0.1" };
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const args = process.argv.slice(2);
if (args[0] === "--tool-version") {
  const version = TOOLS[args[1]];
  if (!version) {
    console.error(`unknown tool ${args[1]}`);
    process.exit(2);
  }
  console.log(version);
  process.exit(0);
}
const outIndex = args.indexOf("--out");
const out = resolve(root, outIndex >= 0 ? args[outIndex + 1] : "sbom");
mkdirSync(out, { recursive: true });

function run(command, commandArgs) {
  // Node will not spawn npx.cmd without a shell; npm ships its JavaScript entry point beside node.exe.
  if (command === "npx" && process.platform === "win32") {
    commandArgs = [join(dirname(process.execPath), "node_modules", "npm", "bin", "npx-cli.js"), ...commandArgs];
    command = process.execPath;
  }
  const result = spawnSync(command, commandArgs, { cwd: root, encoding: "utf8", maxBuffer: 1 << 30 });
  if (result.status !== 0) throw new Error(`${command} ${commandArgs.join(" ")} failed:\n${result.stderr || result.error || result.stdout}`);
  return result.stdout;
}

// ------------------------------------------------------------------ Rust
{
  const version = run("cargo", ["cyclonedx", "--version"]).trim();
  if (!version.endsWith(TOOLS["cargo-cyclonedx"])) console.warn(`warning: expected cargo-cyclonedx ${TOOLS["cargo-cyclonedx"]}, found "${version}"`);
  // With --all the tool writes a file per workspace member (crates/*/ and
  // src-tauri/); the app's own file, which lists every dependency, is the one kept.
  run("cargo", ["cyclonedx", "--manifest-path", "src-tauri/Cargo.toml", "--format", "json", "--spec-version", "1.5", "--all", "--no-build-deps", "--target", "all", "--override-filename", "sevak-sbom-rust"]);
  const written = ["src-tauri", ...readdirSync(join(root, "crates")).map((c) => join("crates", c))].map((d) => join(root, d, "sevak-sbom-rust.json"));
  const app = join(root, "src-tauri", "sevak-sbom-rust.json");
  if (!existsSync(app)) throw new Error("cargo cyclonedx did not write src-tauri/sevak-sbom-rust.json");
  copyFileSync(app, join(out, "sevak-sbom-rust.cdx.json"));
  for (const file of written) rmSync(file, { force: true });
}

// ------------------------------------------------------------------- npm
{
  const raw = join(out, "npm-full.json");
  run("npx", ["--yes", `@cyclonedx/cyclonedx-npm@${TOOLS["@cyclonedx/cyclonedx-npm"]}`, "--output-format", "JSON", "--spec-version", "1.5", "--flatten-components", "--output-reproducible", "--mc-type", "application", "--output-file", raw, "package.json"]);
  const bom = JSON.parse(readFileSync(raw, "utf8"));
  rmSync(raw);

  const bundled = new Set(bundledNpmNames(root));
  const fullName = (c) => (c.group ? `${c.group}/${c.name}` : c.name);
  const kept = bom.components.filter((c) => bundled.has(fullName(c)));
  const missing = [...bundled].filter((name) => !kept.some((c) => fullName(c) === name));
  if (missing.length) throw new Error(`bundled but missing from the npm SBOM: ${missing.join(", ")}`);

  const refs = new Set([bom.metadata.component["bom-ref"], ...kept.map((c) => c["bom-ref"])]);
  bom.components = kept;
  bom.dependencies = bom.dependencies
    .filter((d) => refs.has(d.ref))
    .map((d) => ({ ...d, dependsOn: (d.dependsOn ?? []).filter((r) => refs.has(r)) }));
  // The root's direct dependencies are the bundled packages, whichever section of package.json lists them.
  const rootEntry = bom.dependencies.find((d) => d.ref === bom.metadata.component["bom-ref"]);
  if (rootEntry) rootEntry.dependsOn = kept.map((c) => c["bom-ref"]);
  bom.metadata.properties = [...(bom.metadata.properties ?? []), { name: "sevak:scope", value: "npm packages bundled into the shipped UI (ui/dist), not build tooling" }];
  writeFileSync(join(out, "sevak-sbom-npm.cdx.json"), `${JSON.stringify(bom, null, 2)}\n`);
}

// ------------------------------------------------------------- checksums
{
  const lines = ["sevak-sbom-rust.cdx.json", "sevak-sbom-npm.cdx.json"].map((name) => `${createHash("sha256").update(readFileSync(join(out, name))).digest("hex")}  ${name}`);
  writeFileSync(join(out, "SBOM-SHA256SUMS.txt"), `${lines.join("\n")}\n`);
  console.log(lines.join("\n"));
}
