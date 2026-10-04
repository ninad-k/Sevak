import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

// Kept apart from vite.config.ts so the Tauri dev/build settings stay untouched.
// Tests of pure TypeScript run in plain node; a component test opts into jsdom
// with a `// @vitest-environment jsdom` comment on its first line.
const uiRoot = fileURLToPath(new URL(".", import.meta.url));

export default defineConfig({
  root: uiRoot,
  plugins: [svelte()],
  resolve: {
    // Svelte 5 resolves to its server build unless told this is a browser.
    conditions: process.env.VITEST ? ["browser"] : [],
  },
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
    restoreMocks: true,
    coverage: {
      provider: "v8",
      include: ["src/**/*.{ts,svelte}"],
      exclude: ["src/**/*.test.ts", "src/**/*.d.ts", "src/main.ts"],
      reporter: ["text-summary", "text", "lcov", "json-summary"],
      reportsDirectory: "coverage",
    },
  },
});
