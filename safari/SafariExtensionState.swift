import SafariServices

enum GrabBitSafariExtensionState {
    static let bundleIdentifier = "com.grabbit.desktop.safari-extension"

    static func read(_ completion: @escaping (Bool, Error?) -> Void) {
        SFSafariExtensionManager.getStateOfSafariExtension(
            withIdentifier: bundleIdentifier
        ) { state, error in
            completion(state?.isEnabled == true, error)
        }
    }

    static func openSafariPreferences() {
        SFSafariApplication.showPreferencesForExtension(
            withIdentifier: bundleIdentifier
        ) { _ in }
    }
}
