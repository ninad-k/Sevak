// WebdriverIO drives the real Sevak app through tauri-driver, following
// https://v2.tauri.app/develop/tests/webdriver/ (Linux: WebKitWebDriver,
// Windows: msedgedriver matching the WebView2 runtime; macOS is unsupported).
//
// One worker, one session: all specs share the running app, so the launcher,
// the Settings window and the second-instance forwarding are exercised the way
// a person uses them.

import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";

import { appEnv, binaryPath, createProfile, preflightProblems, profilePaths, ROOT } from "./support/profile.mjs";

const PORT = Number(process.env.TAURI_DRIVER_PORT ?? 4444);
const binary = binaryPath();

// The launcher process creates the profile; workers inherit its location.
const isWorker = Boolean(process.env.WDIO_WORKER_ID);
const keepProfile = Boolean(process.env.SEVAK_E2E_PROFILE);
if (!isWorker) {
  const profile = createProfile(process.env.SEVAK_E2E_PROFILE || undefined);
  process.env.SEVAK_E2E_PROFILE = profile.dir;
}
const profile = profilePaths(process.env.SEVAK_E2E_PROFILE);

let tauriDriver;
let driverExpectedToExit = false;

function tauriDriverPath() {
  if (process.env.TAURI_DRIVER) return process.env.TAURI_DRIVER;
  const name = process.platform === "win32" ? "tauri-driver.exe" : "tauri-driver";
  const cargoBin = path.join(process.env.CARGO_HOME ?? path.join(os.homedir(), ".cargo"), "bin", name);
  return fs.existsSync(cargoBin) ? cargoBin : name;
}

function waitForPort(port, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const socket = net.connect({ host: "127.0.0.1", port });
      socket.once("connect", () => {
        socket.destroy();
        resolve();
      });
      socket.once("error", () => {
        socket.destroy();
        if (Date.now() > deadline) reject(new Error(`tauri-driver did not listen on port ${port}`));
        else setTimeout(attempt, 200);
      });
    };
    attempt();
  });
}

/** Asks the app under test to quit, through the same single-instance path a user's `sevak --quit` takes. */
function quitApp() {
  if (!fs.existsSync(binary)) return;
  spawnSync(binary, ["--config", profile.configFile, "--quit"], {
    env: appEnv(profile),
    stdio: "ignore",
    timeout: 15_000,
  });
}

function stopTauriDriver() {
  driverExpectedToExit = true;
  tauriDriver?.kill();
  tauriDriver = undefined;
}

export const config = {
  runner: "local",
  hostname: "127.0.0.1",
  port: PORT,
  // A nested list runs these files in one worker and one session, in order.
  specs: [["./specs/launcher.e2e.mjs", "./specs/settings.e2e.mjs"]],
  maxInstances: 1,
  capabilities: [
    {
      maxInstances: 1,
      "tauri:options": {
        application: binary,
        // One `--config=PATH` argument: msedgedriver prefixes every argument
        // that lacks one with `--` (it treats them as browser switches), so a
        // separate path argument would arrive as `--D:\...` and be refused.
        args: [`--config=${profile.configFile}`],
      },
    },
  ],
  logLevel: "warn",
  waitforTimeout: 15_000,
  connectionRetryTimeout: 120_000,
  connectionRetryCount: 1,
  reporters: ["spec"],
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 120_000 },

  onPrepare() {
    if (!process.env.SEVAK_E2E_BINARY) {
      // The debug app with embedded assets (`custom-protocol`), as the Tauri guide builds it.
      const build = spawnSync("npx", ["tauri", "build", "--debug", "--no-bundle"], {
        cwd: ROOT,
        stdio: "inherit",
        shell: process.platform === "win32",
      });
      if (build.status !== 0) throw new Error("building the debug app failed");
    }
    const problems = preflightProblems(binary);
    if (problems.length > 0) {
      throw new Error(`cannot run the end-to-end suite here:\n- ${problems.join("\n- ")}`);
    }
    console.log(`[e2e] app ${binary}\n[e2e] profile ${profile.dir}`);

    const driverArgs = ["--port", String(PORT)];
    // Windows: msedgedriver.exe from PATH unless named here; Linux: WebKitWebDriver.
    if (process.env.TAURI_NATIVE_DRIVER) driverArgs.push("--native-driver", process.env.TAURI_NATIVE_DRIVER);
    tauriDriver = spawn(tauriDriverPath(), driverArgs, {
      env: appEnv(profile),
      stdio: [null, process.stdout, process.stderr],
    });
    tauriDriver.on("error", (error) => {
      console.error("tauri-driver could not start (install it with `cargo install tauri-driver --locked`):", error);
      process.exit(1);
    });
    tauriDriver.on("exit", (code) => {
      if (!driverExpectedToExit) {
        console.error("tauri-driver exited with code", code);
        process.exit(1);
      }
    });
    return waitForPort(PORT, 30_000);
  },

  onComplete(exitCode) {
    // Sevak keeps running when its windows close; make it exit so nothing is left behind.
    quitApp();
    stopTauriDriver();
    if (exitCode === 0 && !keepProfile) {
      fs.rmSync(profile.dir, { recursive: true, force: true });
    } else {
      console.log(`[e2e] profile and logs kept in ${profile.dir}`);
    }
  },
};

// tauri-driver must not outlive the test run, even on Ctrl+C.
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP", "SIGBREAK"]) {
  process.on(signal, () => {
    stopTauriDriver();
    process.exit(130);
  });
}
process.on("exit", stopTauriDriver);
