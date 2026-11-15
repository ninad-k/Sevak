import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { prepareSigning, signingPlan, unsignedWarning, verifyPublisherSigning } from "./release-signing.mjs";

const windows = { WINDOWS_SIGN_COMMAND: JSON.stringify({ cmd: "publisher-signer", args: ["sign", "%1"] }), WINDOWS_SIGNER_THUMBPRINT: "A".repeat(40) };
const apple = { APPLE_CERTIFICATE: "test-certificate", APPLE_CERTIFICATE_PASSWORD: "test-password", APPLE_SIGNING_IDENTITY: "Developer ID Application: Test (ABCDE12345)", APPLE_ID: "test@example.test", APPLE_PASSWORD: "test-app-password", APPLE_TEAM_ID: "ABCDE12345" };

test("unconfigured signing is explicit, and required/partial configuration fails closed", () => {
  for (const platform of ["Windows", "macOS", "Linux"]) assert.equal(signingPlan(platform, {}).enabled, false);
  assert.throws(() => signingPlan("Windows", { REQUIRE_WINDOWS_SIGNING: "true" }), /requires/);
  assert.throws(() => signingPlan("macOS", { REQUIRE_MACOS_SIGNING: "true" }), /requires/);
  assert.throws(() => signingPlan("Windows", { REQUIRE_WINDOWS_SIGNING: "typo" }), /must be/);
  for (const fields of [windows, apple]) {
    const platform = fields === windows ? "Windows" : "macOS";
    for (const key of Object.keys(fields)) {
      const missing = { ...fields }; delete missing[key];
      assert.throws(() => signingPlan(platform, missing), /requires/);
    }
  }
});

test("Windows command uses structured arguments and one whole path placeholder", () => {
  assert.deepEqual(signingPlan("Windows", windows).config.bundle.windows.signCommand, { cmd: "publisher-signer", args: ["sign", "%1"] });
  for (const value of ["not-json", "null", JSON.stringify({ cmd: "tool", args: [] }), JSON.stringify({ cmd: "tool", args: ["%1", "%1"] }), JSON.stringify({ cmd: "tool", args: ["--file=%1"] }), JSON.stringify({ cmd: "tool\ninjected", args: ["%1"] })]) {
    assert.throws(() => signingPlan("Windows", { ...windows, WINDOWS_SIGN_COMMAND: value }), /WINDOWS_SIGN_COMMAND/);
  }
  assert.throws(() => signingPlan("Windows", { ...windows, WINDOWS_SIGNER_THUMBPRINT: "bad" }), /thumbprint/);
});

test("Apple signing requires real Developer ID and notarization credentials", () => {
  assert.equal(signingPlan("macOS", apple).config.bundle.macOS.signingIdentity, apple.APPLE_SIGNING_IDENTITY);
  assert.throws(() => signingPlan("macOS", { ...apple, APPLE_SIGNING_IDENTITY: "-" }), /Developer ID/);
  assert.throws(() => signingPlan("macOS", { ...apple, APPLE_TEAM_ID: "wrong" }), /team ID/);
});

