; Custom hooks for the CryptEnv NSIS installer.
; Included by Tauri's generated NSIS script via tauri.conf.json nsis.include.

!include "path_setup.nsh"

; Called by Tauri after installation completes.
Section "PATH Registration" SecPath
  Call AddToUserPath
SectionEnd

; Called by Tauri's uninstaller.
Section "un.PATH Cleanup" SecUnPath
  Call un.RemoveFromUserPath
SectionEnd
