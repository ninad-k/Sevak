// Adds or replaces native extension entries in gallery/index.json:
//
//   node scripts/gallery-add-native.mjs entry.json [entry.json ...]
//
// Each argument is a file holding one entry exactly as `sevak-ext entry
// <packages>` prints it. An entry whose id is already in the index replaces
// that entry in place (keeping its tags, which a person chose; `sevak-ext entry`
// only knows "native"); a new id is appended. The file is written the way
// scripts/gallery-check.mjs writes it, so the diff is only the entry.
//
// It checks nothing about the packages: run `node scripts/gallery-check.mjs`
// and `cargo test -p sevak-plugins --test gallery_content` afterwards. The
// publish job of .github/workflows/native-extensions.yml does both.
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const indexPath = fileURLToPath(new URL("../gallery/index.json", import.meta.url));
const files = process.argv.slice(2);
if (files.length === 0) {
  console.error("usage: node scripts/gallery-add-native.mjs entry.json [entry.json ...]");
  process.exit(2);
}

const index = JSON.parse(readFileSync(indexPath, "utf8"));
for (const file of files) {
  const entry = JSON.parse(readFileSync(file, "utf8"));
  if (entry.kind !== "native" || !/^[a-z0-9-]{1,48}$/.test(entry.id ?? "")) {
    console.error(`${file}: not a native extension entry`);
    process.exit(1);
  }
  const at = index.entries.findIndex((existing) => existing.id === entry.id);
  if (at === -1) {
    index.entries.push(entry);
    console.log(`added ${entry.id} ${entry.version}`);
  } else {
    if (index.entries[at].kind !== "native") {
      console.error(`${file}: ${entry.id} is already a ${index.entries[at].kind}, not a native extension`);
      process.exit(1);
    }
    entry.tags = index.entries[at].tags ?? entry.tags;
    index.entries[at] = entry;
    console.log(`updated ${entry.id} ${index.entries[at].version}`);
  }
}
writeFileSync(indexPath, JSON.stringify(index, null, 2) + "\n");
