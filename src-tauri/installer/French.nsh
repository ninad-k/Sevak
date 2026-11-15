; Sevak's installer text in French (machine-drafted, needs native review).
;
; Registered through bundle.windows.nsis.customLanguageFiles. It defines every
; LangString of English.nsh, with the same variables ($SameVer, ${VERSION}...);
; scripts/installer.test.mjs checks both. Sevak addresses the user with "vous",
; as in the app. See docs/localisation.md.

LangString addOrReinstall ${LANG_FRENCH} "Ajouter/réinstaller des composants"
LangString alreadyInstalled ${LANG_FRENCH} "Déjà installé"
LangString alreadyInstalledLong ${LANG_FRENCH} "${PRODUCTNAME} ${VERSION} est déjà installé. Choisissez l'opération à effectuer, puis cliquez sur Suivant pour continuer."
LangString appRunning ${LANG_FRENCH} "{{product_name}} est en cours d'exécution. Fermez-le, puis réessayez."
LangString appRunningOkKill ${LANG_FRENCH} "{{product_name}} est toujours en cours d'exécution.$\nCliquez sur OK pour le fermer et continuer."
LangString chooseMaintenanceOption ${LANG_FRENCH} "Choisissez l'opération de maintenance à effectuer."
LangString choowHowToInstall ${LANG_FRENCH} "Choisissez comment installer ${PRODUCTNAME}."
LangString createDesktop ${LANG_FRENCH} "Créer un raccourci sur le bureau"
LangString dontUninstall ${LANG_FRENCH} "Ne pas désinstaller"
LangString dontUninstallDowngrade ${LANG_FRENCH} "Ne pas désinstaller (revenir à une version antérieure sans désinstaller est désactivé pour ce programme d'installation)"
LangString failedToKillApp ${LANG_FRENCH} "Impossible de fermer {{product_name}}. Fermez-le, puis réessayez."
LangString installingWebview2 ${LANG_FRENCH} "Installation de WebView2..."
LangString newerVersionInstalled ${LANG_FRENCH} "Une version plus récente de ${PRODUCTNAME} est déjà installée. Il est déconseillé d'installer une version plus ancienne. Si vous voulez vraiment installer cette version plus ancienne, mieux vaut d'abord désinstaller la version actuelle. Choisissez l'opération à effectuer, puis cliquez sur Suivant pour continuer."
LangString older ${LANG_FRENCH} "plus ancienne"
LangString olderOrUnknownVersionInstalled ${LANG_FRENCH} "Une version $R4 de ${PRODUCTNAME} est installée sur votre système. Il est recommandé de désinstaller la version actuelle avant d'installer. Choisissez l'opération à effectuer, puis cliquez sur Suivant pour continuer."
LangString silentDowngrades ${LANG_FRENCH} "Le retour à une version antérieure est désactivé pour ce programme d'installation : l'installation silencieuse ne peut pas continuer. Utilisez plutôt le programme d'installation avec interface graphique.$\n"
LangString unableToUninstall ${LANG_FRENCH} "Impossible de désinstaller !"
LangString uninstallApp ${LANG_FRENCH} "Désinstaller ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_FRENCH} "Désinstaller avant d'installer"
LangString unknown ${LANG_FRENCH} "inconnue"
LangString webview2AbortError ${LANG_FRENCH} "Impossible d'installer WebView2 ! L'application ne peut pas fonctionner sans lui. Relancez le programme d'installation."
LangString webview2DownloadError ${LANG_FRENCH} "Erreur : le téléchargement de WebView2 a échoué - $0"
LangString webview2DownloadSuccess ${LANG_FRENCH} "Programme d'amorçage WebView2 téléchargé"
LangString webview2Downloading ${LANG_FRENCH} "Téléchargement du programme d'amorçage WebView2..."
LangString webview2InstallError ${LANG_FRENCH} "Erreur : l'installation de WebView2 a échoué avec le code de sortie $1"
LangString webview2InstallSuccess ${LANG_FRENCH} "WebView2 installé"
LangString deleteAppData ${LANG_FRENCH} "Supprimer aussi le cache web (vos paramètres sont conservés)"

; ---- Welcome and Finish pages ----------------------------------------------

LangString sevakWelcomeTitle ${LANG_FRENCH} "Bienvenue dans Sevak"
LangString sevakWelcomeText ${LANG_FRENCH} "Sevak signifie $\"celui qui sert$\". C'est un lanceur pensé pour le clavier : appuyez sur une touche, tapez quelques lettres, et ouvrez une application, un fichier, un calcul ou une recherche web.$\r$\n$\r$\nVotre bureau. À votre service.$\r$\n$\r$\nCet assistant installe Sevak ${VERSION}. Sevak ne collecte aucune télémétrie.$\r$\n$\r$\nCliquez sur Suivant pour continuer."

