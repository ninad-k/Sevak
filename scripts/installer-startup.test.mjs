// The Windows-only integration harness compiles the actual NSIS startup
// functions and runs them against a receipt-only fake executable. It never
// installs Sevak or writes a startup entry in the developer's profile.
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const template = readFileSync(join(root, "src-tauri/installer/installer.nsi"), "utf8");
const functionBody = (name) => {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const body = template.match(new RegExp(`^Function ${escaped}\\r?\\n[\\s\\S]*?^FunctionEnd`, "m"))?.[0];
  assert.ok(body, `missing ${name}`);
  return body;
};

test("startup is opt-in, per-user and selected before the installer elevates", () => {
  assert.match(functionBody("PageScope"), /NSD_CreateCheckbox.*sevakStartAtSignIn/);
  assert.match(functionBody("InitStartup"), /StrCpy \$StartupInitial 0/);
  assert.match(functionBody("PageLeaveScope"), /Call ResolveStartupChoice[\s\S]*Call RelaunchElevated/);
  assert.match(functionBody("RelaunchElevated"), /\/LOGININITIAL=\$StartupInitial[\s\S]*\/LOGINRESULT=/);
  assert.match(functionBody("RelaunchElevated"), /\/LOGINREGISTERED=\$StartupRegistered/);
  assert.match(functionBody("RelaunchElevated"), /\/AUTOSTART=\$StartupChoice/);
  assert.match(template, /RunAsUser.*\$StartupCommand --startup-result/);
  assert.doesNotMatch(template, /WriteReg\w+ (HKLM|SHCTX).*CurrentVersion\\Run/);
});

test("uninstall uses the executable-owned cleanup before file deletion and skips moves and updates", () => {
  const uninstall = template.match(/^Section Uninstall\r?\n[\s\S]*?^SectionEnd/m)?.[0];
  assert.ok(uninstall);
  assert.match(uninstall, /\$UpdateMode <> 1[\s\S]*\$MoveMode <> 1[\s\S]*--remove-startup/);
  assert.ok(uninstall.indexOf("Call un.RunStartupCommand") < uninstall.indexOf('Delete "$INSTDIR\\${MAINBINARYNAME}.exe"'));
  assert.doesNotMatch(uninstall, /DeleteRegValue.*CurrentVersion\\Run/);
  assert.match(functionBody("un.RelaunchElevated"), /\/LOGINRESULT=/);
});

test("an unchanged upgrade does not overwrite Windows' startup decision", () => {
  assert.match(functionBody("InitStartup"), /StartupApproved\\Run/);
  assert.match(functionBody("InitStartup"), /\$2 <> 2[\s\S]*\$2 <> 6[\s\S]*StrCpy \$StartupInitial 0/);
  assert.doesNotMatch(functionBody("InitStartup"), /IntOp \$2 \$2 & 255/);
  const install = template.match(/^Section Install\r?\n[\s\S]*?^SectionEnd/m)?.[0];
  assert.ok(install);
  assert.match(install, /Call SelectStartupCommand[\s\S]*\$StartupCommand != ""[\s\S]*Call RunStartupCommand/);
  assert.match(functionBody("SelectStartupCommand"), /--refresh-startup/);
  assert.doesNotMatch(functionBody("SelectStartupCommand"), /\$StartupInitial/);
});

