; ============================================================================
;  BiFlow NSIS installer hooks
;
;  Two concerns live here:
;    1. the privileged SYSTEM Helper (must succeed or the install aborts), and
;    2. the optional Windows 11 Widgets Board package (must NEVER abort).
;
;  The asymmetry is deliberate. A machine without a working Helper cannot
;  connect, so a Helper failure is a real install failure. A machine where the
;  widget package does not apply (Windows 10, no developer mode, an unsigned
;  package the user has not trusted) still has a perfectly good BiFlow
;  installation, so the widget step is best-effort and reports, but does not
;  gate.
;
;  Only plain NSIS control flow is used here - no LogicLib, because the Tauri
;  NSIS template does not include it. A LogicLib conditional here is a compile
;  error, so use labels with IfFileExists / StrCmp / Goto instead.
; ============================================================================

!macro NSIS_HOOK_POSTINSTALL
  ;  $COMMONPROGRAMDATA is the NSIS constant for C:\ProgramData. There is no
  ;  $PROGRAMDATA: NSIS drops an unknown "$..." token as empty, which silently
  ;  left --staging-dir valueless. The helper hard-requires it
  ;  (crates\iran-split-helper\src\main.rs), so the install aborted at the end.
  nsExec::ExecToLog '"$INSTDIR\helper\iran-split-helper.exe" --install --mihomo "$INSTDIR\dependencies\mihomo.exe" --staging-dir "$COMMONPROGRAMDATA\iran-split\staging" --tun-name clash-iran'
  Pop $R0
  StrCmp $R0 "0" helper_install_completed
  DetailPrint "BiFlow Helper installation failed (exit: $R0)."
  IfSilent +2
    MessageBox MB_OK|MB_ICONSTOP "BiFlow Helper installation failed (exit: $R0). The application is not ready to connect."
  SetErrorLevel 1
  Abort "BiFlow Helper installation failed"
  helper_install_completed:

  ; --------------------------------------------------------------------------
  ;  Windows 11 Widgets Board package (optional, best-effort).
  ;
  ;  The MSIX only exists when the release pipeline ran the widget build
  ;  (widgets\WindowsWidgetProvider\build.ps1 -Stage) before bundling. A source
  ;  build, or a build on a machine without the .NET SDK, ships an empty
  ;  widget\ directory and this block is skipped.
  ; --------------------------------------------------------------------------
  ClearErrors
  IfFileExists "$INSTDIR\widget\BiFlow.WidgetProvider.msix" 0 widget_absent

  DetailPrint "Installing the BiFlow Windows 11 widget..."

  ;  Add-AppxPackage rejects an untrusted package and its error text is
  ;  unreadable in the installer log, so capture it and print a human sentence.
  ;  Tee-Object keeps the text on stdout for nsExec while also writing the file.
  ;  The log path is quoted because $TEMP contains the user profile name.
  ;  nsExec::ExecToStack pushes: exit code, then output, then the command.
  nsExec::ExecToStack 'powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Add-AppxPackage -Path ''$INSTDIR\widget\BiFlow.WidgetProvider.msix'' -ErrorAction Stop *>&1 | Tee-Object -FilePath ''$TEMP\biflow-widget-install.log''"'
  Pop $R0
  Pop $R1
  Pop $R2

  StrCmp $R0 "0" widget_installed

  DetailPrint "  The BiFlow widget could not be registered automatically."
  DetailPrint "  Most likely: Windows 10, developer mode is off, or the package is not signed."
  DetailPrint "  Enable it later with: widgets\WindowsWidgetProvider\build.ps1 -Install"
  DetailPrint "  Details: $TEMP\biflow-widget-install.log"
  DetailPrint "$R1"
  Goto widget_done

  widget_installed:
  DetailPrint "  BiFlow widget registered. Open the Widgets Board (Win+W) and pin 'BiFlow Traffic'."

  widget_done:
  widget_absent:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; --------------------------------------------------------------------------
  ;  Remove the widget package first, while we still know its package identity.
  ;  Remove-AppxPackage is by identity, not by file, and the MSIX has already
  ;  been staged away by the time the rest of the uninstall runs.
  ; --------------------------------------------------------------------------
  nsExec::ExecToLog 'powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-AppxPackage -Name ''IranSplit.BiFlow.WidgetProvider'' -ErrorAction SilentlyContinue | Remove-AppxPackage -ErrorAction SilentlyContinue"'
  Pop $R0

  nsExec::ExecToLog '"$INSTDIR\helper\iran-split-helper.exe" --uninstall'
  Pop $R0
  StrCmp $R0 "0" helper_uninstall_completed
  DetailPrint "BiFlow Helper removal failed (exit: $R0)."
  IfSilent +2
    MessageBox MB_OK|MB_ICONSTOP "BiFlow Helper removal failed (exit: $R0). The application was not fully removed."
  SetErrorLevel 1
  Abort "BiFlow Helper removal failed"
  helper_uninstall_completed:
!macroend
