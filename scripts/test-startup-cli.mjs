#!/usr/bin/env node
// Exercises the real headless startup CLI of a built `sevak` binary and checks
// the operating system's own startup artifacts:
//
//   Windows  HKCU\...\CurrentVersion\Run\Sevak and StartupApproved\Run\Sevak
//   macOS    ~/Library/LaunchAgents/Sevak.plist and `launchctl print-disabled`
//   Linux    $XDG_CONFIG_HOME/autostart/Sevak.desktop (Exec, Hidden=true)
//
// Usage: node scripts/test-startup-cli.mjs [--binary PATH] [--real-home]
//                                          [--allow-real-startup]
//
// Safety. The config file always lives in a new temp folder. On Linux the
// autostart folder is a temp folder too, unless --real-home is given (CI does,
// to check the real ~/.config/autostart). Windows and macOS have one per-user
// startup store, so the script refuses to run there unless CI=true or
// --allow-real-startup is given, and refuses whenever a Sevak entry already
// exists (it would be overwritten). Everything it creates is removed at the end.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ENTRY = "Sevak";
const RUN_KEY = String.raw`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`;
const APPROVED_KEY = String.raw`HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run`;

// ---------------------------------------------------------------------------
// Pure parsers (unit-tested in test-startup-cli.test.mjs)
// ---------------------------------------------------------------------------

/** Groups of a desktop entry file: Map<group name, Map<key, raw value>>. */
export function parseDesktopFile(text) {
  const groups = new Map();
  let current = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line === "" || line.startsWith("#")) continue;
    const header = /^\[(.+)\]$/.exec(line);
    if (header) {
      current = new Map();
      groups.set(header[1], current);
      continue;
    }
    const eq = line.indexOf("=");
    if (current && eq > 0) {
      const key = line.slice(0, eq).trim();
      // The first occurrence wins, as in Sevak's own parser.
      if (!current.has(key)) current.set(key, line.slice(eq + 1).trim());
    }
  }
  return groups;
}

/**
 * Arguments of a desktop entry Exec value: general string unescaping first
 * (\\ \s \n \t \r), then the Exec quoting rules (\" \` \$ \\ inside quotes).
 */
export function parseDesktopExec(value) {
  let unescaped = "";
  for (let i = 0; i < value.length; i++) {
    const ch = value[i];
    if (ch === "\\" && i + 1 < value.length) {
      const next = value[++i];
      unescaped += { s: " ", n: "\n", t: "\t", r: "\r", "\\": "\\" }[next] ?? `\\${next}`;
    } else {
      unescaped += ch;
    }
  }
  const args = [];
  let i = 0;
  while (i < unescaped.length) {
    while (unescaped[i] === " ") i++;
    if (i >= unescaped.length) break;
    let arg = "";
    if (unescaped[i] === '"') {
      i++;
      while (i < unescaped.length && unescaped[i] !== '"') {
        if (unescaped[i] === "\\" && i + 1 < unescaped.length) i++;
        arg += unescaped[i++];
      }
      if (unescaped[i] !== '"') throw new Error(`unterminated quote in Exec: ${value}`);
      i++;
    } else {
      while (i < unescaped.length && unescaped[i] !== " ") arg += unescaped[i++];
    }
    args.push(arg.replaceAll("%%", "%"));
  }
  return args;
}

/**
 * The state `launchctl print-disabled` reports for `label`: true (disabled),
 * false (enabled) or null (not listed). Accepts both the current
 * `"label" => disabled|enabled` and the older `"label" => true|false` forms.
 */
export function launchctlDisabled(output, label) {
  for (const line of output.split(/\r?\n/)) {
    const match = /^\s*"(.*)"\s*=>\s*(\S+)\s*$/.exec(line);
    if (!match || match[1] !== label) continue;
    if (match[2] === "disabled" || match[2] === "true") return true;
    if (match[2] === "enabled" || match[2] === "false") return false;
    throw new Error(`unknown launchctl state for ${label}: ${match[2]}`);
  }
  return null;
}

