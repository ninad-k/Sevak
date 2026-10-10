// Tests for scripts/build-marketplace.mjs. Run with `npm run test:scripts`.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { CONTRAST_PAIRS, TOKENS } from "./lib/marketplace-assets.mjs";
import { build, buildCatalog, esc, httpsUrl, renderDetail, renderIndex, trustOf } from "./build-marketplace.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..");
const HASH = "ab".repeat(32);

/** A scratch repository holding the given gallery files. */
function scratch(index, themes = { version: 2, themes: [] }) {
  const root = mkdtempSync(join(tmpdir(), "marketplace-"));
  mkdirSync(join(root, "gallery"));
  writeFileSync(join(root, "gallery", "index.json"), JSON.stringify(index));
  writeFileSync(join(root, "gallery", "themes.json"), JSON.stringify(themes));
  return root;
}

const evil = {
  id: "evil",
  kind: "workflow",
  name: `<script>alert("n")</script>"><img src=x onerror=alert(1)>`,
  description: `'"><script>alert('d')</script> & \`tick\``,
  author: `"><svg onload=alert(1)>`,
  version: `1"><b>`,
  tags: ["ok", `"><script>`, "<b>", "fine-tag"],
  source: "gallery/packages/evil.zip",
  sha256: HASH,
  homepage: `https://example.com/"><script>alert(1)</script>`,
  repository: "javascript:alert(1)",
};

test("escaping covers every character that can leave text or an attribute", () => {
  assert.equal(esc(`<a href="x" onclick='y'>&\``), "&lt;a href=&quot;x&quot; onclick=&#39;y&#39;&gt;&amp;&#96;");
  assert.equal(esc(null), "");
});

test("only plain https addresses are accepted as links", () => {
  assert.equal(httpsUrl("https://github.com/ninad-k/Sevak"), "https://github.com/ninad-k/Sevak");
  for (const bad of [
    "http://example.com",
    "javascript:alert(1)",
    "data:text/html,<script>alert(1)</script>",
    "//example.com",
    "https://user:pw@example.com/",
    "https://exa mple.com",
    'https://example.com/"onmouseover="x',
    "https://example.com/<b>",
    "",
    null,
    42,
  ]) {
    assert.equal(httpsUrl(bad), null, String(bad));
  }
});

