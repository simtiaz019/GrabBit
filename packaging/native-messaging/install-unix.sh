#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cargo build --manifest-path "$repo_root/Cargo.toml" --release --bin grabbit-browser-host
host_source="$repo_root/target/release/grabbit-browser-host"

case "$(uname -s)" in
  Darwin)
    install_root="$HOME/Library/Application Support/GrabBit"
    chromium_roots="$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts
$HOME/Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts
$HOME/Library/Application Support/Microsoft Edge/NativeMessagingHosts"
    firefox_root="$HOME/Library/Application Support/Mozilla/NativeMessagingHosts"
    ;;
  Linux)
    data_root=${XDG_DATA_HOME:-"$HOME/.local/share"}
    config_root=${XDG_CONFIG_HOME:-"$HOME/.config"}
    install_root="$data_root/grabbit"
    chromium_roots="$config_root/google-chrome/NativeMessagingHosts
$config_root/BraveSoftware/Brave-Browser/NativeMessagingHosts
$config_root/microsoft-edge/NativeMessagingHosts
$config_root/chromium/NativeMessagingHosts"
    firefox_root="$HOME/.mozilla/native-messaging-hosts"
    ;;
  *) echo "Unsupported platform" >&2; exit 1 ;;
esac

mkdir -p "$install_root"
install -m 0755 "$host_source" "$install_root/grabbit-browser-host"
escaped_host=$(printf '%s' "$install_root/grabbit-browser-host" | sed 's/[&/]/\\&/g')

printf '%s\n' "$chromium_roots" | while IFS= read -r root; do
  mkdir -p "$root"
  sed "s/__GRABBIT_BROWSER_HOST__/$escaped_host/" "$repo_root/packaging/native-messaging/chromium/com.grabbit.native.json" > "$root/com.grabbit.native.json"
  chmod 0644 "$root/com.grabbit.native.json"
done
mkdir -p "$firefox_root"
sed "s/__GRABBIT_BROWSER_HOST__/$escaped_host/" "$repo_root/packaging/native-messaging/firefox/com.grabbit.native.json" > "$firefox_root/com.grabbit.native.json"
chmod 0644 "$firefox_root/com.grabbit.native.json"
echo "Installed GrabBit native host. Load the extension from extensions/chromium/dist or extensions/firefox/dist."
