// Prints the documentation site's combined print page to docs/pdf/Sevak-User-Guide.pdf.
//
// Usage: build the site first (`mkdocs build`), then `npm run docs:pdf`.
// The built site is served on a local port and opened in headless Chrome or
// Chromium, driven over the DevTools protocol (no npm dependencies). The script
// waits until every Mermaid diagram has been drawn before printing: Chrome's
// own --print-to-pdf prints too early and leaves the diagrams blank.
// Set CHROME_PATH if the browser isn't found.
import { createServer } from "node:http";
import { readFile, stat, mkdir, writeFile, mkdtemp, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { extname, join, normalize, resolve } from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const site = resolve(root, "site");
const out = resolve(root, "docs", "pdf", "Sevak-User-Guide.pdf");
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

const MIME = {
  ".html": "text/html; charset=utf-8", ".css": "text/css", ".js": "text/javascript",
  ".json": "application/json", ".svg": "image/svg+xml", ".png": "image/png",
  ".jpg": "image/jpeg", ".gif": "image/gif", ".woff2": "font/woff2", ".ico": "image/x-icon",
};

function findChrome() {
  if (process.env.CHROME_PATH) return process.env.CHROME_PATH;
  const candidates = {
    win32: [
      "C:/Program Files/Google/Chrome/Application/chrome.exe",
      "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
      "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
    ],
    darwin: ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"],
    linux: ["/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser"],
  }[process.platform] ?? [];
  const found = candidates.find((p) => existsSync(p));
  if (!found) throw new Error("Chrome/Chromium not found; set CHROME_PATH");
  return found;
}

if (!existsSync(join(site, "print_page", "index.html"))) {
  console.error("site/print_page/index.html is missing: run `mkdocs build` first.");
  process.exit(1);
}

// Static file server for the built site.
const server = createServer(async (req, res) => {
  try {
    const path = normalize(decodeURIComponent(new URL(req.url, "http://x").pathname));
    let file = join(site, path);
    if (!file.startsWith(site)) throw new Error("outside site");
    if ((await stat(file)).isDirectory()) file = join(file, "index.html");
    res.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream" });
    res.end(await readFile(file));
  } catch {
    res.writeHead(404).end();
  }
});
await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
const url = `http://127.0.0.1:${server.address().port}/print_page/`;

// Headless browser with a DevTools port.
const profile = await mkdtemp(join(tmpdir(), "sevak-pdf-"));
const port = 9300 + Math.floor(Math.random() * 600);
const chrome = spawn(findChrome(), [
  "--headless=new", "--disable-gpu", "--no-sandbox", `--remote-debugging-port=${port}`,
  `--user-data-dir=${profile}`,
  // Print the light palette whatever the machine's theme: the site follows
  // prefers-color-scheme, and a dark page is unreadable on paper.
  "--blink-settings=preferredColorScheme=1",
  "about:blank",
], { stdio: "ignore" });

async function cleanup() {
  server.close();
  chrome.kill();
  await sleep(500);
  await rm(profile, { recursive: true, force: true }).catch(() => {});
}

try {
  let targets;
  for (let i = 0; i < 100 && !targets; i++) {
    try { targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json(); }
    catch { await sleep(200); }
  }
  const target = targets?.find((t) => t.type === "page");
  if (!target) throw new Error("could not reach the browser's DevTools port");

  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((ok, fail) => { ws.addEventListener("open", ok); ws.addEventListener("error", fail); });
  let nextId = 0;
  const waiting = new Map();
  ws.addEventListener("message", (event) => {
    const msg = JSON.parse(event.data);
    if (msg.id && waiting.has(msg.id)) { waiting.get(msg.id)(msg); waiting.delete(msg.id); }
  });
  const send = (method, params = {}) => new Promise((done, fail) => {
    const id = ++nextId;
    waiting.set(id, (msg) => (msg.error ? fail(new Error(`${method}: ${msg.error.message}`)) : done(msg.result)));
    ws.send(JSON.stringify({ id, method, params }));
  });
  const evaluate = async (expression) =>
    (await send("Runtime.evaluate", { expression, returnByValue: true })).result.value;

  await send("Page.enable");
  await send("Page.navigate", { url });

  // Ready when the page has loaded and every diagram has a drawn size that
  // stayed the same for two checks in a row.
  let last = "";
  let stable = 0;
  for (let i = 0; i < 120 && stable < 2; i++) {
    await sleep(500);
    const state = await evaluate(`(() => {
      if (document.readyState !== "complete") return "loading";
      const d = [...document.querySelectorAll(".mermaid")];
      if (d.some((e) => e.tagName === "PRE" || e.getBoundingClientRect().height < 20)) return "drawing";
      return d.map((e) => Math.round(e.getBoundingClientRect().height)).join(",");
    })()`);
    stable = state !== "loading" && state !== "drawing" && state === last ? stable + 1 : 0;
    last = state;
  }
  if (stable < 2) throw new Error(`diagrams did not finish drawing (last state: ${last})`);

  const pdf = await send("Page.printToPDF", {
    printBackground: true,
    preferCSSPageSize: true,
    displayHeaderFooter: false,
  });
  await mkdir(resolve(root, "docs", "pdf"), { recursive: true });
  await writeFile(out, Buffer.from(pdf.data, "base64"));
  ws.close();
  console.log(`wrote ${out} (${((await stat(out)).size / 1024).toFixed(0)} KB, ${last.split(",").length} diagrams)`);
} catch (err) {
  console.error(`printing failed: ${err.message}`);
  process.exitCode = 1;
} finally {
  await cleanup();
}
