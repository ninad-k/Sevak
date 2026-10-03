// Renders the package-manager manifests in packaging/ for one release.
//
//   node scripts/package-manifests.mjs <version> <SHA256SUMS.txt> <out-dir>
//
// Placeholders in the templates:
//   {{version}}         the release version (no leading v)
//   {{date}}            today, YYYY-MM-DD
//   {{file:KIND}}       the release asset of that kind (see ASSETS)
//   {{sha256:KIND}}     its SHA-256, lowercase; {{SHA256:KIND}} uppercase (winget)
// Fails if an asset is missing from SHA256SUMS.txt or a placeholder is left over.
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const ASSETS = {
  nsis: /_x64-setup\.exe$/,
  msi: /_x64_[\w-]+\.msi$/,
  dmg: /_universal\.dmg$/,
  deb: /_amd64\.deb$/,
};

const [version, sumsFile, outDir] = process.argv.slice(2);
if (!version || !sumsFile || !outDir) {
  console.error("usage: package-manifests.mjs <version> <SHA256SUMS.txt> <out-dir>");
  process.exit(2);
}

// "<hash>  <name>" (sha256sum output; a leading "*" marks binary mode).
const sums = new Map(
  readFileSync(sumsFile, "utf8")
    .split("\n")
    .map((line) => line.trim().match(/^([0-9a-f]{64})\s+\*?(.+)$/i))
    .filter(Boolean)
    .map(([, hash, name]) => [name, hash.toLowerCase()]),
);
const asset = (kind) => {
  const pattern = ASSETS[kind];
  if (!pattern) throw new Error(`unknown asset kind: ${kind}`);
  const name = [...sums.keys()].find((n) => pattern.test(n) && n.includes(version));
  if (!name) throw new Error(`no ${kind} asset for ${version} in ${sumsFile}`);
  return name;
};

function render(template) {
  const out = template.replace(/\{\{(\w+)(?::(\w+))?\}\}/g, (match, key, kind) => {
    switch (key) {
      case "version":
        return version;
      case "date":
        return new Date().toISOString().slice(0, 10);
      case "file":
        return asset(kind);
      case "sha256":
        return sums.get(asset(kind));
      case "SHA256":
        return sums.get(asset(kind)).toUpperCase();
      default:
        throw new Error(`unknown placeholder ${match}`);
    }
  });
  const left = out.match(/\{\{[^}]*\}\}/);
  if (left) throw new Error(`unfilled placeholder ${left[0]}`);
  return out;
}

const outputs = [
  ["scoop/sevak.json", "scoop/sevak.json"],
  ["homebrew/sevak.rb", "homebrew/Casks/sevak.rb"],
  ["aur/PKGBUILD", "aur/PKGBUILD"],
  ...readdirSync(join(root, "packaging", "winget")).map((file) => [
    `winget/${file}`,
    `winget/manifests/n/NinadKulkarni/Sevak/${version}/${file}`,
  ]),
];
for (const [from, to] of outputs) {
  const text = render(readFileSync(join(root, "packaging", from), "utf8"));
  const target = join(outDir, to);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, text);
  console.log(`wrote ${to}`);
}
