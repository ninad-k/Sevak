// Regenerates crates/sevak-plugins/data/emoji.tsv, the offline emoji list of the
// `emoji` plugin: one emoji per line as `glyph<TAB>name<TAB>keyword|keyword|...`.
//
//   node scripts/generate-emoji.mjs [emoji-test.txt] [annotations.json]
//
// Names come from Unicode's emoji-test.txt (version 15.1, which current
// Windows, macOS and Linux fonts draw) and keywords from CLDR's English
// annotations. Without arguments both are downloaded; the plugin itself never
// touches the network. Only fully-qualified emoji without skin-tone modifiers
// are kept, in Unicode's keyboard order. Data: Unicode License v3
// (https://www.unicode.org/license.txt).
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const EMOJI_TEST = "https://unicode.org/Public/emoji/15.1/emoji-test.txt";
const ANNOTATIONS =
  "https://cdn.jsdelivr.net/npm/cldr-annotations-full@46/annotations/en/annotations.json";

async function load(source, url) {
  if (source) return readFileSync(source, "utf8");
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return response.text();
}

const [testPath, annotationsPath] = process.argv.slice(2);
const test = await load(testPath, EMOJI_TEST);
const annotations = JSON.parse(await load(annotationsPath, ANNOTATIONS)).annotations.annotations;

const SKIN_TONES = /[\u{1F3FB}-\u{1F3FF}]/u;
const rows = [];
for (const line of test.split("\n")) {
  const match = /^[0-9A-F ]+;\s*fully-qualified\s*#\s*(\S+)\s+E[\d.]+\s+(.+)$/.exec(line);
  if (!match) continue;
  const [, glyph, name] = match;
  if (SKIN_TONES.test(glyph)) continue;
  const note = annotations[glyph] ?? annotations[glyph.replaceAll("️", "")];
  const words = (note?.default ?? []).filter((word) => !name.includes(word));
  rows.push(`${glyph}\t${name}\t${words.join("|")}`);
}

const out = fileURLToPath(new URL("../crates/sevak-plugins/data/emoji.tsv", import.meta.url));
writeFileSync(out, rows.join("\n") + "\n");
console.log(`${rows.length} emoji written to ${out}`);
