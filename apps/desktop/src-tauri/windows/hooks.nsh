; Hooks of the NSIS installer (bundle.windows.nsis.installerHooks in tauri.conf.json).
;
; Versions up to 0.2.0 installed an MCP server beside the app, platypus-mcp.exe; the app now
; serves MCP itself (platypus-db.exe --mcp). The installer no longer ships the old server,
; so it removes it on upgrade, in silent mode too: agent registrations would otherwise keep
; starting the old code. The uninstaller removes it as well, so that the install folder is
; left empty and goes away. /REBOOTOK: a running server is removed at the next restart.

!macro NSIS_HOOK_PREINSTALL
  Delete /REBOOTOK "$INSTDIR\platypus-mcp.exe"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Delete /REBOOTOK "$INSTDIR\platypus-mcp.exe"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  RMDir "$INSTDIR"
!macroend