LangString sevakFinishTitle ${LANG_FRENCH} "Sevak est prêt"
LangString sevakFinishText ${LANG_FRENCH} "Sevak attend discrètement dans la zone de notification.$\r$\n$\r$\nAppuyez sur Win+Espace pour l'ouvrir (un raccourci existant est conservé). Cliquez sur l'icône de la zone de notification pour les paramètres.$\r$\n$\r$\nEssayez : un nom d'application, 12*7, f report, g rust."
LangString sevakRunNow ${LANG_FRENCH} "Démarrer Sevak maintenant"
LangString sevakOpenSettings ${LANG_FRENCH} "Ouvrir les paramètres de Sevak"
LangString sevakDesktopShortcut ${LANG_FRENCH} "Créer un raccourci sur le bureau"
LangString sevakStartAtSignIn ${LANG_FRENCH} "Démarrer Sevak à l'ouverture de ma session"
LangString sevakStartupHint ${LANG_FRENCH} "Pour votre compte uniquement, même pour une installation pour tous les utilisateurs. Sevak démarre discrètement dans la zone de notification. Modifiable plus tard dans Paramètres > Général."
LangString sevakStartupElevatedHint ${LANG_FRENCH} "Pour choisir le démarrage de votre compte, lancez ce programme d'installation normalement (sans Exécuter en tant qu'administrateur), ou modifiez Paramètres > Général après l'installation."
LangString sevakStartupFailed ${LANG_FRENCH} "Sevak est installé, mais votre préférence de démarrage n'a pas pu être enregistrée. Ouvrez Sevak > Paramètres > Général pour régler 'Démarrer Sevak à l'ouverture de ma session'."
LangString sevakStartupRemoveFailed ${LANG_FRENCH} "Votre entrée de démarrage à l'ouverture de session n'a pas pu être vérifiée. Si Sevak figure toujours dans Paramètres Windows > Applications > Démarrage, désactivez-le à cet endroit. Les préférences des autres comptes n'ont pas été modifiées."

; ---- Who is Sevak for? ------------------------------------------------------

LangString sevakScopeTitle ${LANG_FRENCH} "Pour qui installer Sevak ?"
LangString sevakScopeSubtitle ${LANG_FRENCH} "Choisissez où installer Sevak."
LangString sevakScopeIntro ${LANG_FRENCH} "Installez Sevak pour votre compte seulement, ou pour toutes les personnes qui utilisent ce PC."
LangString sevakScopeUser ${LANG_FRENCH} "Installer pour moi seulement (recommandé)$\r$\nAucune autorisation d'administrateur n'est nécessaire. Sevak est installé dans votre profil utilisateur."
LangString sevakScopeMachine ${LANG_FRENCH} "Installer pour tous les utilisateurs de ce PC$\r$\nWindows demande une autorisation d'administrateur. Sevak est installé dans Program Files."
LangString sevakScopeNameUser ${LANG_FRENCH} "votre compte"
LangString sevakScopeNameMachine ${LANG_FRENCH} "tous les utilisateurs de ce PC"
LangString sevakNeedAdmin ${LANG_FRENCH} "Une autorisation d'administrateur est nécessaire pour continuer. Choisissez $\"Installer pour moi seulement$\", ou réessayez et acceptez la demande de Windows."

; ---- Sevak is already installed ---------------------------------------------

LangString sevakInstalledTitle ${LANG_FRENCH} "Sevak est déjà installé"
LangString sevakInstalledSubtitle ${LANG_FRENCH} "Choisissez ce que vous voulez faire."
LangString sevakUpgradeText ${LANG_FRENCH} "Sevak $SameVer est installé pour $SameScopeName. Ce programme d'installation contient Sevak ${VERSION}.$\r$\n$\r$\nVos paramètres et vos données sont conservés."
LangString sevakReinstallText ${LANG_FRENCH} "Sevak ${VERSION} est déjà installé pour $SameScopeName.$\r$\n$\r$\nVos paramètres et vos données sont conservés."
LangString sevakDowngradeText ${LANG_FRENCH} "Un Sevak plus récent, $SameVer, est installé pour $SameScopeName. Ce programme d'installation contient Sevak ${VERSION}, plus ancien.$\r$\n$\r$\nVos paramètres et vos données sont conservés, mais un Sevak plus ancien risque de ne pas comprendre tous les réglages enregistrés par un plus récent."
LangString sevakUnknownText ${LANG_FRENCH} "Sevak est installé pour $SameScopeName, mais sa version n'a pas pu être lue. Ce programme d'installation contient Sevak ${VERSION}.$\r$\n$\r$\nVos paramètres et vos données sont conservés."
LangString sevakUpgradeButton ${LANG_FRENCH} "Mettre à jour vers Sevak ${VERSION}"
LangString sevakReinstallButton ${LANG_FRENCH} "Réinstaller Sevak ${VERSION}"
LangString sevakDowngradeButton ${LANG_FRENCH} "Revenir à Sevak ${VERSION} (déconseillé)"
LangString sevakUninstallButton ${LANG_FRENCH} "Désinstaller Sevak"
LangString sevakAlsoRemove ${LANG_FRENCH} "Supprimer aussi la copie installée pour $OtherScopeName (Sevak $OtherVer)"
LangString sevakOtherText ${LANG_FRENCH} "Sevak $OtherVer est déjà installé pour $OtherScopeName. L'installer aussi pour $SameScopeName laisserait deux copies qui se disputent le même raccourci.$\r$\n$\r$\nVos paramètres et vos données sont partagés et conservés dans les deux cas."
LangString sevakMoveButton ${LANG_FRENCH} "Remplacer : supprimer l'autre copie, puis installer ici (recommandé)"
LangString sevakKeepBothButton ${LANG_FRENCH} "Garder les deux copies"

; ---- While installing -------------------------------------------------------

LangString sevakCloseRunning ${LANG_FRENCH} "Sevak est en cours d'exécution. Le programme d'installation va le fermer ; vous pourrez le redémarrer ensuite.$\r$\n$\r$\nCliquez sur OK pour continuer."
LangString sevakRemovingOther ${LANG_FRENCH} "Suppression de la copie installée pour $OtherScopeName..."
LangString sevakRemoveOtherFailed ${LANG_FRENCH} "La copie de Sevak installée pour $OtherScopeName n'a pas pu être supprimée. Supprimez-la dans Paramètres > Applications, puis relancez l'installation."
LangString sevakRemovingMsi ${LANG_FRENCH} "Suppression de l'ancienne copie de Sevak installée par Windows Installer (MSI)..."
