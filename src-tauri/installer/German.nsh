; Sevak's installer text in German (machine-drafted, needs native review).
;
; Registered through bundle.windows.nsis.customLanguageFiles. It defines every
; LangString of English.nsh, with the same variables ($SameVer, ${VERSION}...);
; scripts/installer.test.mjs checks both. Sevak addresses the user with "du",
; as in the app. See docs/localisation.md.

LangString addOrReinstall ${LANG_GERMAN} "Komponenten hinzufügen/neu installieren"
LangString alreadyInstalled ${LANG_GERMAN} "Bereits installiert"
LangString alreadyInstalledLong ${LANG_GERMAN} "${PRODUCTNAME} ${VERSION} ist bereits installiert. Wähle den gewünschten Vorgang und klicke auf Weiter."
LangString appRunning ${LANG_GERMAN} "{{product_name}} läuft gerade. Bitte schließe es und versuche es dann erneut."
LangString appRunningOkKill ${LANG_GERMAN} "{{product_name}} läuft noch.$\nKlicke auf OK, um es zu schließen und fortzufahren."
LangString chooseMaintenanceOption ${LANG_GERMAN} "Wähle den gewünschten Wartungsvorgang."
LangString choowHowToInstall ${LANG_GERMAN} "Wähle, wie ${PRODUCTNAME} installiert werden soll."
LangString createDesktop ${LANG_GERMAN} "Verknüpfung auf dem Desktop erstellen"
LangString dontUninstall ${LANG_GERMAN} "Nicht deinstallieren"
LangString dontUninstallDowngrade ${LANG_GERMAN} "Nicht deinstallieren (ein Downgrade ohne Deinstallation ist bei diesem Installationsprogramm deaktiviert)"
LangString failedToKillApp ${LANG_GERMAN} "{{product_name}} konnte nicht beendet werden. Bitte schließe es und versuche es dann erneut."
LangString installingWebview2 ${LANG_GERMAN} "WebView2 wird installiert..."
LangString newerVersionInstalled ${LANG_GERMAN} "Eine neuere Version von ${PRODUCTNAME} ist bereits installiert. Eine ältere Version zu installieren wird nicht empfohlen. Wenn du diese ältere Version wirklich installieren möchtest, deinstalliere am besten zuerst die aktuelle Version. Wähle den gewünschten Vorgang und klicke auf Weiter."
LangString older ${LANG_GERMAN} "ältere"
LangString olderOrUnknownVersionInstalled ${LANG_GERMAN} "Auf deinem System ist eine $R4 Version von ${PRODUCTNAME} installiert. Es wird empfohlen, die aktuelle Version vor der Installation zu deinstallieren. Wähle den gewünschten Vorgang und klicke auf Weiter."
LangString silentDowngrades ${LANG_GERMAN} "Downgrades sind bei diesem Installationsprogramm deaktiviert, daher kann die stille Installation nicht fortgesetzt werden. Bitte verwende stattdessen das Installationsprogramm mit Benutzeroberfläche.$\n"
LangString unableToUninstall ${LANG_GERMAN} "Deinstallation nicht möglich!"
LangString uninstallApp ${LANG_GERMAN} "${PRODUCTNAME} deinstallieren"
LangString uninstallBeforeInstalling ${LANG_GERMAN} "Vor der Installation deinstallieren"
LangString unknown ${LANG_GERMAN} "unbekannte"
LangString webview2AbortError ${LANG_GERMAN} "WebView2 konnte nicht installiert werden! Ohne WebView2 kann die App nicht laufen. Starte das Installationsprogramm erneut."
LangString webview2DownloadError ${LANG_GERMAN} "Fehler: Herunterladen von WebView2 fehlgeschlagen - $0"
LangString webview2DownloadSuccess ${LANG_GERMAN} "WebView2-Bootstrapper erfolgreich heruntergeladen"
LangString webview2Downloading ${LANG_GERMAN} "WebView2-Bootstrapper wird heruntergeladen..."
LangString webview2InstallError ${LANG_GERMAN} "Fehler: Die Installation von WebView2 ist mit Exitcode $1 fehlgeschlagen"
LangString webview2InstallSuccess ${LANG_GERMAN} "WebView2 erfolgreich installiert"
LangString deleteAppData ${LANG_GERMAN} "Auch den Web-Cache löschen (deine Einstellungen bleiben erhalten)"