test("preparation writes only configuration and safe GitHub outputs, never credentials", () => {
  const directory = mkdtempSync(join(tmpdir(), "sevak-signing-"));
  try {
    const output = join(directory, "outputs"); const summary = join(directory, "summary");
    const { configPath } = prepareSigning("macOS", { ...apple, GITHUB_OUTPUT: output, GITHUB_STEP_SUMMARY: summary }, directory);
    assert.deepEqual(JSON.parse(readFileSync(configPath, "utf8")), signingPlan("macOS", apple).config);
    const files = [configPath, output, summary].map((path) => readFileSync(path, "utf8")).join("\n");
    for (const secret of [apple.APPLE_CERTIFICATE, apple.APPLE_CERTIFICATE_PASSWORD, apple.APPLE_PASSWORD]) assert.ok(!files.includes(secret));
    assert.match(readFileSync(output, "utf8"), /enabled=true/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("unconfigured signing is reported as a GitHub warning annotation, not silently", () => {
  for (const platform of ["Windows", "macOS"]) {
    const logged = [];
    const result = verifyPublisherSigning(platform, {}, ".", () => assert.fail("must not run"), (line) => logged.push(line));
    assert.deepEqual(result, { platform, status: "not-configured", verified: false });
    assert.equal(logged.length, 1);
    assert.match(logged[0], /^::warning title=[^:]+::/);
    assert.match(logged[0], new RegExp(`REQUIRE_${platform === "macOS" ? "MACOS" : "WINDOWS"}_SIGNING=true`));
    assert.equal(logged[0], unsignedWarning(platform));
  }
  const linux = [];
  verifyPublisherSigning("Linux", {}, ".", () => assert.fail("must not run"), (line) => linux.push(line));
  assert.deepEqual(linux, []);
  // A requirement still fails closed rather than downgrading to a warning.
  assert.throws(() => verifyPublisherSigning("Windows", { REQUIRE_WINDOWS_SIGNING: "true" }, ".", () => {}, () => {}), /requires/);
  // Configured signing does not warn.
  const configured = [];
  verifyPublisherSigning("Windows", windows, ".", () => {}, (line) => configured.push(line));
  assert.deepEqual(configured, []);
});

test("verification never labels unconfigured or failed artifacts as verified", () => {
  assert.equal(verifyPublisherSigning("Windows", {}, ".", () => assert.fail("must not run"), () => {}).verified, false);
  assert.throws(() => verifyPublisherSigning("Windows", windows, ".", () => { throw new Error("invalid signature"); }), /invalid signature/);
  const calls = [];
  assert.equal(verifyPublisherSigning("Windows", windows, "repo", (...args) => calls.push(args)).verified, true);
  assert.ok(calls[0][1].includes(windows.WINDOWS_SIGNER_THUMBPRINT));
});

test("macOS verification requires codesign, Gatekeeper and stapled notarization checks of the app and disk image", () => {
  const root = mkdtempSync(join(tmpdir(), "sevak-notarization-"));
  try {
    const release = join(root, "target/universal-apple-darwin/release/bundle");
    const bundle = join(release, "macos");
    mkdirSync(join(bundle, "Sevak.app"), { recursive: true });
    mkdirSync(join(release, "dmg"), { recursive: true });
    writeFileSync(join(release, "dmg", "Sevak_1.0.0_universal.dmg"), "");
    const calls = [];
    verifyPublisherSigning("macOS", apple, root, (command, args) => calls.push([command, args]));
    assert.deepEqual(calls.map(([command]) => command), ["codesign", "spctl", "xcrun", "codesign", "spctl"]);
    const [dmgSignature, dmgGatekeeper] = calls.slice(3).map(([, args]) => args);
    assert.ok(dmgSignature.includes("--verify") && dmgSignature.at(-1).endsWith(".dmg"));
    assert.deepEqual(dmgGatekeeper.slice(0, 5), ["-a", "-t", "open", "--context", "context:primary-signature"]);
    assert.ok(dmgGatekeeper.at(-1).endsWith(".dmg"));
    assert.throws(() => verifyPublisherSigning("macOS", apple, root, (command) => { if (command === "xcrun") throw new Error("no ticket"); }), /no ticket/);
    assert.throws(() => verifyPublisherSigning("macOS", apple, root, (command, args) => {
      if (command === "spctl" && args.includes("open")) throw new Error("dmg rejected");
    }), /dmg rejected/);
    writeFileSync(join(release, "dmg", "Stale_0.9.0_universal.dmg"), "");
    assert.throws(() => verifyPublisherSigning("macOS", apple, root, () => {}), /exactly one universal macOS disk image/);
    rmSync(join(release, "dmg"), { recursive: true });
    mkdirSync(join(release, "dmg"));
    assert.throws(() => verifyPublisherSigning("macOS", apple, root, () => {}), /disk image/);
    mkdirSync(join(bundle, "Other.app"));
    assert.throws(() => verifyPublisherSigning("macOS", apple, root, () => {}), /exactly one/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
