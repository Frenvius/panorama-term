; A running image can be renamed but not overwritten, so files still held open by surviving
; agent hooks move aside and the app deletes the leftovers on its next start
Function PanoramaRetireFile
  Exch $0
  Push $1
  StrCpy $1 0
  IfFileExists "$0" 0 retire_done
  ClearErrors
  Delete "$0"
  IfErrors 0 retire_done
  retire_next:
    IntOp $1 $1 + 1
    IfFileExists "$0.old-$1" retire_next
  Rename "$0" "$0.old-$1"
  retire_done:
  Pop $1
  Pop $0
FunctionEnd

!macro PANORAMA_RETIRE FILE
  Push "$INSTDIR\${FILE}"
  Call PanoramaRetireFile
!macroend

!macro NSIS_HOOK_PREINSTALL
  ; sidecar.exe is left by builds before the binary split; the updater never removes files that left the bundle
  !insertmacro PANORAMA_RETIRE "sidecar.exe"
  !insertmacro PANORAMA_RETIRE "panorama-host.exe"
  !insertmacro PANORAMA_RETIRE "panorama-brain.exe"
  !insertmacro PANORAMA_RETIRE "panorama-cli.exe"
  !insertmacro PANORAMA_RETIRE "conpty.dll"
  !insertmacro PANORAMA_RETIRE "OpenConsole.exe"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    Delete /REBOOTOK "$INSTDIR\*.old-*"
    RMDir /REBOOTOK "$INSTDIR"
  ${EndIf}
!macroend
