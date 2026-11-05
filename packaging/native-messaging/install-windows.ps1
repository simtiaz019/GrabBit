$ErrorActionPreference = 'Stop'
$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot '..\..')
cargo build --manifest-path (Join-Path $RepoRoot 'Cargo.toml') --release --bin grabbit-browser-host
$InstallRoot = Join-Path $env:LOCALAPPDATA 'GrabBit'
New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null
$HostPath = Join-Path $InstallRoot 'grabbit-browser-host.exe'
Copy-Item (Join-Path $RepoRoot 'target\release\grabbit-browser-host.exe') $HostPath -Force

function Install-Manifest($Template, $Output, $RegistryPaths) {
  New-Item -ItemType Directory -Force -Path (Split-Path $Output) | Out-Null
  $Content = (Get-Content $Template -Raw).Replace('__GRABBIT_BROWSER_HOST__', $HostPath.Replace('\', '\\'))
  [System.IO.File]::WriteAllText($Output, $Content, [System.Text.UTF8Encoding]::new($false))
  foreach ($RegistryPath in $RegistryPaths) {
    New-Item -Force $RegistryPath | Out-Null
    Set-ItemProperty $RegistryPath -Name '(default)' -Value $Output
  }
}

$ChromiumManifest = Join-Path $InstallRoot 'com.grabbit.native.chromium.json'
Install-Manifest (Join-Path $RepoRoot 'packaging\native-messaging\chromium\com.grabbit.native.json') $ChromiumManifest @(
  'HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.grabbit.native',
  'HKCU:\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.grabbit.native',
  'HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.grabbit.native',
  'HKCU:\Software\Chromium\NativeMessagingHosts\com.grabbit.native'
)
$FirefoxManifest = Join-Path $InstallRoot 'com.grabbit.native.firefox.json'
Install-Manifest (Join-Path $RepoRoot 'packaging\native-messaging\firefox\com.grabbit.native.json') $FirefoxManifest @(
  'HKCU:\Software\Mozilla\NativeMessagingHosts\com.grabbit.native'
)
Write-Host 'Installed GrabBit native messaging manifests for Chrome, Brave, Edge, and Firefox.'
