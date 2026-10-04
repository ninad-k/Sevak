#!/usr/bin/env node
// Markdown table of the latest criterion results, for the job summary of
// .github/workflows/bench.yml and for reading the numbers locally.
//
//   node scripts/bench-summary.mjs [target/criterion]
//
// Reads <dir>/**/new/{benchmark,estimates}.json. Times are per call.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const root = process.argv[2] ?? "target/criterion";
/** The engine warns about queries slower than this (sevak_core::engine::LATENCY_BUDGET). */
const BUDGET_MS = 16;

function* benchmarks(dir) {
  const run = join(dir, "new");
  if (existsSync(join(run, "estimates.json")) && existsSync(join(run, "benchmark.json"))) {
    yield {
      id: JSON.parse(readFileSync(join(run, "benchmark.json"), "utf8")).full_id,
      estimates: JSON.parse(readFileSync(join(run, "estimates.json"), "utf8")),
    };
    return;
  }
  for (const entry of readdirSync(dir).sort()) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory() && entry !== "report") yield* benchmarks(path);
  }
}

if (!existsSync(root)) {
  console.error(`no criterion results in ${root}`);
  process.exit(1);
}

const ms = (ns) => (ns / 1e6).toFixed(ns < 1e5 ? 4 : ns < 1e6 ? 3 : 2);
const rows = [...benchmarks(root)].map(({ id, estimates }) => {
  const mean = estimates.mean.point_estimate;
  const median = estimates.median.point_estimate;
  const high = estimates.mean.confidence_interval.upper_bound;
  return { id, mean, median, high };
});

console.log("| Benchmark | Mean | Median | Mean, upper 95% bound | Within 16 ms |");
console.log("|---|---|---|---|---|");
for (const { id, mean, median, high } of rows) {
  // The budget is for a keystroke's query, not for the one-off startup work.
  const within = !id.startsWith("query/") ? "n/a" : mean / 1e6 <= BUDGET_MS ? "yes" : "**no**";
  console.log(`| ${id} | ${ms(mean)} ms | ${ms(median)} ms | ${ms(high)} ms | ${within} |`);
}
