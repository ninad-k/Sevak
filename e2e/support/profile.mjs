// The isolated profile every end-to-end run uses: its own config file, data
// folder (logs, usage) and, on Linux, XDG folders, so a run never reads or
// writes the developer's real Sevak settings or startup entries.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

/**
 * The config the app starts with. Everything that would reach outside the
 * profile is off: no update checks, no indexing of real folders, no clipboard
 * or contacts readers, no app index, and a hotkey nobody else uses.
 */
export const SEED_CONFIG = `# Written by e2e/support/profile.mjs for an end-to-end run.
[general]
hotkey = "Ctrl+Alt+Shift+F11"
actions_hotkey = ""
hide_on_blur = false
launch_at_login = false
check_for_updates = false

[search]
query_history = false

[plugins]
disabled = ["apps", "files", "bookmarks", "clipboard", "contacts", "1password", "media", "windows", "selection", "shell", "tasks"]

[calculator]
currency = false

[files]
directories = []
use_os_index = false
`;

export function binaryPath() {
  // Relative to the repository root, wherever the runner was started from.
  if (process.env.SEVAK_E2E_BINARY) return path.resolve(ROOT, process.env.SEVAK_E2E_BINARY);
  return path.join(ROOT, "target", "debug", process.platform === "win32" ? "sevak.exe" : "sevak");
}

/** Paths of the profile in `dir`. */
export function profilePaths(dir) {
  return {
    dir,
    configDir: path.join(dir, "config"),
    configFile: path.join(dir, "config", "config.toml"),
    dataDir: path.join(dir, "data"),
    logDir: path.join(dir, "data", "logs"),
    xdg: path.join(dir, "xdg"),
  };
}

/** Creates a fresh profile (in `dir`, or a new temp folder) and returns its paths. */
export function createProfile(dir = fs.mkdtempSync(path.join(os.tmpdir(), "sevak-e2e-"))) {
  const paths = profilePaths(path.resolve(dir));
  fs.rmSync(paths.configDir, { recursive: true, force: true });
  fs.rmSync(paths.dataDir, { recursive: true, force: true });
  fs.mkdirSync(paths.configDir, { recursive: true });
  fs.mkdirSync(paths.dataDir, { recursive: true });
  fs.writeFileSync(paths.configFile, SEED_CONFIG);
  return paths;
}

/** The environment for the app (and the drivers that start it). */
export function appEnv(paths, base = process.env) {
  const env = { ...base, SEVAK_CONFIG_DIR: paths.configDir, SEVAK_DATA_DIR: paths.dataDir };
  if (process.platform === "linux") {
    // Startup entries (autostart), WebKit's caches and any other per-user
    // file end up inside the profile too.
    for (const [name, sub] of [
      ["XDG_CONFIG_HOME", "config"],
      ["XDG_DATA_HOME", "data"],
      ["XDG_CACHE_HOME", "cache"],
      ["XDG_STATE_HOME", "state"],
    ]) {
      env[name] = path.join(paths.xdg, sub);
      fs.mkdirSync(env[name], { recursive: true });
    }
  }
  return env;
}

/** The text of all log files of the profile. */
export function readLogs(paths) {
  if (!fs.existsSync(paths.logDir)) return "";
  return fs
    .readdirSync(paths.logDir)
    .filter((name) => name.endsWith(".log"))
    .map((name) => fs.readFileSync(path.join(paths.logDir, name), "utf8"))
    .join("\n");
}

/**
 * Reasons this machine cannot run the suite without touching the developer's
 * own Sevak. Empty when it is safe.
 */
export function preflightProblems(binary) {
  const problems = [];
  if (process.platform === "darwin") {
    problems.push("macOS has no WKWebView WebDriver; the end-to-end suite runs on Linux and Windows only.");
  }
  if (!fs.existsSync(binary)) {
    problems.push(`No app at ${binary}. Build it with \`npx tauri build --debug --no-bundle\` or set SEVAK_E2E_BINARY.`);
  }
  // Single instance: a running Sevak would receive the launch instead of the
  // app under test.
  const running =
    process.platform === "win32"
      ? spawnSync("tasklist", ["/FI", "IMAGENAME eq sevak.exe", "/FO", "CSV", "/NH"], { encoding: "utf8" }).stdout ?? ""
      : spawnSync("pgrep", ["-x", "sevak"], { encoding: "utf8" }).stdout ?? "";
  if (/sevak/i.test(running)) {
    problems.push("Sevak is running. Quit it first (`sevak --quit`): the app under test is a single instance.");
  }
  // Windows keeps startup entries in the registry, which no profile can
  // isolate. The app's routine startup sync would remove an existing entry,
  // because the test profile has launch_at_login = false.
  if (process.platform === "win32" && process.env.CI !== "true") {
    const run = spawnSync(
      "reg",
      ["query", String.raw`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, "/v", "Sevak"],
      { encoding: "utf8" },
    );
    if (run.status === 0) {
      problems.push(
        "Sevak is registered to start at sign-in for this user. The app under test would remove that entry; turn startup off in Settings (or `sevak --set-startup off`) before running the suite, and on again afterwards.",
      );
    }
  }
  return problems;
}
