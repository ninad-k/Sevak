; Sevak's installer text in Spanish (machine-drafted, needs native review).
;
; Registered through bundle.windows.nsis.customLanguageFiles. It defines every
; LangString of English.nsh, with the same variables ($SameVer, ${VERSION}...);
; scripts/installer.test.mjs checks both. Sevak addresses the user with "tú",
; as in the app. See docs/localisation.md.

LangString addOrReinstall ${LANG_SPANISH} "Añadir o reinstalar componentes"
LangString alreadyInstalled ${LANG_SPANISH} "Ya está instalado"
LangString alreadyInstalledLong ${LANG_SPANISH} "${PRODUCTNAME} ${VERSION} ya está instalado. Elige la operación que quieres realizar y haz clic en Siguiente para continuar."
LangString appRunning ${LANG_SPANISH} "{{product_name}} está en ejecución. Ciérralo y vuelve a intentarlo."
LangString appRunningOkKill ${LANG_SPANISH} "{{product_name}} sigue en ejecución.$\nHaz clic en Aceptar para cerrarlo y continuar."
LangString chooseMaintenanceOption ${LANG_SPANISH} "Elige la operación de mantenimiento que quieres realizar."
LangString choowHowToInstall ${LANG_SPANISH} "Elige cómo quieres instalar ${PRODUCTNAME}."
LangString createDesktop ${LANG_SPANISH} "Crear un acceso directo en el escritorio"
LangString dontUninstall ${LANG_SPANISH} "No desinstalar"
LangString dontUninstallDowngrade ${LANG_SPANISH} "No desinstalar (este instalador no permite volver a una versión anterior sin desinstalar)"
LangString failedToKillApp ${LANG_SPANISH} "No se pudo cerrar {{product_name}}. Ciérralo y vuelve a intentarlo."
LangString installingWebview2 ${LANG_SPANISH} "Instalando WebView2..."
LangString newerVersionInstalled ${LANG_SPANISH} "Ya hay instalada una versión más reciente de ${PRODUCTNAME}. No se recomienda instalar una versión anterior. Si de verdad quieres instalar esta versión anterior, es mejor desinstalar antes la versión actual. Elige la operación que quieres realizar y haz clic en Siguiente para continuar."
LangString older ${LANG_SPANISH} "anterior"
LangString olderOrUnknownVersionInstalled ${LANG_SPANISH} "Hay una versión $R4 de ${PRODUCTNAME} instalada en tu sistema. Se recomienda desinstalar la versión actual antes de instalar. Elige la operación que quieres realizar y haz clic en Siguiente para continuar."
LangString silentDowngrades ${LANG_SPANISH} "Este instalador no permite volver a una versión anterior, así que la instalación silenciosa no puede continuar. Usa el instalador con interfaz gráfica.$\n"
LangString unableToUninstall ${LANG_SPANISH} "¡No se pudo desinstalar!"
LangString uninstallApp ${LANG_SPANISH} "Desinstalar ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_SPANISH} "Desinstalar antes de instalar"
LangString unknown ${LANG_SPANISH} "desconocida"
LangString webview2AbortError ${LANG_SPANISH} "¡No se pudo instalar WebView2! La aplicación no puede funcionar sin él. Prueba a reiniciar el instalador."
LangString webview2DownloadError ${LANG_SPANISH} "Error: no se pudo descargar WebView2 - $0"
LangString webview2DownloadSuccess ${LANG_SPANISH} "Programa de arranque de WebView2 descargado correctamente"
LangString webview2Downloading ${LANG_SPANISH} "Descargando el programa de arranque de WebView2..."
LangString webview2InstallError ${LANG_SPANISH} "Error: la instalación de WebView2 falló con el código de salida $1"
LangString webview2InstallSuccess ${LANG_SPANISH} "WebView2 instalado correctamente"
LangString deleteAppData ${LANG_SPANISH} "Borrar también la caché web (tus ajustes se conservan)"

; ---- Welcome and Finish pages ----------------------------------------------

LangString sevakWelcomeTitle ${LANG_SPANISH} "Te damos la bienvenida a Sevak"
LangString sevakWelcomeText ${LANG_SPANISH} "Sevak significa $\"el que sirve$\". Es un lanzador pensado para el teclado: pulsa una tecla, escribe unas letras y abre una aplicación, un archivo, un cálculo o una búsqueda web.$\r$\n$\r$\nTu escritorio. A tu servicio.$\r$\n$\r$\nEste asistente instala Sevak ${VERSION}. Sevak no recopila telemetría.$\r$\n$\r$\nHaz clic en Siguiente para continuar."

