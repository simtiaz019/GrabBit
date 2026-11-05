$ErrorActionPreference = 'Stop'
$RegistryPaths = @(
  'HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.grabbit.native',
  'HKCU:\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.grabbit.native',
  'HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.grabbit.native',
  'HKCU:\Software\Chromium\NativeMessagingHosts\com.grabbit.native',
  'HKCU:\Software\Mozilla\NativeMessagingHosts\com.grabbit.native'
)
foreach ($RegistryPath in $RegistryPaths) {
  if (Test-Path $RegistryPath) { Remove-Item $RegistryPath -Recurse -Force }
}
$InstallRoot = Join-Path $env:LOCALAPPDATA 'GrabBit'
foreach ($Name in @('com.grabbit.native.chromium.json', 'com.grabbit.native.firefox.json', 'grabbit-browser-host.exe')) {
  $Path = Join-Path $InstallRoot $Name
  if (Test-Path $Path) { Remove-Item $Path -Force }
}
$Endpoint = Join-Path $env:APPDATA 'com.grabbit.desktop\browser-ipc.endpoint'
if (Test-Path $Endpoint) { Remove-Item $Endpoint -Force }
Write-Host 'Removed GrabBit native messaging registrations and host files.'