/** `{ type, data }` of value `name` in `reg query` output, or null. */
export function parseRegQuery(output, name) {
  for (const line of output.split(/\r?\n/)) {
    const match = /^\s+(.+?)\s{4}(REG_[A-Z_]+)\s{4}(.*)$/.exec(line);
    if (match && match[1] === name) return { type: match[2], data: match[3] };
    const empty = /^\s+(.+?)\s{4}(REG_[A-Z_]+)\s*$/.exec(line);
    if (empty && empty[1] === name) return { type: empty[2], data: "" };
  }
  return null;
}

/** Splits a Windows command line by the CommandLineToArgvW rules. */
export function parseWindowsCommandLine(text) {
  const args = [];
  let i = 0;
  while (i < text.length) {
    while (text[i] === " " || text[i] === "\t") i++;
    if (i >= text.length) break;
    let arg = "";
    let quoted = false;
    while (i < text.length && (quoted || (text[i] !== " " && text[i] !== "\t"))) {
      if (text[i] === "\\") {
        let slashes = 0;
        while (text[i] === "\\") {
          slashes++;
          i++;
        }
        if (text[i] === '"') {
          arg += "\\".repeat(Math.floor(slashes / 2));
          if (slashes % 2 === 1) {
            arg += '"';
            i++;
          }
        } else {
          arg += "\\".repeat(slashes);
        }
      } else if (text[i] === '"') {
        quoted = !quoted;
        i++;
      } else {
        arg += text[i++];
      }
    }
    args.push(arg);
  }
  return args;
}