LangString sevakFinishTitle ${LANG_SPANISH} "Sevak está listo"
LangString sevakFinishText ${LANG_SPANISH} "Sevak espera en silencio en la bandeja del sistema.$\r$\n$\r$\nPulsa Win+Espacio para abrirlo (se conserva un atajo que ya exista). Haz clic en el icono de la bandeja para abrir los Ajustes.$\r$\n$\r$\nPrueba: el nombre de una aplicación, 12*7, f report, g rust."
LangString sevakRunNow ${LANG_SPANISH} "Iniciar Sevak ahora"
LangString sevakOpenSettings ${LANG_SPANISH} "Abrir los Ajustes de Sevak"
LangString sevakDesktopShortcut ${LANG_SPANISH} "Crear un acceso directo en el escritorio"
LangString sevakStartAtSignIn ${LANG_SPANISH} "Iniciar Sevak al iniciar sesión"
LangString sevakStartupHint ${LANG_SPANISH} "Solo para tu cuenta, también en una instalación para todos los usuarios. Sevak se inicia en silencio en la bandeja. Puedes cambiarlo más tarde en Ajustes > General."
LangString sevakStartupElevatedHint ${LANG_SPANISH} "Para elegir el inicio de tu cuenta, abre este instalador de la forma normal (sin Ejecutar como administrador), o cambia Ajustes > General después de la instalación."
LangString sevakStartupFailed ${LANG_SPANISH} "Sevak se instaló, pero no se pudo guardar tu preferencia de inicio. Abre Sevak > Ajustes > General para configurar 'Iniciar Sevak al iniciar sesión'."
LangString sevakStartupRemoveFailed ${LANG_SPANISH} "No se pudo comprobar tu entrada de inicio de sesión. Si Sevak sigue apareciendo en Configuración de Windows > Aplicaciones > Inicio, desactívalo allí. No se cambiaron las preferencias de otras cuentas."

; ---- Who is Sevak for? ------------------------------------------------------

LangString sevakScopeTitle ${LANG_SPANISH} "¿Para quién es Sevak?"
LangString sevakScopeSubtitle ${LANG_SPANISH} "Elige dónde instalar Sevak."
LangString sevakScopeIntro ${LANG_SPANISH} "Instala Sevak solo para tu cuenta, o para todas las personas que usan este equipo."
LangString sevakScopeUser ${LANG_SPANISH} "Instalar solo para mí (recomendado)$\r$\nNo hace falta permiso de administrador. Sevak se instala en tu perfil de usuario."
LangString sevakScopeMachine ${LANG_SPANISH} "Instalar para todos los usuarios de este equipo$\r$\nWindows pide permiso de administrador. Sevak se instala en Archivos de programa."
LangString sevakScopeNameUser ${LANG_SPANISH} "tu cuenta"
LangString sevakScopeNameMachine ${LANG_SPANISH} "todos los usuarios de este equipo"
LangString sevakNeedAdmin ${LANG_SPANISH} "Hace falta permiso de administrador para continuar. Elige $\"Instalar solo para mí$\", o vuelve a intentarlo y acepta el aviso de Windows."

; ---- Sevak is already installed ---------------------------------------------

LangString sevakInstalledTitle ${LANG_SPANISH} "Sevak ya está instalado"
LangString sevakInstalledSubtitle ${LANG_SPANISH} "Elige qué quieres hacer."
LangString sevakUpgradeText ${LANG_SPANISH} "Sevak $SameVer está instalado para $SameScopeName. Este instalador contiene Sevak ${VERSION}.$\r$\n$\r$\nTus ajustes y datos se conservan."
LangString sevakReinstallText ${LANG_SPANISH} "Sevak ${VERSION} ya está instalado para $SameScopeName.$\r$\n$\r$\nTus ajustes y datos se conservan."
LangString sevakDowngradeText ${LANG_SPANISH} "Hay un Sevak más reciente, $SameVer, instalado para $SameScopeName. Este instalador contiene el Sevak ${VERSION}, más antiguo.$\r$\n$\r$\nTus ajustes y datos se conservan, pero es posible que un Sevak más antiguo no entienda todos los ajustes que guardó uno más reciente."
LangString sevakUnknownText ${LANG_SPANISH} "Sevak está instalado para $SameScopeName, pero no se pudo leer su versión. Este instalador contiene Sevak ${VERSION}.$\r$\n$\r$\nTus ajustes y datos se conservan."
LangString sevakUpgradeButton ${LANG_SPANISH} "Actualizar a Sevak ${VERSION}"
LangString sevakReinstallButton ${LANG_SPANISH} "Reinstalar Sevak ${VERSION}"
LangString sevakDowngradeButton ${LANG_SPANISH} "Volver a Sevak ${VERSION} (no recomendado)"
LangString sevakUninstallButton ${LANG_SPANISH} "Desinstalar Sevak"
LangString sevakAlsoRemove ${LANG_SPANISH} "Quitar también la copia instalada para $OtherScopeName (Sevak $OtherVer)"
LangString sevakOtherText ${LANG_SPANISH} "Sevak $OtherVer ya está instalado para $OtherScopeName. Instalarlo también para $SameScopeName dejaría dos copias que compiten por el mismo atajo.$\r$\n$\r$\nTus ajustes y datos se comparten y se conservan en ambos casos."
LangString sevakMoveButton ${LANG_SPANISH} "Reemplazar: quitar la otra copia e instalar aquí (recomendado)"
LangString sevakKeepBothButton ${LANG_SPANISH} "Conservar las dos copias"

; ---- While installing -------------------------------------------------------

LangString sevakCloseRunning ${LANG_SPANISH} "Sevak está en ejecución. El instalador lo cerrará ahora; podrás volver a iniciarlo después.$\r$\n$\r$\nHaz clic en Aceptar para continuar."
LangString sevakRemovingOther ${LANG_SPANISH} "Quitando la copia instalada para $OtherScopeName..."
LangString sevakRemoveOtherFailed ${LANG_SPANISH} "No se pudo quitar la copia de Sevak instalada para $OtherScopeName. Quítala en Configuración > Aplicaciones y vuelve a ejecutar el instalador."
LangString sevakRemovingMsi ${LANG_SPANISH} "Quitando la copia anterior de Sevak instalada con Windows Installer (MSI)..."
