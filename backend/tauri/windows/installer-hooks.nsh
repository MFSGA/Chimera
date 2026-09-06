; Windows uninstall hooks for Chimera.
;
; Keep normal updater replacements intact: Tauri invokes the uninstaller with
; /UPDATE during an in-place update, so every destructive cleanup below must be
; guarded by $UpdateMode <> 1.
;
; The structure mirrors ref's installer cleanup policy (service first, user data
; after Tauri removes the app files), while using Tauri's installerHooks surface
; instead of forking the full upstream installer.nsi template.

!macro CHIMERA_DELETE_CONFIG_CONTENTS ROOT
  RMDir /r "${ROOT}\profiles"
  RMDir /r "${ROOT}\runtime"
  Delete "${ROOT}\profiles.yaml"
  Delete "${ROOT}\chimera-config.yaml"
  Delete "${ROOT}\application.yaml"
  Delete "${ROOT}\session-state.yaml"
  Delete "${ROOT}\clash-config.yaml"
  Delete "${ROOT}\clash-guard-overrides.yaml"
  Delete "${ROOT}\migration-state.yaml"

  ; Legacy pre-typed-config names are intentionally included so a real uninstall
  ; does not leave migration inputs behind on machines upgraded from older builds.
  Delete "${ROOT}\verge.yaml"
  Delete "${ROOT}\clash.yaml"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ; Resolve ProgramData at uninstall time. NSIS does not expose a portable
    ; $COMMONAPPDATA variable in this template; ref resolves the same Windows
    ; known folder explicitly. The environment variable is present on supported
    ; Windows versions and lets this hook stay self-contained.
    ReadEnvStr $R3 "ProgramData"
    ${If} $R3 == ""
      DetailPrint "ProgramData is unavailable; skipping Chimera service data cleanup."
      Goto chimera_service_cleanup_done
    ${EndIf}

    ; chimera-service installs an elevated copy under
    ; %ProgramData%\chimera-service\data.  Do not use the bundled sidecar itself
    ; as the installation signal: it exists for every install and would make a
    ; normal uninstall request UAC even when service mode was never enabled.
    ReadRegStr $R0 HKLM "SYSTEM\CurrentControlSet\Services\moe.elaina.chimera-service" "ImagePath"
    IfFileExists "$R3\chimera-service\*.*" chimera_service_cleanup_needed 0
    ${If} $R0 == ""
      Goto chimera_service_cleanup_done
    ${EndIf}

    chimera_service_cleanup_needed:
      ; Prefer the service's installed copy, matching ref's teardown path.  If it
      ; is missing, use the still-bundled sidecar before Tauri removes app files.
      StrCpy $R1 "$R3\chimera-service\data\chimera-service.exe"
      IfFileExists "$R1" chimera_service_binary_ready 0
      StrCpy $R1 "$INSTDIR\chimera-service.exe"
      IfFileExists "$R1" chimera_service_binary_ready chimera_service_fallback

    chimera_service_binary_ready:
      ClearErrors
      ExecShellWait "runas" "$SYSDIR\cmd.exe" '/C ""$R1" uninstall >NUL 2>&1 & sc.exe stop "moe.elaina.chimera-service" >NUL 2>&1 & sc.exe delete "moe.elaina.chimera-service" >NUL 2>&1 & rmdir /S /Q "$R3\chimera-service""' SW_HIDE
      ${If} ${Errors}
        DetailPrint "Failed to launch elevated Chimera service cleanup; continuing uninstall best-effort."
      ${EndIf}
      Goto chimera_service_cleanup_done

    chimera_service_fallback:
      ; If both service binaries are already gone but SCM/data residue remains,
      ; remove those persistence points directly under one elevated command.
      ClearErrors
      ExecShellWait "runas" "$SYSDIR\cmd.exe" '/C "sc.exe stop "moe.elaina.chimera-service" >NUL 2>&1 & sc.exe delete "moe.elaina.chimera-service" >NUL 2>&1 & rmdir /S /Q "$R3\chimera-service""' SW_HIDE
      ${If} ${Errors}
        DetailPrint "Failed to launch elevated Chimera service fallback cleanup; continuing uninstall best-effort."
      ${EndIf}

    chimera_service_cleanup_done:
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Older Chimera builds bundled the service sidecar as nyanpasu-service.exe.
  ; It is an obsolete installation artifact, not user data, so remove it for
  ; both a real uninstall and Tauri's /UPDATE uninstall phase.
  Delete "$INSTDIR\nyanpasu-service.exe"

  ${If} $UpdateMode <> 1
    ; Chimera stores per-user integration state regardless of whether the bundle
    ; itself was installed per-user or per-machine.
    SetShellVarContext current

    ; auto_launch uses the executable stem as its Windows Run value. Keep the
    ; historical product spelling as compatibility cleanup for older builds.
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "chimera"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Chimera"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Clash Chimera"

    ; Deep-link registration is normally removed by Tauri's generated uninstaller,
    ; but runtime registration also targets HKCU. This fallback makes a real
    ; uninstall idempotently remove the Chimera-owned scheme.
    DeleteRegKey HKCU "Software\Classes\chimera"

    ; Match ref/Tauri uninstall semantics: application data is removed only when
    ; the user selects the "Delete app data" checkbox on the uninstall page.
    ${If} $DeleteAppDataCheckboxState = 1
      ; AppDir is an application-managed override and can point to an arbitrary
      ; absolute directory. Never recursively remove that root: delete only files
      ; and subdirectories Chimera owns, then remove the registry override itself.
      ReadRegStr $R8 HKCU "Software\Clash Chimera" "AppDir"
      ${If} $R8 != ""
        !insertmacro CHIMERA_DELETE_CONFIG_CONTENTS "$R8"
      ${EndIf}

      ; Current Windows path resolver:
      ;   config -> %APPDATA%\Clash Chimera\config
      ;   data   -> %LOCALAPPDATA%\Clash Chimera\data
      ; The parent folders are dedicated to Chimera, so remove them recursively.
      RMDir /r "$APPDATA\Clash Chimera"
      RMDir /r "$LOCALAPPDATA\Clash Chimera"

      ; Tauri/WebView2 state is keyed by the bundle identifier. Removing both
      ; roots covers EBWebView/localStorage as well as any app-local plugin state.
      RMDir /r "$APPDATA\com.chimera.app"
      RMDir /r "$LOCALAPPDATA\com.chimera.app"

      ; Portable builds resolve state inside the install directory. Delete only
      ; the Chimera-owned leaves/flag and remove parents only when they became empty.
      RMDir /r "$INSTDIR\.config\clash-chimera"
      RMDir /r "$INSTDIR\.data\clash-chimera"
      Delete "$INSTDIR\.config\PORTABLE"
      RMDir "$INSTDIR\.config"
      RMDir "$INSTDIR\.data"
      RMDir "$INSTDIR"

      ; Remove AppDir and any other Chimera-owned per-user registry metadata last,
      ; after the custom path has been consumed above.
      DeleteRegKey HKCU "Software\Clash Chimera"
    ${EndIf}
  ${EndIf}
!macroend
