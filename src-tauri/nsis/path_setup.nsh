; PATH management for CryptEnv installer
; Modifies HKCU\Environment\Path — no elevation required (currentUser install).

!ifndef PATH_SETUP_NSH
!define PATH_SETUP_NSH

!include "WordFunc.nsh"

; AddToUserPath — appends $INSTDIR to HKCU\Environment\Path if not already present.
Function AddToUserPath
  Var /GLOBAL PathOld
  Var /GLOBAL PathNew

  ReadRegStr $PathOld HKCU "Environment" "Path"

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

  already_present:
FunctionEnd

; RemoveFromUserPath — removes $INSTDIR from HKCU\Environment\Path.
Function un.RemoveFromUserPath
  Var /GLOBAL UnPathOld
  Var /GLOBAL UnPathNew

  ReadRegStr $UnPathOld HKCU "Environment" "Path"

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
FunctionEnd

!endif ; PATH_SETUP_NSH
