#!/bin/sh
set -eu

case "$(uname -s)" in
  Darwin)
    install_root="$HOME/Library/Application Support/GrabBit"
    roots="$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts
$HOME/Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts
$HOME/Library/Application Support/Microsoft Edge/NativeMessagingHosts
$HOME/Library/Application Support/Mozilla/NativeMessagingHosts"
    endpoint="$HOME/Library/Application Support/com.grabbit.desktop/browser-ipc.sock"
    ;;
  Linux)
    data_root=${XDG_DATA_HOME:-"$HOME/.local/share"}
    config_root=${XDG_CONFIG_HOME:-"$HOME/.config"}
    install_root="$data_root/grabbit"
    roots="$config_root/google-chrome/NativeMessagingHosts
$config_root/BraveSoftware/Brave-Browser/NativeMessagingHosts
$config_root/microsoft-edge/NativeMessagingHosts
$config_root/chromium/NativeMessagingHosts
$HOME/.mozilla/native-messaging-hosts"
    endpoint="$data_root/com.grabbit.desktop/browser-ipc.sock"
    ;;
  *) echo "Unsupported platform" >&2; exit 1 ;;
esac

printf '%s\n' "$roots" | while IFS= read -r root; do
  manifest="$root/com.grabbit.native.json"
  if [ -f "$manifest" ]; then rm "$manifest"; fi
done
if [ -f "$install_root/grabbit-browser-host" ]; then rm "$install_root/grabbit-browser-host"; fi
if [ -S "$endpoint" ]; then rm "$endpoint"; fi
echo "Removed GrabBit native messaging manifests and host files."
