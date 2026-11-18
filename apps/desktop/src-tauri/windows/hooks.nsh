; Étapes ajoutées à l'installeur NSIS de Tauri.

; Désinstallation : le menu clic droit (clés HKCU) est retiré s'il lance ce programme-ci.
; Pas lors d'une mise à jour (/UPDATE) : le nouveau programme est installé au même endroit et
; le menu reste valable. Le programme ne fait que ça avec cette option, sans ouvrir de fenêtre.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\chud-converter.exe" --uninstall-menu'
  ${EndIf}
!macroend
