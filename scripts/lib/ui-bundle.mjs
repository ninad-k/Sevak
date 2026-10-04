// Which npm packages end up inside the shipped UI bundle.
//
// package.json cannot answer this: Svelte's runtime (and its helper `clsx`) is a
// devDependency, yet it is bundled into the app. The answer is read from the
// source map of a throwaway production `vite build` instead. Used by
// generate-third-party-notices.mjs and generate-sbom.mjs.
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export function bundledNpmNames(root) {
  if (!existsSync(join(root, "node_modules"))) throw new Error("node_modules is missing: run `npm ci` first");
  const out = mkdtempSync(join(tmpdir(), "sevak-bundle-"));
  try {
    const build = spawnSync(
      process.execPath,
      [join(root, "node_modules", "vite", "bin", "vite.js"), "build", "--config", "ui/vite.config.ts", "--outDir", out, "--emptyOutDir", "--sourcemap", "--logLevel", "error"],
      { cwd: root, encoding: "utf8" },
    );
    if (build.status !== 0) throw new Error(`vite build failed:\n${build.stderr || build.stdout}`);

    const maps = [];
    const walk = (dir) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const path = join(dir, entry.name);
        if (entry.isDirectory()) walk(path);
        else if (entry.name.endsWith(".map")) maps.push(path);
      }
    };
    walk(out);
    if (maps.length === 0) throw new Error("the Vite build produced no source maps");

    const names = new Set();
    for (const map of maps) {
      for (const source of JSON.parse(readFileSync(map, "utf8")).sources) {
        const at = source.lastIndexOf("node_modules/");
        if (at < 0) continue;
        const parts = source.slice(at + "node_modules/".length).split("/");
        names.add(parts[0].startsWith("@") ? `${parts[0]}/${parts[1]}` : parts[0]);
      }
    }
    return [...names].sort();
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
