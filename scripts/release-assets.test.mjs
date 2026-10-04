// Tests for scripts/updater-manifest.mjs and scripts/verify-release.mjs, the
// scripts the release, promote and rollback workflows share.
// Run with `npm run test:scripts`.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { manifest } from "./updater-manifest.mjs";
import { parseSums, verify } from "./verify-release.mjs";

const VERSION = "1.3.0-beta.2";
const BASE = `https://github.com/ninad-k/Sevak/releases/download/v${VERSION}`;
const INSTALLERS = [
  `Sevak_${VERSION}_x64-setup.exe`,
  `Sevak_${VERSION}_x64_en-US.msi`,
  "Sevak_universal.app.tar.gz",
  `Sevak_${VERSION}_amd64.AppImage`,
  `Sevak_${VERSION}_amd64.deb`,
  `Sevak-${VERSION}-1.x86_64.rpm`,
];

/** A release directory as the build leaves it, with a manifest and checksums. */
function release({ manifestName = "latest-beta.json" } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "sevak-assets-"));
  for (const name of INSTALLERS) {
    writeFileSync(join(dir, name), `binary of ${name}`);
    writeFileSync(join(dir, `${name}.sig`), `signature of ${name}\n`);
  }
  const result = manifest(
    INSTALLERS.flatMap((n) => [n, `${n}.sig`]),
    (file) => readFileSync(join(dir, file), "utf8"),
    VERSION,
    BASE,
    "notes",
    "2026-01-01T00:00:00.000Z",
  );
  writeFileSync(join(dir, manifestName), JSON.stringify(result, null, 2));
  const sums = (names) =>
    names
      .map((n) => `${createHash("sha256").update(readFileSync(join(dir, n))).digest("hex")}  ${n}`)
      .join("\n");
  const names = [...INSTALLERS, ...INSTALLERS.map((n) => `${n}.sig`), manifestName];
  writeFileSync(join(dir, "SHA256SUMS.txt"), sums(names) + "\n");
  return { dir, manifestName, resums: () => writeFileSync(join(dir, "SHA256SUMS.txt"), sums(names) + "\n"), done: () => rmSync(dir, { recursive: true, force: true }) };
}

test("the manifest has an entry per platform with the matching signature", () => {
  const r = release();
  try {
    const data = JSON.parse(readFileSync(join(r.dir, r.manifestName), "utf8"));
    assert.equal(data.version, VERSION);
    assert.deepEqual(
      Object.keys(data.platforms).sort(),
      [
        "darwin-aarch64", "darwin-aarch64-app", "darwin-x86_64", "darwin-x86_64-app",
        "linux-x86_64", "linux-x86_64-appimage", "linux-x86_64-deb", "linux-x86_64-rpm",
        "windows-x86_64", "windows-x86_64-msi", "windows-x86_64-nsis",
      ],
    );
    assert.equal(data.platforms["windows-x86_64"].signature, `signature of Sevak_${VERSION}_x64-setup.exe`);
    assert.equal(data.platforms["windows-x86_64"].url, `${BASE}/Sevak_${VERSION}_x64-setup.exe`);
  } finally {
    r.done();
  }
});

test("a manifest cannot be written without a signed installer for every platform", () => {
  assert.throws(
    () => manifest(["a_x64-setup.exe", "a_x64-setup.exe.sig"], () => "sig", "1.0.0", BASE, "", "now"),
    /no signed artifact for darwin/,
  );
});

test("--name chooses the manifest file name (the CLI)", () => {
  const r = release();
  try {
    const script = fileURLToPath(new URL("./updater-manifest.mjs", import.meta.url));
    execFileSync("node", [script, r.dir, VERSION, BASE, join(r.dir, "none.md"), "--name", "latest.json"]);
    const data = JSON.parse(readFileSync(join(r.dir, "latest.json"), "utf8"));
    assert.equal(data.version, VERSION);
    assert.equal(data.notes, "");
  } finally {
    r.done();
  }
});

