# Safari Web Extension packaging

Safari integration must be embedded in an Apple-signed macOS containing app; a
standalone unsigned WebExtension is not distributable. Run
`./safari/prepare-resources.sh` to produce `safari/Resources` from the shared
TypeScript extension sources. `SafariWebExtensionHandler.swift` is the native
app-extension request/reply bridge.

Create the shipping target with Xcode's **Safari Web Extension** template,
using these identifiers:

- containing Tauri app: `com.grabbit.desktop`
- extension: `com.grabbit.desktop.safari-extension`
- app group: `group.com.grabbit.desktop`

Add `SafariWebExtensionHandler.swift`, `SafariExtensionState.swift`,
`SafariExtension.entitlements`, `ContainingApp.entitlements`, and the generated
resources to their corresponding targets. Merge `Info.plist.fragment` into the
extension target's Info.plist. The handler accepts only the versioned GrabBit
request dictionary, never cookies or headers, and uses random, atomic files in
the app-group container for an eight-second request/reply exchange. The Tauri
application watches that container and dispatches the request through the same
validated command path used by other browsers. A capture is reported as
accepted only after the desktop queue transaction is durable.

The generated `manifest.json` uses Safari's `browser.runtime.sendNativeMessage`
bridge and intentionally omits Chromium's native-host permission. The desktop
bridge is available only on macOS; unsigned local runs may not have access to
the app-group container.

Enable the extension in Safari Settings > Extensions after installing the
signed containing app. Development requires an Apple Development team;
distribution requires Developer ID signing, hardened runtime, notarization,
and an App Store-compatible Safari extension profile. This repository does not
contain credentials and Safari functionality must not be reported as verified
until run in Safari on a signed build. Without a full Xcode installation and a
signing team, only resource generation, plist validation, and Swift parsing can
be verified.
