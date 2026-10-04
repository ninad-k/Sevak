; ============================================================================
; Sevak's copy of Tauri's NSIS installer template (bundle.windows.nsis.template).
;
; Based on tauri-bundler 2.10.1 (Tauri CLI 2.12.1): crates/tauri-bundler/src/
; bundle/windows/nsis/installer.nsi, Apache-2.0 OR MIT, (c) Tauri Programme
; within The Commons Conservancy. The unmodified original is kept next to this
; file in upstream/installer.nsi; `diff upstream/installer.nsi installer.nsi`
; shows everything Sevak changed. Every change is between `SEVAK:` comments.
; Re-sync after a Tauri upgrade: docs/development.md, "The Windows installer".
;
; Why a copy at all: Tauri's `installMode: "both"` is built on MultiUser.nsh with
; RequestExecutionLevel highest, so every administrator gets a UAC prompt even
; for a per-user install, silent installs and the in-app updater default to
; per-machine for them, and updates of a per-user copy would prompt too. Sevak
; wants per-user to need no administrator, ever, and to ask for elevation only
; when "all users" is chosen. That is not reachable through the config options.
; ============================================================================
Unicode true
ManifestDPIAware true
; Add in `dpiAwareness` `PerMonitorV2` to manifest for Windows 10 1607+ (note this should not affect lower versions since they should be able to ignore this and pick up `dpiAware` `true` set by `ManifestDPIAware true`)
; Currently undocumented on NSIS's website but is in the Docs folder of source tree, see
; https://github.com/kichik/nsis/blob/5fc0b87b819a9eec006df4967d08e522ddd651c9/Docs/src/attributes.but#L286-L300
; https://github.com/tauri-apps/tauri/pull/10106
ManifestDPIAwareness PerMonitorV2

!if "{{compression}}" == "none"
  SetCompress off
!else
  ; Set the compression algorithm. We default to LZMA.
  SetCompressor /SOLID "{{compression}}"
!endif

