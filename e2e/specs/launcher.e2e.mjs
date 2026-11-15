// The launcher window: it opens with the isolated profile and answers a
// calculation. Only text is typed; nothing presses Enter, so no result runs.

import assert from "node:assert/strict";

import { readLogs } from "../support/profile.mjs";
import { profile, switchToWindow, texts } from "../support/app.mjs";

describe("launcher", () => {
  it("opens the search bar", async () => {
    await switchToWindow("");
    const search = await $('input[aria-label="Search"]');
    await search.waitForExist({ timeoutMsg: "the search box never appeared" });
    assert.equal(await search.getAttribute("placeholder"), "Type to search…");
  });

  it("runs with the isolated test profile", async () => {
    await browser.waitUntil(() => readLogs(profile).includes("sevak is ready"), {
      timeoutMsg: `no "sevak is ready" in the logs under ${profile.logDir}`,
    });
    // The app loaded our seed config, not the developer's.
    assert.ok(readLogs(profile).includes(profile.configFile), "the log names the test profile's config.toml");
  });

  it("shows the calculator result for 2+2", async () => {
    const search = await $('input[aria-label="Search"]');
    await search.setValue("2+2");
    await browser.waitUntil(async () => (await texts('[aria-label="Results"] [role="option"] .title')).includes("4"), {
      timeoutMsg: "no result titled 4 for 2+2",
    });
    const titles = await texts('[aria-label="Results"] [role="option"] .title');
    const subtitles = await texts('[aria-label="Results"] [role="option"] .subtitle');
    const index = titles.indexOf("4");
    assert.match(subtitles[index] ?? "", /2 \+ 2/, "the calculator row shows the normalized expression");
    await search.clearValue();
  });
});
