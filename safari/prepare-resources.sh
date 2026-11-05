#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"
npm run extension:build
destination="$repo_root/safari/Resources"
mkdir -p "$destination"
cp extensions/chromium/dist/background.js "$destination/background.js"
cp extensions/chromium/dist/contracts.js "$destination/contracts.js"
cp extensions/chromium/dist/native-client.js "$destination/native-client.js"
cp extensions/chromium/dist/policy.js "$destination/policy.js"
cp extensions/chromium/dist/popup.js "$destination/popup.js"
cp extensions/chromium/dist/popup.html "$destination/popup.html"
cp extensions/chromium/dist/popup.css "$destination/popup.css"
cp safari/manifest.json "$destination/manifest.json"
echo "Prepared Safari Web Extension resources in safari/Resources"
