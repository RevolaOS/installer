!macro NSIS_HOOK_POSTINSTALL
  Push $0
  FileOpen $0 "$INSTDIR\.revolaos-nsis" w
  FileWriteUTF16LE $0 "$INSTDIR"
  FileClose $0
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\.revolaos-nsis"
!macroend
