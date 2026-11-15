// Starts msedgedriver with verbose logging and asks it for a WebView2 session
// on the Sevak debug app, the same capabilities tauri-driver 2.1.0 sends.
// usage: node repro.mjs <msedgedriver.exe> <sevak.exe> <profileDir> [extra JSON for ms:edgeOptions]
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const [driver, app, profileDir, extra] = process.argv.slice(2);
const configDir = path.join(profileDir, "config");
const dataDir = path.join(profileDir, "data");
fs.mkdirSync(configDir, { recursive: true });
fs.mkdirSync(dataDir, { recursive: true });
const configFile = path.join(configDir, "config.toml");
if (!fs.existsSync(configFile)) {
  fs.writeFileSync(
    configFile,
    `[general]\nhotkey = "Ctrl+Alt+Shift+F11"\nactions_hotkey = ""\nhide_on_blur = false\nlaunch_at_login = false\ncheck_for_updates = false\n[plugins]\ndisabled = ["apps", "files", "bookmarks", "clipboard", "contacts", "1password", "media", "windows", "selection", "shell", "tasks"]\n`,
  );
}
const log = path.join(profileDir, "msedgedriver.log");
const env = { ...process.env, SEVAK_CONFIG_DIR: configDir, SEVAK_DATA_DIR: dataDir };
const proc = spawn(driver, ["--port=4445", "--verbose", `--log-path=${log}`], { env, stdio: "inherit" });
await new Promise((r) => setTimeout(r, 1500));

const edgeOptions = { binary: app, args: [`--config=${configFile}`], ...(extra ? JSON.parse(extra) : {}) };
const body = {
  capabilities: {
    alwaysMatch: { browserName: "webview2", "ms:edgeChromium": true, "ms:edgeOptions": edgeOptions },
  },
};
const started = Date.now();
let sessionId;
try {
  const res = await fetch("http://127.0.0.1:4445/session", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const json = await res.json();
  sessionId = json.value?.sessionId;
  console.log(`[repro] ${res.status} after ${Date.now() - started} ms`, sessionId ? `session ${sessionId}` : JSON.stringify(json.value).slice(0, 400));
  if (sessionId) {
    const title = await (await fetch(`http://127.0.0.1:4445/session/${sessionId}/title`)).json();
    console.log("[repro] title", JSON.stringify(title.value));
    await fetch(`http://127.0.0.1:4445/session/${sessionId}`, { method: "DELETE" });
  }
} finally {
  proc.kill();
  console.log(`[repro] driver log ${log}`);
}
