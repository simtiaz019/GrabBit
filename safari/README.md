# Safari Web Extension packaging

Safari integration must be embedded in an Apple-signed macOS containing app; a
standalone unsigned WebExtension is not distributable. `Shared/` reuses the
extension's HTML/TypeScript build output and `SafariWebExtensionHandler.swift`
is the native app-extension message bridge.

Create the shipping target with Xcode's **Safari Web Extension** template,
using these identifiers:

- containing Tauri app: `com.grabbit.desktop`
- extension: `com.grabbit.desktop.safari-extension`
- app group: `group.com.grabbit.desktop`

Add `SafariWebExtensionHandler.swift`, `SafariExtension.entitlements`, and the
built shared extension resources. The handler accepts only the versioned
GrabBit request dictionary, never cookies or headers, and forwards it through
the app group to the containing app. The containing app must pass the request
to the same private IPC dispatcher used by other browsers and return a durable
queue acknowledgement before the page reports success.

Enable the extension in Safari Settings > Extensions after installing the
signed containing app. Development requires an Apple Development team;
distribution requires Developer ID signing, hardened runtime, notarization,
and an App Store-compatible Safari extension profile. This repository does not
contain credentials and Safari functionality must not be reported as verified
until run in Safari on a signed build.
