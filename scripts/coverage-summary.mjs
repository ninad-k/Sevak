#!/usr/bin/env node
// Turns coverage reports into Markdown tables for the GitHub job summary
// (.github/workflows/coverage.yml); also handy to read the numbers locally.
//
//   node scripts/coverage-summary.mjs --rust coverage/summary.json --ui ui/coverage/coverage-summary.json
//
// --rust  the JSON of `cargo llvm-cov report --json --summary-only`
// --ui    the `json-summary` report of `npm run test:coverage` (vitest, v8)
//
// Either may be left out. The Markdown goes to stdout; the workflow appends it
// to $GITHUB_STEP_SUMMARY. Nothing here gates anything: the numbers are reports.

import { readFileSync } from "node:fs";

const args = process.argv.slice(2);
const option = (name) => {
  const at = args.indexOf(`--${name}`);
  return at >= 0 ? args[at + 1] : undefined;
};

const pct = (covered, total) => (total === 0 ? "n/a" : `${((100 * covered) / total).toFixed(1)}%`);
const cell = (covered, total) => `${pct(covered, total)} (${covered}/${total})`;

function rustTable(file) {
  const report = JSON.parse(readFileSync(file, "utf8"));
  const crates = new Map();
  const add = (name, summary) => {
    const row = crates.get(name) ?? { lines: [0, 0], functions: [0, 0], regions: [0, 0] };
    for (const kind of ["lines", "functions", "regions"]) {
      row[kind][0] += summary[kind].covered;
      row[kind][1] += summary[kind].count;
    }
    crates.set(name, row);
  };
  for (const { filename, summary } of report.data[0].files) {
    const match = /[\\/]crates[\\/]([^\\/]+)[\\/]/.exec(filename);
    add(match ? match[1] : "(other)", summary);
  }
  const total = { lines: [0, 0], functions: [0, 0], regions: [0, 0] };
  const rows = [...crates.entries()].sort(([a], [b]) => a.localeCompare(b));
  for (const [, row] of rows) {
    for (const kind of Object.keys(total)) {
      total[kind][0] += row[kind][0];
      total[kind][1] += row[kind][1];
    }
  }
  const line = (name, row) =>
    `| ${name} | ${cell(...row.lines)} | ${cell(...row.functions)} | ${cell(...row.regions)} |`;
  return [
    "### Rust (cargo-llvm-cov)",
    "",
    "| Crate | Lines | Functions | Regions |",
    "|---|---|---|---|",
    ...rows.map(([name, row]) => line(name, row)),
    line("**total**", total),
    "",
  ].join("\n");
}

function uiTable(file) {
  const report = JSON.parse(readFileSync(file, "utf8"));
  const short = (name) => name.replace(/\\/g, "/").replace(/^.*\/ui\/src\//, "");
  const files = Object.entries(report)
    .filter(([name]) => name !== "total")
    .map(([name, summary]) => [short(name), summary])
    .sort(([a], [b]) => a.localeCompare(b));
  const cellOf = (m) => cell(m.covered, m.total);
  const row = (name, s) => `| ${name} | ${cellOf(s.lines)} | ${cellOf(s.functions)} | ${cellOf(s.branches)} |`;
  const logic = files.filter(([name]) => name.endsWith(".ts") && !/(^|\/)(mock[-\w]*|ipc|settings-ipc|theme-ipc|main)\.ts$/.test(name));
  return [
    "### UI (vitest, v8)",
    "",
    "Components (`.svelte`) are mostly untested; the numbers that matter are the pure logic modules.",
    "",
    "| File | Lines | Functions | Branches |",
    "|---|---|---|---|",
    ...logic.map(([name, s]) => row(name, s)),
    row("**all files in ui/src**", report.total),
    "",
  ].join("\n");
}

const out = [];
if (option("rust")) out.push(rustTable(option("rust")));
if (option("ui")) out.push(uiTable(option("ui")));
if (out.length === 0) {
  console.error("usage: coverage-summary.mjs [--rust summary.json] [--ui coverage-summary.json]");
  process.exit(2);
}
console.log(out.join("\n"));
