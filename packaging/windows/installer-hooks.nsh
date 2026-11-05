!macro NSIS_HOOK_POSTUNINSTALL
  ; Remove only GrabBit-owned per-user native messaging registration and files.
  ; Browser profiles, extensions, downloads, history, and preferences are preserved.
  DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\com.grabbit.native"
  DeleteRegKey HKCU "Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.grabbit.native"
  DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\com.grabbit.native"
  DeleteRegKey HKCU "Software\Chromium\NativeMessagingHosts\com.grabbit.native"
  DeleteRegKey HKCU "Software\Mozilla\NativeMessagingHosts\com.grabbit.native"
  Delete "$LOCALAPPDATA\GrabBit\com.grabbit.native.chromium.json"
  Delete "$LOCALAPPDATA\GrabBit\com.grabbit.native.firefox.json"
  Delete "$LOCALAPPDATA\GrabBit\grabbit-browser-host.exe"
  RMDir "$LOCALAPPDATA\GrabBit"
!macroend
