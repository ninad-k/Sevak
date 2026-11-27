// A small, dependency-free Markdown renderer that produces SAFE HTML.
//
// Safety rules (see markdown.test.ts for the cases):
//  - every piece of text is escaped; raw HTML is never passed through. Inline
//    tags (`<kbd>`, `<b>`...) are dropped and their inner text is kept;
//    `<script>` and `<style>` are dropped together with their content;
//  - links are only emitted for http:, https: and mailto: addresses (always with
//    rel="noopener noreferrer"); anything else is shown as plain text;
//  - images never fetch anything by themselves: the host decides, through
//    `resolveImage`, which source an image gets, and only `data:image/...`
//    base64 sources are accepted from it. Without a resolver an image is its
//    alt text (linked when its address is http/https);
//  - attribute values are generated here (escaped titles, numbers, class names
//    from a fixed set), never copied from the source.
//
// Supported: ATX headings, paragraphs, emphasis, strong, strikethrough, code
// spans and fenced code, block quotes, GitHub alerts (`> [!NOTE]`...), ordered,
// unordered and task lists (nested), thematic breaks, autolinked URLs and GFM
// pipe tables with alignment and column widths weighted by content.

export type AlertKind = "note" | "tip" | "important" | "warning" | "caution";

export interface MarkdownOptions {
  /**
   * The picture for an image's source, or `null` for none. The result is used
   * only when it is a `data:image/(png|jpeg|gif|webp);base64,...` URL.
   */
  resolveImage?: (src: string, alt: string) => string | null;
  /** Labels of the alert callouts (defaults are English). */
  alertLabels?: Partial<Record<AlertKind, string>>;
  /** Longest input rendered (characters); the rest is dropped. */
  maxChars?: number;
}

const DEFAULT_MAX_CHARS = 1_000_000;
const MAX_DEPTH = 12;
const MAX_INLINE_DEPTH = 8;
const MAX_TABLE_COLUMNS = 24;
/** Widest a column's weight can get (characters), so one long cell cannot starve the rest. */
const MAX_COLUMN_WEIGHT = 48;
const MIN_COLUMN_WEIGHT = 4;

const ALERT_LABELS: Record<AlertKind, string> = {
  note: "Note",
  tip: "Tip",
  important: "Important",
  warning: "Warning",
  caution: "Caution",
};

const ALERT_GLYPHS: Record<AlertKind, string> = {
  note: "i",
  tip: "★",
  important: "!",
  warning: "▲",
  caution: "×",
};

interface Context {
  /** Characters the link scanners may still look at; a pathological input runs out instead of going quadratic. */
  scan: { left: number };
  options: MarkdownOptions;
  labels: Record<AlertKind, string>;
}

