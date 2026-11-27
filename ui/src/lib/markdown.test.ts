// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { isMarkdownName, renderMarkdown as md, safeUrl } from "./markdown";

/** The parsed attributes of every element: what a browser would actually see. */
function attributes(html: string): { tag: string; name: string; value: string }[] {
  const root = document.createElement("div");
  root.innerHTML = html;
  const found: { tag: string; name: string; value: string }[] = [];
  for (const el of root.querySelectorAll("*")) {
    for (const attr of el.attributes) found.push({ tag: el.tagName.toLowerCase(), name: attr.name, value: attr.value });
  }
  return found;
}

/** No dangerous element, event handler or script URL once the HTML is parsed. */
function expectInert(html: string) {
  const root = document.createElement("div");
  root.innerHTML = html;
  expect(root.querySelector("script, iframe, style, object, embed, svg, math, form, link, meta, base")).toBeNull();
  for (const { name, value } of attributes(html)) {
    expect(name.startsWith("on"), `${name} on an element`).toBe(false);
    if (name === "href" || name === "src") expect(value, `${name}=${value}`).not.toMatch(/^\s*(javascript|vbscript):/i);
  }
}

describe("blocks", () => {
  it("renders headings of every level and ignores a missing space", () => {
    expect(md("# One")).toBe('<h1 class="md-h">One</h1>');
    expect(md("###### Six ##")).toBe('<h6 class="md-h">Six</h6>');
    expect(md("####### Seven")).toBe("<p>####### Seven</p>");
    expect(md("#NoSpace")).toBe("<p>#NoSpace</p>");
    expect(md("#")).toBe('<h1 class="md-h"></h1>');
  });

  it("joins paragraph lines and splits at blank lines", () => {
    expect(md("a\nb\n\nc")).toBe("<p>a\nb</p><p>c</p>");
  });

  it("makes hard breaks from two spaces or a backslash", () => {
    expect(md("a  \nb")).toBe("<p>a<br>b</p>");
    expect(md("a\\\nb")).toBe("<p>a<br>b</p>");
  });

  it("renders thematic breaks", () => {
    expect(md("---")).toBe("<hr>");
    expect(md("* * *")).toBe("<hr>");
    expect(md("___")).toBe("<hr>");
    expect(md("a\n\n---\n\nb")).toBe("<p>a</p><hr><p>b</p>");
  });

  it("renders fenced code verbatim, with and without a language", () => {
    expect(md("```\n<b>x</b>\n# not a heading\n```")).toBe(
      '<pre class="md-code"><code>&lt;b&gt;x&lt;/b&gt;\n# not a heading</code></pre>',
    );
    expect(md("```rust\nfn main() {}\n```")).toContain('class="language-rust"');
    expect(md("~~~\ntilde\n~~~")).toContain("tilde");
  });

  it("does not trust a fence's info string", () => {
    const html = md('```"><script>alert(1)</script>\nx\n```');
    expectInert(html);
    expect(html).not.toContain("language-");
  });

  it("survives an unterminated fence (the rest is code)", () => {
    expect(md("```\nopen\nforever")).toBe('<pre class="md-code"><code>open\nforever</code></pre>');
    expect(md("```")).toBe('<pre class="md-code"><code></code></pre>');
  });

  it("does not close a fence with a shorter one", () => {
    expect(md("````\n```\nstill code\n````")).toContain("```\nstill code");
  });

  it("renders block quotes, nested ones and a lone >", () => {
    expect(md("> quoted\n> more")).toBe("<blockquote><p>quoted\nmore</p></blockquote>");
    expect(md("> a\n>> b")).toContain("<blockquote><p>a</p><blockquote><p>b</p></blockquote></blockquote>");
    expect(md(">")).toBe("<blockquote></blockquote>");
    expect(md("> ")).toBe("<blockquote></blockquote>");
    expect(md("> > >")).toBe("<blockquote><blockquote><blockquote></blockquote></blockquote></blockquote>");
  });

  it("lets a quoted paragraph continue lazily", () => {
    expect(md("> one\ntwo")).toBe("<blockquote><p>one\ntwo</p></blockquote>");
  });

  it("holds other blocks inside a quote", () => {
    const html = md("> # Title\n> - a\n> - b");
    expect(html).toContain('<h1 class="md-h">Title</h1>');
    expect(html).toContain("<ul><li>a</li><li>b</li></ul>");
  });

  it("does not recurse without bound", () => {
    const deep = ">".repeat(5000) + " x";
    expect(() => md(deep)).not.toThrow();
    expect(md(deep)).toContain("x");
    const nested = Array.from({ length: 400 }, (_, i) => " ".repeat(i * 2) + "- x").join("\n");
    expect(() => md(nested)).not.toThrow();
  });
});

