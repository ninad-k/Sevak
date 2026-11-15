// The Settings window: opened with `sevak --settings` (forwarded to the app
// under test), a toggle is flipped and the change cancelled, and the Gallery
// page stays unloaded without any network request.

import assert from "node:assert/strict";
import fs from "node:fs";

import { readLogs } from "../support/profile.mjs";
import { button, profile, runSecondInstance, switchToWindow, texts } from "../support/app.mjs";

describe("settings", () => {
  let launcher;
  let settings;
  let configBefore;

  before(async () => {
    launcher = await switchToWindow("");
    configBefore = fs.readFileSync(profile.configFile);
  });

  it("opens from a second `sevak --settings`", async () => {
    const forwarded = runSecondInstance("--settings");
    assert.equal(forwarded.status, 0, `sevak --settings exited ${forwarded.status}: ${forwarded.stderr}`);
    settings = await switchToWindow("#settings");
    await (await $("#tab-general")).waitForExist({ timeoutMsg: "the settings sections never loaded" });
  });

  it("flips a toggle without saving it", async () => {
    const updates = await $('button[role="switch"][aria-label="Check for updates"]');
    await updates.waitForExist();
    assert.equal(await updates.getAttribute("aria-checked"), "false", "the seed config has update checks off");
    await updates.click();
    await browser.waitUntil(async () => (await updates.getAttribute("aria-checked")) === "true", {
      timeoutMsg: "the toggle did not switch on",
    });
    // A changed form offers Cancel instead of Close.
    await (await button("Cancel")).waitForExist({ timeoutMsg: "the footer does not offer Cancel" });
  });

  it("shows the Gallery as not loaded, without fetching it", async () => {
    await (await $("#tab-gallery")).click();
    await browser.waitUntil(async () => (await texts('[role="tabpanel"] h1')).includes("Gallery"), {
      timeoutMsg: "the Gallery page did not open",
    });
    const load = await button("Load gallery");
    await load.waitForExist({ timeoutMsg: "no Load gallery button" });
    assert.ok(await load.isEnabled(), "Load gallery is ready to press");
    assert.equal(await (await $('ul[aria-label="Gallery entries"]')).isExisting(), false, "no entries are listed");
    const alerts = await texts('[role="alert"]');
    assert.ok(!alerts.some((text) => text.includes("Could not reach")), `unexpected alert: ${alerts}`);
    assert.ok(!readLogs(profile).includes("gallery: fetching the index"), "the gallery was not requested");
  });

  it("cancels: the window closes and config.toml is unchanged", async () => {
    await (await button("Cancel")).click();
    await browser.waitUntil(async () => !(await browser.getWindowHandles()).includes(settings), {
      timeoutMsg: "the settings window did not close",
    });
    await browser.switchToWindow(launcher);
    assert.deepEqual(fs.readFileSync(profile.configFile), configBefore, "config.toml was not written");
  });
});
