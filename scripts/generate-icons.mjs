import { copyFileSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const require = createRequire(import.meta.url);
const output = join(root, "target", "generated-brand-icons");
const desktop = join(root, "src-tauri", "icons");
const publicDir = join(root, "ui", "public");

// Tauri also generates mobile assets. Stage everything in ignored build output
// and publish only the desktop assets used by this app.
const result = spawnSync(
  process.execPath,
  [
    require.resolve("@tauri-apps/cli/tauri.js"),
    "icon",
    join(root, "assets", "sevak-icon.png"),
    "--output",
    output,
  ],
  { cwd: root, stdio: "inherit" },
);
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);

mkdirSync(desktop, { recursive: true });
mkdirSync(publicDir, { recursive: true });
const desktopFiles = [
  "32x32.png", "64x64.png", "128x128.png", "128x128@2x.png",
  "icon.png", "icon.ico", "icon.icns", "StoreLogo.png",
  ...[30, 44, 71, 89, 107, 142, 150, 284, 310].map(
    (size) => `Square${size}x${size}Logo.png`,
  ),
];
for (const name of desktopFiles) {
  copyFileSync(join(output, name), join(desktop, name));
}
// One modest-size asset serves both the search bar and browser favicon.
copyFileSync(join(output, "128x128.png"), join(publicDir, "sevak-icon.png"));
console.log("Updated desktop icons and ui/public/sevak-icon.png.");