test("promotion: a verified beta gets a latest.json and refreshed checksums, and still verifies", () => {
  const r = release();
  try {
    // promote.yml: verify first, then write latest.json from the same assets...
    assert.deepEqual(verify(r.dir, VERSION, [r.manifestName]), []);
    const script = fileURLToPath(new URL("./updater-manifest.mjs", import.meta.url));
    execFileSync("node", [script, r.dir, VERSION, BASE, join(r.dir, "none.md")]);
    // ...rewrite SHA256SUMS.txt over every asset (what `sha256sum -- *` does)...
    const names = readdirSync(r.dir).filter((n) => n !== "SHA256SUMS.txt").sort();
    const sums = names
      .map((n) => `${createHash("sha256").update(readFileSync(join(r.dir, n))).digest("hex")}  ${n}`)
      .join("\n");
    writeFileSync(join(r.dir, "SHA256SUMS.txt"), sums + "\n");
    // ...and the result passes with both manifests, carrying identical platforms.
    assert.deepEqual(verify(r.dir, VERSION, ["latest.json", r.manifestName]), []);
    const [stable, beta] = ["latest.json", r.manifestName].map((f) => JSON.parse(readFileSync(join(r.dir, f), "utf8")));
    assert.deepEqual(stable.platforms, beta.platforms);
  } finally {
    r.done();
  }
});

test("a consistent release verifies", () => {
  const r = release();
  try {
    assert.deepEqual(verify(r.dir, VERSION, [r.manifestName]), []);
  } finally {
    r.done();
  }
});

test("a tampered asset, a missing signature or a stale manifest is caught", () => {
  const r = release();
  try {
    const installer = INSTALLERS[0];

    writeFileSync(join(r.dir, installer), "tampered");
    assert.match(verify(r.dir, VERSION, [r.manifestName]).join("\n"), /x64-setup\.exe: checksum does not match/);
    writeFileSync(join(r.dir, installer), `binary of ${installer}`);

    writeFileSync(join(r.dir, `${installer}.sig`), "a different signature\n");
    r.resums();
    assert.match(verify(r.dir, VERSION, [r.manifestName]).join("\n"), /signature differs/);
    writeFileSync(join(r.dir, `${installer}.sig`), `signature of ${installer}\n`);
    r.resums();

    assert.match(verify(r.dir, "1.3.0-beta.3", [r.manifestName]).join("\n"), /is for version 1\.3\.0-beta\.2, expected 1\.3\.0-beta\.3/);
    assert.match(verify(r.dir, VERSION, ["latest.json"]).join("\n"), /latest\.json is missing/);

    writeFileSync(join(r.dir, "stray.txt"), "x");
    assert.match(verify(r.dir, VERSION, [r.manifestName]).join("\n"), /stray\.txt is not listed/);
    rmSync(join(r.dir, "stray.txt"));

    rmSync(join(r.dir, `${installer}.sig`));
    r.resums = () => {};
    const problems = verify(r.dir, VERSION).join("\n");
    assert.match(problems, new RegExp(`${installer} has no ${installer.replace(/\./g, "\\.")}\\.sig`));
  } finally {
    r.done();
  }
});

test("missing checksums are reported", () => {
  const r = release();
  try {
    rmSync(join(r.dir, "SHA256SUMS.txt"));
    assert.match(verify(r.dir, VERSION).join("\n"), /SHA256SUMS\.txt is missing/);
  } finally {
    r.done();
  }
});

test("parseSums reads sha256sum output in text and binary mode", () => {
  const h = "a".repeat(64);
  const sums = parseSums(`${h}  one.exe\n${h.toUpperCase()} *two.dmg\nnot a line\n`);
  assert.deepEqual([...sums.keys()], ["one.exe", "two.dmg"]);
  assert.equal(sums.get("two.dmg"), h);
});