; Keep above !include to stay ahead of any plugin command
; see https://github.com/tauri-apps/tauri/pull/15422#discussion_r3289239624
{{#if signed_plugins_path}}
!addplugindir "{{signed_plugins_path}}"
{{/if}}

!include MUI2.nsh
!include FileFunc.nsh
!include x64.nsh
!include WordFunc.nsh
!include "utils.nsh"
!include "FileAssociation.nsh"
!include "Win\COM.nsh"
!include "Win\Propkey.nsh"
!include "Win\RestartManager.nsh"
!include "StrFunc.nsh"
${StrCase}
${StrLoc}

{{#if installer_hooks}}
!include "{{installer_hooks}}"
{{/if}}

!define WEBVIEW2APPGUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

!define MANUFACTURER "{{manufacturer}}"
!define PRODUCTNAME "{{product_name}}"
!define VERSION "{{version}}"
!define VERSIONWITHBUILD "{{version_with_build}}"
!define HOMEPAGE "{{homepage}}"
!define INSTALLMODE "{{install_mode}}"
!define LICENSE "{{license}}"
!define INSTALLERICON "{{installer_icon}}"
!define SIDEBARIMAGE "{{sidebar_image}}"
!define HEADERIMAGE "{{header_image}}"
!define UNINSTALLERICON "{{uninstaller_icon}}"
!define UNINSTALLERHEADERIMAGE "{{uninstaller_header_image}}"
!define MAINBINARYNAME "{{main_binary_name}}"
!define MAINBINARYSRCPATH "{{main_binary_path}}"
!define BUNDLEID "{{bundle_id}}"
!define COPYRIGHT "{{copyright}}"
!define OUTFILE "{{out_file}}"
!define ARCH "{{arch}}"
!define ADDITIONALPLUGINSPATH "{{additional_plugins_path}}"
!define ALLOWDOWNGRADES "{{allow_downgrades}}"
!define DISPLAYLANGUAGESELECTOR "{{display_language_selector}}"
!define INSTALLWEBVIEW2MODE "{{install_webview2_mode}}"
!define WEBVIEW2INSTALLERARGS "{{webview2_installer_args}}"
!define WEBVIEW2BOOTSTRAPPERPATH "{{webview2_bootstrapper_path}}"
!define WEBVIEW2INSTALLERPATH "{{webview2_installer_path}}"
!define MINIMUMWEBVIEW2VERSION "{{minimum_webview2_version}}"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
!define MANUKEY "Software\${MANUFACTURER}"
!define MANUPRODUCTKEY "${MANUKEY}\${PRODUCTNAME}"
!define UNINSTALLERSIGNCOMMAND "{{uninstaller_sign_cmd}}"
!define ESTIMATEDSIZE "{{estimated_size}}"
!define STARTMENUFOLDER "{{start_menu_folder}}"

Var PassiveMode
Var UpdateMode
Var NoShortcutMode
Var WixMode
Var OldMainBinaryName

; SEVAK: install scope and what is already installed.
;   "user"    one account, HKCU, %LOCALAPPDATA%\Sevak, no administrator needed
;   "machine" all users, HKLM, Program Files, administrator needed
Var InstallScope
Var ScopeExplicit      ; 1 when /ALLUSERS or /CURRENTUSER was given
Var IsElevated         ; 1 when this process runs with administrator rights
Var ElevatedMode       ; 1 in the copy started by RelaunchElevated: skip Welcome, License, scope
Var MoveMode           ; 1 in an uninstaller started because Sevak moves to the other scope
Var RemoveOther        ; 1 when the copy in the other scope is to be removed
Var CustomInstDir      ; 1 when /D= chose the folder
Var CompareResult      ; this installer's version against the installed one: 1 newer, 0 same, -1 older, 2 unknown
Var UserUninst         ; per-user copy: UninstallString, DisplayVersion, folder
Var UserVer
Var UserDir
Var MachUninst         ; per-machine copy
Var MachVer
Var MachDir
Var SameUninst         ; the copy in the chosen scope ...
Var SameVer
Var SameDir
Var SameScopeName
Var OtherUninst        ; ... and the one in the other scope
Var OtherVer
Var OtherDir
Var OtherScopeName
Var WixKey             ; registry subkey of an earlier MSI install, when there is one
; SEVAK end

Name "${PRODUCTNAME}"
BrandingText "${COPYRIGHT}"
OutFile "${OUTFILE}"

; We don't actually use this value as default install path,
; it's just for nsis to append the product name folder in the directory selector
; https://nsis.sourceforge.io/Reference/InstallDir
!define PLACEHOLDER_INSTALL_DIR "placeholder\${PRODUCTNAME}"
InstallDir "${PLACEHOLDER_INSTALL_DIR}"

VIProductVersion "${VERSIONWITHBUILD}"
VIAddVersionKey "ProductName" "${PRODUCTNAME}"
VIAddVersionKey "FileDescription" "${PRODUCTNAME}"
VIAddVersionKey "LegalCopyright" "${COPYRIGHT}"
; SEVAK: the publisher shows in the file's Details tab
VIAddVersionKey "CompanyName" "${MANUFACTURER}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

# additional plugins
!addplugindir "${ADDITIONALPLUGINSPATH}"

; Uninstaller signing command
!if "${UNINSTALLERSIGNCOMMAND}" != ""
  !uninstfinalize '${UNINSTALLERSIGNCOMMAND}'
!endif

; Handle install mode, `perUser`, `perMachine` or `both`
!if "${INSTALLMODE}" == "perMachine"
  RequestExecutionLevel admin
!endif

!if "${INSTALLMODE}" == "currentUser"
  RequestExecutionLevel user
!endif

; SEVAK: "both" without MultiUser.nsh. The installer starts without elevation
; (so a per-user install and every silent per-user run never prompt) and
; RelaunchElevated starts it again with a UAC prompt when "all users" is
; chosen. The scope logic is in the Sevak functions below.
!if "${INSTALLMODE}" == "both"
  RequestExecutionLevel user
!endif
; SEVAK end

; Installer icon
!if "${INSTALLERICON}" != ""
  !define MUI_ICON "${INSTALLERICON}"
!endif

; Installer sidebar image
!if "${SIDEBARIMAGE}" != ""
  !define MUI_WELCOMEFINISHPAGE_BITMAP "${SIDEBARIMAGE}"
!endif

; Enable header images for installer and uninstaller pages when either image is configured.
!if "${HEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE
!else if "${UNINSTALLERHEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE
!endif

; Installer header image
!if "${HEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE_BITMAP "${HEADERIMAGE}"
!endif

; Uninstaller header image
!if "${UNINSTALLERHEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE_UNBITMAP "${UNINSTALLERHEADERIMAGE}"
!endif

; Uninstaller icon
!if "${UNINSTALLERICON}" != ""
  !define MUI_UNICON "${UNINSTALLERICON}"
!endif

; Define registry key to store installer language
!define MUI_LANGDLL_REGISTRY_ROOT "HKCU"
!define MUI_LANGDLL_REGISTRY_KEY "${MANUPRODUCTKEY}"
!define MUI_LANGDLL_REGISTRY_VALUENAME "Installer Language"

; SEVAK: Modern UI colours are the app's light theme (ui/src: --bg and --fg), so the
; page header, Welcome and Finish pages match the app. Not a dark page: NSIS cannot
; recolour the text of the system-drawn check boxes (NSIS bug #443), which would
; leave them unreadable on dark.
!define MUI_BGCOLOR "FCFCFD"
!define MUI_TEXTCOLOR "1C1B2E"
; The header artwork (src-tauri/installer/header.bmp) is drawn for the right-hand
; side, with the page title on the left.
!define MUI_HEADERIMAGE_RIGHT
; SEVAK end

; Installer pages, must be ordered as they appear
; 1. Welcome Page
; SEVAK: Sevak's own words. Skipped in the elevated copy, which resumes after the scope page.
!define MUI_WELCOMEPAGE_TITLE "$(sevakWelcomeTitle)"
!define MUI_WELCOMEPAGE_TEXT "$(sevakWelcomeText)"
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassiveOrElevated
!insertmacro MUI_PAGE_WELCOME

; 2. License Page (if defined)
!if "${LICENSE}" != ""
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassiveOrElevated
  !insertmacro MUI_PAGE_LICENSE "${LICENSE}"
!endif

; 3. SEVAK: who is Sevak for? Replaces MultiUser's install mode page.
;    "Install for me only" is the default and needs no administrator. "All users"
;    restarts the installer with a UAC prompt (RelaunchElevated); the elevated copy
;    skips this page and carries on from the next one.
Var ScopePageUser
Var ScopePageMachine
Page custom PageScope PageLeaveScope
Function PageScope
  ; Nothing to ask in silent and passive runs, or in the elevated copy.
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
  ${OrIf} $ElevatedMode = 1
    Abort
  ${EndIf}

  !insertmacro MUI_HEADER_TEXT "$(sevakScopeTitle)" "$(sevakScopeSubtitle)"
  nsDialogs::Create 1018
  Pop $0
  ${IfThen} $0 == error ${|} Abort ${|}
  ${IfThen} $(^RTL) = 1 ${|} nsDialogs::SetRTL $(^RTL) ${|}

  ${NSD_CreateLabel} 0 0 100% 20u "$(sevakScopeIntro)"
  Pop $0
  ${NSD_CreateFirstRadioButton} 12u 30u -12u 24u "$(sevakScopeUser)"
  Pop $ScopePageUser
  ${NSD_CreateAdditionalRadioButton} 12u 62u -12u 24u "$(sevakScopeMachine)"
  Pop $ScopePageMachine

  ${If} $InstallScope == "machine"
    SendMessage $ScopePageMachine ${BM_SETCHECK} ${BST_CHECKED} 0
    ${NSD_SetFocus} $ScopePageMachine
  ${Else}
    SendMessage $ScopePageUser ${BM_SETCHECK} ${BST_CHECKED} 0
    ${NSD_SetFocus} $ScopePageUser
  ${EndIf}
  nsDialogs::Show
FunctionEnd
Function PageLeaveScope
  ${NSD_GetState} $ScopePageMachine $0
  ${If} $0 = ${BST_CHECKED}
    StrCpy $InstallScope "machine"
    Call CheckElevated
    ${If} $IsElevated <> 1
      ; "All users" needs administrator rights: run this installer again with them.
      Call RelaunchElevated
      ${If} $0 = 1
        Quit
      ${EndIf}
      ; Refused or failed: stay on this page so "for me only" can be chosen.
      MessageBox MB_ICONEXCLAMATION "$(sevakNeedAdmin)"
      Abort
    ${EndIf}
  ${Else}
    StrCpy $InstallScope "user"
  ${EndIf}
  Call SetScopeDefaults
FunctionEnd

; 4. SEVAK: Sevak is already installed. Shown when Sevak exists in the chosen scope
;    (upgrade, reinstall or downgrade, or uninstall) or only in the other one (move
;    it here, or keep both). Silent and passive runs never show it; their choices are
;    the command line switches.
;    Upstream's page uninstalled the old copy before installing. Here an upgrade
;    installs over the old files, which is what the in-app updater has always done;
;    settings and data live in %APPDATA%\sevak and are never touched.
Var ReinstallPageKind   ; "same" or "other"
Var ReinstallRadio1
Var ReinstallRadio2
Var ReinstallRemoveBox
Var ReinstallText
Var ReinstallText1
Var ReinstallText2
Page custom PageReinstall PageLeaveReinstall
Function PageReinstall
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    Abort
  ${EndIf}

  ${If} $SameUninst != ""
    StrCpy $ReinstallPageKind "same"
    StrCpy $ReinstallText2 "$(sevakUninstallButton)"
    ${If} $CompareResult = 1
      StrCpy $ReinstallText "$(sevakUpgradeText)"
      StrCpy $ReinstallText1 "$(sevakUpgradeButton)"
    ${ElseIf} $CompareResult = 0
      StrCpy $ReinstallText "$(sevakReinstallText)"
      StrCpy $ReinstallText1 "$(sevakReinstallButton)"
    ${ElseIf} $CompareResult = -1
      StrCpy $ReinstallText "$(sevakDowngradeText)"
      StrCpy $ReinstallText1 "$(sevakDowngradeButton)"
    ${Else}
      StrCpy $ReinstallText "$(sevakUnknownText)"
      StrCpy $ReinstallText1 "$(sevakUpgradeButton)"
    ${EndIf}
  ${ElseIf} $OtherUninst != ""
    StrCpy $ReinstallPageKind "other"
    StrCpy $ReinstallText "$(sevakOtherText)"
    StrCpy $ReinstallText1 "$(sevakMoveButton)"
    StrCpy $ReinstallText2 "$(sevakKeepBothButton)"
  ${Else}
    Abort ; nothing installed
  ${EndIf}

  !insertmacro MUI_HEADER_TEXT "$(sevakInstalledTitle)" "$(sevakInstalledSubtitle)"
  nsDialogs::Create 1018
  Pop $0
  ${IfThen} $0 == error ${|} Abort ${|}
  ${IfThen} $(^RTL) = 1 ${|} nsDialogs::SetRTL $(^RTL) ${|}

  ${NSD_CreateLabel} 0 0 100% 48u "$ReinstallText"
  Pop $0
  ${NSD_CreateFirstRadioButton} 12u 56u -12u 10u "$ReinstallText1"
  Pop $ReinstallRadio1
  ${NSD_CreateAdditionalRadioButton} 12u 72u -12u 10u "$ReinstallText2"
  Pop $ReinstallRadio2

  ; A copy in the other scope as well: offer to remove it while upgrading this one.
  StrCpy $ReinstallRemoveBox ""
  ${If} $ReinstallPageKind == "same"
  ${AndIf} $OtherUninst != ""
    ${NSD_CreateCheckbox} 12u 94u -12u 10u "$(sevakAlsoRemove)"
    Pop $ReinstallRemoveBox
  ${EndIf}

  SendMessage $ReinstallRadio1 ${BM_SETCHECK} ${BST_CHECKED} 0
  ${NSD_SetFocus} $ReinstallRadio1
  nsDialogs::Show
FunctionEnd
Function PageLeaveReinstall
  ${NSD_GetState} $ReinstallRadio1 $0 ; 1: the first choice (upgrade, reinstall, downgrade or move)

  ${If} $ReinstallPageKind == "other"
    StrCpy $RemoveOther 0
    ${If} $0 = 1
      StrCpy $RemoveOther 1
    ${EndIf}
    Return
  ${EndIf}

  ${If} $0 = 1
    StrCpy $RemoveOther 0
    ${If} $ReinstallRemoveBox != ""
      ${NSD_GetState} $ReinstallRemoveBox $1
      ${If} $1 = 1
        StrCpy $RemoveOther 1
      ${EndIf}
    ${EndIf}
    Return
  ${EndIf}

  ; Uninstall. The uninstaller shows its own pages (including the cache option).
  HideWindow
  ClearErrors
  ExecWait '$SameUninst _?=$SameDir' $0
  BringToFront
  ${IfThen} ${Errors} ${|} StrCpy $0 2 ${|} ; ExecWait failed, set fake exit code

  ${If} $0 <> 0
  ${OrIf} ${FileExists} "$SameDir\${MAINBINARYNAME}.exe"
    ; Cancelled in the uninstaller? Back to this page.
    ${If} $0 = 1
      Abort
    ${EndIf}
    MessageBox MB_ICONEXCLAMATION "$(unableToUninstall)"
    Abort
  ${EndIf}

  ; Run in place (_?=), the uninstaller cannot delete itself: finish the job, and
  ; there is nothing left to install.
  Delete "$SameDir\uninstall.exe"
  RMDir "$SameDir"
  Quit
FunctionEnd

; 5. Choose install directory page
; SEVAK: not when upgrading an existing copy in place: it stays where it is.
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassiveOrInstalled
!insertmacro MUI_PAGE_DIRECTORY

; 6. Start menu shortcut page
Var AppStartMenuFolder
!if "${STARTMENUFOLDER}" != ""
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !define MUI_STARTMENUPAGE_DEFAULTFOLDER "${STARTMENUFOLDER}"
!else
  !define MUI_PAGE_CUSTOMFUNCTION_PRE Skip
!endif
!insertmacro MUI_PAGE_STARTMENU Application $AppStartMenuFolder

; 7. Installation page
!insertmacro MUI_PAGE_INSTFILES

; 8. Finish page
;
; Don't auto jump to finish page after installation page,
; because the installation page has useful info that can be used debug any issues with the installer.
!define MUI_FINISHPAGE_NOAUTOCLOSE
; SEVAK: three choices instead of upstream's two. "Start Sevak now" is checked; the
; "readme" slot becomes "Open Sevak Settings"; the desktop shortcut that used to
; take that slot is a third check box added in FinishShow.
!define MUI_FINISHPAGE_TITLE "$(sevakFinishTitle)"
!define MUI_FINISHPAGE_TEXT "$(sevakFinishText)"
!define MUI_FINISHPAGE_TEXT_LARGE
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "$(sevakOpenSettings)"
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION OpenSettings
; Show run app after installation.
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "$(sevakRunNow)"
!define MUI_FINISHPAGE_RUN_FUNCTION RunMainBinary
!define MUI_PAGE_CUSTOMFUNCTION_SHOW FinishShow
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE FinishLeave
; SEVAK end
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_FINISH

Function RunMainBinary
  nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" ""
FunctionEnd

; SEVAK: Finish page extras
Function OpenSettings
  nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" "--settings"
FunctionEnd
Var DesktopShortcutBox
Function FinishShow
  ; A third check box under the two the Modern UI draws (at 110u and 130u with the
  ; large text area; its own boxes are 20u apart).
  ${NSD_CreateCheckbox} 120u 150u 195u 10u "$(sevakDesktopShortcut)"
  Pop $DesktopShortcutBox
  SetCtlColors $DesktopShortcutBox "${MUI_TEXTCOLOR}" "${MUI_BGCOLOR}"
FunctionEnd
Function FinishLeave
  ${NSD_GetState} $DesktopShortcutBox $0
  ${If} $0 = ${BST_CHECKED}
    Call CreateOrUpdateDesktopShortcut
  ${EndIf}
FunctionEnd
; SEVAK end

; Uninstaller Pages
; 1. Confirm uninstall page
Var DeleteAppDataCheckbox
Var DeleteAppDataCheckboxState
!define /ifndef WS_EX_LAYOUTRTL         0x00400000
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.ConfirmShow
Function un.ConfirmShow ; Add add a `Delete app data` check box
  ; $1 inner dialog HWND
  ; $2 window DPI
  ; $3 style
  ; $4 x
  ; $5 y
  ; $6 width
  ; $7 height
  FindWindow $1 "#32770" "" $HWNDPARENT ; Find inner dialog
  System::Call "user32::GetDpiForWindow(p r1) i .r2"
  ${If} $(^RTL) = 1
    StrCpy $3 "${__NSD_CheckBox_EXSTYLE} | ${WS_EX_LAYOUTRTL}"
    IntOp $4 50 * $2
  ${Else}
    StrCpy $3 "${__NSD_CheckBox_EXSTYLE}"
    IntOp $4 0 * $2
  ${EndIf}
  IntOp $5 100 * $2
  IntOp $6 400 * $2
  IntOp $7 25 * $2
  IntOp $4 $4 / 96
  IntOp $5 $5 / 96
  IntOp $6 $6 / 96
  IntOp $7 $7 / 96
  System::Call 'user32::CreateWindowEx(i r3, w "${__NSD_CheckBox_CLASS}", w "$(deleteAppData)", i ${__NSD_CheckBox_STYLE}, i r4, i r5, i r6, i r7, p r1, i0, i0, i0) i .s'
  Pop $DeleteAppDataCheckbox
  SendMessage $HWNDPARENT ${WM_GETFONT} 0 0 $1
  SendMessage $DeleteAppDataCheckbox ${WM_SETFONT} $1 1
FunctionEnd
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE un.ConfirmLeave
Function un.ConfirmLeave
  SendMessage $DeleteAppDataCheckbox ${BM_GETCHECK} 0 0 $DeleteAppDataCheckboxState
FunctionEnd
!define MUI_PAGE_CUSTOMFUNCTION_PRE un.SkipIfPassive
!insertmacro MUI_UNPAGE_CONFIRM

; 2. Uninstalling Page
!insertmacro MUI_UNPAGE_INSTFILES

;Languages
{{#each languages}}
!insertmacro MUI_LANGUAGE "{{this}}"
{{/each}}
!insertmacro MUI_RESERVEFILE_LANGDLL
{{#each language_files}}
  !include "{{this}}"
{{/each}}

Function .onInit
  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/NS" $NoShortcutMode
  ${IfNot} ${Errors}
    StrCpy $NoShortcutMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}

  ; SEVAK: /ELEVATED marks the copy RelaunchElevated started; /UNINSTALLOTHER (silent
  ; and passive runs) removes the copy installed in the other scope. Switch names
  ; must not begin with one of the others ("/R" would match "/REMOVE...").
  ${GetOptions} $CMDLINE "/ELEVATED" $ElevatedMode
  ${IfNot} ${Errors}
    StrCpy $ElevatedMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/UNINSTALLOTHER" $RemoveOther
  ${IfNot} ${Errors}
    StrCpy $RemoveOther 1
  ${Else}
    StrCpy $RemoveOther 0
  ${EndIf}
  ; SEVAK end

  !if "${DISPLAYLANGUAGESELECTOR}" == "true"
    !insertmacro MUI_LANGDLL_DISPLAY
  !endif

  !insertmacro SetContext

  ; SEVAK: /D= wins over every default below
  ${If} $INSTDIR != "${PLACEHOLDER_INSTALL_DIR}"
    StrCpy $CustomInstDir 1
  ${EndIf}
  ; SEVAK end

  ${If} $INSTDIR == "${PLACEHOLDER_INSTALL_DIR}"
    ; Set default install location
    !if "${INSTALLMODE}" == "perMachine"
      ${If} ${RunningX64}
        !if "${ARCH}" == "x64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else if "${ARCH}" == "arm64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else
          StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
        !endif
      ${Else}
        StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
      ${EndIf}
    !else if "${INSTALLMODE}" == "currentUser"
      StrCpy $INSTDIR "$LOCALAPPDATA\${PRODUCTNAME}"
    !endif

    Call RestorePreviousInstallLocation
  ${EndIf}

  ; SEVAK: where is Sevak installed, which scope is this run for, and does that
  ; need administrator rights? (Upstream called MULTIUSER_INIT here.)
  !if "${INSTALLMODE}" == "both"
    Call DetectWix
    Call DetectInstalls
    Call ChooseInitialScope
    Call SetScopeDefaults
    Call CheckElevated
    ${If} $InstallScope == "machine"
    ${AndIf} $IsElevated <> 1
      ; The wizard asks on the scope page, once the user has chosen. Everything that
      ; cannot ask (silent, passive, the in-app updater) or named /ALLUSERS elevates now.
      ${If} ${Silent}
      ${OrIf} $PassiveMode = 1
      ${OrIf} $ScopeExplicit = 1
        Call RelaunchElevated
        ${If} $0 = 1
          Quit
        ${EndIf}
        SetErrorLevel 1223 ; ERROR_CANCELLED: the prompt was refused
        ${IfNot} ${Silent}
          MessageBox MB_ICONEXCLAMATION "$(sevakNeedAdmin)"
        ${EndIf}
        Quit
      ${EndIf}
    ${EndIf}
  !endif
  ; SEVAK end
FunctionEnd

; SEVAK: scope helpers ---------------------------------------------------------

; The same two small helpers for the installer and, as un.*, the uninstaller.
!macro SevakScopeFunctions PREFIX
  ; $IsElevated = 1 when this process has administrator rights. IsUserAnAdmin is
  ; false for an administrator's filtered (not elevated) token, which is what
  ; UserInfo::GetAccountType would not tell apart.
  Function ${PREFIX}CheckElevated
    System::Call 'shell32::IsUserAnAdmin() i .r0'
    ${If} $0 <> 0
      StrCpy $IsElevated 1
    ${Else}
      StrCpy $IsElevated 0
    ${EndIf}
  FunctionEnd

  ; Makes $SMPROGRAMS, $DESKTOP and SHCTX (HKCU or HKLM) follow the scope.
  Function ${PREFIX}ApplyContext
    ${If} $InstallScope == "machine"
      SetShellVarContext all
    ${Else}
      SetShellVarContext current
    ${EndIf}
  FunctionEnd
!macroend
!insertmacro SevakScopeFunctions ""
!insertmacro SevakScopeFunctions "un."

; Runs this installer again with administrator rights (the UAC prompt) and the same
; switches plus /ELEVATED /ALLUSERS. $0 = 1 when Windows started it (the caller
; quits), 0 when the prompt was refused. The strings go through registers because a
; quote inside the switches would end the System plugin's string.
Function RelaunchElevated
  ${GetParameters} $1
  StrCpy $3 "/ELEVATED /ALLUSERS $1"
  StrCpy $4 "$EXEPATH"
  System::Call 'shell32::ShellExecuteW(p $HWNDPARENT, t "runas", t r4, t r3, p 0, i 1) i .r2'
  ${If} $2 > 32 ; ShellExecute returns a value above 32 on success
    StrCpy $0 1
  ${Else}
    StrCpy $0 0
  ${EndIf}
FunctionEnd

; Fills $UserUninst/$UserVer/$UserDir and $MachUninst/$MachVer/$MachDir from the
; registry: the uninstall command, version and folder of an earlier install in each
; scope. An empty ...Uninst means "not installed there".
Function DetectInstalls
  ReadRegStr $UserUninst HKCU "${UNINSTKEY}" "UninstallString"
  ReadRegStr $UserVer HKCU "${UNINSTKEY}" "DisplayVersion"
  ReadRegStr $UserDir HKCU "${MANUPRODUCTKEY}" ""
  ReadRegStr $MachUninst HKLM "${UNINSTKEY}" "UninstallString"
  ReadRegStr $MachVer HKLM "${UNINSTKEY}" "DisplayVersion"
  ReadRegStr $MachDir HKLM "${MANUPRODUCTKEY}" ""

  ; The folder is also kept, quoted, as InstallLocation: use that when the first
  ; record is missing, so the uninstaller can always be told where it runs.
  ${If} $UserDir == ""
  ${AndIf} $UserUninst != ""
    ReadRegStr $R0 HKCU "${UNINSTKEY}" "InstallLocation"
    Call TrimQuotes
    StrCpy $UserDir $R0
  ${EndIf}
  ${If} $MachDir == ""
  ${AndIf} $MachUninst != ""
    ReadRegStr $R0 HKLM "${UNINSTKEY}" "InstallLocation"
    Call TrimQuotes
    StrCpy $MachDir $R0
  ${EndIf}
FunctionEnd

; Removes one leading and one trailing double quote from $R0.
Function TrimQuotes
  StrCpy $R1 $R0 1
  ${If} $R1 == "$\""
    StrCpy $R0 $R0 "" 1
  ${EndIf}
  StrCpy $R1 $R0 1 -1
  ${If} $R1 == "$\""
    StrCpy $R0 $R0 -1
  ${EndIf}
FunctionEnd

; The default scope, in order: /ALLUSERS, /CURRENTUSER, where Sevak is already
; installed (an update must stay where it is: the in-app updater passes neither
; switch), and otherwise this user alone.
Function ChooseInitialScope
  StrCpy $InstallScope "user"
  StrCpy $ScopeExplicit 0

  ${If} $UserUninst == ""
  ${AndIf} $MachUninst != ""
    StrCpy $InstallScope "machine"
  ${EndIf}

  ${GetOptions} $CMDLINE "/CURRENTUSER" $0
  ${IfNot} ${Errors}
    StrCpy $InstallScope "user"
    StrCpy $ScopeExplicit 1
  ${EndIf}
  ${GetOptions} $CMDLINE "/ALLUSERS" $0
  ${IfNot} ${Errors}
    StrCpy $InstallScope "machine"
    StrCpy $ScopeExplicit 1
  ${EndIf}
FunctionEnd

; Splits what is installed into "this scope" (Same...) and "the other one"
; (Other...), and compares versions for the "already installed" page.
Function RefreshInstallState
  ${If} $InstallScope == "machine"
    StrCpy $SameUninst $MachUninst
    StrCpy $SameVer $MachVer
    StrCpy $SameDir $MachDir
    StrCpy $SameScopeName "$(sevakScopeNameMachine)"
    StrCpy $OtherUninst $UserUninst
    StrCpy $OtherVer $UserVer
    StrCpy $OtherDir $UserDir
    StrCpy $OtherScopeName "$(sevakScopeNameUser)"
  ${Else}
    StrCpy $SameUninst $UserUninst
    StrCpy $SameVer $UserVer
    StrCpy $SameDir $UserDir
    StrCpy $SameScopeName "$(sevakScopeNameUser)"
    StrCpy $OtherUninst $MachUninst
    StrCpy $OtherVer $MachVer
    StrCpy $OtherDir $MachDir
    StrCpy $OtherScopeName "$(sevakScopeNameMachine)"
  ${EndIf}

  ; 1: this installer is newer (upgrade), 0: same version, -1: older (downgrade),
  ; anything else: the installed version is missing or not a version
  StrCpy $CompareResult 2
  ${If} $SameUninst != ""
  ${AndIf} $SameVer != ""
    nsis_tauri_utils::SemverCompare "${VERSION}" "$SameVer"
    Pop $CompareResult
  ${EndIf}
FunctionEnd

; Applies the chosen scope: shell folders and registry root, what is installed
; there, and the default folder (unless /D= gave one): %LOCALAPPDATA%\Sevak, the
; folder every earlier Sevak installer used, or Program Files; an existing
; install's own folder wins.
Function SetScopeDefaults
  Call ApplyContext
  Call RefreshInstallState
  ${If} $CustomInstDir <> 1
    ${If} $InstallScope == "machine"
      ${If} ${RunningX64}
        !if "${ARCH}" == "x64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else if "${ARCH}" == "arm64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else
          StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
        !endif
      ${Else}
        StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
      ${EndIf}
    ${Else}
      StrCpy $INSTDIR "$LOCALAPPDATA\${PRODUCTNAME}"
    ${EndIf}
    Call RestorePreviousInstallLocation
  ${EndIf}
FunctionEnd

; An earlier MSI (WiX) install of Sevak. Sets $WixMode and $WixKey.
; This is upstream's detection, unchanged, moved out of the "already installed"
; page so it also runs when that page does not (silent, passive).
Function DetectWix
  ; Uninstall previous WiX installation if exists.
  ;
  ; A WiX installer stores the installation info in registry
  ; using a UUID and so we have to loop through all keys under
  ; `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`
  ; and check if `DisplayName` and `Publisher` keys match ${PRODUCTNAME} and ${MANUFACTURER}
  ;
  ; This has a potential issue that there maybe another installation that matches
  ; our ${PRODUCTNAME} and ${MANUFACTURER} but wasn't installed by our WiX installer,
  ; however, this should be fine since the user will have to confirm the uninstallation
  ; and they can chose to abort it if doesn't make sense.
  StrCpy $0 0
  wix_loop:
    EnumRegKey $1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall" $0
    StrCmp $1 "" wix_loop_done ; Exit loop if there is no more keys to loop on
    IntOp $0 $0 + 1
    ReadRegStr $R0 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "DisplayName"
    ReadRegStr $R1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "Publisher"
    StrCmp "$R0$R1" "${PRODUCTNAME}${MANUFACTURER}" 0 wix_loop
    ReadRegStr $R0 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "UninstallString"
    ${StrCase} $R1 $R0 "L"
    ${StrLoc} $R0 $R1 "msiexec" ">"
    StrCmp $R0 0 0 wix_loop_done
    StrCpy $WixMode 1
    StrCpy $WixKey "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1"
  wix_loop_done:
FunctionEnd
; SEVAK end


Section EarlyChecks
  ; Abort silent installer if downgrades is disabled
  !if "${ALLOWDOWNGRADES}" == "false"
  ${If} ${Silent}
    ; If downgrading
    ; SEVAK: the comparison is kept in $CompareResult (upstream relied on $R0 surviving the pages)
    ${If} $CompareResult = -1
      System::Call 'kernel32::AttachConsole(i -1)i.r0'
      ${If} $0 <> 0
        System::Call 'kernel32::GetStdHandle(i -11)i.r0'
        System::call 'kernel32::SetConsoleTextAttribute(i r0, i 0x0004)' ; set red color
        FileWrite $0 "$(silentDowngrades)"
      ${EndIf}
      Abort
    ${EndIf}
  ${EndIf}
  !endif

SectionEnd

Section WebView2
  ; Check if Webview2 is already installed and skip this section
  ${If} ${RunningX64}
    ReadRegStr $4 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${Else}
    ReadRegStr $4 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}
  ${If} $4 == ""
    ReadRegStr $4 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}

  ${If} $4 == ""
    ; Webview2 installation
    ;
    ; Skip if updating
    ${If} $UpdateMode <> 1
      !if "${INSTALLWEBVIEW2MODE}" == "downloadBootstrapper"
        Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        DetailPrint "$(webview2Downloading)"
        NSISdl::download "https://go.microsoft.com/fwlink/p/?LinkId=2124703" "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Pop $0
        ${If} $0 == "success"
          DetailPrint "$(webview2DownloadSuccess)"
        ${Else}
          DetailPrint "$(webview2DownloadError)"
          Abort "$(webview2AbortError)"
        ${EndIf}
        StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Goto install_webview2
      !endif

      !if "${INSTALLWEBVIEW2MODE}" == "embedBootstrapper"
        Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        File "/oname=$TEMP\MicrosoftEdgeWebview2Setup.exe" "${WEBVIEW2BOOTSTRAPPERPATH}"
        DetailPrint "$(installingWebview2)"
        StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Goto install_webview2
      !endif

      !if "${INSTALLWEBVIEW2MODE}" == "offlineInstaller"
        Delete "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
        File "/oname=$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe" "${WEBVIEW2INSTALLERPATH}"
        DetailPrint "$(installingWebview2)"
        StrCpy $6 "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
        Goto install_webview2
      !endif

      Goto webview2_done

      install_webview2:
        DetailPrint "$(installingWebview2)"
        ; $6 holds the path to the webview2 installer
        ExecWait "$6 ${WEBVIEW2INSTALLERARGS} /install" $1
        ${If} $1 = 0
          DetailPrint "$(webview2InstallSuccess)"
        ${Else}
          DetailPrint "$(webview2InstallError)"
          Abort "$(webview2AbortError)"
        ${EndIf}
      webview2_done:
    ${EndIf}
  ${Else}
    !if "${MINIMUMWEBVIEW2VERSION}" != ""
      ${VersionCompare} "${MINIMUMWEBVIEW2VERSION}" "$4" $R0
      ${If} $R0 = 1
        update_webview:
          DetailPrint "$(installingWebview2)"
          ${If} ${RunningX64}
            ReadRegStr $R1 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate" "path"
          ${Else}
            ReadRegStr $R1 HKLM "SOFTWARE\Microsoft\EdgeUpdate" "path"
          ${EndIf}
          ${If} $R1 == ""
            ReadRegStr $R1 HKCU "SOFTWARE\Microsoft\EdgeUpdate" "path"
          ${EndIf}
          ${If} $R1 != ""
            ; Chromium updater docs: https://source.chromium.org/chromium/chromium/src/+/main:docs/updater/user_manual.md
            ; Modified from "HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Microsoft EdgeWebView\ModifyPath"
            ExecWait `"$R1" /install appguid=${WEBVIEW2APPGUID}&needsadmin=true` $1
            ${If} $1 = 0
              DetailPrint "$(webview2InstallSuccess)"
            ${Else}
              MessageBox MB_ICONEXCLAMATION|MB_ABORTRETRYIGNORE "$(webview2InstallError)" IDIGNORE ignore IDRETRY update_webview
              Quit
              ignore:
            ${EndIf}
          ${EndIf}
      ${EndIf}
    !endif
  ${EndIf}
SectionEnd

; SEVAK: remove what this install replaces, before any file is written:
;   - an earlier MSI (WiX) install, as upstream always did;
;   - the copy in the other scope, when the user chose to move (the "already
;     installed" page) or passed /UNINSTALLOTHER.
; An upgrade in the same scope installs over the old files and removes nothing.
Section ReplaceExisting
  ${If} $WixMode = 1
    DetailPrint "$(sevakRemovingMsi)"
    ReadRegStr $R1 HKLM "$WixKey" "UninstallString"
    ClearErrors
    ExecWait '$R1' $0
    ${IfThen} ${Errors} ${|} StrCpy $0 2 ${|} ; ExecWait failed, set fake exit code
    ${If} $0 <> 0
    ${OrIf} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
      Abort "$(unableToUninstall)"
    ${EndIf}
  ${EndIf}

  ${If} $RemoveOther = 1
  ${AndIf} $OtherUninst != ""
    DetailPrint "$(sevakRemovingOther)"
    !insertmacro SEVAK_CLOSE_RUNNING_APP "$OtherDir\${MAINBINARYNAME}.exe"
    Call UninstallOtherScope
    ${If} $0 <> 0
      Abort "$(sevakRemoveOtherFailed)"
    ${EndIf}
  ${EndIf}
SectionEnd

; Quietly uninstalls the copy in the other scope. $0 = 0 when it is gone.
;
; Its uninstaller runs with /MOVE: shortcuts go, but the autostart entry and the
; data stay, since Sevak is only changing scope. A per-machine copy needs administrator
; rights: its uninstaller asks for them itself (un.onInit) and returns at once with
; exit code 0, leaving an elevated copy to finish, so wait for the registry entry to
; disappear; a refused prompt comes back as exit code 1223.
Function UninstallOtherScope
  ${If} $OtherDir == ""
    StrCpy $0 1
    Return
  ${EndIf}

  ${If} $InstallScope == "machine"
    StrCpy $2 "/CURRENTUSER" ; the other copy is the per-user one
  ${Else}
    StrCpy $2 "/ALLUSERS"
  ${EndIf}
  ClearErrors
  ExecWait '$OtherUninst $2 /S /MOVE _?=$OtherDir' $0
  ${IfThen} ${Errors} ${|} StrCpy $0 1 ${|}
  ${If} $0 <> 0
    Return
  ${EndIf}

  StrCpy $3 0
  other_wait:
    ${If} $InstallScope == "machine"
      ReadRegStr $4 HKCU "${UNINSTKEY}" "UninstallString"
    ${Else}
      ReadRegStr $4 HKLM "${UNINSTKEY}" "UninstallString"
    ${EndIf}
    ${If} $4 == ""
      Goto other_gone
    ${EndIf}
    Sleep 500
    IntOp $3 $3 + 1
    ${If} $3 < 120 ; 60 seconds
      Goto other_wait
    ${EndIf}
  StrCpy $0 1
  Return

  other_gone:
  ${If} $InstallScope == "machine"
    ; The per-user uninstaller ran in place, inside this process (_?=), so it
    ; could not delete itself: tidy up.
    Delete "$OtherDir\uninstall.exe"
    RMDir "$OtherDir"
  ${EndIf}
  StrCpy $0 0
FunctionEnd
; SEVAK end

Section Install
  ; SEVAK: SHCTX and the shell folders follow the chosen scope
  !if "${INSTALLMODE}" == "both"
    Call ApplyContext
  !endif
  ; SEVAK end

  SetOutPath $INSTDIR

  !ifmacrodef NSIS_HOOK_PREINSTALL
    !insertmacro NSIS_HOOK_PREINSTALL
  !endif

  !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

  ; Copy main executable
  File "${MAINBINARYSRCPATH}"

  ; Copy resources
  {{#each resources_dirs}}
    CreateDirectory "$INSTDIR\\{{this}}"
  {{/each}}
  {{#each resources}}
    File /a "/oname={{this.[1]}}" "{{no-escape @key}}"
  {{/each}}

  ; Copy external binaries
  {{#each binaries}}
    File /a "/oname={{this}}" "{{no-escape @key}}"
  {{/each}}

  ; Create file associations
  {{#each file_associations as |association| ~}}
    {{#each association.ext as |ext| ~}}
       !insertmacro APP_ASSOCIATE "{{ext}}" "{{or association.name ext}}" "{{association-description association.description ext}}" "$INSTDIR\${MAINBINARYNAME}.exe,0" "Open with ${PRODUCTNAME}" "$INSTDIR\${MAINBINARYNAME}.exe $\"%1$\""
    {{/each}}
  {{/each}}

  ; Register deep links
  {{#each deep_link_protocols as |protocol| ~}}
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}" "URL Protocol" ""
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}" "" "URL:${BUNDLEID} protocol"
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
  {{/each}}

  ; Create uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Save $INSTDIR in registry for future installations
  WriteRegStr SHCTX "${MANUPRODUCTKEY}" "" $INSTDIR

  ; SEVAK: upstream saved MultiUser's mode here; the scope is the registry root now
  ; (HKCU or HKLM) and the switch written into UninstallString below.

  ; Remove old main binary if it doesn't match new main binary name
  ReadRegStr $OldMainBinaryName SHCTX "${UNINSTKEY}" "MainBinaryName"
  ${If} $OldMainBinaryName != ""
  ${AndIf} $OldMainBinaryName != "${MAINBINARYNAME}.exe"
    Delete "$INSTDIR\$OldMainBinaryName"
  ${EndIf}

  ; Save current MAINBINARYNAME for future updates
  WriteRegStr SHCTX "${UNINSTKEY}" "MainBinaryName" "${MAINBINARYNAME}.exe"

  ; Registry information for add/remove programs
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\""
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr SHCTX "${UNINSTKEY}" "Publisher" "${MANUFACTURER}"
  WriteRegStr SHCTX "${UNINSTKEY}" "InstallLocation" "$\"$INSTDIR$\""
  ; SEVAK: name the scope in the command so the uninstaller knows it (and, per-machine,
  ; asks for administrator rights) when Settings > Apps starts it
  !if "${INSTALLMODE}" == "both"
    ${If} $InstallScope == "machine"
      WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\" /ALLUSERS"
    ${Else}
      WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\" /CURRENTUSER"
    ${EndIf}
  !else
    WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  !endif
  ; SEVAK end
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoModify" "1"
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoRepair" "1"

  ${GetSize} "$INSTDIR" "/M=uninstall.exe /S=0K /G=0" $0 $1 $2
  IntOp $0 $0 + ${ESTIMATEDSIZE}
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD SHCTX "${UNINSTKEY}" "EstimatedSize" "$0"

  !if "${HOMEPAGE}" != ""
    WriteRegStr SHCTX "${UNINSTKEY}" "URLInfoAbout" "${HOMEPAGE}"
    WriteRegStr SHCTX "${UNINSTKEY}" "URLUpdateInfo" "${HOMEPAGE}"
    WriteRegStr SHCTX "${UNINSTKEY}" "HelpLink" "${HOMEPAGE}"
  !endif

  ; Create start menu shortcut
  !insertmacro MUI_STARTMENU_WRITE_BEGIN Application
    Call CreateOrUpdateStartMenuShortcut
  !insertmacro MUI_STARTMENU_WRITE_END

  ; Create desktop shortcut for silent and passive installers
  ; because finish page will be skipped
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    Call CreateOrUpdateDesktopShortcut
  ${EndIf}

  !ifmacrodef NSIS_HOOK_POSTINSTALL
    !insertmacro NSIS_HOOK_POSTINSTALL
  !endif

  ; Auto close this page for passive mode
  ${If} $PassiveMode = 1
    SetAutoClose true
  ${EndIf}
SectionEnd

Function .onInstSuccess
  ; Check for `/R` flag only in silent and passive installers because
  ; GUI installer has a toggle for the user to (re)start the app
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    ${GetOptions} $CMDLINE "/R" $R0
    ${IfNot} ${Errors}
      ${GetOptions} $CMDLINE "/ARGS" $R0
      nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" "$R0"
    ${EndIf}
  ${EndIf}
FunctionEnd

Function un.onInit
  !insertmacro SetContext

  !insertmacro MUI_UNGETLANGUAGE

  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}

  ; SEVAK: /MOVE: Sevak is being removed from one scope only to be installed in the other
  ${GetOptions} $CMDLINE "/MOVE" $MoveMode
  ${IfNot} ${Errors}
    StrCpy $MoveMode 1
  ${EndIf}

  ; SEVAK: which scope is this uninstall for, and does it need administrator rights?
  ; Upstream called MULTIUSER_UNINIT here. A per-machine uninstall started without
  ; them (Settings > Apps) runs the uninstaller again elevated and quits; it runs
  ; in place, so the elevated copy makes its own temporary copy as usual.
  !if "${INSTALLMODE}" == "both"
    Call un.ChooseScope
    ${If} $InstallScope == "machine"
      Call un.CheckElevated
      ${If} $IsElevated <> 1
        Call un.RelaunchElevated
        ${If} $0 = 1
          Quit
        ${EndIf}
        SetErrorLevel 1223 ; ERROR_CANCELLED: the prompt was refused
        ${IfNot} ${Silent}
          MessageBox MB_ICONEXCLAMATION "$(sevakNeedAdmin)"
        ${EndIf}
        Quit
      ${EndIf}
    ${EndIf}
    Call un.ApplyContext
  !endif
  ; SEVAK end
FunctionEnd

; SEVAK: uninstaller scope helpers (the two shared ones come from SevakScopeFunctions)

; Per-machine when the uninstall switch says so, or when HKLM records this very
; folder as the install location; otherwise per-user (an uninstaller from an earlier,
; per-user-only Sevak installer carries no switch).
Function un.ChooseScope
  StrCpy $InstallScope "user"
  ReadRegStr $0 HKLM "${UNINSTKEY}" "InstallLocation"
  ${If} $0 == "$\"$INSTDIR$\""
    StrCpy $InstallScope "machine"
  ${EndIf}

  ${GetOptions} $CMDLINE "/CURRENTUSER" $0
  ${IfNot} ${Errors}
    StrCpy $InstallScope "user"
  ${EndIf}
  ${GetOptions} $CMDLINE "/ALLUSERS" $0
  ${IfNot} ${Errors}
    StrCpy $InstallScope "machine"
  ${EndIf}
FunctionEnd

; Starts uninstall.exe again from the install folder, elevated, with the same
; switches. $0 = 1 when Windows started it, 0 when the prompt was refused.
Function un.RelaunchElevated
  StrCpy $3 "/ELEVATED /ALLUSERS"
  ${If} ${Silent}
    StrCpy $3 "$3 /S"
  ${EndIf}
  ${If} $PassiveMode = 1
    StrCpy $3 "$3 /P"
  ${EndIf}
  ${If} $UpdateMode = 1
    StrCpy $3 "$3 /UPDATE"
  ${EndIf}
  ${If} $MoveMode = 1
    StrCpy $3 "$3 /MOVE"
  ${EndIf}
  StrCpy $4 "$INSTDIR\uninstall.exe"
  System::Call 'shell32::ShellExecuteW(p $HWNDPARENT, t "runas", t r4, t r3, p 0, i 1) i .r2'
  ${If} $2 > 32
    StrCpy $0 1
  ${Else}
    StrCpy $0 0
  ${EndIf}
FunctionEnd
; SEVAK end

Section Uninstall

  !ifmacrodef NSIS_HOOK_PREUNINSTALL
    !insertmacro NSIS_HOOK_PREUNINSTALL
  !endif

  !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

  ; Delete the app directory and its content from disk
  ; Copy main executable
  Delete "$INSTDIR\${MAINBINARYNAME}.exe"

  ; Delete resources
  {{#each resources}}
    Delete "$INSTDIR\\{{this.[1]}}"
  {{/each}}

  ; Delete external binaries
  {{#each binaries}}
    Delete "$INSTDIR\\{{this}}"
  {{/each}}

  ; Delete app associations
  {{#each file_associations as |association| ~}}
    {{#each association.ext as |ext| ~}}
      !insertmacro APP_UNASSOCIATE "{{ext}}" "{{or association.name ext}}"
    {{/each}}
  {{/each}}

  ; Delete deep links
  {{#each deep_link_protocols as |protocol| ~}}
    ReadRegStr $R7 SHCTX "Software\Classes\\{{protocol}}\shell\open\command" ""
    ${If} $R7 == "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
      DeleteRegKey SHCTX "Software\Classes\\{{protocol}}"
    ${EndIf}
  {{/each}}


  ; Delete uninstaller
  Delete "$INSTDIR\uninstall.exe"

  {{#each resources_ancestors}}
  RMDir /REBOOTOK "$INSTDIR\\{{this}}"
  {{/each}}
  RMDir "$INSTDIR"

  ; Remove shortcuts if not updating
  ${If} $UpdateMode <> 1
    !insertmacro DeleteAppUserModelId

    ; Remove start menu shortcut
    !insertmacro MUI_STARTMENU_GETFOLDER Application $AppStartMenuFolder
    !insertmacro IsShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
      Delete "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
      RMDir "$SMPROGRAMS\$AppStartMenuFolder"
    ${EndIf}
    !insertmacro IsShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk"
      Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    ${EndIf}

    ; Remove desktop shortcuts
    !insertmacro IsShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$DESKTOP\${PRODUCTNAME}.lnk"
      Delete "$DESKTOP\${PRODUCTNAME}.lnk"
    ${EndIf}
  ${EndIf}

  ; Remove registry information for add/remove programs
  !if "${INSTALLMODE}" == "both"
    DeleteRegKey SHCTX "${UNINSTKEY}"
  !else if "${INSTALLMODE}" == "perMachine"
    DeleteRegKey HKLM "${UNINSTKEY}"
  !else
    DeleteRegKey HKCU "${UNINSTKEY}"
  !endif

  ; Removes the Autostart entry for ${PRODUCTNAME} from the HKCU Run key if it exists.
  ; This ensures the program does not launch automatically after uninstallation if it exists.
  ; If it doesn't exist, it does nothing.
  ; We do this when not updating (to preserve the registry value on updates)
  ; SEVAK: nor when Sevak only moves to the other scope (/MOVE)
  ${If} $UpdateMode <> 1
  ${AndIf} $MoveMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
  ${EndIf}

  ; Delete app data if the checkbox is selected
  ; and if not updating
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ; Clear the install location $INSTDIR from registry
    DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty SHCTX "${MANUKEY}"

    ; Clear the install language from registry
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"
    DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty HKCU "${MANUKEY}"

    SetShellVarContext current
    RmDir /r "$APPDATA\${BUNDLEID}"
    RmDir /r "$LOCALAPPDATA\${BUNDLEID}"
  ${EndIf}

  !ifmacrodef NSIS_HOOK_POSTUNINSTALL
    !insertmacro NSIS_HOOK_POSTUNINSTALL
  !endif

  ; Auto close if passive mode or updating
  ${If} $PassiveMode = 1
  ${OrIf} $UpdateMode = 1
  ${OrIf} $MoveMode = 1 ; SEVAK
    SetAutoClose true
  ${EndIf}
SectionEnd

Function RestorePreviousInstallLocation
  ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""
  StrCmp $4 "" +2 0
    StrCpy $INSTDIR $4
FunctionEnd

; SEVAK: only referenced when there is no start menu folder, which Sevak sets
!if "${STARTMENUFOLDER}" == ""
Function Skip
  Abort
FunctionEnd
!endif
; SEVAK end

Function SkipIfPassive
  ${IfThen} $PassiveMode = 1  ${|} Abort ${|}
FunctionEnd
; SEVAK: the elevated copy resumes after the scope page; an existing copy is
; upgraded where it is, so its folder is not asked again.
Function SkipIfPassiveOrElevated
  ${IfThen} $PassiveMode = 1  ${|} Abort ${|}
  ${IfThen} $ElevatedMode = 1 ${|} Abort ${|}
FunctionEnd
Function SkipIfPassiveOrInstalled
  ${IfThen} $PassiveMode = 1  ${|} Abort ${|}
  ${IfThen} $SameUninst != "" ${|} Abort ${|}
FunctionEnd
; SEVAK end
Function un.SkipIfPassive
  ${IfThen} $PassiveMode = 1  ${|} Abort ${|}
FunctionEnd

Function CreateOrUpdateStartMenuShortcut
  ; We used to use product name as MAINBINARYNAME
  ; migrate old shortcuts to target the new MAINBINARYNAME
  StrCpy $R0 0

  !insertmacro IsShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    StrCpy $R0 1
  ${EndIf}

  !insertmacro IsShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    StrCpy $R0 1
  ${EndIf}

  ${If} $R0 = 1
    Return
  ${EndIf}

  ; Skip creating shortcut if in update mode or no shortcut mode
  ; but always create if migrating from wix
  ${If} $WixMode = 0
    ${If} $UpdateMode = 1
    ${OrIf} $NoShortcutMode = 1
      Return
    ${EndIf}
  ${EndIf}

  !if "${STARTMENUFOLDER}" != ""
    CreateDirectory "$SMPROGRAMS\$AppStartMenuFolder"
    CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
  !else
    CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !endif
FunctionEnd

Function CreateOrUpdateDesktopShortcut
  ; We used to use product name as MAINBINARYNAME
  ; migrate old shortcuts to target the new MAINBINARYNAME
  !insertmacro IsShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Return
  ${EndIf}

  ; Skip creating shortcut if in update mode or no shortcut mode
  ; but always create if migrating from wix
  ${If} $WixMode = 0
    ${If} $UpdateMode = 1
    ${OrIf} $NoShortcutMode = 1
      Return
    ${EndIf}
  ${EndIf}

  CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  !insertmacro SetLnkAppUserModelId "$DESKTOP\${PRODUCTNAME}.lnk"
FunctionEnd