describe("alerts", () => {
  it.each(["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"])("renders [!%s]", (kind) => {
    const html = md(`> [!${kind}]\n> Body **text**`);
    expect(html).toContain(`md-alert-${kind.toLowerCase()}`);
    expect(html).toContain("<strong>text</strong>");
    expect(html).toContain('class="md-alert-title"');
    expect(html).not.toContain("[!");
    expect(html).not.toContain("<blockquote>");
  });

  it("accepts lowercase markers and custom labels", () => {
    const html = md("> [!warning]\n> careful", { alertLabels: { warning: "Achtung" } });
    expect(html).toContain("md-alert-warning");
    expect(html).toContain("<span>Achtung</span>");
  });

  it("is a plain quote when the marker is not alone on its line", () => {
    expect(md("> [!NOTE] inline")).toContain("<blockquote>");
    expect(md("> [!BOGUS]\n> x")).toContain("<blockquote>");
  });

  it("renders an alert without a body", () => {
    expect(md("> [!TIP]")).toContain("md-alert-tip");
  });

  it("escapes custom labels", () => {
    expect(md("> [!NOTE]\n> x", { alertLabels: { note: "<img onerror=x>" } })).not.toContain("<img");
  });
});

describe("lists", () => {
  it("renders unordered and ordered lists", () => {
    expect(md("- a\n- b")).toBe("<ul><li>a</li><li>b</li></ul>");
    expect(md("* a\n+ b")).toBe("<ul><li>a</li><li>b</li></ul>");
    expect(md("1. a\n2. b")).toBe("<ol><li>a</li><li>b</li></ol>");
    expect(md("1) a\n2) b")).toBe("<ol><li>a</li><li>b</li></ol>");
  });

  it("keeps a start number", () => {
    expect(md("3. a\n4. b")).toBe('<ol start="3"><li>a</li><li>b</li></ol>');
  });

  it("nests lists by indentation, even with two spaces under a numbered item", () => {
    expect(md("- a\n  - b\n    - c\n- d")).toBe(
      "<ul><li>a\n<ul><li>b\n<ul><li>c</li></ul></li></ul></li><li>d</li></ul>",
    );
    const html = md("1. a\n  - b\n2. c");
    expect(html).toContain("<ol><li>a\n<ul><li>b</li></ul></li><li>c</li></ol>");
  });

  it("renders task lists", () => {
    const html = md("- [ ] todo\n- [x] done\n- [X] also");
    expect(html).toContain('<ul class="md-tasks">');
    expect(html.match(/type="checkbox"/g)).toHaveLength(3);
    expect(html.match(/ checked>/g)).toHaveLength(2);
    expect(html).toContain("disabled");
    expect(html).toContain("todo");
  });

  it("leaves a bracket that is not a task as text", () => {
    expect(md("- [link](https://a.test)")).toContain('<a href="https://a.test"');
    expect(md("- [x]no space")).not.toContain("checkbox");
  });

  it("makes a list with blank lines between items loose", () => {
    expect(md("- a\n\n- b")).toBe("<ul><li><p>a</p></li><li><p>b</p></li></ul>");
  });

  it("holds several paragraphs and code in an item", () => {
    const html = md("- a\n\n  second\n\n  ```\n  code\n  ```\n- b");
    expect(html).toContain("<p>a</p>");
    expect(html).toContain("<p>second</p>");
    expect(html).toContain("<code>code</code>");
  });

  it("continues an item lazily", () => {
    expect(md("- a\nb")).toBe("<ul><li>a\nb</li></ul>");
  });

  it("allows empty items and a lone marker", () => {
    expect(md("-")).toBe("<ul><li></li></ul>");
    expect(md("- \n- b")).toContain("<li></li><li>b</li>");
  });

  it("ends a list at a paragraph after a blank line", () => {
    expect(md("- a\n\npara")).toBe("<ul><li>a</li></ul><p>para</p>");
  });

  it("does not take a thematic break for a list", () => {
    expect(md("- - -")).toBe("<hr>");
  });
});