const nsisHome = join(process.env.LOCALAPPDATA || "", "tauri", "NSIS");
const makensis = process.env.MAKENSIS_PATH || join(nsisHome, "makensis.exe");
const plugins = join(nsisHome, "Plugins", "x86-unicode", "additional");
const canCompile = process.platform === "win32" && existsSync(makensis);
const canRunHelper = canCompile && existsSync(join(plugins, "nsis_tauri_utils.dll"));
// NSIS expands $ inside quoted strings; retain it literally in filesystem paths.
const nsisQuote = (value) => value.replaceAll("$", "$$");
const run = (exe, args = []) => {
  const result = spawnSync(exe, args, { encoding: "utf8", windowsHide: true, timeout: 30_000 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `${exe}\n${result.stdout}\n${result.stderr}`);
};
const compile = (folder, name, source) => {
  const script = join(folder, `${name}.nsi`);
  const output = join(folder, `${name}.exe`);
  writeFileSync(script, `Unicode true\nRequestExecutionLevel user\nSilentInstall silent\nOutFile "${nsisQuote(output)}"\n!include LogicLib.nsh\n!include FileFunc.nsh\n${source}`);
  run(makensis, ["/V2", script]);
  return output;
};

test("compiled NSIS accepts only complete known StartupApproved DWORD states", { skip: !canCompile && "requires the Windows Tauri NSIS toolchain" }, () => {
  const folder = mkdtempSync(join(tmpdir(), "sevak-startup-approval-"));
  try {
    const predicate = functionBody("InitStartup").match(/\$\{If\} \$2 <> 2[\s\S]*?\$\{EndIf\}/)?.[0];
    assert.ok(predicate);
    const cases = [[2, 1], [6, 1], [3, 0], [7, 0], [0, 0], [99, 0], [258, 0], [262, 0], [268435458, 0]];
    const checks = cases.map(([state, enabled]) => `
      StrCpy $2 ${state}
      Call TestApproval
      StrCmp $StartupInitial ${enabled} +3
      SetErrorLevel 1
      Quit
    `).join("\n");
    const exe = compile(folder, "approval", `
      Var StartupInitial
      Function TestApproval
        StrCpy $StartupInitial 1
        ${predicate}
      FunctionEnd
      Section
        ${checks}
      SectionEnd
    `);
    run(exe);
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
});

test("compiled NSIS honors fresh, upgrade, explicit CLI and backtracked checkbox choices", { skip: !canCompile && "requires the Windows Tauri NSIS toolchain" }, () => {
  const folder = mkdtempSync(join(tmpdir(), "sevak-startup-policy-"));
  try {
    const cases = [
      ["fresh opt-out overwrites an old saved preference", 0, 0, 0, 1, 0, "", "off"],
      ["fresh opt-in", 0, 1, 0, 1, 0, "", "on"],
      ["upgrade remains on", 1, 1, 0, 0, 1, "", ""],
      ["upgrade preserves Windows-disabled entry", 0, 0, 0, 0, 1, "", ""],
      ["upgrade unchecked missing entry clears saved true", 0, 0, 0, 0, 0, "", "off"],
      ["upgrade opt-out", 1, 0, 0, 0, 1, "", "off"],
      ["explicit enable", 1, 1, 1, 0, 1, "on", "on"],
      ["explicit disable", 0, 0, 1, 0, 1, "off", "off"],
      ["back then unchecked preserves disabled entry", 0, 0, 0, 0, 1, "on", ""],
      ["back then unchecked with missing entry saves off", 0, 0, 0, 0, 0, "on", "off"],
      ["back then checked", 1, 1, 0, 0, 1, "off", ""],
      ["override CLI on", 0, 0, 1, 0, 0, "on", "off"],
      ["override CLI off", 1, 1, 1, 0, 1, "off", "on"],
    ];
    const checks = cases.map(([name, initial, selected, explicit, fresh, registered, prior, expected]) => `
      StrCpy $StartupInitial ${initial}
      StrCpy $StartupSelected ${selected}
      StrCpy $StartupExplicit ${explicit}
      StrCpy $StartupFresh ${fresh}
      StrCpy $StartupRegistered ${registered}
      StrCpy $StartupChoice "${prior}"
      Call ResolveStartupChoice
      StrCmp $StartupChoice "${expected}" +3
      SetErrorLevel 1
      Quit ; ${name}
    `).join("\n");
    const exe = compile(folder, "policy", `
      Var StartupInitial
      Var StartupSelected
      Var StartupExplicit
      Var StartupFresh
      Var StartupRegistered
      Var StartupChoice
      ${functionBody("ResolveStartupChoice")}
      Section
        ${checks}
      SectionEnd
    `);
    run(exe);
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
});

test("compiled NSIS refreshes changed paths without creating or reenabling startup", { skip: !canCompile && "requires the Windows Tauri NSIS toolchain" }, () => {
  const folder = mkdtempSync(join(tmpdir(), "sevak-startup-moves-"));
  try {
    const cases = [
      ["in-place upgrade", "", "installed", "same", 0, "", "", ""],
      ["explicit new directory", "", "installed", "old", 0, "", "", "--refresh-startup"],
      ["move scopes", "", "", "", 1, "installed", "old", "--refresh-startup"],
      ["keep both", "", "", "", 0, "installed", "other", ""],
      ["first install silent preserves", "", "", "", 0, "", "", ""],
      ["stale directory without installation", "", "", "old", 0, "", "", ""],
      ["explicit enable wins", "on", "installed", "old", 0, "", "", "--set-startup on"],
      ["explicit disable wins", "off", "", "", 1, "installed", "old", "--set-startup off"],
    ];
    const checks = cases.map(([name, choice, same, sameDir, remove, other, otherDir, expected]) => `
      StrCpy $StartupChoice "${choice}"
      StrCpy $SameUninst "${same}"
      StrCpy $SameDir "${sameDir}"
      StrCpy $RemoveOther ${remove}
      StrCpy $OtherUninst "${other}"
      StrCpy $OtherDir "${otherDir}"
      Call SelectStartupCommand
      StrCmp $StartupCommand "${expected}" +3
      SetErrorLevel 1
      Quit ; ${name}
    `).join("\n");
    const exe = compile(folder, "moves", `
      Var StartupChoice
      Var StartupCommand
      Var SameUninst
      Var SameDir
      Var RemoveOther
      Var OtherUninst
      Var OtherDir
      ${functionBody("SelectStartupCommand")}
      Section
        StrCpy $INSTDIR "same"
        ${checks}
      SectionEnd
    `);
    run(exe);
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
});

test("compiled NSIS parses explicit and elevated startup options without enrolling a profile", { skip: !canCompile && "requires the Windows Tauri NSIS toolchain" }, () => {
  const folder = mkdtempSync(join(tmpdir(), "sevak-startup-options-"));
  try {
    const resultPath = join(folder, "parsed.txt");
    const exe = compile(folder, "options", `
      !include x64.nsh
      !define PRODUCTNAME "Sevak"
      Var StartupInitial
      Var StartupRegistered
      Var StartupExplicit
      Var StartupChoice
      Var StartupResultFile
      Function InitStartupResult
        StrCpy $StartupResultFile "" ; prevent all registry reads in this fixture
      FunctionEnd
      ${functionBody("InitStartup")}
      Function .onInit
        Call InitStartup
      FunctionEnd
      Section
        FileOpen $0 "${nsisQuote(resultPath)}" w
        FileWrite $0 "$StartupChoice|$StartupExplicit|$StartupInitial|$StartupRegistered"
        FileClose $0
      SectionEnd
    `);
    for (const [args, expected] of [
      [[], "|0|0|0"],
      [["/AUTOSTART=on"], "on|1|0|0"],
      [["/AUTOSTART=off"], "off|1|0|0"],
      [["/AUTOSTART=ON"], "on|1|0|0"],
      [["/AUTOSTART=OFF"], "off|1|0|0"],
      [["/LOGININITIAL=1", "/LOGINREGISTERED=1"], "|0|1|1"],
      [["/AUTOSTART=off", "/LOGININITIAL=1", "/LOGINREGISTERED=1"], "off|1|1|1"],
      [["/LOGININITIAL=0", "/LOGINREGISTERED=1", "/ELEVATED", "/ALLUSERS"], "|0|0|1"],
      [["/AUTOSTART=on", "/LOGININITIAL=0", "/LOGINREGISTERED=0", "/ELEVATED", "/ALLUSERS"], "on|1|0|0"],
    ]) {
      run(exe, args);
      assert.equal(readFileSync(resultPath, "utf8"), expected, args.join(" "));
    }
    for (const arg of ["/AUTOSTART=invalid", "/AUTOSTART="]) {
      const invalid = spawnSync(exe, [arg], { windowsHide: true, timeout: 10_000 });
      assert.ifError(invalid.error);
      assert.equal(invalid.status, 1639, "invalid startup options fail before installation");
    }
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
});

test("compiled NSIS waits for the original user's helper result and detects failure", { skip: !canRunHelper && "requires Windows NSIS and nsis_tauri_utils", timeout: 45_000 }, () => {
  const folder = mkdtempSync(join(tmpdir(), "sevak-startup-receipt-"));
  try {
    compile(folder, "startup-helper", `
      Var Receipt
      Var Result
      Function .onInit
        \${GetOptions} $CMDLINE "--startup-result " $Receipt
        \${GetOptions} $CMDLINE "--mock-code=" $Result
        \${If} $Result != "none"
          Sleep 150 ; prove the installer waits rather than assuming spawn means success
          FileOpen $0 "$Receipt" w
          FileWrite $0 "$Result$\\n"
          FileClose $0
        \${EndIf}
        Quit
      FunctionEnd
      Section
      SectionEnd
    `);
    const macro = template.match(/^!macro SevakStartupFunctions PREFIX\r?\n[\s\S]*?^!macroend/m)?.[0];
    assert.ok(macro);
    const cases = [["0", 0], ["1", 1], ["2", 2], ["none", 2]];
    const checks = cases.map(([code, expected]) => `
      StrCpy $StartupCommand "--mock-code=${code}"
      Call RunStartupCommand
      StrCmp $StartupCommandResult ${expected} +3
      SetErrorLevel 1
      Quit
    `).join("\n");
    const exe = compile(folder, "receipts", `
      !addplugindir "${nsisQuote(plugins)}"
      !define MAINBINARYNAME "startup-helper"
      Var StartupResultFile
      Var StartupCommand
      Var StartupCommandResult
      Var IsElevated
      ${macro}
      !insertmacro SevakStartupFunctions ""
      Function CheckElevated
        StrCpy $IsElevated 0
      FunctionEnd
      Section
        StrCpy $INSTDIR "${nsisQuote(folder)}"
        StrCpy $StartupResultFile "${nsisQuote(join(folder, "receipt.txt"))}"
        ${checks}
        StrCpy $StartupResultFile ""
        Call RunStartupCommand
        StrCmp $StartupCommandResult 2 +3
        SetErrorLevel 1
        Quit
      SectionEnd
    `);
    run(exe);
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
});
