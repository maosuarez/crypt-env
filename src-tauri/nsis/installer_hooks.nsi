; Custom hooks for the CryptEnv NSIS installer.
; Included by Tauri's generated NSIS script via tauri.conf.json nsis.installerHooks.

; --- PATH management (inlined to avoid relative-path issues at NSIS build time) ---

!include "WordFunc.nsh"

; AddToUserPath — appends $INSTDIR to HKCU\Environment\Path if not already present.
Function AddToUserPath
  Var /GLOBAL PathOld
  Var /GLOBAL PathNew

  ReadRegStr $PathOld HKCU "Environment" "Path"

  ; SAFETY GUARD — the default NSIS build caps strings at NSIS_MAX_STRLEN (1024).
  ; When the real PATH is longer, ReadRegStr SILENTLY TRUNCATES it. Writing that
  ; truncated value back would permanently destroy the tail of the user's PATH.
  ; If the value we read is at/near the cap, refuse to modify PATH and tell the
  ; user to add the folder manually. Better no feature than a wiped PATH.
  StrLen $R1 $PathOld
  IntCmp $R1 1000 path_too_long path_ok path_too_long

  path_ok:
  ; Check if already present (case-insensitive substring search).
  ${WordFind} "$PathOld" "$INSTDIR" "E+1{" $R0
  IfErrors 0 already_present

  ; Not present — append.
  StrCmp $PathOld "" 0 +3
    StrCpy $PathNew "$INSTDIR"
    Goto write_path
  StrCpy $PathNew "$PathOld;$INSTDIR"

  write_path:
    WriteRegExpandStr HKCU "Environment" "Path" "$PathNew"
    ; Notify all windows (Explorer, open shells) that the environment changed.
    SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
    Goto already_present

  path_too_long:
    MessageBox MB_OK|MB_ICONINFORMATION "CryptEnv did NOT modify your PATH because it is too long to edit safely (a known Windows installer limit).$\r$\n$\r$\nTo run 'crypt-env' from any terminal, add this folder to your PATH manually:$\r$\n$\r$\n$INSTDIR"

  already_present:
FunctionEnd

; RemoveFromUserPath — removes $INSTDIR from HKCU\Environment\Path.
Function un.RemoveFromUserPath
  Var /GLOBAL UnPathOld
  Var /GLOBAL UnPathNew

  ReadRegStr $UnPathOld HKCU "Environment" "Path"

  ; SAFETY GUARD — same NSIS_MAX_STRLEN truncation risk as AddToUserPath. If the
  ; read value is at/near the cap it was truncated; rewriting it would destroy the
  ; tail of PATH. Skip cleanup entirely in that case (leaving a stale entry is
  ; harmless; destroying PATH is not).
  StrLen $R1 $UnPathOld
  IntCmp $R1 1000 un_path_done un_path_ok un_path_done

  un_path_ok:
  ; Remove all occurrences of $INSTDIR (with surrounding semicolons).
  ${WordReplace} "$UnPathOld" "$INSTDIR" "" "+" $UnPathNew

  ; Clean up any double semicolons left behind.
  ${WordReplace} "$UnPathNew" ";;" ";" "+" $R0
  StrCpy $UnPathNew $R0

  ; Trim leading/trailing semicolons.
  StrCpy $R0 $UnPathNew 1
  StrCmp $R0 ";" 0 +2
    StrCpy $UnPathNew $UnPathNew "" 1
  StrLen $R0 $UnPathNew
  IntOp $R0 $R0 - 1
  StrCpy $R0 $UnPathNew 1 $R0
  StrCmp $R0 ";" 0 +2
    StrCpy $UnPathNew $UnPathNew -1

  WriteRegExpandStr HKCU "Environment" "Path" "$UnPathNew"
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000

  un_path_done:
FunctionEnd

; --- Installer sections ---

; Called by Tauri after installation completes.
Section "PATH Registration" SecPath
  Call AddToUserPath
SectionEnd

; Called by Tauri's uninstaller.
Section "un.PATH Cleanup" SecUnPath
  Call un.RemoveFromUserPath
SectionEnd