; ---- Welcome and Finish pages ----------------------------------------------

LangString sevakWelcomeTitle ${LANG_GERMAN} "Willkommen bei Sevak"
LangString sevakWelcomeText ${LANG_GERMAN} "Sevak bedeutet $\"einer, der dient$\". Es ist ein Launcher für die Tastatur: Drück eine Taste, tippe ein paar Buchstaben und öffne eine App, eine Datei, eine Rechnung oder eine Websuche.$\r$\n$\r$\nDein Desktop. Zu Diensten.$\r$\n$\r$\nDieser Assistent installiert Sevak ${VERSION}. Sevak sammelt keine Telemetriedaten.$\r$\n$\r$\nKlicke auf Weiter."

LangString sevakFinishTitle ${LANG_GERMAN} "Sevak ist bereit"
LangString sevakFinishText ${LANG_GERMAN} "Sevak wartet still im Infobereich der Taskleiste.$\r$\n$\r$\nDrück Win+Leertaste, um es zu öffnen (ein vorhandenes Tastenkürzel bleibt erhalten). Klicke auf das Symbol im Infobereich für die Einstellungen.$\r$\n$\r$\nProbier: einen App-Namen, 12*7, f report, g rust."
LangString sevakRunNow ${LANG_GERMAN} "Sevak jetzt starten"
LangString sevakOpenSettings ${LANG_GERMAN} "Sevak-Einstellungen öffnen"
LangString sevakDesktopShortcut ${LANG_GERMAN} "Verknüpfung auf dem Desktop erstellen"
LangString sevakStartAtSignIn ${LANG_GERMAN} "Sevak bei meiner Anmeldung starten"
LangString sevakStartupHint ${LANG_GERMAN} "Nur für dein Konto, auch bei einer Installation für alle Benutzer. Sevak startet still im Infobereich. Du kannst das später unter Einstellungen > Allgemein ändern."
LangString sevakStartupElevatedHint ${LANG_GERMAN} "Um den Autostart für dein Konto festzulegen, starte dieses Installationsprogramm normal (nicht als Administrator) oder ändere nach der Installation Einstellungen > Allgemein."
LangString sevakStartupFailed ${LANG_GERMAN} "Sevak wurde installiert, aber deine Autostart-Einstellung konnte nicht gespeichert werden. Öffne Sevak > Einstellungen > Allgemein und lege 'Sevak bei meiner Anmeldung starten' fest."
LangString sevakStartupRemoveFailed ${LANG_GERMAN} "Dein Autostart-Eintrag für die Anmeldung konnte nicht geprüft werden. Wenn Sevak weiterhin unter Windows-Einstellungen > Apps > Autostart aufgeführt ist, schalte es dort aus. Die Einstellungen anderer Konten wurden nicht geändert."

; ---- Who is Sevak for? ------------------------------------------------------

LangString sevakScopeTitle ${LANG_GERMAN} "Für wen ist Sevak?"
LangString sevakScopeSubtitle ${LANG_GERMAN} "Wähle, wo Sevak installiert werden soll."
LangString sevakScopeIntro ${LANG_GERMAN} "Installiere Sevak nur für dein Konto oder für alle, die diesen PC benutzen."
LangString sevakScopeUser ${LANG_GERMAN} "Nur für mich installieren (empfohlen)$\r$\nKeine Administratorrechte nötig. Sevak wird in deinem Benutzerprofil installiert."
LangString sevakScopeMachine ${LANG_GERMAN} "Für alle Benutzer dieses PCs installieren$\r$\nWindows fragt nach Administratorrechten. Sevak wird unter Programme installiert."
LangString sevakScopeNameUser ${LANG_GERMAN} "dein Konto"
LangString sevakScopeNameMachine ${LANG_GERMAN} "alle Benutzer dieses PCs"
LangString sevakNeedAdmin ${LANG_GERMAN} "Zum Fortfahren sind Administratorrechte nötig. Wähle $\"Nur für mich installieren$\" oder versuche es erneut und erlaube die Windows-Abfrage."