describe("inline", () => {
  it("renders emphasis, strong and strikethrough", () => {
    expect(md("*a* _b_")).toBe("<p><em>a</em> <em>b</em></p>");
    expect(md("**a** __b__")).toBe("<p><strong>a</strong> <strong>b</strong></p>");
    expect(md("~~gone~~")).toBe("<p><del>gone</del></p>");
    expect(md("***both***")).toBe("<p><em><strong>both</strong></em></p>");
    expect(md("**bold *and em* text**")).toBe("<p><strong>bold <em>and em</em> text</strong></p>");
  });

  it("leaves stray markers as text", () => {
    expect(md("2 * 3 * 4")).toBe("<p>2 * 3 * 4</p>");
    expect(md("snake_case_name")).toBe("<p>snake_case_name</p>");
    expect(md("a ** b")).toBe("<p>a ** b</p>");
    expect(md("**unclosed")).toBe("<p>**unclosed</p>");
    expect(md("~single~")).toBe("<p>~single~</p>");
  });

  it("renders code spans verbatim", () => {
    expect(md("`a <b> *c*`")).toBe("<p><code>a &lt;b&gt; *c*</code></p>");
    expect(md("`` a`b ``")).toBe("<p><code>a`b</code></p>");
    expect(md("`open")).toBe("<p>`open</p>");
  });

  it("honours backslash escapes", () => {
    expect(md("\\*not em\\*")).toBe("<p>*not em*</p>");
    expect(md("a \\x b")).toBe("<p>a \\x b</p>");
  });

  it("renders links with titles", () => {
    expect(md('[Sevak](https://example.com/a?b=1&c=2 "The title")')).toBe(
      '<p><a href="https://example.com/a?b=1&amp;c=2" title="The title" rel="noopener noreferrer">Sevak</a></p>',
    );
    expect(md("[a](<https://example.com/a b>)")).not.toContain("<a ");
    expect(md("[a](https://x.test/(p))")).toContain('href="https://x.test/(p)"');
    expect(md("[mail](mailto:me@example.com)")).toContain('href="mailto:me@example.com"');
  });

  it("puts emphasis inside link text", () => {
    expect(md("[**b**](https://a.test)")).toContain("><strong>b</strong></a>");
  });

  it("autolinks bare URLs and trims the punctuation after them", () => {
    expect(md("see https://example.com/x.")).toBe(
      '<p>see <a href="https://example.com/x" rel="noopener noreferrer">https://example.com/x</a>.</p>',
    );
    expect(md("(https://example.com/a)")).toContain('href="https://example.com/a"');
    expect(md("https://en.wikipedia.org/wiki/Rust_(language)")).toContain('href="https://en.wikipedia.org/wiki/Rust_(language)"');
    expect(md("<https://example.com>")).toContain('<a href="https://example.com"');
    expect(md("<mailto:a@b.test>")).toContain('href="mailto:a@b.test"');
  });

  it("does not link inside code or inside a link's text", () => {
    expect(md("`https://example.com`")).not.toContain("<a ");
    expect(md("[https://a.test](https://b.test)").match(/<a /g)).toHaveLength(1);
  });

  it("leaves a bare ftp: or file: address alone", () => {
    expect(md("ftp://x.test/a")).not.toContain("<a ");
    expect(md("file:///etc/passwd")).not.toContain("<a ");
  });

  it("drops inline HTML and keeps the inner text", () => {
    expect(md("press <kbd>Ctrl</kbd>+<kbd>C</kbd>")).toBe("<p>press Ctrl+C</p>");
    expect(md("a <b>bold</b> <br/> c")).toBe("<p>a bold  c</p>");
    expect(md("<!-- hidden --> shown")).toBe("<p> shown</p>");
    expect(md("1 < 2 and 3 > 2")).toBe("<p>1 &lt; 2 and 3 &gt; 2</p>");
  });

  it("escapes everything else", () => {
    expect(md('a & b "q" \'s\'')).toBe("<p>a &amp; b &quot;q&quot; &#39;s&#39;</p>");
  });
});

