// Helpers the specs share. `browser` is WebdriverIO's global session.

import { spawnSync } from "node:child_process";

import { appEnv, binaryPath, profilePaths } from "./profile.mjs";

export const profile = profilePaths(process.env.SEVAK_E2E_PROFILE);

/** Switches to the window whose URL hash is `hash` ("" is the launcher) and returns its handle. */
export async function switchToWindow(hash, timeout = 30_000) {
  let found = null;
  await browser.waitUntil(
    async () => {
      for (const handle of await browser.getWindowHandles()) {
        try {
          await browser.switchToWindow(handle);
          const current = await browser.execute(() => window.location.hash);
          if (current === hash) {
            found = handle;
            return true;
          }
        } catch {
          // The window closed while we looked at it.
        }
      }
      return false;
    },
    { timeout, interval: 500, timeoutMsg: `no window with location hash "${hash}"` },
  );
  return found;
}

/**
 * Runs the app binary again with `args`, as a user would from a terminal. The
 * single-instance plugin forwards the request to the app under test.
 */
export function runSecondInstance(...args) {
  const result = spawnSync(binaryPath(), ["--config", profile.configFile, ...args], {
    env: appEnv(profile),
    encoding: "utf8",
    timeout: 30_000,
  });
  if (result.error) throw result.error;
  return result;
}

/** Visible text of every element matching `selector`, trimmed. */
export async function texts(selector) {
  return browser.execute(
    (sel) => Array.from(document.querySelectorAll(sel), (el) => (el.textContent ?? "").trim()),
    selector,
  );
}

/** The button whose visible text is exactly `label`. */
export function button(label) {
  return $(`//button[normalize-space()="${label}"]`);
}