; ---- Sevak is already installed ---------------------------------------------

LangString sevakInstalledTitle ${LANG_GERMAN} "Sevak ist bereits installiert"
LangString sevakInstalledSubtitle ${LANG_GERMAN} "Wähle, was du tun möchtest."
LangString sevakUpgradeText ${LANG_GERMAN} "Sevak $SameVer ist installiert für: $SameScopeName. Dieses Installationsprogramm enthält Sevak ${VERSION}.$\r$\n$\r$\nDeine Einstellungen und Daten bleiben erhalten."
LangString sevakReinstallText ${LANG_GERMAN} "Sevak ${VERSION} ist bereits installiert für: $SameScopeName.$\r$\n$\r$\nDeine Einstellungen und Daten bleiben erhalten."
LangString sevakDowngradeText ${LANG_GERMAN} "Ein neueres Sevak, $SameVer, ist installiert für: $SameScopeName. Dieses Installationsprogramm enthält das ältere Sevak ${VERSION}.$\r$\n$\r$\nDeine Einstellungen und Daten bleiben erhalten, aber ein älteres Sevak versteht vielleicht nicht jede Einstellung, die ein neueres gespeichert hat."
LangString sevakUnknownText ${LANG_GERMAN} "Sevak ist installiert für: $SameScopeName, aber seine Version konnte nicht gelesen werden. Dieses Installationsprogramm enthält Sevak ${VERSION}.$\r$\n$\r$\nDeine Einstellungen und Daten bleiben erhalten."
LangString sevakUpgradeButton ${LANG_GERMAN} "Auf Sevak ${VERSION} aktualisieren"
LangString sevakReinstallButton ${LANG_GERMAN} "Sevak ${VERSION} neu installieren"
LangString sevakDowngradeButton ${LANG_GERMAN} "Auf Sevak ${VERSION} zurückstufen (nicht empfohlen)"
LangString sevakUninstallButton ${LANG_GERMAN} "Sevak deinstallieren"
LangString sevakAlsoRemove ${LANG_GERMAN} "Auch die Kopie entfernen, die installiert ist für: $OtherScopeName (Sevak $OtherVer)"
LangString sevakOtherText ${LANG_GERMAN} "Sevak $OtherVer ist bereits installiert für: $OtherScopeName. Eine zusätzliche Installation für $SameScopeName ergäbe zwei Kopien, die um dasselbe Tastenkürzel konkurrieren.$\r$\n$\r$\nDeine Einstellungen und Daten werden geteilt und bleiben in jedem Fall erhalten."
LangString sevakMoveButton ${LANG_GERMAN} "Ersetzen: die andere Kopie entfernen und dann hier installieren (empfohlen)"
LangString sevakKeepBothButton ${LANG_GERMAN} "Beide Kopien behalten"

; ---- While installing -------------------------------------------------------

LangString sevakCloseRunning ${LANG_GERMAN} "Sevak läuft. Das Setup schließt es jetzt; danach kannst du es wieder starten.$\r$\n$\r$\nKlicke auf OK, um fortzufahren."
LangString sevakRemovingOther ${LANG_GERMAN} "Die Kopie für $OtherScopeName wird entfernt..."
LangString sevakRemoveOtherFailed ${LANG_GERMAN} "Die Kopie von Sevak für $OtherScopeName konnte nicht entfernt werden. Entferne sie unter Einstellungen > Apps und führe das Setup dann erneut aus."
LangString sevakRemovingMsi ${LANG_GERMAN} "Die frühere Windows Installer-Kopie (MSI) von Sevak wird entfernt..."
