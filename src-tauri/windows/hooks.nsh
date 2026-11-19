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

!macro PANORAMA_KILL_SIDECAR
  nsExec::Exec `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-Process sidecar -ErrorAction SilentlyContinue | Where-Object { $$_.Path -eq '$INSTDIR\sidecar.exe' } | Stop-Process -Force -PassThru | Wait-Process -Timeout 5 -ErrorAction SilentlyContinue"`
  Pop $0
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro PANORAMA_RETIRE "sidecar.exe"
  !insertmacro PANORAMA_RETIRE "conpty.dll"
  !insertmacro PANORAMA_RETIRE "OpenConsole.exe"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    !insertmacro PANORAMA_KILL_SIDECAR
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    Delete /REBOOTOK "$INSTDIR\*.old-*"
    RMDir /REBOOTOK "$INSTDIR"
  ${EndIf}
!macroend
