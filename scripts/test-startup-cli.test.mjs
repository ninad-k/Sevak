// Unit tests of the parsers the cross-OS startup CLI check relies on. The check
// itself (scripts/test-startup-cli.mjs) runs in CI against a built binary.
import assert from "node:assert/strict";
import test from "node:test";

import {
  launchAtLogin,
  launchctlDisabled,
  parseDesktopExec,
  parseDesktopFile,
  parseRegQuery,
  parseWindowsCommandLine,
} from "./test-startup-cli.mjs";

test("desktop entry groups keep the first key and ignore comments", () => {
  const groups = parseDesktopFile(
    "# c\n[Desktop Entry]\nType=Application\nExec=\"/a b/sevak\" \"--background\"\nHidden=true\nHidden=false\n[Other]\nType=x\n",
  );
  assert.equal(groups.get("Desktop Entry").get("Hidden"), "true");
  assert.equal(groups.get("Desktop Entry").get("Type"), "Application");
  assert.equal(groups.get("Other").get("Type"), "x");
});

// A port of `desktop_quote` in crates/sevak-platform/src/startup/files.rs.
function desktopQuote(value) {
  let quoted = '"';
  for (const ch of value) {
    if (ch === "%") quoted += "%%";
    else if ("\\\"$`".includes(ch)) quoted += `\\${ch}`;
    else quoted += ch;
  }
  return `${quoted}"`.replaceAll("\\", "\\\\");
}

test("Exec values are unescaped like Sevak writes them", () => {
  const args = ["/home/p/Sevak's \"100%\" $`\\.AppImage", "--background", "--config", "/tmp/profile dir/config.toml"];
  assert.deepEqual(parseDesktopExec(args.map(desktopQuote).join(" ")), args);
  assert.deepEqual(parseDesktopExec("sevak --toggle"), ["sevak", "--toggle"]);
  assert.throws(() => parseDesktopExec('"unterminated'));
});

test("launchctl print-disabled is read in both formats", () => {
  const modern = 'disabled services = {\n\t"com.apple.x" => enabled\n\t"Sevak" => disabled\n}\n';
  assert.equal(launchctlDisabled(modern, "Sevak"), true);
  assert.equal(launchctlDisabled(modern, "com.apple.x"), false);
  assert.equal(launchctlDisabled('\t"Sevak" => true\n', "Sevak"), true);
  assert.equal(launchctlDisabled('\t"Sevak" => false\n', "Sevak"), false);
  assert.equal(launchctlDisabled('\t"Sevak.helper" => disabled\n', "Sevak"), null);
  assert.equal(launchctlDisabled("", "Sevak"), null);
  assert.throws(() => launchctlDisabled('\t"Sevak" => maybe\n', "Sevak"));
});

test("reg query output gives the value type and data", () => {
  const output =
    "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n" +
    '    Sevak    REG_SZ    "C:\\Program Files\\Sevak\\sevak.exe" "--background"\r\n' +
    "    Other    REG_SZ    x\r\n\r\n";
  assert.deepEqual(parseRegQuery(output, "Sevak"), {
    type: "REG_SZ",
    data: '"C:\\Program Files\\Sevak\\sevak.exe" "--background"',
  });
  assert.deepEqual(parseRegQuery("    Sevak    REG_BINARY    030000000000000000000000\r\n", "Sevak"), {
    type: "REG_BINARY",
    data: "030000000000000000000000",
  });
  assert.equal(parseRegQuery(output, "Missing"), null);
});

test("Windows command lines split by CommandLineToArgvW rules", () => {
  assert.deepEqual(
    parseWindowsCommandLine('"C:\\a b\\sevak.exe" "--background" "--config" "C:\\p d\\config.toml"'),
    ["C:\\a b\\sevak.exe", "--background", "--config", "C:\\p d\\config.toml"],
  );
  // A trailing backslash is doubled before the closing quote (see windows.rs `quote`).
  assert.deepEqual(parseWindowsCommandLine('"C:\\dir\\\\" "a\\"b"'), ["C:\\dir\\", 'a"b']);
  assert.deepEqual(parseWindowsCommandLine("C:\\sevak.exe --background"), ["C:\\sevak.exe", "--background"]);
});

test("launch_at_login is read from the [general] table only", () => {
  assert.equal(launchAtLogin("[general]\nhotkey = 'x'\nlaunch_at_login = true # on\n"), true);
  assert.equal(launchAtLogin("[general]\nlaunch_at_login = false\n"), false);
  assert.equal(launchAtLogin("[other]\nlaunch_at_login = true\n"), null);
});
