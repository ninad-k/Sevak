import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, existsSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { parseColor, hslColor, contrastRatio, results as colors } from "../../examples/plugins/color-tools/colors.mjs";
import { results as translate } from "../../examples/plugins/translate/translate.mjs";
import { results as docs, pages } from "../../examples/plugins/tauri-docs/docs.mjs";
import { Timer } from "../../examples/plugins/pomodoro/timer.mjs";

const root = fileURLToPath(new URL("../../", import.meta.url));
function sandbox(t) {
  const dir = mkdtempSync(join(tmpdir(), "sevak-extension-test-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  return dir;
}

test("color conversions handle endpoints, hue wrapping and round trips", () => {
  assert.deepEqual(parseColor("#fa0"), [255, 170, 0]);
  assert.deepEqual(parseColor("rgb(0, 128, 255)"), [0, 128, 255]);
  assert.deepEqual(parseColor("hsl(-240, 100%, 50%)"), [0, 255, 0]);
  assert.deepEqual(parseColor("hsl(360, 100%, 50%)"), [255, 0, 0]);
  for (const rgb of [[0,0,0], [255,255,255], [245,181,44], [12,108,223]]) {
    const roundtrip = parseColor(hslColor(rgb));
    assert.ok(roundtrip.every((v,i) => Math.abs(v - rgb[i]) <= 1));
  }
});

test("invalid and transparent colors produce help, never misleading conversions", () => {
  for (const q of ["#ffff", "#xyz", "rgb(256,0,0)", "hsl(20,101%,50%)", "rgba(0,0,0,0.5)", "red", "#fff on #000 on #111"]) {
    assert.equal(colors(q)[0].view, "text", q);
    assert.equal(colors(q)[0].key, "help", q);
  }
});

test("contrast uses correct luminance, symmetry and unrounded thresholds", () => {
  assert.equal(contrastRatio([0,0,0], [255,255,255]), 21);
  assert.equal(contrastRatio([40,80,20], [40,80,20]), 1);
  assert.equal(contrastRatio([10,20,30], [40,50,60]), contrastRatio([40,50,60], [10,20,30]));
  assert.match(colors("#777 on #fff")[0].text, /Normal text: AA Fail/);
  assert.match(colors("#767676 on #fff")[0].text, /Normal text: AA Pass/);
  assert.equal(colors("#f5b52c")[0].action.text, "#F5B52C");
});

test("translation URL keeps Unicode and special characters inside the text parameter", () => {
  const input = "नमस्ते & tl=ar + #? 😀";
  const result = translate(`fr ${input}`)[0];
  const url = new URL(result.action.url);
  assert.equal(url.origin, "https://translate.google.com");
  assert.equal(url.searchParams.get("sl"), "auto");
  assert.equal(url.searchParams.get("tl"), "fr");
  assert.equal(url.searchParams.get("text"), input);
  assert.equal(translate("CHINESE hello")[0].key, "help");
  assert.equal(new URL(translate("zh-cn hello")[0].action.url).searchParams.get("tl"), "zh-CN");
  for (const input of ["", "fr", "xx hello", "en " + "a".repeat(5001)]) assert.equal(translate(input)[0].key, "help");
});

test("documentation search matches all words and opens official pages only", () => {
  assert.equal(docs("updater")[0].action.url, "https://v2.tauri.app/plugin/updater/");
  assert.equal(docs("global shortcut")[0].title, "Global shortcut");
  assert.equal(docs("no-such-guide")[0].key, "no-match");
  assert.equal(new Set(pages.map(p => p[1])).size, pages.length);
  assert.ok(docs("").every(item => new URL(item.action.url).origin === "https://v2.tauri.app"));
});

test("one-shot entry points run from an installed-style directory and emit valid JSON", () => {
  for (const [folder,query,key] of [["color-tools", "#fa0", "hex"], ["translate", "fr hello", "translate-fr"], ["tauri-docs", "updater", "plugin/updater/"]]) {
    const cwd = join(root, "examples/plugins", folder);
    const run = spawnSync(process.execPath, ["main.mjs", query], { cwd, encoding: "utf8", timeout: 5000 });
    assert.equal(run.status, 0, run.stderr);
    assert.equal(JSON.parse(run.stdout).items[0].key, key);
  }
});

test("timer queries do not start a session; pause/resume preserves remaining time", t => {
  const dir = sandbox(t); let now = 1_000_000;
  const timer = new Timer(dir, () => now);
  const start = timer.results("start 25")[0];
  assert.equal(existsSync(join(dir, "timer.json")), false);
  timer.execute(start.action.payload);
  now += 65_000;
  assert.match(timer.results("status")[0].title, /23:55 remaining/);
  timer.execute(JSON.stringify({op:"pause"}));
  now += 100_000;
  const reopened = new Timer(dir, () => now);
  assert.match(reopened.results("status")[0].title, /23:55 paused/);
  reopened.execute(JSON.stringify({op:"resume"}));
  now += 35_000;
  assert.match(reopened.results("status")[0].title, /23:20 remaining/);
});

test("expired timers survive restarts, count focus once, and do not auto-start a break", t => {
  const dir = sandbox(t); let now = 1_000_000;
  const timer = new Timer(dir, () => now);
  timer.execute(JSON.stringify({op:"start", minutes:1}));
  now += 61_000;
  const reopened = new Timer(dir, () => now);
  assert.equal(reopened.current().status, "complete");
  assert.equal(reopened.current().completed, 1);
  assert.equal(new Timer(dir, () => now).current().completed, 1);
  reopened.execute(JSON.stringify({op:"break", minutes:1}));
  now += 61_000;
  assert.equal(reopened.current().completed, 1);
  reopened.execute(JSON.stringify({op:"reset"}));
  assert.equal(reopened.current().status, "idle");
  assert.equal(reopened.current().completed, 1);
});

test("timer rejects invalid durations and actions, preserving state", t => {
  const timer = new Timer(sandbox(t));
  for (const minutes of [0, -1, 181, 1.5, "25", null]) assert.throws(() => timer.execute(JSON.stringify({op:"start",minutes})));
  for (const op of ["pause", "resume", "anything"]) assert.throws(() => timer.execute(JSON.stringify({op})));
  for (const q of ["start 0", "break 181", "start 1.5", "pause"]) assert.equal(timer.results(q)[0].key, "help");
  assert.equal(timer.current().status, "idle");
});

test("corrupt saved state is not overwritten", t => {
  const dir = sandbox(t), file = join(dir,"timer.json");
  writeFileSync(file, "{bad-json");
  assert.throws(() => new Timer(dir), /Back up or rename/);
  assert.equal(readFileSync(file,"utf8"), "{bad-json");
});

test("persistent protocol initializes, queries, executes and shuts down cleanly", t => {
  const dir = sandbox(t);
  const input = [
    {type:"initialize", protocol:1}, {type:"query",request_id:1,input:"start 25"},
    {type:"execute",key:"start",payload:JSON.stringify({op:"start",minutes:25})},
    {type:"query",request_id:2,input:"status"}, {type:"shutdown"},
  ].map(m => JSON.stringify(m)).join("\n") + "\n";
  const run = spawnSync(process.execPath, ["main.mjs"], { cwd:join(root,"examples/plugins/pomodoro"), env:{...process.env,SEVAK_PLUGIN_DATA:dir}, input, encoding:"utf8", timeout:5000 });
  assert.equal(run.status, 0, run.stderr);
  const replies = run.stdout.trim().split("\n").map(line => JSON.parse(line));
  assert.equal(replies[0].type,"ready");
  assert.equal(replies[1].request_id,1);
  assert.equal(replies[2].request_id,2);
  assert.match(replies[2].items[0].title,/remaining/);
  assert.equal(JSON.parse(readFileSync(join(dir,"timer.json"),"utf8")).status,"running");
});