// ---- escaping and URLs -----------------------------------------------------

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/** The address if it is one we link to (http, https, mailto), else `null`. */
export function safeUrl(raw: string): string | null {
  const url = raw.trim();
  if (url === "" || url.length > 8192) return null;
  // Whitespace, control characters and quote-like characters never belong in a link.
  // eslint-disable-next-line no-control-regex
  if (/[\u0000- \u007f-\u009f"<>\\`]/.test(url)) return null;
  if (/^https?:\/\/[^/?#\s]+/i.test(url)) return url;
  if (/^mailto:[^\s]+$/i.test(url)) return url;
  return null;
}

const DATA_IMAGE = /^data:image\/(?:png|jpe?g|gif|webp);base64,[A-Za-z0-9+/]+=*$/;

// ---- inline ----------------------------------------------------------------

const PUNCTUATION = /[!-/:-@[-`{-~‐-‧‰-⁞]/;
const isSpace = (ch: string) => ch === "" || /\s/.test(ch);
const isPunct = (ch: string) => ch !== "" && PUNCTUATION.test(ch);

interface DelimToken {
  kind: "delim";
  ch: string;
  count: number;
  canOpen: boolean;
  canClose: boolean;
  before: string;
  after: string;
}
interface HtmlToken {
  kind: "html";
  html: string;
}
type Token = DelimToken | HtmlToken;

/** Text between `[`...`]` with nesting, searched within a window. Returns the index of the closing bracket. */
function findClosingBracket(src: string, open: number, ctx: Context): number {
  let depth = 0;
  const end = Math.min(src.length, open + 2000);
  for (let i = open; i < end; i++) {
    if (--ctx.scan.left < 0) return -1;
    const ch = src[i];
    if (ch === "\\") {
      i++;
    } else if (ch === "`") {
      const run = /^`+/.exec(src.slice(i, i + 20))![0];
      const close = src.slice(0, end).indexOf(run, i + run.length);
      i = (close !== -1 ? close + run.length : i + run.length) - 1;
    } else if (ch === "[") {
      depth++;
    } else if (ch === "]") {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

interface Destination {
  url: string;
  title: string | null;
  end: number;
}

/** Parses `(url "title")` starting at the `(`; `end` is just past the `)`. */
function parseDestination(src: string, open: number, ctx: Context): Destination | null {
  const limit = Math.min(src.length, open + 4096);
  ctx.scan.left -= 16;
  let i = open + 1;
  const skipSpace = () => {
    while (i < limit && /[ \t\n]/.test(src[i])) i++;
  };
  skipSpace();
  let url = "";
  if (src[i] === "<") {
    const close = src.indexOf(">", i);
    if (close === -1 || close >= limit || src.slice(i, close).includes("\n")) return null;
    url = src.slice(i + 1, close);
    i = close + 1;
  } else {
    let parens = 0;
    const start = i;
    while (i < limit) {
      if (--ctx.scan.left < 0) return null;
      const ch = src[i];
      if (ch === "\\" && i + 1 < limit) {
        i += 2;
        continue;
      }
      if (/\s/.test(ch)) break;
      if (ch === "(") parens++;
      else if (ch === ")") {
        if (parens === 0) break;
        parens--;
      }
      i++;
    }
    url = src.slice(start, i);
  }
  skipSpace();
  let title: string | null = null;
  const quote = src[i];
  if (quote === '"' || quote === "'" || quote === "(") {
    const closer = quote === "(" ? ")" : quote;
    let j = i + 1;
    let text = "";
    while (j < limit && src[j] !== closer) {
      if (src[j] === "\\" && j + 1 < limit) {
        text += src[j + 1];
        j += 2;
        continue;
      }
      text += src[j];
      j++;
    }
    if (j >= limit) return null;
    title = text;
    i = j + 1;
    skipSpace();
  }
  if (src[i] !== ")") return null;
  return { url: url.replace(/\\([!-/:-@[-`{-~])/g, "$1"), title, end: i + 1 };
}

const BARE_URL = /^https?:\/\/[^\s<>"'`\\]+/i;

/** Trims what a sentence puts after a URL (`.`, `)`, `,`...) off a matched address. */
function trimUrl(url: string): string {
  let out = url;
  for (;;) {
    const last = out[out.length - 1];
    if (last && /[.,:;!?*_~'"]/.test(last)) {
      out = out.slice(0, -1);
    } else if (last === ")") {
      const opens = out.split("(").length - 1;
      const closes = out.split(")").length - 1;
      if (closes > opens) out = out.slice(0, -1);
      else break;
    } else break;
  }
  return out;
}

function linkHtml(url: string, title: string | null, inner: string): string {
  const titleAttr = title ? ` title="${escapeHtml(title)}"` : "";
  return `<a href="${escapeHtml(url)}"${titleAttr} rel="noopener noreferrer">${inner}</a>`;
}

function imageHtml(alt: string, src: string, title: string | null, ctx: Context): string {
  const resolved = ctx.options.resolveImage?.(src, alt) ?? null;
  const titleAttr = title ? ` title="${escapeHtml(title)}"` : "";
  if (resolved && DATA_IMAGE.test(resolved)) {
    return `<img class="md-img" src="${escapeHtml(resolved)}" alt="${escapeHtml(alt)}"${titleAttr}>`;
  }
  const label = `<span class="md-img-alt">${escapeHtml(alt || src)}</span>`;
  const url = safeUrl(src);
  return url ? linkHtml(url, title, label) : label;
}

/** `*`/`_`/`~` runs become emphasis through the CommonMark delimiter-stack algorithm (simplified). */
function resolveEmphasis(tokens: Token[]): string {
  const stack: DelimToken[] = [];
  for (const token of tokens) {
    if (token.kind !== "delim") continue;
    if (token.canClose) {
      let searched = 0;
      for (let s = stack.length - 1; s >= 0 && token.count > 0 && searched < 200; s--, searched++) {
        const opener = stack[s];
        if (opener.ch !== token.ch || !opener.canOpen || opener.count === 0) continue;
        // "Rule of 3": a run that can both open and close does not pair with a
        // run whose combined length is a multiple of 3 (unless both are).
        if (
          (opener.canClose || token.canOpen) &&
          token.ch !== "~" &&
          (opener.count + token.count) % 3 === 0 &&
          (opener.count % 3 !== 0 || token.count % 3 !== 0)
        ) {
          continue;
        }
        const use = token.ch === "~" ? (opener.count >= 2 && token.count >= 2 ? 2 : 0) : opener.count >= 2 && token.count >= 2 ? 2 : 1;
        if (use === 0) continue;
        const tag = token.ch === "~" ? "del" : use === 2 ? "strong" : "em";
        opener.count -= use;
        token.count -= use;
        opener.after = `<${tag}>${opener.after}`;
        token.before += `</${tag}>`;
        // Openers between the pair can no longer match anything.
        stack.length = s + (opener.count > 0 ? 1 : 0);
        s = stack.length;
        searched = 0;
      }
    }
    if (token.canOpen && token.count > 0) stack.push(token);
  }
  let out = "";
  for (const token of tokens) {
    if (token.kind === "html") out += token.html;
    else out += token.before + escapeHtml(token.ch.repeat(token.count)) + token.after;
  }
  return out;
}

function renderInline(src: string, ctx: Context, depth = 0, inLink = false): string {
  if (depth > MAX_INLINE_DEPTH) return escapeHtml(src);
  const tokens: Token[] = [];
  let text = "";
  const flush = () => {
    if (text !== "") {
      tokens.push({ kind: "html", html: escapeHtml(text) });
      text = "";
    }
  };
  const html = (value: string) => {
    flush();
    tokens.push({ kind: "html", html: value });
  };

  let i = 0;
  while (i < src.length) {
    const ch = src[i];

    if (ch === "\\") {
      const next = src[i + 1];
      if (next === "\n") {
        html("<br>");
        i += 2;
      } else if (next !== undefined && /[!-/:-@[-`{-~]/.test(next)) {
        text += next;
        i += 2;
      } else {
        text += ch;
        i++;
      }
      continue;
    }

    if (ch === "`") {
      const run = /^`+/.exec(src.slice(i, i + 64))![0];
      const close = findBackticks(src, i + run.length, run.length);
      if (close === -1) {
        text += run;
        i += run.length;
      } else {
        let code = src.slice(i + run.length, close).replace(/\n/g, " ");
        if (code.length > 2 && code.startsWith(" ") && code.endsWith(" ") && code.trim() !== "") {
          code = code.slice(1, -1);
        }
        html(`<code>${escapeHtml(code)}</code>`);
        i = close + run.length;
      }
      continue;
    }

    if (ch === "!" && src[i + 1] === "[") {
      const close = findClosingBracket(src, i + 1, ctx);
      if (close !== -1 && src[close + 1] === "(") {
        const dest = parseDestination(src, close + 1, ctx);
        if (dest) {
          const alt = plainText(src.slice(i + 2, close));
          html(imageHtml(alt, dest.url, dest.title, ctx));
          i = dest.end;
          continue;
        }
      }
      text += ch;
      i++;
      continue;
    }

    if (ch === "[") {
      const close = findClosingBracket(src, i, ctx);
      if (close !== -1 && src[close + 1] === "(") {
        const dest = parseDestination(src, close + 1, ctx);
        if (dest) {
          const inner = renderInline(src.slice(i + 1, close), ctx, depth + 1, true);
          const url = inLink ? null : safeUrl(dest.url);
          html(url ? linkHtml(url, dest.title, inner) : inner);
          i = dest.end;
          continue;
        }
      }
      text += ch;
      i++;
      continue;
    }

    if (ch === "<") {
      const rest = src.slice(i, i + 2100);
      const auto = /^<((?:https?:\/\/|mailto:)[^\s<>]+)>/i.exec(rest);
      if (auto) {
        const url = safeUrl(auto[1]);
        html(url && !inLink ? linkHtml(url, null, escapeHtml(auto[1])) : escapeHtml(auto[1]));
        i += auto[0].length;
        continue;
      }
      const comment = /^<!--[\s\S]*?-->/.exec(src.slice(i, i + 20000));
      if (comment) {
        i += comment[0].length;
        continue;
      }
      const dropped = /^<(script|style)\b[^>]*>[\s\S]*?<\/\1\s*>/i.exec(src.slice(i, i + 20000));
      if (dropped) {
        i += dropped[0].length;
        continue;
      }
      const tag = /^<\/?[A-Za-z][A-Za-z0-9-]*(?:\s[^<>]*)?\/?>/.exec(rest);
      if (tag) {
        // The tag is dropped; its inner text stays (and is escaped like any text).
        i += tag[0].length;
        continue;
      }
      text += ch;
      i++;
      continue;
    }

    if (!inLink && (ch === "h" || ch === "H") && /^https?:\/\//i.test(src.slice(i, i + 8))) {
      const prev = src[i - 1] ?? "";
      if (prev === "" || !/[A-Za-z0-9]/.test(prev)) {
        const found = BARE_URL.exec(src.slice(i, i + 2100));
        const url = found ? safeUrl(trimUrl(found[0])) : null;
        if (url) {
          html(linkHtml(url, null, escapeHtml(url)));
          i += url.length;
          continue;
        }
      }
    }

    if (ch === "*" || ch === "_" || ch === "~") {
      let run = 1;
      while (src[i + run] === ch) run++;
      if (ch === "~" && run !== 2) {
        text += ch.repeat(run);
        i += run;
        continue;
      }
      const before = src[i - 1] ?? "";
      const after = src[i + run] ?? "";
      const leftFlanking = !isSpace(after) && (!isPunct(after) || isSpace(before) || isPunct(before));
      const rightFlanking = !isSpace(before) && (!isPunct(before) || isSpace(after) || isPunct(after));
      const intraword = ch === "_";
      const canOpen = leftFlanking && (!intraword || !rightFlanking || isPunct(before));
      const canClose = rightFlanking && (!intraword || !leftFlanking || isPunct(after));
      flush();
      tokens.push({ kind: "delim", ch, count: run, canOpen, canClose, before: "", after: "" });
      i += run;
      continue;
    }

    if (ch === " " && src[i + 1] === " ") {
      let j = i;
      while (src[j] === " ") j++;
      if (src[j] === "\n") {
        html("<br>");
        i = j + 1;
        continue;
      }
    }

    if (ch === "\n") {
      text += "\n";
      i++;
      while (src[i] === " ") i++;
      continue;
    }

    text += ch;
    i++;
  }
  flush();
  return resolveEmphasis(tokens);
}

function findBackticks(src: string, from: number, length: number): number {
  const limit = Math.min(src.length, from + 20000);
  let i = from;
  while (i < limit) {
    const at = src.indexOf("`", i);
    if (at === -1 || at >= limit) return -1;
    let run = 0;
    while (src[at + run] === "`") run++;
    if (run === length) return at;
    i = at + run;
  }
  return -1;
}

/** The text of inline markup without its marks (image alt text, table column sizes). */
function plainText(src: string): string {
  return src
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<\/?[A-Za-z][^<>]*>/g, "")
    .replace(/[`*_~]/g, "")
    .replace(/\\([!-/:-@[-`{-~])/g, "$1");
}

// ---- blocks ----------------------------------------------------------------

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([^\s`]*)[^`]*$/;
const HEADING = /^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$/;
const RULE = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const QUOTE = /^ {0,3}>/;
const LIST_ITEM = /^( *)([-*+]|\d{1,9}[.)])(?:([ \t]+)(.*)|$)/;
const TABLE_DELIM = /^[ \t]*\|?[ \t]*:?-+:?[ \t]*(?:\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*$/;
const RAW_BLOCK = /^[ \t]*<(script|style)\b/i;

const isBlank = (line: string) => line.trim() === "";

const indentOf = (line: string) => {
  let n = 0;
  for (const ch of line) {
    if (ch === " ") n++;
    else if (ch === "\t") n += 4;
    else break;
  }
  return n;
};

function splitRow(line: string): string[] {
  let row = line.trim();
  if (row.startsWith("|")) row = row.slice(1);
  if (row.endsWith("|") && !row.endsWith("\\|")) row = row.slice(0, -1);
  const cells: string[] = [];
  let cell = "";
  let inCode = false;
  for (let i = 0; i < row.length; i++) {
    const ch = row[i];
    if (ch === "\\" && row[i + 1] === "|") {
      cell += "|";
      i++;
    } else if (ch === "`") {
      inCode = !inCode;
      cell += ch;
    } else if (ch === "|" && !inCode) {
      cells.push(cell.trim());
      cell = "";
    } else {
      cell += ch;
    }
  }
  cells.push(cell.trim());
  return cells;
}

function startsBlock(line: string, next: string | undefined): boolean {
  if (FENCE.test(line) || RULE.test(line) || QUOTE.test(line) || RAW_BLOCK.test(line)) return true;
  if (HEADING.test(line)) return true;
  const item = LIST_ITEM.exec(line);
  // Only a bullet, or a list that starts at 1, can interrupt a paragraph.
  if (item && item[4] !== undefined && item[4].trim() !== "" && (/^[-*+]$/.test(item[2]) || /^1[.)]$/.test(item[2]))) {
    return true;
  }
  return isTableStart(line, next);
}

function isTableStart(line: string, next: string | undefined): boolean {
  if (next === undefined || !line.includes("|") || !TABLE_DELIM.test(next) || !next.includes("-")) return false;
  if (!next.includes("|") && !line.includes("|")) return false;
  const header = splitRow(line);
  const delim = splitRow(next);
  return header.length === delim.length && delim.every((cell) => /^:?-+:?$/.test(cell));
}

function renderTable(lines: string[], start: number, ctx: Context): { html: string; next: number } {
  const header = splitRow(lines[start]);
  const aligns = splitRow(lines[start + 1]).map((cell) => {
    const left = cell.startsWith(":");
    const right = cell.endsWith(":");
    return left && right ? "center" : right ? "right" : left ? "left" : "";
  });
  const columns = Math.min(header.length, MAX_TABLE_COLUMNS);
  const rows: string[][] = [];
  let i = start + 2;
  while (i < lines.length && !isBlank(lines[i]) && lines[i].includes("|") && !QUOTE.test(lines[i]) && !FENCE.test(lines[i])) {
    const cells = splitRow(lines[i]);
    while (cells.length < columns) cells.push("");
    rows.push(cells.slice(0, columns));
    i++;
  }

  // Column widths follow the content: the longest cell of a column, bounded so a
  // long paragraph in one column does not squeeze the others to nothing.
  const weights = Array.from({ length: columns }, (_, c) => {
    const longest = Math.max(plainText(header[c] ?? "").length, ...rows.map((row) => plainText(row[c] ?? "").length));
    return Math.min(Math.max(longest, MIN_COLUMN_WEIGHT), MAX_COLUMN_WEIGHT);
  });
  const total = weights.reduce((sum, w) => sum + w, 0);
  const cols = weights.map((w) => `<col width="${((w / total) * 100).toFixed(1)}%">`).join("");

  const cell = (tag: string, text: string, c: number) => {
    const align = aligns[c] ? ` class="md-${aligns[c]}"` : "";
    return `<${tag}${align}>${renderInline(text, ctx)}</${tag}>`;
  };
  const head = `<tr>${header.slice(0, columns).map((text, c) => cell("th", text, c)).join("")}</tr>`;
  const body = rows.map((row) => `<tr>${row.map((text, c) => cell("td", text, c)).join("")}</tr>`).join("");
  const html =
    `<div class="md-table-wrap"><table class="md-table"><colgroup>${cols}</colgroup>` +
    `<thead>${head}</thead>${body ? `<tbody>${body}</tbody>` : ""}</table></div>`;
  return { html, next: i };
}

interface ListResult {
  html: string;
  next: number;
}

function renderList(lines: string[], start: number, depth: number, ctx: Context): ListResult {
  const first = LIST_ITEM.exec(lines[start])!;
  const ordered = /\d/.test(first[2]);
  const baseIndent = first[1].length;
  const items: { lines: string[] }[] = [];
  let i = start;
  let loose = false;
  let sawBlank = false;

  while (i < lines.length) {
    const match = LIST_ITEM.exec(lines[i]);
    if (!match || match[1].length > baseIndent + 3 || match[1].length < baseIndent) break;
    if (/\d/.test(match[2]) !== ordered) break;
    if (match[1].length > baseIndent && items.length > 0) break;
    if (items.length > 0 && sawBlank) loose = true;
    const gap = match[3] ?? " ";
    const spaces = Math.min(gap.length, 4);
    const contentIndent = match[1].length + match[2].length + (match[4] === undefined ? 1 : spaces);
    const own: string[] = [match[4] ?? ""];
    sawBlank = false;
    i++;
    let blanks = 0;
    for (; i < lines.length; i++) {
      const line = lines[i];
      if (isBlank(line)) {
        blanks++;
        continue;
      }
      const indent = indentOf(line);
      const nestedItem = LIST_ITEM.test(line) && indent > baseIndent;
      const continues = indent >= contentIndent || nestedItem;
      const lazy = blanks === 0 && !startsBlock(line, lines[i + 1]) && !LIST_ITEM.test(line);
      if (!continues && !lazy) break;
      if (blanks > 0) own.push(...Array<string>(blanks).fill(""));
      blanks = 0;
      own.push(continues ? line.slice(Math.min(indent, contentIndent)) : line.trim());
    }
    items.push({ lines: own });
    if (blanks > 0) sawBlank = true;
    if (i >= lines.length) break;
    const upcoming = LIST_ITEM.exec(lines[i]);
    if (!upcoming || upcoming[1].length !== baseIndent || /\d/.test(upcoming[2]) !== ordered) break;
  }

  // A blank line inside an item between two blocks makes the list loose too.
  loose = loose || items.some((item) => hasInnerBlank(item.lines));
  const tag = ordered ? "ol" : "ul";
  const startNumber = ordered ? parseInt(first[2], 10) : 1;
  const startAttr = ordered && Number.isFinite(startNumber) && startNumber !== 1 ? ` start="${startNumber}"` : "";
  let hasTask = false;
  const rendered = items.map((item) => {
    let body = item.lines;
    let task = "";
    const taskMatch = /^\[([ xX])\](?:[ \t]+|$)/.exec(body[0]);
    if (taskMatch) {
      hasTask = true;
      const checked = taskMatch[1] !== " ";
      task = `<input type="checkbox" class="md-task" disabled${checked ? " checked" : ""}> `;
      body = [body[0].slice(taskMatch[0].length), ...body.slice(1)];
    }
    const inner = renderBlocks(body, depth + 1, ctx, !loose);
    return `<li${task ? ' class="md-task-item"' : ""}>${task}${inner}</li>`;
  });
  const cls = hasTask ? ' class="md-tasks"' : "";
  return { html: `<${tag}${startAttr}${cls}>${rendered.join("")}</${tag}>`, next: i };
}

/** Whether the lines have a blank between two non-blank lines that are not a nested list. */
function hasInnerBlank(lines: string[]): boolean {
  let seenText = false;
  let blank = false;
  for (const line of lines) {
    if (isBlank(line)) {
      if (seenText) blank = true;
    } else {
      if (blank && !LIST_ITEM.test(line.trimStart())) return true;
      seenText = true;
      blank = false;
    }
  }
  return false;
}

const ALERT_MARK = /^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\][ \t]*$/i;

function renderQuote(lines: string[], start: number, depth: number, ctx: Context): ListResult {
  const inner: string[] = [];
  let i = start;
  while (i < lines.length && QUOTE.test(lines[i])) {
    inner.push(lines[i].replace(/^ {0,3}> ?/, ""));
    i++;
  }
  // Lazy continuation: a plain line right after a quoted paragraph belongs to it.
  while (i < lines.length && !isBlank(lines[i]) && !startsBlock(lines[i], lines[i + 1]) && inner.length > 0 && !isBlank(inner[inner.length - 1])) {
    inner.push(lines[i].trim());
    i++;
  }
  const alert = inner.length > 0 ? ALERT_MARK.exec(inner[0].trim()) : null;
  if (alert) {
    const kind = alert[1].toLowerCase() as AlertKind;
    const body = renderBlocks(inner.slice(1), depth + 1, ctx, false);
    const html =
      `<div class="md-alert md-alert-${kind}" role="note">` +
      `<div class="md-alert-title"><span class="md-alert-icon" aria-hidden="true">${ALERT_GLYPHS[kind]}</span>` +
      `<span>${escapeHtml(ctx.labels[kind])}</span></div>${body}</div>`;
    return { html, next: i };
  }
  return { html: `<blockquote>${renderBlocks(inner, depth + 1, ctx, false)}</blockquote>`, next: i };
}

function renderBlocks(lines: string[], depth: number, ctx: Context, tight: boolean): string {
  if (depth > MAX_DEPTH) {
    const flat = lines.join("\n").trim();
    return flat ? `<p>${escapeHtml(flat)}</p>` : "";
  }
  const out: string[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (isBlank(line)) {
      i++;
      continue;
    }

    if (RAW_BLOCK.test(line)) {
      const name = /^[ \t]*<(script|style)/i.exec(line)![1].toLowerCase();
      const closing = new RegExp(`</${name}\\s*>`, "i");
      while (i < lines.length && !closing.test(lines[i])) i++;
      i++;
      continue;
    }

    const fence = FENCE.exec(line);
    if (fence) {
      const marker = fence[1];
      const language = /^[\w+#.-]{1,30}$/.test(fence[2]) ? fence[2] : "";
      const body: string[] = [];
      const indent = /^ */.exec(line)![0].length;
      i++;
      const closer = new RegExp(`^ {0,3}${marker[0] === "`" ? "`" : "~"}{${marker.length},}[ \\t]*$`);
      while (i < lines.length && !closer.test(lines[i])) {
        body.push(lines[i].replace(new RegExp(`^ {0,${indent}}`), ""));
        i++;
      }
      i++;
      const cls = language ? ` class="language-${escapeHtml(language)}"` : "";
      out.push(`<pre class="md-code"><code${cls}>${escapeHtml(body.join("\n"))}</code></pre>`);
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      const level = heading[1].length;
      out.push(`<h${level} class="md-h">${renderInline((heading[2] ?? "").trim(), ctx)}</h${level}>`);
      i++;
      continue;
    }

    if (RULE.test(line)) {
      out.push("<hr>");
      i++;
      continue;
    }

    if (QUOTE.test(line)) {
      const quote = renderQuote(lines, i, depth, ctx);
      out.push(quote.html);
      i = quote.next;
      continue;
    }

    if (LIST_ITEM.test(line)) {
      const list = renderList(lines, i, depth, ctx);
      out.push(list.html);
      i = Math.max(list.next, i + 1);
      continue;
    }

    if (isTableStart(line, lines[i + 1])) {
      const table = renderTable(lines, i, ctx);
      out.push(table.html);
      i = table.next;
      continue;
    }

    const paragraph: string[] = [line.trimStart()];
    i++;
    while (i < lines.length && !isBlank(lines[i]) && !startsBlock(lines[i], lines[i + 1])) {
      paragraph.push(lines[i].trimStart());
      i++;
    }
    const inline = renderInline(paragraph.join("\n").trimEnd(), ctx);
    if (inline.trim() !== "") out.push(tight ? `${inline}` : `<p>${inline}</p>`);
  }
  return out.join(tight ? "\n" : "");
}

/** Renders Markdown to HTML that is safe to insert into the page (see the file header). */
export function renderMarkdown(src: string, options: MarkdownOptions = {}): string {
  const max = options.maxChars ?? DEFAULT_MAX_CHARS;
  const text = (src.length > max ? src.slice(0, max) : src).replace(/\r\n?/g, "\n").replace(/\u0000/g, "�");
  const ctx: Context = { scan: { left: Math.max(300_000, text.length * 3) }, options, labels: { ...ALERT_LABELS, ...options.alertLabels } as Record<AlertKind, string> };
  const lines = text.split("\n").map((line) => line.replace(/^\t+/, (tabs) => "    ".repeat(tabs.length)));
  return renderBlocks(lines, 0, ctx, false);
}

/** Whether a file name is Markdown (the preview and the viewer render these). */
export function isMarkdownName(name: string): boolean {
  return /\.(md|markdown)$/i.test(name);
}
