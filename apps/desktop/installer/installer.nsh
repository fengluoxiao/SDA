!include "LogicLib.nsh"
!include "nsDialogs.nsh"

!ifndef BUILD_UNINSTALLER
Var SdaHeadTrackingPage
Var SdaEnableTestSigningCheckbox
Var SdaInstallDriverCheckbox
Var SdaEnableTestSigning
Var SdaInstallDriver

!macro customPageAfterChangeDir
  Page custom SdaHeadTrackingOptionsCreate SdaHeadTrackingOptionsLeave
!macroend

Function SdaHeadTrackingOptionsCreate
  nsDialogs::Create 1018
  Pop $SdaHeadTrackingPage
  ${If} $SdaHeadTrackingPage == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0 0 100% 24u "AirPods head tracking (experimental)"
  Pop $0

  ${NSD_CreateLabel} 0 24u 100% 30u "These system changes are optional. Both options require administrator approval; TestSigning requires a Windows restart."
  Pop $0

  ${NSD_CreateCheckbox} 0 62u 100% 12u "Enable Windows TestSigning"
  Pop $SdaEnableTestSigningCheckbox
  ${NSD_Uncheck} $SdaEnableTestSigningCheckbox

  ${NSD_CreateCheckbox} 0 84u 100% 12u "Install the SDA AirPods head-tracking driver"
  Pop $SdaInstallDriverCheckbox
  ${NSD_Uncheck} $SdaInstallDriverCheckbox

  ${NSD_CreateLabel} 0 108u 100% 42u "The driver is test-signed and limited to supported AirPods models. Enabling TestSigning reduces Windows driver-signature enforcement. No option restarts Windows automatically."
  Pop $0

  nsDialogs::Show
FunctionEnd

Function SdaHeadTrackingOptionsLeave
  ${NSD_GetState} $SdaEnableTestSigningCheckbox $SdaEnableTestSigning
  ${NSD_GetState} $SdaInstallDriverCheckbox $SdaInstallDriver
FunctionEnd

!macro customInstall
  ${If} $SdaEnableTestSigning == ${BST_CHECKED}
  ${OrIf} $SdaInstallDriver == ${BST_CHECKED}
    StrCpy $0 ""
    ${If} $SdaEnableTestSigning == ${BST_CHECKED}
      StrCpy $0 "$0 -EnableTestSigning"
    ${EndIf}
    ${If} $SdaInstallDriver == ${BST_CHECKED}
      StrCpy $0 "$0 -InstallDriver"
    ${EndIf}

    ExecWait '\"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe\" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"$INSTDIR\resources\head-tracking-driver\install-head-tracking-driver.ps1\"$0' $1
    ${If} $1 != 0
      MessageBox MB_OK|MB_ICONEXCLAMATION "SDA was installed, but the selected head-tracking system setup did not complete (error $1). See SDA\Logs\head-tracking-driver-install.log under the Windows ProgramData folder."
    ${Else}
      MessageBox MB_OK|MB_ICONINFORMATION "The selected head-tracking setup completed. Restart Windows before using the driver."
    ${EndIf}
  ${EndIf}
!macroend
!endif