describe("images", () => {
  it("shows the alt text and links the address; never an img by itself", () => {
    const html = md("![A cat](https://example.com/cat.png)");
    expect(html).not.toContain("<img");
    expect(html).toContain("A cat");
    expect(html).toContain('href="https://example.com/cat.png"');
  });

  it("shows alt text only for local or odd sources", () => {
    const html = md("![local](/home/me/pic.png)");
    expect(html).toBe('<p><span class="md-img-alt">local</span></p>');
  });

  it("uses what the host resolves, if it is a data image", () => {
    const data = "data:image/png;base64,iVBORw0KGgo=";
    const html = md("![pic](a.png)", { resolveImage: (src) => (src === "a.png" ? data : null) });
    expect(html).toContain(`<img class="md-img" src="${data}" alt="pic">`);
  });

  it("refuses anything but a data image from the host", () => {
    for (const bad of ["javascript:alert(1)", "https://e.test/x.png", "data:text/html;base64,AAAA", 'data:image/png;base64,AA" onerror="x']) {
      const html = md("![pic](a.png)", { resolveImage: () => bad });
      expect(html).not.toContain("<img");
      expectInert(html);
    }
  });
});

describe("tables", () => {
  const table = "| Name | Qty | Price |\n|:--|:-:|--:|\n| apple | 3 | 1.5 |\n| kiwi | 12 | 0.25 |";

  it("renders header, body and alignment", () => {
    const html = md(table);
    expect(html).toContain("<thead><tr><th class=\"md-left\">Name</th><th class=\"md-center\">Qty</th><th class=\"md-right\">Price</th></tr></thead>");
    expect(html).toContain('<td class="md-left">apple</td><td class="md-center">3</td><td class="md-right">1.5</td>');
    expect(html.match(/<tr>/g)).toHaveLength(3);
  });

  it("marks left alignment", () => {
    expect(md("| a |\n|:--|\n| b |")).toContain('<th class="md-left">a</th>');
  });

  it("weights columns by content and caps them", () => {
    const long = "x".repeat(500);
    const html = md(`| id | text |\n|---|---|\n| 1 | ${long} |\n| 22 | short |`);
    const widths = [...html.matchAll(/<col width="([\d.]+)%">/g)].map((m) => Number(m[1]));
    expect(widths).toHaveLength(2);
    expect(widths[0] + widths[1]).toBeCloseTo(100, 0);
    expect(widths[1]).toBeGreaterThan(widths[0]);
    // 4 (minimum) against 48 (cap): the long cell cannot take more than its cap.
    expect(widths[1]).toBeLessThan(95);
  });

  it("gives equal short columns equal widths", () => {
    const widths = [...md("| a | b |\n|---|---|\n| c | d |").matchAll(/width="([\d.]+)%"/g)].map((m) => Number(m[1]));
    expect(widths).toEqual([50, 50]);
  });

  it("pads short rows, trims long ones and handles escaped pipes", () => {
    const html = md("| a | b |\n|---|---|\n| only |\n| 1 | 2 | 3 |\n| x \\| y | `p|q` |");
    expect(html).toContain("<td>only</td><td></td>");
    expect(html).not.toContain(">3<");
    expect(html).toContain("<td>x | y</td>");
    expect(html).toContain("<code>p|q</code>");
  });

  it("works without outer pipes and renders inline markup in cells", () => {
    const html = md("a | b\n--|--\n**x** | [l](https://a.test)");
    expect(html).toContain("<strong>x</strong>");
    expect(html).toContain('<a href="https://a.test"');
  });

  it("is not a table when the delimiter row does not match", () => {
    expect(md("| a | b |\n|---|\n| c | d |")).not.toContain("<table");
    expect(md("a | b\nno delimiter")).not.toContain("<table");
  });

  it("ends at a blank line", () => {
    expect(md("| a |\n|---|\n| b |\n\nafter")).toContain("</table></div><p>after</p>");
  });

  it("limits the columns", () => {
    const row = Array.from({ length: 60 }, (_, i) => `c${i}`);
    const html = md(`|${row.join("|")}|\n|${row.map(() => "-").join("|")}|`);
    expect(html.match(/<th[ >]/g)).toHaveLength(24);
  });
});

