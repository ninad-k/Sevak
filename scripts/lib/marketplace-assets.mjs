// The stylesheet and the filter script of the generated marketplace pages
// (scripts/build-marketplace.mjs). Kept apart from the generator so the colors
// can be checked for contrast by scripts/build-marketplace.test.mjs.

/**
 * Colors of the two schemes. The dark one is the product page's palette
 * (landing/index.html); the light one is its daytime counterpart. `link` is the
 * color of text links, which differs from the brand amber in the light scheme
 * because amber on white does not reach 4.5:1.
 */
export const TOKENS = {
  dark: {
    bg: "#0f0e1d",
    bg2: "#15132a",
    panel: "#1b1932",
    panel2: "#221f3d",
    ink: "#f3f0ff",
    muted: "#b3afcc",
    border: "#37334f",
    link: "#f5b52c",
    code: "#ffe2a1",
    btnBg: "#f5b52c",
    btnInk: "#1a1405",
    okBg: "#12301f",
    okInk: "#8fe3ae",
    scriptBg: "#3a2d0c",
    scriptInk: "#ffd97a",
    nativeBg: "#3b1824",
    nativeInk: "#ffa3b8",
    kindBg: "#272352",
    kindInk: "#c9c2ff",
  },
  light: {
    bg: "#fbfaff",
    bg2: "#f1effb",
    panel: "#ffffff",
    panel2: "#f4f2fc",
    ink: "#1a1730",
    muted: "#55507a",
    border: "#cfcae6",
    link: "#8a5a00",
    code: "#6b4500",
    btnBg: "#f5b52c",
    btnInk: "#1a1405",
    okBg: "#dff5e7",
    okInk: "#0f5a2c",
    scriptBg: "#fdf0cc",
    scriptInk: "#6b4500",
    nativeBg: "#fde1e8",
    nativeInk: "#8a1233",
    kindBg: "#e7e3fb",
    kindInk: "#3a2f99",
  },
};

/** Text-on-background pairs that must reach WCAG AA (4.5:1) in both schemes. */
export const CONTRAST_PAIRS = [
  ["ink", "bg"],
  ["ink", "bg2"],
  ["ink", "panel"],
  ["muted", "bg"],
  ["muted", "bg2"],
  ["muted", "panel"],
  ["link", "bg"],
  ["link", "bg2"],
  ["link", "panel"],
  ["code", "panel2"],
  ["btnInk", "btnBg"],
  ["okInk", "okBg"],
  ["scriptInk", "scriptBg"],
  ["nativeInk", "nativeBg"],
  ["kindInk", "kindBg"],
];

const vars = (t) =>
  Object.entries(t)
    .map(([k, v]) => `--${k.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase())}: ${v};`)
    .join(" ");

