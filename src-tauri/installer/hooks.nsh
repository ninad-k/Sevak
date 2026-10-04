; Sevak installer hooks (bundle.windows.nsis.installerHooks).
;
; Included by installer.nsi before its variables are declared, so this file only
; defines macros; their bodies are expanded later, inside the sections.

; Closes a running Sevak politely before files are replaced or removed.
;
; Sevak is a single-instance tray app. `sevak.exe --quit` is forwarded to the
; running copy, which then exits cleanly (saving its state), so no one's
; clipboard history or settings are cut off by a forced kill. Whatever is still
; alive afterwards is handled by Tauri's CheckIfAppIsRunning (Restart Manager),
; which asks again in the wizard and never blocks a silent or passive run.
;
;   exePath  the sevak.exe of the copy that is about to be changed
!macro SEVAK_CLOSE_RUNNING_APP exePath
  !define SevakCloseId ${__LINE__}

  nsis_tauri_utils::FindProcessCurrentUser "${MAINBINARYNAME}.exe"
  Pop $R8
  ${If} $R8 = 0 ; 0: a Sevak of this user is running
  ${AndIf} ${FileExists} "${exePath}"
    ; Ask first in the wizard. Silent and passive runs (winget, the updater)
    ; carry on without a prompt.
    ${If} $PassiveMode <> 1
    ${AndIfNot} ${Silent}
      MessageBox MB_OKCANCEL|MB_ICONINFORMATION "$(sevakCloseRunning)" IDOK sevak_close_${SevakCloseId}
      Abort
    ${EndIf}
    sevak_close_${SevakCloseId}:
    Exec '"${exePath}" --quit'
    ; Give it up to 5 seconds to leave. Never wait longer: a hung Sevak is
    ; left to the Restart Manager step.
    StrCpy $R8 0
    sevak_wait_${SevakCloseId}:
      Sleep 500
      nsis_tauri_utils::FindProcessCurrentUser "${MAINBINARYNAME}.exe"
      Pop $R7
      ${If} $R7 <> 0
        Goto sevak_closed_${SevakCloseId}
      ${EndIf}
      IntOp $R8 $R8 + 1
      ${If} $R8 < 10
        Goto sevak_wait_${SevakCloseId}
      ${EndIf}
  ${EndIf}
  sevak_closed_${SevakCloseId}:

  !undef SevakCloseId
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro SEVAK_CLOSE_RUNNING_APP "$INSTDIR\${MAINBINARYNAME}.exe"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro SEVAK_CLOSE_RUNNING_APP "$INSTDIR\${MAINBINARYNAME}.exe"
!macroend
