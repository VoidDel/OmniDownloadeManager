; Omni Download installer hooks (Tauri NSIS). Uninstalling removes what the app registered for
; the current user while it ran: the browsers' Native Messaging host entries (the manifests live
; in the app's data folder) and starting with Windows.

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\app.omnidownload.host"
  DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\app.omnidownload.host"
  DeleteRegKey HKCU "Software\Mozilla\NativeMessagingHosts\app.omnidownload.host"
  DeleteRegKey HKCU "Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\app.omnidownload.host"
  DeleteRegKey HKCU "Software\Chromium\NativeMessagingHosts\app.omnidownload.host"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Omni Download"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Omni Download"
!macroend