export const CSS = `
:root { ${vars(TOKENS.dark)} --radius: 16px; --max: 1120px; color-scheme: dark;
  --mono: ui-monospace, "Cascadia Code", "SF Mono", Menlo, Consolas, monospace; }
@media (prefers-color-scheme: light) { :root { ${vars(TOKENS.light)} color-scheme: light; } }
* { box-sizing: border-box; }
html { scroll-behavior: smooth; }
body { margin: 0; background: var(--bg); color: var(--ink);
  font: 17px/1.6 "Segoe UI", system-ui, -apple-system, "Helvetica Neue", Ubuntu, Cantarell, "Noto Sans", sans-serif;
  -webkit-font-smoothing: antialiased; }
a { color: var(--link); }
a:hover { text-decoration: none; }
:focus-visible { outline: 3px solid var(--link); outline-offset: 2px; border-radius: 4px; }
.wrap { width: min(var(--max), 100% - 32px); margin-inline: auto; }
.skip { position: absolute; left: -999px; top: 8px; background: var(--btn-bg); color: var(--btn-ink); padding: 8px 14px; border-radius: 8px; z-index: 20; }
.skip:focus { left: 8px; }
code, .code { font: 0.88em var(--mono); background: var(--panel-2); border: 1px solid var(--border); border-radius: 6px; padding: 0.08em 0.4em; color: var(--code); overflow-wrap: anywhere; }
pre.code { display: block; padding: 12px 14px; white-space: pre-wrap; overflow-wrap: anywhere; margin: 8px 0; }

.nav { position: sticky; top: 0; z-index: 10; background: var(--bg); border-bottom: 1px solid var(--border); }
.nav .wrap { display: flex; align-items: center; gap: 20px; min-height: 60px; flex-wrap: wrap; }
.brand { display: flex; align-items: center; gap: 10px; color: var(--ink); font-weight: 700; font-size: 20px; text-decoration: none; }
.brand img { width: 30px; height: 30px; border-radius: 8px; }
.nav-links { display: flex; gap: 18px; margin-left: auto; align-items: center; flex-wrap: wrap; }
.nav-links a { color: var(--muted); font-size: 15px; text-decoration: none; }
.nav-links a:hover, .nav-links a[aria-current] { color: var(--ink); text-decoration: underline; }
.nav-links a.btn-primary { color: var(--btn-ink); font-size: 15px; padding: 8px 16px; text-decoration: none; }

.btn { display: inline-flex; align-items: center; gap: 8px; padding: 11px 20px; border-radius: 12px; font: inherit; font-weight: 650; font-size: 16px;
  border: 1px solid var(--border); background: var(--panel); color: var(--ink); text-decoration: none; cursor: pointer; }
.btn:hover { border-color: var(--link); }
.btn-primary { background: var(--btn-bg); color: var(--btn-ink); border-color: transparent; }
.btn-primary:hover { filter: brightness(1.08); border-color: transparent; }

.hero { padding: 56px 0 28px; background: radial-gradient(ellipse 760px 320px at 80% 0%, rgba(245, 181, 44, 0.13), transparent 70%); }
.kicker { color: var(--link); font-weight: 700; font-size: 14px; letter-spacing: 1.6px; text-transform: uppercase; margin: 0 0 8px; }
h1 { font-size: clamp(32px, 5vw, 52px); line-height: 1.1; letter-spacing: -1px; margin: 0 0 14px; }
h2 { font-size: 24px; line-height: 1.25; margin: 36px 0 10px; }
h3 { margin: 0; font-size: 19px; line-height: 1.3; }
.lede { font-size: 19px; color: var(--muted); max-width: 40em; margin: 0 0 22px; }
.cta { display: flex; gap: 12px; flex-wrap: wrap; }

.submit { margin: 28px 0 8px; padding: 22px; border: 1px solid var(--link); border-radius: var(--radius); background: var(--panel); display: flex; gap: 18px; align-items: center; justify-content: space-between; flex-wrap: wrap; }
.submit h2 { margin: 0 0 4px; font-size: 22px; }
.submit p { margin: 0; color: var(--muted); max-width: 38em; }

.controls { display: grid; gap: 14px; margin: 26px 0 8px; }
.controls[hidden] { display: none; }
.row { display: flex; gap: 12px; flex-wrap: wrap; align-items: end; }
.field { display: grid; gap: 4px; min-width: 0; }
.field.grow { flex: 1 1 260px; }
label, .label { font-size: 14px; color: var(--muted); font-weight: 600; }
input[type="search"], select { font: inherit; font-size: 16px; color: var(--ink); background: var(--panel); border: 1px solid var(--border); border-radius: 10px; padding: 9px 12px; min-height: 44px; width: 100%; }
.chips { display: flex; gap: 8px; flex-wrap: wrap; }
.chip { font: inherit; font-size: 15px; color: var(--ink); background: var(--panel); border: 1px solid var(--border); border-radius: 999px; padding: 7px 14px; min-height: 40px; cursor: pointer; }
.chip:hover { border-color: var(--link); }
.chip[aria-pressed="true"] { background: var(--btn-bg); color: var(--btn-ink); border-color: transparent; font-weight: 650; }
.status { color: var(--muted); font-size: 15px; margin: 6px 0 14px; }
.empty { border: 1px dashed var(--border); border-radius: var(--radius); padding: 28px; text-align: center; color: var(--muted); }

.grid { list-style: none; padding: 0; margin: 0 0 56px; display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 16px; }
.card { background: var(--panel); border: 1px solid var(--border); border-radius: var(--radius); padding: 20px; display: flex; flex-direction: column; gap: 10px; min-width: 0; }
.card[hidden] { display: none; }
.card:hover { border-color: var(--link); }
.card h3 a { color: var(--ink); text-decoration: none; }
.card h3 a:hover { text-decoration: underline; }
.card .desc { margin: 0; color: var(--muted); font-size: 15.5px; flex: 1; overflow-wrap: anywhere; }
.meta { margin: 0; font-size: 14px; color: var(--muted); }
.badges { display: flex; gap: 6px; flex-wrap: wrap; }
.badge { font-size: 12.5px; font-weight: 650; border-radius: 999px; padding: 2px 10px; border: 1px solid transparent; white-space: nowrap; }
.badge.kind { background: var(--kind-bg); color: var(--kind-ink); }
.badge.trust-none { background: var(--ok-bg); color: var(--ok-ink); }
.badge.trust-script { background: var(--script-bg); color: var(--script-ink); }
.badge.trust-native { background: var(--native-bg); color: var(--native-ink); }
.tags { list-style: none; display: flex; gap: 6px; flex-wrap: wrap; margin: 0; padding: 0; }
.tags li { font: 12.5px var(--mono); color: var(--muted); border: 1px solid var(--border); border-radius: 6px; padding: 1px 7px; }
.links { display: flex; gap: 16px; flex-wrap: wrap; margin: 0; font-size: 15px; }

.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
.crumbs { font-size: 14px; color: var(--muted); margin: 28px 0 0; }
.crumbs a { color: var(--muted); }
.detail { display: grid; grid-template-columns: minmax(0, 1fr) 320px; gap: 40px; padding-bottom: 64px; align-items: start; }
.detail h1 { font-size: clamp(28px, 4vw, 40px); margin: 12px 0; }
.side { background: var(--panel); border: 1px solid var(--border); border-radius: var(--radius); padding: 18px 20px; position: sticky; top: 80px; }
.side dl { margin: 0; display: grid; gap: 10px; }
.side dt { font-size: 13px; color: var(--muted); font-weight: 600; }
.side dd { margin: 0; overflow-wrap: anywhere; }
ol.steps { padding-left: 22px; } ol.steps li { margin: 6px 0; }
.note { color: var(--muted); font-size: 15px; }
table { border-collapse: collapse; width: 100%; font-size: 15px; display: block; overflow-x: auto; }
th, td { text-align: left; padding: 6px 10px; border-bottom: 1px solid var(--border); vertical-align: top; }
td.hash { font: 13px var(--mono); overflow-wrap: anywhere; word-break: break-all; }
.actions { display: flex; gap: 12px; flex-wrap: wrap; margin: 12px 0; }

footer { border-top: 1px solid var(--border); padding: 28px 0; color: var(--muted); font-size: 14.5px; }
footer .wrap { display: flex; flex-wrap: wrap; gap: 12px 24px; justify-content: space-between; }
footer nav { display: flex; gap: 18px; flex-wrap: wrap; }
footer a { color: var(--muted); }

@media (max-width: 860px) { .detail { grid-template-columns: 1fr; } .side { position: static; } }
@media (max-width: 560px) { body { font-size: 16px; } .hero { padding: 36px 0 16px; } .grid { grid-template-columns: 1fr; } .nav-links a:not(.btn) { font-size: 14px; } }
@media (prefers-reduced-motion: reduce) { html { scroll-behavior: auto; } * { transition: none !important; animation: none !important; } }
`.trim();

