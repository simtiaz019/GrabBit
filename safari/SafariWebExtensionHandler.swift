import SafariServices
import os.log

final class SafariWebExtensionHandler: NSObject, NSExtensionRequestHandling {
    private let log = Logger(subsystem: "com.grabbit.desktop.safari-extension", category: "bridge")
    private let allowedTypes: Set<String> = ["PING", "GET_CAPABILITIES", "CAPTURE_DOWNLOAD", "INSPECT_MEDIA", "CREATE_MEDIA_DOWNLOAD", "GET_REQUEST_STATUS", "CANCEL_PENDING_CAPTURE", "OPEN_APPLICATION"]

    func beginRequest(with context: NSExtensionContext) {
        guard let item = context.inputItems.first as? NSExtensionItem,
              let message = item.userInfo?[SFExtensionMessageKey] as? [String: Any],
              message["schemaVersion"] as? Int == 1,
              let type = message["type"] as? String,
              allowedTypes.contains(type),
              JSONSerialization.isValidJSONObject(message),
              let encoded = try? JSONSerialization.data(withJSONObject: message),
              encoded.count <= 1_048_576 else {
            complete(context, ["schemaVersion": 1, "ok": false, "code": "invalid_request", "message": "Invalid GrabBit request"])
            return
        }
        // The containing app consumes this app-group queue and replaces the reply.
        // Never log the message: URLs can contain sensitive query strings.
        let defaults = UserDefaults(suiteName: "group.com.grabbit.desktop")
        defaults?.set(encoded, forKey: "pendingSafariRequest")
        defaults?.synchronize()
        log.info("Forwarded a validated Safari extension request")
        // Do not claim acceptance until the containing app implements a reply
        // channel and confirms durable queue creation.
        complete(context, ["schemaVersion": 1, "ok": false, "code": "app_unavailable", "message": "Open GrabBit to complete this request"])
    }

    private func complete(_ context: NSExtensionContext, _ response: [String: Any]) {
        let item = NSExtensionItem()
        item.userInfo = [SFExtensionMessageKey: response]
        context.completeRequest(returningItems: [item])
    }
}
