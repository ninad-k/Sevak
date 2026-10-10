; Sevak's installer text (English only).
;
; This replaces Tauri's languages/English.nsh through
; bundle.windows.nsis.customLanguageFiles. Every string of the upstream file is
; kept, because installer.nsi uses them; "SEVAK" marks what was reworded.
; The strings named sevak* are new. A string may use variables ($SameVer, ...):
; they are expanded when the string is shown.
;
; Source of the upstream file: src-tauri/installer/upstream/English.nsh

LangString addOrReinstall ${LANG_ENGLISH} "Add/Reinstall components"
LangString alreadyInstalled ${LANG_ENGLISH} "Already Installed"
LangString alreadyInstalledLong ${LANG_ENGLISH} "${PRODUCTNAME} ${VERSION} is already installed. Select the operation you want to perform and click Next to continue."
LangString appRunning ${LANG_ENGLISH} "{{product_name}} is running! Please close it first then try again."
; SEVAK: kinder wording than "kill it"
LangString appRunningOkKill ${LANG_ENGLISH} "{{product_name}} is still running.$\nClick OK to close it and continue."
LangString chooseMaintenanceOption ${LANG_ENGLISH} "Choose the maintenance option to perform."
LangString choowHowToInstall ${LANG_ENGLISH} "Choose how you want to install ${PRODUCTNAME}."
LangString createDesktop ${LANG_ENGLISH} "Create desktop shortcut"
LangString dontUninstall ${LANG_ENGLISH} "Do not uninstall"
LangString dontUninstallDowngrade ${LANG_ENGLISH} "Do not uninstall (Downgrading without uninstall is disabled for this installer)"
LangString failedToKillApp ${LANG_ENGLISH} "Failed to kill {{product_name}}. Please close it first then try again"
LangString installingWebview2 ${LANG_ENGLISH} "Installing WebView2..."
LangString newerVersionInstalled ${LANG_ENGLISH} "A newer version of ${PRODUCTNAME} is already installed! It is not recommended that you install an older version. If you really want to install this older version, it's better to uninstall the current version first. Select the operation you want to perform and click Next to continue."
LangString older ${LANG_ENGLISH} "older"
LangString olderOrUnknownVersionInstalled ${LANG_ENGLISH} "An $R4 version of ${PRODUCTNAME} is installed on your system. It's recommended that you uninstall the current version before installing. Select the operation you want to perform and click Next to continue."
LangString silentDowngrades ${LANG_ENGLISH} "Downgrades are disabled for this installer, can't proceed with the silent installer, please use the graphical interface installer instead.$\n"
LangString unableToUninstall ${LANG_ENGLISH} "Unable to uninstall!"
LangString uninstallApp ${LANG_ENGLISH} "Uninstall ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_ENGLISH} "Uninstall before installing"
LangString unknown ${LANG_ENGLISH} "unknown"
LangString webview2AbortError ${LANG_ENGLISH} "Failed to install WebView2! The app can't run without it. Try restarting the installer."
LangString webview2DownloadError ${LANG_ENGLISH} "Error: Downloading WebView2 Failed - $0"
LangString webview2DownloadSuccess ${LANG_ENGLISH} "WebView2 bootstrapper downloaded successfully"
LangString webview2Downloading ${LANG_ENGLISH} "Downloading WebView2 bootstrapper..."
LangString webview2InstallError ${LANG_ENGLISH} "Error: Installing WebView2 failed with exit code $1"
LangString webview2InstallSuccess ${LANG_ENGLISH} "WebView2 installed successfully"
; SEVAK: the checkbox removes only the WebView2 cache (%APPDATA% and %LOCALAPPDATA%
; \com.ninad.sevak), never Sevak's own settings in %APPDATA%\sevak. Say so.
LangString deleteAppData ${LANG_ENGLISH} "Also delete the web cache (your settings are kept)"

; ---- Welcome and Finish pages ----------------------------------------------

LangString sevakWelcomeTitle ${LANG_ENGLISH} "Welcome to Sevak"
LangString sevakWelcomeText ${LANG_ENGLISH} "Sevak means $\"one who serves$\". It is a keyboard-first launcher: press a key, type a few letters, and open an app, a file, a calculation or a web search.$\r$\n$\r$\nYour desktop. At your service.$\r$\n$\r$\nThis wizard installs Sevak ${VERSION}. Sevak collects no telemetry.$\r$\n$\r$\nClick Next to continue."