/** Filtering, searching and sorting of the card list. Without it every card is shown. */
export const JS = `
(function () {
  "use strict";
  var root = document.querySelector("[data-market]");
  if (!root) return;
  var list = root.querySelector("[data-list]");
  var cards = Array.prototype.slice.call(list.querySelectorAll("[data-card]"));
  var controls = root.querySelector("[data-controls]");
  var search = root.querySelector("[data-search]");
  var tagSel = root.querySelector("[data-tag]");
  var sortSel = root.querySelector("[data-sort]");
  var chips = Array.prototype.slice.call(root.querySelectorAll("[data-kind-chip]"));
  var status = root.querySelector("[data-status]");
  var empty = root.querySelector("[data-empty]");
  var reset = root.querySelector("[data-reset]");
  var state = { q: "", kind: "all", tag: "all", sort: "recommended" };

  var params = new URLSearchParams(location.search);
  ["q", "kind", "tag", "sort"].forEach(function (k) { if (params.has(k)) state[k] = params.get(k) || state[k]; });
  search.value = state.q;
  tagSel.value = state.tag; if (tagSel.value !== state.tag) { state.tag = "all"; tagSel.value = "all"; }
  sortSel.value = state.sort; if (sortSel.value !== state.sort) { state.sort = "recommended"; sortSel.value = "recommended"; }

  function matches(card) {
    if (state.kind !== "all" && card.getAttribute("data-kind") !== state.kind) return false;
    if (state.tag !== "all" && (" " + card.getAttribute("data-tags") + " ").indexOf(" " + state.tag + " ") < 0) return false;
    var words = state.q.toLowerCase().split(/\\s+/).filter(Boolean);
    var text = card.getAttribute("data-text");
    return words.every(function (w) { return text.indexOf(w) >= 0; });
  }
  function order(a, b) {
    if (state.sort === "name" || state.sort === "name-desc") {
      var c = a.getAttribute("data-name").localeCompare(b.getAttribute("data-name"));
      return state.sort === "name" ? c : -c;
    }
    return Number(a.getAttribute("data-index")) - Number(b.getAttribute("data-index"));
  }
  function apply(save) {
    var shown = 0;
    cards.slice().sort(order).forEach(function (card) {
      var ok = matches(card);
      card.hidden = !ok;
      if (ok) shown++;
      list.appendChild(card);
    });
    chips.forEach(function (chip) { chip.setAttribute("aria-pressed", String(chip.getAttribute("data-kind-chip") === state.kind)); });
    status.textContent = shown + " of " + cards.length + " listed";
    empty.hidden = shown !== 0;
    if (save) {
      var p = new URLSearchParams();
      if (state.q) p.set("q", state.q);
      if (state.kind !== "all") p.set("kind", state.kind);
      if (state.tag !== "all") p.set("tag", state.tag);
      if (state.sort !== "recommended") p.set("sort", state.sort);
      var qs = p.toString();
      try { history.replaceState(null, "", location.pathname + (qs ? "?" + qs : "")); } catch (e) { /* file: pages */ }
    }
  }
  search.addEventListener("input", function () { state.q = search.value; apply(true); });
  tagSel.addEventListener("change", function () { state.tag = tagSel.value; apply(true); });
  sortSel.addEventListener("change", function () { state.sort = sortSel.value; apply(true); });
  chips.forEach(function (chip) {
    chip.addEventListener("click", function () { state.kind = chip.getAttribute("data-kind-chip"); apply(true); });
  });
  reset.addEventListener("click", function () {
    state = { q: "", kind: "all", tag: "all", sort: "recommended" };
    search.value = ""; tagSel.value = "all"; sortSel.value = "recommended";
    apply(true); search.focus();
  });
  controls.hidden = false;
  apply(false);
})();
`.trim();
