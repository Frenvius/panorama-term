!macro PANORAMA_KILL_SIDECAR
  nsExec::Exec `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-Process sidecar -ErrorAction SilentlyContinue | Where-Object { $$_.Path -eq '$INSTDIR\sidecar.exe' } | Stop-Process -Force -PassThru | Wait-Process -Timeout 5 -ErrorAction SilentlyContinue"`
  Pop $0
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro PANORAMA_KILL_SIDECAR
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro PANORAMA_KILL_SIDECAR
!macroend
