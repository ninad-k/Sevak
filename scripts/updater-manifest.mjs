// Writes the update manifest (latest.json) that installed copies of Sevak poll
// (tauri-plugin-updater; endpoint in src-tauri/tauri.conf.json).
//
//   node scripts/updater-manifest.mjs <assets-dir> <version> <download-base-url> [notes-file]
//
// <assets-dir> holds a release's files, each signed artifact next to its
// `.sig`. The updater looks up `{os}-{arch}-{installer}` and then `{os}-{arch}`,
// so every installer gets its own key and each OS gets a default.
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

// [file name pattern, manifest keys]. The first match for a key wins.
const TARGETS = [
  [/_x64-setup\.exe$/, ["windows-x86_64-nsis", "windows-x86_64"]],
  [/_x64_[\w-]+\.msi$/, ["windows-x86_64-msi"]],
  // The universal app runs on both architectures.
  [
    /\.app\.tar\.gz$/,
    ["darwin-aarch64-app", "darwin-aarch64", "darwin-x86_64-app", "darwin-x86_64"],
  ],
  [/_amd64\.AppImage$/, ["linux-x86_64-appimage", "linux-x86_64"]],
  [/_amd64\.deb$/, ["linux-x86_64-deb"]],
  [/\.x86_64\.rpm$/, ["linux-x86_64-rpm"]],
];

function manifest(files, readSignature, version, baseUrl, notes, pubDate) {
  const platforms = {};
  for (const file of [...files].sort()) {
    if (!files.includes(`${file}.sig`)) continue;
    const target = TARGETS.find(([pattern]) => pattern.test(file));
    if (!target) continue;
    const entry = {
      signature: readSignature(`${file}.sig`).trim(),
      url: `${baseUrl}/${encodeURIComponent(file)}`,
    };
    for (const key of target[1]) platforms[key] ??= entry;
  }
  for (const required of ["windows-x86_64", "darwin-aarch64", "darwin-x86_64", "linux-x86_64"]) {
    if (!platforms[required]) throw new Error(`no signed artifact for ${required}`);
  }
  return { version, notes, pub_date: pubDate, platforms };
}

const [dir, version, baseUrl, notesFile] = process.argv.slice(2);
if (!dir || !version || !baseUrl) {
  console.error("usage: updater-manifest.mjs <assets-dir> <version> <download-base-url> [notes-file]");
  process.exit(2);
}
const notes = notesFile && existsSync(notesFile) ? readFileSync(notesFile, "utf8").trim() : "";
const result = manifest(
  readdirSync(dir),
  (name) => readFileSync(join(dir, name), "utf8"),
  version,
  baseUrl.replace(/\/$/, ""),
  notes,
  new Date().toISOString(),
);
writeFileSync(join(dir, "latest.json"), JSON.stringify(result, null, 2) + "\n");
console.log(`latest.json: ${Object.keys(result.platforms).join(", ")}`);
