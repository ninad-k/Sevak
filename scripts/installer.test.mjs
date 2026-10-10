// Checks on the Windows installer sources that CI can make without running an
// installer: the images are what NSIS and WiX need, the config points at files that
// exist, and the template and the language file agree. Run with `npm run test:scripts`.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { encodeBmp24, readBmpInfo } from "./bmp.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const tauri = join(root, "src-tauri");
const conf = JSON.parse(readFileSync(join(tauri, "tauri.conf.json"), "utf8"));
const read = (...parts) => readFileSync(join(tauri, ...parts), "utf8");
const windows = conf.bundle.windows;

test("encodeBmp24 writes an uncompressed 24-bit bottom-up BMP", () => {
  // 3x2 pixels: the rows are padded from 9 to 12 bytes.
  const rgba = Buffer.from([
    255, 0, 0, 255, /**/ 0, 255, 0, 255, /**/ 0, 0, 255, 255, // top row: red, green, blue
    10, 20, 30, 255, /**/ 40, 50, 60, 255, /**/ 70, 80, 90, 255, // bottom row
  ]);
  const bmp = encodeBmp24(3, 2, rgba);
  assert.deepEqual(readBmpInfo(bmp), {
    width: 3,
    height: 2,
    bitsPerPixel: 24,
    compression: 0,
    size: 54 + 12 * 2,
  });
  assert.equal(bmp.length, 54 + 24);
  // The bottom row comes first in the file, stored as BGR.
  assert.deepEqual([...bmp.subarray(54, 57)], [30, 20, 10]);
  // Then the top row: red is B=0, G=0, R=255.
  assert.deepEqual([...bmp.subarray(54 + 12, 54 + 15)], [0, 0, 255]);
  assert.throws(() => encodeBmp24(2, 2, Buffer.alloc(4)));
  assert.throws(() => readBmpInfo(Buffer.from("not an image")));
});

test("the installer images have the sizes NSIS and WiX expect", () => {
  const expected = {
    [windows.nsis.sidebarImage]: [164, 314],
    [windows.nsis.headerImage]: [150, 57],
    [windows.nsis.uninstallerHeaderImage]: [150, 57],
    [windows.wix.bannerPath]: [493, 58],
    [windows.wix.dialogImagePath]: [493, 312],
  };
  for (const [path, [width, height]] of Object.entries(expected)) {
    const info = readBmpInfo(readFileSync(join(tauri, path)));
    assert.equal(info.bitsPerPixel, 24, path);
    assert.equal(info.compression, 0, `${path} must be uncompressed`);
    assert.deepEqual([info.width, info.height], [width, height], path);
  }
});

test("the NSIS config points at files that exist", () => {
  const { nsis } = windows;
  assert.equal(nsis.installMode, "both");
  assert.deepEqual(nsis.languages, ["English"]);
  assert.equal(nsis.displayLanguageSelector, false);
  for (const path of [
    nsis.template,
    nsis.installerHooks,
    nsis.installerIcon,
    nsis.uninstallerIcon,
    ...Object.values(nsis.customLanguageFiles),
  ]) {
    assert.ok(readFileSync(join(tauri, path)).length > 0, path);
  }
});

const strings = (text) =>
  new Set([...text.matchAll(/^LangString\s+(\w+)\s/gm)].map((m) => m[1]));

test("every string the installer uses is defined in English.nsh", () => {
  const defined = strings(read("installer", "English.nsh"));
  const used = new Set();
  for (const file of ["installer.nsi", "hooks.nsh"]) {
    for (const [, name] of read("installer", file).matchAll(/\$\((\w+)\)/g)) {
      if (!name.startsWith("MUI_") && !name.startsWith("^")) used.add(name);
    }
  }
  for (const name of used) assert.ok(defined.has(name), `LangString ${name} is missing`);
});

test("English.nsh still has every string of Tauri's own", () => {
  const ours = strings(read("installer", "English.nsh"));
  for (const name of strings(read("installer", "upstream", "English.nsh"))) {
    assert.ok(ours.has(name), `upstream LangString ${name} is missing`);
  }
});

test("the template keeps the pieces the in-app updater and Tauri rely on", () => {
  const template = read("installer", "installer.nsi");
  // Tauri fills these in; a template without them would build a broken installer.
  for (const placeholder of ["{{version}}", "{{main_binary_name}}", "{{installer_hooks}}"]) {
    assert.ok(template.includes(placeholder), placeholder);
  }
  // The updater runs `<installer> /P /UPDATE /R /ARGS ...` without a scope switch.
  for (const switchName of ["/P", "/UPDATE", "/R", "/ARGS", "/ALLUSERS", "/CURRENTUSER"]) {
    assert.ok(template.includes(`"${switchName}"`), `${switchName} is not handled`);
  }
  // Per-user must never prompt: the installer starts without elevation.
  assert.match(template, /RequestExecutionLevel user/);
  assert.doesNotMatch(template, /MULTIUSER_EXECUTIONLEVEL/);
});

test("switches are not prefixes of each other (GetOptions matches substrings)", () => {
  const template = read("installer", "installer.nsi");
  const switches = [
    ...new Set([...template.matchAll(/\$\{GetOptions\} \$CMDLINE "(\/\w+)=?"/g)].map((m) => m[1])),
  ];
  assert.ok(switches.length >= 8, `found only ${switches.join(" ")}`);
  for (const a of switches) {
    for (const b of switches) {
      if (a !== b) assert.ok(!b.toUpperCase().startsWith(a.toUpperCase()), `${a} matches inside ${b}`);
    }
  }
});

test("the updater manifest section and installer mode agree on a passive update", () => {
  assert.equal(conf.plugins.updater.windows.installMode, "passive");
});
