// Checks the documentation media: every image or file the docs link to exists,
// every file in docs/media is used somewhere, and images carry alt text.
// No dependencies: `node scripts/docs-media/check-media.mjs` (or `npm run docs:media-check`).
import { readdir, readFile, stat } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const mediaDir = join(root, "docs", "media");
const MEDIA_EXT = /\.(png|gif|mp4|svg|srt|jpg|jpeg|webp)$/i;

async function walk(dir, keep, out = []) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!["node_modules", ".git", "target", "site", "dist", ".claude"].includes(entry.name)) await walk(path, keep, out);
    } else if (keep(entry.name)) out.push(path);
  }
  return out;
}

const sources = [
  join(root, "README.md"),
  ...(await walk(join(root, "docs"), (n) => n.endsWith(".md"))),
  ...(await walk(join(root, "landing"), (n) => n.endsWith(".html"))),
];
const problems = [];
const used = new Set();

for (const file of sources) {
  const text = await readFile(file, "utf8");
  const shown = relative(root, file).replaceAll("\\", "/");
  // Markdown images and links, and HTML src/href attributes.
  const refs = [
    ...[...text.matchAll(/!?\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g)].map((m) => ({ alt: m[1], target: m[2], image: m[0].startsWith("!") })),
    ...[...text.matchAll(/<(img|a|source|video)\b[^>]*?(?:src|href)="([^"]+)"[^>]*>/g)].map((m) => ({ alt: /\balt="([^"]*)"/.exec(m[0])?.[1] ?? null, target: m[2], image: m[1] === "img" })),
  ];
  for (const { alt, target, image } of refs) {
    if (/^(https?:|mailto:|#|data:)/.test(target)) continue;
    const clean = decodeURIComponent(target.split("#")[0].split("?")[0]);
    if (!MEDIA_EXT.test(clean)) continue;
    // The product page is published next to docs/ at the site root, so its paths are repo-relative.
    const candidates = [resolve(dirname(file), clean), resolve(root, clean)];
    let path = null;
    for (const candidate of candidates) { try { await stat(candidate); path = candidate; break; } catch { /* try the next */ } }
    if (path === null) { problems.push(`${shown}: missing file ${target}`); continue; }
    used.add(path);
    // Empty alt text is right for decoration (the landing page's logos), so only Markdown is held to this.
    if (image && file.endsWith(".md") && (alt === null || alt.trim().length < 10)) problems.push(`${shown}: image ${target} needs descriptive alt text`);
  }
}

// docs/media/README.md links every asset in its gallery, so an asset nobody
// mentions anywhere is a leftover.
const mkdocs = await readFile(join(root, "mkdocs.yml"), "utf8");
for (const file of await walk(mediaDir, (n) => MEDIA_EXT.test(n))) {
  // The site logo and favicon are set in mkdocs.yml.
  if (mkdocs.includes(`media/${relative(mediaDir, file).replaceAll("\\", "/")}`)) used.add(file);
  if (!used.has(file)) problems.push(`docs/media/${relative(mediaDir, file).replaceAll("\\", "/")}: not used by any page`);
}

if (problems.length > 0) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log(`Media check passed: ${used.size} files referenced from ${sources.length} pages.`);