describe("XSS", () => {
  it("never passes a script tag through", () => {
    for (const src of [
      "<script>alert(1)</script>",
      "<SCRIPT SRC=//evil.test/x.js></SCRIPT>",
      "text <script>alert(1)</script> text",
      "<script>\nalert(1)\n</script>",
      "<style>body{display:none}</style>",
      "<script",
      "<scr<script>ipt>alert(1)</scr</script>ipt>",
    ]) {
      const html = md(src);
      expectInert(html);
      expect(html).not.toContain("alert(1)</");
    }
    expect(md("a <script>alert(1)</script> b")).toBe("<p>a  b</p>");
  });

  it("escapes a script tag in code, headings and tables", () => {
    for (const src of [
      "`<script>alert(1)</script>`",
      "```\n<script>alert(1)</script>\n```",
      "# <script>alert(1)</script>",
      "| <script>x</script> |\n|---|\n| <img src=x onerror=alert(1)> |",
    ]) {
      expectInert(md(src));
    }
    expect(md("`<script>`")).toContain("&lt;script&gt;");
  });

  it("refuses javascript: links in every spelling", () => {
    for (const url of [
      "javascript:alert(1)",
      "JaVaScRiPt:alert(1)",
      " javascript:alert(1)",
      "java\tscript:alert(1)",
      "java&#x09;script:alert(1)",
      "&#106;avascript:alert(1)",
      "\u0001javascript:alert(1)",
      "data:text/html;base64,PHNjcmlwdD4=",
      "vbscript:msgbox(1)",
      "file:///etc/passwd",
      "//evil.test/x",
      "/relative",
      "#anchor",
      "https:evil.test",
    ]) {
      const html = md(`[click](${url})`);
      expect(html, url).not.toContain("<a ");
      expect(html, url).not.toContain("href=");
      expectInert(html);
    }
    expect(md("[x](<javascript:alert(1)>)")).not.toContain("href");
  });

  it("cannot break out of a link title or address", () => {
    const html = md('[x](https://e.test "a\\" onmouseover=\\"alert(1)")');
    expect(html).toContain("title=");
    expect(attributes(html).map((a) => a.name)).toEqual(["href", "title", "rel"]);
    expect(html).toContain("&quot; onmouseover=&quot;alert(1)");

    const single = md("[x](https://e.test 'a\" onfocus=\"alert(1)')");
    expect(attributes(single).map((a) => a.name)).toEqual(["href", "title", "rel"]);

    const url = md('[x](https://e.test/"onmouseover="alert(1))');
    expectInert(url);
    expectInert(url);

    const paren = md('[x](https://e.test (a" onclick="b))');
    expectInert(paren);
  });

  it("cannot break out of link text", () => {
    const html = md('[<img src=x onerror=alert(1)>](https://e.test) [">< onclick=x](https://e.test)');
    expect(html).not.toContain("<img");
    expectInert(html);
  });

  it("drops <img onerror> and every other tag", () => {
    for (const src of [
      "<img src=x onerror=alert(1)>",
      '<img src="x" onerror="alert(1)"/>',
      "<svg onload=alert(1)>",
      "<iframe src=javascript:alert(1)></iframe>",
      '<a href="javascript:alert(1)">x</a>',
      "<body onload=alert(1)>",
      "<details open ontoggle=alert(1)>",
      "<math><mi xlink:href=javascript:alert(1)>",
    ]) {
      const html = md(src);
      expect(html).not.toMatch(/<(img|svg|iframe|body|details|math|a)\b/i);
      expectInert(html);
    }
  });

  it("escapes alt text and image titles", () => {
    const html = md('![a" onerror="alert(1)](https://e.test/x.png "t\\" onload=\\"1")');
    expect(html).not.toContain("<img");
    expectInert(html);
    const withData = md('![a" onerror="x](p.png "t\\" onload=\\"1")', {
      resolveImage: () => "data:image/png;base64,AAAA",
    });
    expectInert(withData);
    expect(attributes(withData).map((a) => a.name)).toEqual(["class", "src", "alt", "title"]);
  });

  it("neutralises entities and encoded tags", () => {
    const html = md("&lt;script&gt;alert(1)&lt;/script&gt; &#60;script&#62;");
    expect(html).not.toContain("<script");
    expect(html).toContain("&amp;lt;script");
  });

  it("escapes a task checkbox's text and alert labels", () => {
    expect(md("- [x] <script>x</script>")).not.toContain("<script");
  });

  it("does not let a code language class carry markup", () => {
    expect(md("```a\"b\nx\n```")).not.toContain('class="language-a"');
  });

  it("only emits tags from its own set", () => {
    const hostile = [
      "<script>1</script>",
      "<img src=x onerror=1>",
      "[a](javascript:1)",
      "# <b>x</b>",
      "> [!NOTE] <i>x</i>",
      "- <u>x</u>",
      "| <s>a</s> |\n|---|\n| <em>b</em> |",
      "`<x>` **<y>** ~~<z>~~",
    ].join("\n\n");
    const tags = new Set([...md(hostile).matchAll(/<\/?([a-z0-9]+)/g)].map((m) => m[1]));
    const allowed = new Set([
      "p", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "blockquote", "pre", "code", "a", "em", "strong", "del", "br", "hr",
      "table", "thead", "tbody", "tr", "th", "td", "colgroup", "col", "div", "span", "input", "img",
    ]);
    for (const tag of tags) expect(allowed.has(tag), tag).toBe(true);
  });
});

