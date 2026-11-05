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
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            self?.forward(encoded, context: context)
        }
    }

    private func forward(_ encoded: Data, context: NSExtensionContext) {
        guard let container = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: "group.com.grabbit.desktop"
        ) else {
            complete(context, unavailable("Safari app-group container is unavailable"))
            return
        }
        let bridge = container.appendingPathComponent("GrabBitBridge", isDirectory: true)
        do { try FileManager.default.createDirectory(at: bridge, withIntermediateDirectories: true) }
        catch {
            complete(context, unavailable("GrabBit bridge directory is unavailable"))
            return
        }
        let identifier = UUID().uuidString.lowercased()
        let request = bridge.appendingPathComponent("request-\(identifier).json")
        let reply = bridge.appendingPathComponent("reply-\(identifier).json")
        do { try encoded.write(to: request, options: [.atomic, .completeFileProtection]) }
        catch {
            complete(context, unavailable("Could not queue the GrabBit request"))
            return
        }
        let deadline = Date().addingTimeInterval(8)
        while Date() < deadline {
            if let data = try? Data(contentsOf: reply),
               data.count <= 1_048_576,
               let response = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                try? FileManager.default.removeItem(at: reply)
                log.info("Received a GrabBit desktop reply")
                complete(context, response)
                return
            }
            Thread.sleep(forTimeInterval: 0.1)
        }
        try? FileManager.default.removeItem(at: request)
        complete(context, unavailable("GrabBit did not respond; the browser download was left intact"))
    }

    private func unavailable(_ message: String) -> [String: Any] {
        ["schemaVersion": 1, "ok": false, "code": "app_unavailable", "message": message]
    }

    private func complete(_ context: NSExtensionContext, _ response: [String: Any]) {
        let item = NSExtensionItem()
        item.userInfo = [SFExtensionMessageKey: response]
        context.completeRequest(returningItems: [item])
    }
}
