// Prepare publisher-signing configuration without printing credentials. Updater
// signatures are independent: this script configures Authenticode / Developer ID.
import { appendFileSync, mkdirSync, writeFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const WINDOWS_FIELDS = ["WINDOWS_SIGN_COMMAND", "WINDOWS_SIGNER_THUMBPRINT"];
const APPLE_FIELDS = ["APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD", "APPLE_SIGNING_IDENTITY", "APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID"];

function requiredFlag(value, name) {
  if (!value || value === "false") return false;
  if (value === "true") return true;
  throw new Error(`${name} must be true, false, or unset.`);
}

export function signingPlan(platform, env) {
  if (!["Windows", "macOS", "Linux"].includes(platform)) throw new Error("Unknown signing platform.");
  if (platform === "Linux") return { enabled: false, platform, config: {} };
  const fields = platform === "Windows" ? WINDOWS_FIELDS : APPLE_FIELDS;
  const flag = platform === "Windows" ? "REQUIRE_WINDOWS_SIGNING" : "REQUIRE_MACOS_SIGNING";
  const required = requiredFlag(env[flag], flag);
  const any = fields.some((key) => Boolean(env[key]?.trim()));
  if (!any && !required) return { enabled: false, platform, config: {} };
  const missing = fields.filter((key) => !env[key]?.trim());
  if (missing.length) throw new Error(`Publisher signing requires: ${missing.join(", ")}. Partial signing configuration never falls back to unsigned.`);
  if (platform === "Windows") {
    let command;
    try { command = JSON.parse(env.WINDOWS_SIGN_COMMAND); } catch { throw new Error("WINDOWS_SIGN_COMMAND must be JSON with cmd and args (never include secrets in arguments)."); }
    if (!command || typeof command !== "object" || Array.isArray(command) ||
        Object.keys(command).some((key) => !["cmd", "args"].includes(key)) ||
        typeof command.cmd !== "string" || !command.cmd.trim() || /[\r\n\0]/.test(command.cmd) ||
        !Array.isArray(command.args) || !command.args.every((arg) => typeof arg === "string" && !/[\r\n\0]/.test(arg)) ||
        command.args.filter((arg) => arg === "%1").length !== 1 || command.args.some((arg) => arg.includes("%1") && arg !== "%1")) {
      throw new Error("WINDOWS_SIGN_COMMAND must have a cmd and an args array with exactly one standalone %1 path argument.");
    }
    const thumbprint = env.WINDOWS_SIGNER_THUMBPRINT.replace(/\s/g, "").toUpperCase();
    if (!/^[0-9A-F]{40}$/.test(thumbprint)) throw new Error("WINDOWS_SIGNER_THUMBPRINT must be a SHA-1 certificate thumbprint (40 hex digits).");
    return { enabled: true, platform, thumbprint, config: { bundle: { windows: { signCommand: command } } } };
  }
  const identity = env.APPLE_SIGNING_IDENTITY.trim();
  if (!identity.startsWith("Developer ID Application: ") || /[\r\n\0]/.test(identity)) {
    throw new Error("APPLE_SIGNING_IDENTITY must name a Developer ID Application certificate, not an ad-hoc identity.");
  }
  if (!/^[A-Z0-9]{10}$/.test(env.APPLE_TEAM_ID)) throw new Error("APPLE_TEAM_ID must be a 10-character Apple team ID.");
  return { enabled: true, platform, config: { bundle: { macOS: { signingIdentity: identity } } } };
}

export function prepareSigning(platform, env, directory) {
  const plan = signingPlan(platform, env);
  mkdirSync(directory, { recursive: true });
  const configPath = resolve(directory, "sevak-publisher-signing.json");
  writeFileSync(configPath, JSON.stringify(plan.config, null, 2) + "\n", { mode: 0o600 });
  if (env.GITHUB_OUTPUT) appendFileSync(env.GITHUB_OUTPUT, `config_path=${configPath.replaceAll("\\", "/")}\nenabled=${plan.enabled}\n`);
  if (env.GITHUB_STEP_SUMMARY) appendFileSync(env.GITHUB_STEP_SUMMARY,
    `### ${platform} publisher signing\n\n${plan.enabled ? "Configured; artifact verification must pass before publication." : "Not configured. This build does not establish publisher identity."}\n\nUpdater signatures are checked separately.\n`);
  return { plan, configPath };
}

// A GitHub annotation, so an unsigned build is visible on the run page instead
// of passing silently. REQUIRE_*_SIGNING turns the same condition into an error.
export function unsignedWarning(platform) {
  return `::warning title=${platform} publisher signing not configured::This ${platform} build is not publisher-signed` +
    `${platform === "macOS" ? " (ad-hoc signature, not notarized)" : ""}. Set REQUIRE_${platform === "macOS" ? "MACOS" : "WINDOWS"}_SIGNING=true once signing is provisioned (docs/publisher-signing.md).`;
}

export function verifyPublisherSigning(platform, env, root, run = execFileSync, log = console.log) {
  const plan = signingPlan(platform, env);
  if (!plan.enabled) {
    // Linux packages have no publisher-signing mechanism here; nothing to warn about.
    if (platform !== "Linux") log(unsignedWarning(platform));
    return { platform, status: "not-configured", verified: false };
  }
  if (platform === "Windows") {
    run("pwsh", ["-NoProfile", "-File", join(root, "scripts/verify-windows-signatures.ps1"), "-BuildRoot", join(root, "target/release"), "-ExpectedThumbprint", plan.thumbprint], { stdio: "inherit" });
  } else {
    const directory = join(root, "target/universal-apple-darwin/release/bundle/macos");
    const apps = readdirSync(directory).filter((name) => name.endsWith(".app"));
    if (apps.length !== 1) throw new Error("Expected exactly one universal macOS app to verify.");
    const app = join(directory, apps[0]);
    run("codesign", ["--verify", "--deep", "--strict", "--verbose=2", app], { stdio: "inherit" });
    run("spctl", ["--assess", "--type", "execute", "--verbose=2", app], { stdio: "inherit" });
    run("xcrun", ["stapler", "validate", app], { stdio: "inherit" });
    // The disk image is what people download: its own signature must be a
    // Developer ID one that Gatekeeper accepts for opening, not only the app's.
    const dmgDirectory = join(root, "target/universal-apple-darwin/release/bundle/dmg");
    const dmgs = readdirSync(dmgDirectory).filter((name) => name.endsWith(".dmg"));
    if (dmgs.length !== 1) throw new Error("Expected exactly one universal macOS disk image to verify.");
    const dmg = join(dmgDirectory, dmgs[0]);
    run("codesign", ["--verify", "--strict", "--verbose=2", dmg], { stdio: "inherit" });
    run("spctl", ["-a", "-t", "open", "--context", "context:primary-signature", "-v", dmg], { stdio: "inherit" });
  }
  return { platform, status: "verified", verified: true };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [command, argument] = process.argv.slice(2);
    const platform = process.env.RUNNER_OS ?? ({ win32: "Windows", darwin: "macOS", linux: "Linux" })[process.platform];
    if (command === "prepare") {
      prepareSigning(platform, process.env, argument ?? process.env.RUNNER_TEMP ?? "target/signing");
      console.log(`${platform} publisher signing configuration checked.`);
    } else if (command === "verify") {
      console.log(JSON.stringify(verifyPublisherSigning(platform, process.env, resolve(argument ?? "."))));
    } else throw new Error("Usage: node scripts/release-signing.mjs prepare [directory] | verify [repository]");
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