describe("edge input", () => {
  it.each([
    "",
    "\n\n\n",
    ">",
    "> ",
    "-",
    "1.",
    "#",
    "|",
    "||\n||",
    "| a |\n|",
    "```",
    "~~~~\n",
    "[",
    "]",
    "[]()",
    "![]()",
    "[a](",
    "[a](<",
    "![a](",
    "**",
    "*_*_*",
    "`",
    "``` ` ```",
    "<",
    "<a",
    "<!--",
    "&",
    "\\",
    "\u0000",
    "\r\n\r\n",
    "\t- a\n\t\t- b",
    "- [ ]",
    "- [x]\n",
    "* * *\n*\n* a",
    "1. a\n   1. b\n      1. c",
  ])("does not crash on %j", (src) => {
    const html = md(src);
    expect(typeof html).toBe("string");
    expectInert(html);
  });

  it("treats \\r\\n like \\n", () => {
    expect(md("a\r\n\r\nb")).toBe(md("a\n\nb"));
  });

  it("cuts oversized input", () => {
    expect(md("a".repeat(100), { maxChars: 10 })).toBe(`<p>${"a".repeat(10)}</p>`);
  });

  it("handles long pathological runs quickly", () => {
    const started = Date.now();
    for (const src of ["*".repeat(50_000), "[".repeat(50_000), "`".repeat(50_000), "_a".repeat(20_000), "![](".repeat(10_000), "<".repeat(50_000), "- ".repeat(5_000), "|".repeat(50_000)]) {
      md(src);
    }
    expect(Date.now() - started).toBeLessThan(10_000);
  }, 30_000);
});

describe("helpers", () => {
  it("accepts only http, https and mailto addresses", () => {
    expect(safeUrl("https://a.test/x")).toBe("https://a.test/x");
    expect(safeUrl("HTTP://a.test")).toBe("HTTP://a.test");
    expect(safeUrl("mailto:a@b.test")).toBe("mailto:a@b.test");
    expect(safeUrl("javascript:1")).toBeNull();
    expect(safeUrl("https://")).toBeNull();
    expect(safeUrl("https://a.test/ x")).toBeNull();
    expect(safeUrl('https://a.test/"')).toBeNull();
    expect(safeUrl("")).toBeNull();
  });

  it("knows Markdown file names", () => {
    expect(isMarkdownName("README.md")).toBe(true);
    expect(isMarkdownName("notes.MARKDOWN")).toBe(true);
    expect(isMarkdownName("a.mdx")).toBe(false);
    expect(isMarkdownName("md")).toBe(false);
  });
});