LangString sevakFinishTitle ${LANG_ENGLISH} "Sevak is ready"
LangString sevakFinishText ${LANG_ENGLISH} "Sevak waits quietly in the system tray.$\r$\n$\r$\nPress Win+Space to open it (an existing hotkey is kept). Click the tray icon for Settings.$\r$\n$\r$\nTry: an app name, 12*7, f report, g rust."
LangString sevakRunNow ${LANG_ENGLISH} "Start Sevak now"
LangString sevakOpenSettings ${LANG_ENGLISH} "Open Sevak Settings"
LangString sevakDesktopShortcut ${LANG_ENGLISH} "Create a desktop shortcut"
LangString sevakStartAtSignIn ${LANG_ENGLISH} "Start Sevak when I sign in"
LangString sevakStartupHint ${LANG_ENGLISH} "For your account only, including an all-users installation. Sevak starts quietly in the tray. Change this later in Settings > General."
LangString sevakStartupElevatedHint ${LANG_ENGLISH} "To choose startup for your account, start this installer normally (without Run as administrator), or change Settings > General after installation."
LangString sevakStartupFailed ${LANG_ENGLISH} "Sevak was installed, but your startup preference could not be saved. Open Sevak > Settings > General to set 'Start Sevak when I sign in'."
LangString sevakStartupRemoveFailed ${LANG_ENGLISH} "Your sign-in startup entry could not be checked. If Sevak remains listed in Windows Settings > Apps > Startup, turn it off there. Other accounts' preferences were not changed."

; ---- Who is Sevak for? ------------------------------------------------------

LangString sevakScopeTitle ${LANG_ENGLISH} "Who is Sevak for?"
LangString sevakScopeSubtitle ${LANG_ENGLISH} "Choose where to install Sevak."
LangString sevakScopeIntro ${LANG_ENGLISH} "Install Sevak just for your account, or for everyone who uses this PC."
LangString sevakScopeUser ${LANG_ENGLISH} "Install for me only (recommended)$\r$\nNo administrator permission is needed. Sevak goes in your user profile."
LangString sevakScopeMachine ${LANG_ENGLISH} "Install for all users on this PC$\r$\nWindows asks for administrator permission. Sevak goes in Program Files."
LangString sevakScopeNameUser ${LANG_ENGLISH} "your account"
LangString sevakScopeNameMachine ${LANG_ENGLISH} "all users of this PC"
LangString sevakNeedAdmin ${LANG_ENGLISH} "Administrator permission is needed to continue. Choose $\"Install for me only$\", or try again and allow the Windows prompt."

; ---- Sevak is already installed ---------------------------------------------

LangString sevakInstalledTitle ${LANG_ENGLISH} "Sevak is already installed"
LangString sevakInstalledSubtitle ${LANG_ENGLISH} "Choose what you would like to do."
LangString sevakUpgradeText ${LANG_ENGLISH} "Sevak $SameVer is installed for $SameScopeName. This installer has Sevak ${VERSION}.$\r$\n$\r$\nYour settings and data are kept."
LangString sevakReinstallText ${LANG_ENGLISH} "Sevak ${VERSION} is already installed for $SameScopeName.$\r$\n$\r$\nYour settings and data are kept."
LangString sevakDowngradeText ${LANG_ENGLISH} "A newer Sevak, $SameVer, is installed for $SameScopeName. This installer has the older Sevak ${VERSION}.$\r$\n$\r$\nYour settings and data are kept, but an older Sevak may not understand every setting a newer one saved."
LangString sevakUnknownText ${LANG_ENGLISH} "Sevak is installed for $SameScopeName, but its version could not be read. This installer has Sevak ${VERSION}.$\r$\n$\r$\nYour settings and data are kept."
LangString sevakUpgradeButton ${LANG_ENGLISH} "Upgrade to Sevak ${VERSION}"
LangString sevakReinstallButton ${LANG_ENGLISH} "Reinstall Sevak ${VERSION}"
LangString sevakDowngradeButton ${LANG_ENGLISH} "Downgrade to Sevak ${VERSION} (not recommended)"
LangString sevakUninstallButton ${LANG_ENGLISH} "Uninstall Sevak"
LangString sevakAlsoRemove ${LANG_ENGLISH} "Also remove the copy installed for $OtherScopeName (Sevak $OtherVer)"
LangString sevakOtherText ${LANG_ENGLISH} "Sevak $OtherVer is already installed for $OtherScopeName. Installing it for $SameScopeName as well would leave two copies that compete for the same hotkey.$\r$\n$\r$\nYour settings and data are shared and are kept either way."
LangString sevakMoveButton ${LANG_ENGLISH} "Replace it: remove the other copy, then install here (recommended)"
LangString sevakKeepBothButton ${LANG_ENGLISH} "Keep both copies"

; ---- While installing -------------------------------------------------------

LangString sevakCloseRunning ${LANG_ENGLISH} "Sevak is running. Setup will close it now; you can start it again afterwards.$\r$\n$\r$\nClick OK to continue."
LangString sevakRemovingOther ${LANG_ENGLISH} "Removing the copy installed for $OtherScopeName..."
LangString sevakRemoveOtherFailed ${LANG_ENGLISH} "The copy of Sevak installed for $OtherScopeName could not be removed. Remove it in Settings > Apps, then run setup again."
LangString sevakRemovingMsi ${LANG_ENGLISH} "Removing the earlier Windows Installer (MSI) copy of Sevak..."
