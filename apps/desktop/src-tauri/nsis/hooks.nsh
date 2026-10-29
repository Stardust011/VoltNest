!include LogicLib.nsh

!macro NSIS_HOOK_POSTUNINSTALL
  ; 只有用户在卸载器中勾选“删除应用数据”时才执行
  ${If} $DeleteAppDataCheckboxState == 1
    SetShellVarContext current
    RMDir /r "$APPDATA\VoltNest"
  ${EndIf}
!macroend