/** The `[general] launch_at_login` value written to config.toml, or null. */
export function launchAtLogin(toml) {
  let inGeneral = false;
  for (const raw of toml.split(/\r?\n/)) {
    const line = raw.replace(/#.*$/, "").trim();
    const header = /^\[([^\]]+)\]$/.exec(line);
    if (header) {
      inGeneral = header[1].trim() === "general";
      continue;
    }
    const match = /^launch_at_login\s*=\s*(true|false)$/.exec(line);
    if (inGeneral && match) return match[1] === "true";
  }
  return null;
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

class Check {
  constructor() {
    this.count = 0;
  }
  ok(condition, message, details) {
    this.count++;
    if (!condition) {
      const extra = details === undefined ? "" : `\n  ${JSON.stringify(details, null, 2).replaceAll("\n", "\n  ")}`;
      throw new Error(`not ok ${this.count} - ${message}${extra}`);
    }
    console.log(`ok ${this.count} - ${message}`);
  }
  equal(actual, expected, message) {
    this.ok(JSON.stringify(actual) === JSON.stringify(expected), message, { actual, expected });
  }
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", ...options });
  if (result.error) throw result.error;
  return { code: result.status, stdout: result.stdout ?? "", stderr: result.stderr ?? "" };
}

function samePath(a, b) {
  const real = (p) => {
    try {
      return fs.realpathSync.native(p);
    } catch {
      return path.resolve(p);
    }
  };
  const [x, y] = [real(a), real(b)];
  return process.platform === "win32" ? x.toLowerCase() === y.toLowerCase() : x === y;
}

function parseArgs(argv) {
  const options = { binary: null, realHome: false, allowReal: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--binary") options.binary = argv[++i];
    else if (arg.startsWith("--binary=")) options.binary = arg.slice("--binary=".length);
    else if (arg === "--real-home") options.realHome = true;
    else if (arg === "--allow-real-startup") options.allowReal = true;
    else throw new Error(`unknown argument ${arg}`);
  }
  if (!options.binary) {
    const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
    options.binary = path.join(root, "target", "debug", process.platform === "win32" ? "sevak.exe" : "sevak");
  }
  return options;
}

// ---------------------------------------------------------------------------
// Per-OS artifact inspection
// ---------------------------------------------------------------------------

function windowsPlatform() {
  const query = (key) => {
    const result = run("reg", ["query", key, "/v", ENTRY]);
    return result.code === 0 ? parseRegQuery(result.stdout, ENTRY) : null;
  };
  const remove = (key) => run("reg", ["delete", key, "/v", ENTRY, "/f"]);
  // A StartupApproved value can outlive its Run value (Windows leaves it when
  // a Run value is deleted elsewhere). It is kept aside and put back at the end.
  let approvedBefore = null;
  return {
    name: "Windows registry",
    existing() {
      if (query(RUN_KEY)) return [RUN_KEY];
      approvedBefore = query(APPROVED_KEY);
      if (approvedBefore) {
        if (approvedBefore.type !== "REG_BINARY" || !/^[0-9A-F]*$/i.test(approvedBefore.data)) {
          return [`${APPROVED_KEY} (unexpected ${approvedBefore.type} value)`];
        }
        console.log(`# keeping aside a leftover StartupApproved value (${approvedBefore.data}); it is restored afterwards`);
        remove(APPROVED_KEY);
      }
      return [];
    },
    entry() {
      const value = query(RUN_KEY);
      return value && { type: value.type, args: parseWindowsCommandLine(value.data), approved: query(APPROVED_KEY) };
    },
    checkEnabledEntry(check, entry) {
      check.equal(entry.type, "REG_SZ", "Run value is a string");
      check.ok(entry.approved !== null, "StartupApproved value was written by the explicit opt-in");
      check.ok(/^0[26]/.test(entry.approved?.data ?? ""), "StartupApproved state is enabled (2 or 6)", entry.approved);
    },
    disableLikeTheOs(check) {
      // What Task Manager > Startup apps > Disable writes: state 3 plus a timestamp.
      const r = run("reg", ["add", APPROVED_KEY, "/v", ENTRY, "/t", "REG_BINARY", "/d", "030000000000000000000000", "/f"]);
      check.equal(r.code, 0, "disabled the entry the way Task Manager does (StartupApproved = 3)");
    },
    absent: () => query(RUN_KEY) === null && query(APPROVED_KEY) === null,
    cleanup() {
      remove(RUN_KEY);
      remove(APPROVED_KEY);
    },
    restore() {
      if (approvedBefore) {
        run("reg", ["add", APPROVED_KEY, "/v", ENTRY, "/t", "REG_BINARY", "/d", approvedBefore.data, "/f"]);
      }
    },
  };
}

function macPlatform() {
  const file = path.join(os.homedir(), "Library", "LaunchAgents", `${ENTRY}.plist`);
  const domain = `gui/${process.getuid()}`;
  const printDisabled = () => {
    const r = run("/bin/launchctl", ["print-disabled", domain]);
    if (r.code !== 0) throw new Error(`launchctl print-disabled ${domain} failed: ${r.stderr}`);
    return launchctlDisabled(r.stdout, ENTRY);
  };
  let disabledBefore = null;
  return {
    name: "macOS LaunchAgent",
    existing() {
      disabledBefore = printDisabled();
      return fs.existsSync(file) ? [file] : [];
    },
    entry() {
      if (!fs.existsSync(file)) return null;
      const r = run("/usr/bin/plutil", ["-convert", "json", "-o", "-", file]);
      if (r.code !== 0) throw new Error(`plutil could not read ${file}: ${r.stderr}`);
      const plist = JSON.parse(r.stdout);
      return { plist, args: plist.ProgramArguments ?? [], disabled: printDisabled() };
    },
    checkEnabledEntry(check, entry) {
      check.equal(entry.plist.Label, ENTRY, "plist Label is Sevak");
      check.equal(entry.plist.RunAtLoad, true, "plist RunAtLoad is true");
      check.ok(entry.plist.Disabled !== true, "plist is not Disabled");
      check.ok(entry.disabled !== true, "launchctl print-disabled does not list Sevak as disabled", entry.disabled);
    },
    disableLikeTheOs(check) {
      const r = run("/bin/launchctl", ["disable", `${domain}/${ENTRY}`]);
      check.equal(r.code, 0, `launchctl disable ${domain}/${ENTRY}`);
      check.equal(printDisabled(), true, "launchctl print-disabled now lists Sevak as disabled");
    },
    afterReenable(check) {
      check.equal(printDisabled(), false, "--set-startup on re-enabled the label in launchctl");
    },
    absent: () => !fs.existsSync(file),
    cleanup() {
      fs.rmSync(file, { force: true });
      // Leave the disabled database as it was before the run.
      run("/bin/launchctl", [disabledBefore === true ? "disable" : "enable", `${domain}/${ENTRY}`]);
    },
  };
}

function linuxPlatform(env) {
  const configHome =
    env.XDG_CONFIG_HOME && path.isAbsolute(env.XDG_CONFIG_HOME)
      ? env.XDG_CONFIG_HOME
      : path.join(env.HOME ?? os.homedir(), ".config");
  const file = path.join(configHome, "autostart", `${ENTRY}.desktop`);
  const read = () => fs.readFileSync(file, "utf8");
  return {
    name: `Linux autostart (${file})`,
    file,
    existing: () => (fs.existsSync(file) ? [file] : []),
    entry() {
      if (!fs.existsSync(file)) return null;
      const group = parseDesktopFile(read()).get("Desktop Entry");
      return { group, args: group?.has("Exec") ? parseDesktopExec(group.get("Exec")) : [] };
    },
    checkEnabledEntry(check, entry) {
      check.ok(entry.group !== undefined, "desktop file has a [Desktop Entry] group");
      check.equal(entry.group.get("Type"), "Application", "Type=Application");
      check.equal(entry.group.get("X-GNOME-Autostart-enabled"), "true", "X-GNOME-Autostart-enabled=true");
      check.ok(entry.group.get("Hidden") !== "true", "not Hidden");
    },
    disableLikeTheOs(check) {
      // What a desktop's startup manager writes to turn an entry off.
      fs.appendFileSync(file, "Hidden=true\n");
      check.equal(parseDesktopFile(read()).get("Desktop Entry").get("Hidden"), "true", "appended Hidden=true");
    },
    extraDisableCases(check, sevak) {
      // GNOME's own switch, and an XDG tombstone with only Hidden=true and no Exec.
      for (const [label, content] of [
        ["X-GNOME-Autostart-enabled=false", read().replace("X-GNOME-Autostart-enabled=true", "X-GNOME-Autostart-enabled=false")],
        ["a Hidden-only tombstone without Exec", "[Desktop Entry]\nHidden=true\n"],
      ]) {
        fs.writeFileSync(file, content);
        const status = sevak("--startup-status");
        check.equal([status.code, status.stdout.trim()], [1, "off"], `--startup-status reports off for ${label}`);
        const on = sevak("--set-startup", "on");
        check.equal(on.code, 0, `--set-startup on re-enables after ${label}`);
        check.equal(sevak("--startup-status").code, 0, `--startup-status is on again after ${label}`);
      }
    },
    absent: () => !fs.existsSync(file),
    cleanup() {
      fs.rmSync(file, { force: true });
    },
  };
}

// ---------------------------------------------------------------------------
// The scenario
// ---------------------------------------------------------------------------

export async function main(argv = process.argv.slice(2)) {
  const options = parseArgs(argv);
  const check = new Check();
  const binary = path.resolve(options.binary);
  if (!fs.existsSync(binary)) throw new Error(`no sevak binary at ${binary}; build it with \`cargo build -p sevak\``);

  const temp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "sevak-startup-cli-")));
  const config = path.join(temp, "profile dir", "config.toml");
  fs.mkdirSync(path.dirname(config), { recursive: true });
  const env = { ...process.env, SEVAK_DATA_DIR: path.join(temp, "data"), SEVAK_CONFIG_DIR: path.join(temp, "unused-config") };
  delete env.APPIMAGE;

  let platform;
  if (process.platform === "win32") platform = windowsPlatform();
  else if (process.platform === "darwin") platform = macPlatform();
  else if (process.platform === "linux") {
    if (!options.realHome) env.XDG_CONFIG_HOME = path.join(temp, "xdg-config");
    platform = linuxPlatform(env);
  } else throw new Error(`unsupported platform ${process.platform}`);

  const touchesRealStore = process.platform !== "linux" || options.realHome;
  if (touchesRealStore && process.env.CI !== "true" && !options.allowReal) {
    throw new Error(
      `${platform.name} is this user's real startup store. Run this in CI, or pass --allow-real-startup to accept a temporary Sevak entry (it is removed afterwards).`,
    );
  }
  const existing = platform.existing();
  if (existing.length > 0) {
    throw new Error(`a Sevak startup entry already exists (${existing.join(", ")}); refusing to overwrite it`);
  }

  const sevak = (...args) => run(binary, ["--config", config, ...args], { env, cwd: temp });
  const expectedArgs = (entry) => {
    check.ok(entry !== null, `${platform.name} entry exists`);
    check.ok(samePath(entry.args[0] ?? "", binary), "entry starts this binary", { args: entry.args, binary });
    check.equal(entry.args.slice(1, 3), ["--background", "--config"], "entry starts in the background with --config");
    check.ok(samePath(entry.args[3] ?? "", config), "entry passes the isolated config file", { args: entry.args, config });
    check.equal(entry.args.length, 4, "entry has no other arguments");
  };
  const status = (expected, message) => {
    const r = sevak("--startup-status");
    check.equal([r.code, r.stdout.trim()], expected ? [0, "on"] : [1, "off"], message);
  };

  console.log(`# ${binary}\n# ${platform.name}\n# config ${config}`);
  let clean = false;
  try {
    status(false, "--startup-status exits 1 and prints off before anything is registered");
    check.ok(platform.absent(), "no entry exists before the test");

    const on = sevak("--set-startup", "on");
    check.equal(on.code, 0, `--set-startup on exits 0 ${on.stderr.trim()}`);
    check.equal(launchAtLogin(fs.readFileSync(config, "utf8")), true, "config.toml now has launch_at_login = true");
    status(true, "--startup-status exits 0 and prints on");
    const entry = platform.entry();
    expectedArgs(entry);
    platform.checkEnabledEntry(check, entry);

    const refresh = sevak("--refresh-startup");
    check.equal(refresh.code, 0, "--refresh-startup exits 0");
    expectedArgs(platform.entry());

    platform.disableLikeTheOs(check);
    status(false, "--startup-status reports off for an entry the OS disabled");
    const receipt = path.join(temp, "receipt.txt");
    const withReceipt = sevak("--startup-status", "--startup-result", receipt);
    check.equal(withReceipt.code, 1, "--startup-status --startup-result exits 1 while disabled");
    check.equal(fs.readFileSync(receipt, "utf8"), "1\n", "--startup-result receipt holds 1");

    const reenable = sevak("--set-startup", "on");
    check.equal(reenable.code, 0, "an explicit --set-startup on exits 0 after an OS disable");
    status(true, "--startup-status is on again after the explicit opt-in");
    const reenabled = platform.entry();
    expectedArgs(reenabled);
    platform.checkEnabledEntry(check, reenabled);
    platform.afterReenable?.(check);
    platform.extraDisableCases?.(check, sevak);

    const off = sevak("--set-startup", "off");
    check.equal(off.code, 0, `--set-startup off exits 0 ${off.stderr.trim()}`);
    check.ok(platform.absent(), "--set-startup off removed the entry");
    check.equal(launchAtLogin(fs.readFileSync(config, "utf8")), false, "config.toml now has launch_at_login = false");
    status(false, "--startup-status exits 1 and prints off after --set-startup off");

    check.equal(sevak("--set-startup", "on").code, 0, "--set-startup on again");
    const remove = sevak("--remove-startup");
    check.equal(remove.code, 0, "--remove-startup exits 0");
    check.ok(platform.absent(), "--remove-startup removed this copy's entry");
    check.equal(launchAtLogin(fs.readFileSync(config, "utf8")), true, "--remove-startup keeps the saved preference");
    status(false, "--startup-status is off after --remove-startup");

    const bad = sevak("--set-startup", "maybe");
    check.equal(bad.code, 2, "--set-startup with a bad value exits 2");
    check.ok(platform.absent(), "a rejected command registers nothing");
  } finally {
    platform.cleanup();
    clean = platform.absent();
    platform.restore?.();
    fs.rmSync(temp, { recursive: true, force: true });
  }
  check.ok(clean, "cleanup left no startup entry behind");
  console.log(`# ${check.count} checks passed`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch((error) => {
    console.error(error.message ?? error);
    process.exit(1);
  });
}