test("third-party text cannot inject markup into any generated page", () => {
  const root = scratch({ format: 2, entries: [evil] });
  const out = join(root, "landing", "marketplace");
  try {
    build({ root, out });
    const pages = [readFileSync(join(out, "index.html"), "utf8"), readFileSync(join(out, "workflow", "evil", "index.html"), "utf8")];
    for (const html of pages) {
      // The only scripts are ours: none in the detail page, one external file on the list.
      const scripts = html.match(/<script/gi) ?? [];
      assert.ok(scripts.length <= 1, `unexpected <script in page: ${scripts.length}`);
      assert.ok(!html.includes("<img src=x"), "raw <img> from data");
      assert.ok(!html.includes("<svg onload"), "raw <svg> from data");
      assert.ok(!/<b>/.test(html), "raw <b> from data");
      for (const raw of ['"><script', '"><svg', '"><b>', '"><img src=x', "'><script"]) assert.ok(!html.includes(raw), `a quote closed an attribute: ${raw}`);
      assert.ok(!/<[^>]*\son[a-z]+=/i.test(html.replace(/="[^"]*"/g, '=""')), "an event-handler attribute exists");
      assert.ok(html.includes("&lt;script&gt;alert(&quot;n&quot;)&lt;/script&gt;"), "name is shown escaped");
    }
    // The hostile homepage has its quote and tag characters refused; the repository is not https.
    assert.ok(!/example\.com/.test(pages[1]), "unsafe homepage was linked");
    assert.ok(!pages[1].includes("javascript:"), "javascript: link");
    // Tags that are not lower case a-z, 0-9 and dashes are dropped.
    const catalog = JSON.parse(readFileSync(join(out, "catalog.json"), "utf8"));
    assert.deepEqual(catalog.entries[0].tags, ["ok", "fine-tag"]);
    assert.equal(catalog.entries[0].homepage, null);
    assert.equal(catalog.entries[0].repository, null);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("every absolute link in the generated site is https", () => {
  const root = scratch(
    { format: 2, entries: [evil, { ...evil, id: "http-home", name: "Plain", description: "d", tags: [], homepage: "http://example.com/x", repository: "https://example.com/r" }] },
    { version: 2, themes: [{ id: "t", name: "T", author: "a", description: "d", mode: "dark", url: "gallery/themes/T.toml", sha256: HASH }] },
  );
  const out = join(root, "landing", "marketplace");
  try {
    const entries = build({ root, out });
    const pages = [renderIndex(entries), ...entries.map(renderDetail)];
    let checked = 0;
    for (const html of pages) {
      for (const [, url] of html.matchAll(/(?:href|src)="([^"]*)"/g)) {
        checked++;
        assert.ok(!/^[a-z][a-z0-9+.-]*:/i.test(url) || url.startsWith("https://"), `non-https link ${url}`);
        assert.ok(!url.startsWith("//"), `protocol-relative link ${url}`);
      }
    }
    assert.ok(checked > 20);
    assert.ok(!/http:\/\/example\.com/.test(renderDetail(entries[1])));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("an id that could escape the output folder is refused", () => {
  for (const id of ["../x", "a/b", "A", "", "x y", ".."]) {
    assert.throws(() => buildCatalog({ entries: [{ ...evil, id }] }, { themes: [] }), /id/);
  }
  assert.throws(() => buildCatalog({ entries: [evil, evil] }, { themes: [] }), /duplicate/);
});

test("unknown kinds are skipped and the trust badge follows the kind", () => {
  const catalog = buildCatalog(
    {
      entries: [
        { ...evil, id: "a", kind: "future" },
        { ...evil, id: "b", kind: "workflow", tags: ["no-code"] },
        { ...evil, id: "c", kind: "workflow", tags: ["needs-python"] },
        { ...evil, id: "d", kind: "plugin", tags: [] },
        { ...evil, id: "e", kind: "native", tags: [], platforms: { "linux-x86_64": { source: "gallery/extensions/e/e.sevakext", sha256: HASH } } },
      ],
    },
    { themes: [{ id: "t", name: "T", mode: "light", url: "gallery/themes/T.toml", sha256: HASH }] },
  );
  assert.deepEqual(
    catalog.map((e) => `${e.id}:${e.trust}`),
    ["b:none", "c:script", "d:script", "e:native", "t:none"],
  );
  assert.equal(trustOf({ kind: "theme", tags: [] }), "none");
  const html = renderDetail(catalog[3]);
  assert.ok(html.includes("linux-x86_64"));
  assert.ok(html.includes(HASH));
});

test("a build replaces its own output but never a folder it did not write", () => {
  const root = scratch({ format: 2, entries: [{ ...evil, id: "one", name: "One", homepage: null, repository: null }] });
  const out = join(root, "landing", "marketplace");
  try {
    build({ root, out });
    writeFileSync(join(out, "stale.html"), "old");
    build({ root, out });
    assert.ok(!existsSync(join(out, "stale.html")));
    assert.ok(existsSync(join(root, "landing", "sitemap.xml")));
    const foreign = join(root, "docs-site");
    mkdirSync(foreign);
    writeFileSync(join(foreign, "keep.txt"), "mine");
    assert.throws(() => build({ root, out: foreign }), /not generated/);
    assert.ok(existsSync(join(foreign, "keep.txt")));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the real galleries build: every entry has a page, a card, a hash and a catalog row", () => {
  const out = mkdtempSync(join(tmpdir(), "marketplace-real-"));
  try {
    const entries = build({ root: repoRoot, out: join(out, "marketplace") });
    const index = JSON.parse(readFileSync(join(repoRoot, "gallery", "index.json"), "utf8"));
    const themes = JSON.parse(readFileSync(join(repoRoot, "gallery", "themes.json"), "utf8"));
    assert.equal(entries.length, index.entries.length + themes.themes.length);
    const listing = readFileSync(join(out, "marketplace", "index.html"), "utf8");
    const catalog = JSON.parse(readFileSync(join(out, "marketplace", "catalog.json"), "utf8"));
    assert.equal(catalog.entries.length, entries.length);
    for (const e of entries) {
      const page = readFileSync(join(out, "marketplace", e.kind, e.id, "index.html"), "utf8");
      assert.ok(listing.includes(`href="${e.kind}/${e.id}/"`), `${e.id} missing from the list`);
      if (e.sha256) assert.ok(page.includes(e.sha256), `${e.id} page lacks its hash`);
      assert.ok(page.includes("Report a problem") && page.includes("Request a change"));
      assert.ok(page.includes("Settings"), `${e.id} has no install steps`);
    }
    const sitemap = readFileSync(join(out, "sitemap.xml"), "utf8");
    assert.match(sitemap, /https:\/\/ninad-k\.github\.io\/Sevak\/marketplace\//);
    assert.ok(listing.includes("template=extension_submission.yml"));
    assert.match(listing, /https:\/\/ninad-k\.github\.io\/Sevak\/docs\/marketplace\/publishing\//);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("the product page only links to marketplace entries that exist", () => {
  const html = readFileSync(join(repoRoot, "landing", "index.html"), "utf8");
  assert.ok(html.includes('href="marketplace/"'), "no link to the marketplace");
  const out = mkdtempSync(join(tmpdir(), "marketplace-links-"));
  try {
    const entries = build({ root: repoRoot, out: join(out, "marketplace") });
    const have = new Set(entries.map((e) => `${e.kind}/${e.id}`));
    const links = [...html.matchAll(/href="marketplace\/([a-z]+\/[a-z0-9-]+)\/"/g)].map((m) => m[1]);
    assert.ok(links.length >= 3, "expected a few highlights");
    for (const l of links) assert.ok(have.has(l), `landing links to a missing entry ${l}`);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

function luminance(hex) {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
}
const contrast = (a, b) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

test("text reaches WCAG AA in the dark and the light scheme", () => {
  for (const [scheme, tokens] of Object.entries(TOKENS)) {
    for (const [fg, bg] of CONTRAST_PAIRS) {
      const ratio = contrast(tokens[fg], tokens[bg]);
      assert.ok(ratio >= 4.5, `${scheme}: ${fg} on ${bg} is ${ratio.toFixed(2)}:1`);
    }
  }
});
