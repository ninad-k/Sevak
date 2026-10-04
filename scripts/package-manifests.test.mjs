// Renders the package-manager manifests for a made-up release and checks the
// Windows ones: they must point at the NSIS installer, declare the per-user scope
// the installer's silent default gives, and leave no placeholder behind.
// Run with `npm run test:scripts`.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const script = join(dirname(fileURLToPath(import.meta.url)), "package-manifests.mjs");
const version = "1.2.3";
const sha = (n) => String(n).repeat(64).slice(0, 64);
const assets = {
  [`Sevak_${version}_x64-setup.exe`]: sha("a"),
  [`Sevak_${version}_x64_en-US.msi`]: sha("b"),
  [`Sevak_${version}_universal.dmg`]: sha("c"),
  [`Sevak_${version}_amd64.deb`]: sha("d"),
};

function render(files) {
  const dir = mkdtempSync(join(tmpdir(), "sevak-manifests-"));
  const sums = join(dir, "SHA256SUMS.txt");
  writeFileSync(
    sums,
    Object.entries(files)
      .map(([name, hash]) => `${hash}  ${name}`)
      .join("\n"),
  );
  const out = join(dir, "out");
  const run = spawnSync(process.execPath, [script, version, sums, out], { encoding: "utf8" });
  return { run, out, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

test("winget installer manifest targets the NSIS installer, per user", () => {
  const { run, out, cleanup } = render(assets);
  try {
    assert.equal(run.status, 0, run.stderr);
    const dir = join(out, "winget", "manifests", "n", "NinadKulkarni", "Sevak", version);
    const installer = readFileSync(join(dir, "NinadKulkarni.Sevak.installer.yaml"), "utf8");
    assert.match(installer, /^InstallerType: nullsoft$/m);
    assert.match(installer, /^Scope: user$/m);
    assert.match(installer, /^PackageVersion: 1\.2\.3$/m);
    assert.ok(
      installer.includes(
        `InstallerUrl: https://github.com/ninad-k/Sevak/releases/download/v${version}/Sevak_${version}_x64-setup.exe`,
      ),
    );
    assert.ok(installer.includes(`InstallerSha256: ${sha("a").toUpperCase()}`));
    assert.match(installer, /^\s+ProductCode: Sevak$/m);
    // winget runs nullsoft installers with /S; a per-user /S install needs no switch.
    assert.doesNotMatch(installer, /^\s*InstallerSwitches:/m);
    for (const file of ["NinadKulkarni.Sevak.yaml", "NinadKulkarni.Sevak.locale.en-US.yaml"]) {
      assert.match(readFileSync(join(dir, file), "utf8"), /^PackageVersion: 1\.2\.3$/m);
    }
  } finally {
    cleanup();
  }
});

test("the Scoop manifest still extracts the MSI, which the NSIS changes do not touch", () => {
  const { run, out, cleanup } = render(assets);
  try {
    assert.equal(run.status, 0, run.stderr);
    const scoop = JSON.parse(readFileSync(join(out, "scoop", "sevak.json"), "utf8"));
    assert.equal(scoop.version, version);
    assert.ok(scoop.architecture["64bit"].url.endsWith(`Sevak_${version}_x64_en-US.msi`));
    assert.equal(scoop.architecture["64bit"].hash, sha("b"));
  } finally {
    cleanup();
  }
});

test("rendering fails when the installer is missing from SHA256SUMS.txt", () => {
  const { [`Sevak_${version}_x64-setup.exe`]: _omitted, ...rest } = assets;
  const { run, cleanup } = render(rest);
  try {
    assert.notEqual(run.status, 0);
    assert.match(run.stderr, /no nsis asset/);
  } finally {
    cleanup();
  }
});
