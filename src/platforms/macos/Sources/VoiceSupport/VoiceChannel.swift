import Foundation

public struct VoiceIdentity: Codable, Equatable, Sendable {
    public let device: Int32
    public let inode: UInt64
    public let owner: UInt32
    public init(device: Int32, inode: UInt64, owner: UInt32) {
        self.device = device
        self.inode = inode
        self.owner = owner
    }
}

public struct VoiceRequest: Codable, Equatable, Sendable {
    public let directory: String
    public let directoryIdentity: VoiceIdentity
    public let leaseIdentity: VoiceIdentity
    public let modelDirectory: String
    public init(
        directory: String, directoryIdentity: VoiceIdentity, leaseIdentity: VoiceIdentity,
        modelDirectory: String
    ) {
        self.directory = directory
        self.directoryIdentity = directoryIdentity
        self.leaseIdentity = leaseIdentity
        self.modelDirectory = modelDirectory
    }
}

public enum VoiceHelperFailure: String, Codable, Sendable {
    case input, models, recognition, limit
}
public enum VoiceResult: Equatable, Sendable {
    case text(String)
    case silence
    case failure(VoiceHelperFailure)
}

public enum VoiceChannel {
    public static let maximumRequestBytes = 8192
    public static let maximumResultBytes = 32768
    public static let maximumTextBytes = 16384
    public static func encode(_ result: VoiceResult) throws -> Data {
        let value: [String: String]
        switch result {
        case .text(let text):
            guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                text.utf8.count <= maximumTextBytes
            else { throw VoiceFailure.invalidChannel }
            value = ["status": "text", "text": text]
        case .silence: value = ["status": "silence"]
        case .failure(let code): value = ["status": "error", "error": code.rawValue]
        }
        let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
        guard data.count <= maximumResultBytes else { throw VoiceFailure.invalidChannel }
        return data
    }
    public static func decode(_ data: Data) throws -> VoiceResult {
        let value = try object(data, limit: maximumResultBytes)
        let result: VoiceResult
        switch value["status"] as? String {
        case "text":
            guard Set(value.keys) == ["status", "text"], let text = value["text"] as? String else {
                throw VoiceFailure.invalidChannel
            }
            result = .text(text)
        case "silence":
            guard Set(value.keys) == ["status"] else { throw VoiceFailure.invalidChannel }
            result = .silence
        case "error":
            guard Set(value.keys) == ["status", "error"], let raw = value["error"] as? String,
                let code = VoiceHelperFailure(rawValue: raw)
            else { throw VoiceFailure.invalidChannel }
            result = .failure(code)
        default: throw VoiceFailure.invalidChannel
        }
        _ = try encode(result)
        return result
    }
    public static func encodeRequest(_ request: VoiceRequest) throws -> Data {
        try validate(request)
        let data = try JSONEncoder().encode(request)
        guard data.count <= maximumRequestBytes else { throw VoiceFailure.invalidChannel }
        return data
    }
    public static func decodeRequest(_ data: Data) throws -> VoiceRequest {
        let value = try object(data, limit: maximumRequestBytes)
        guard
            Set(value.keys) == [
                "directory", "directoryIdentity", "leaseIdentity", "modelDirectory",
            ]
        else { throw VoiceFailure.invalidChannel }
        for name in ["directoryIdentity", "leaseIdentity"] {
            guard let identity = value[name] as? [String: Any],
                Set(identity.keys) == ["device", "inode", "owner"]
            else { throw VoiceFailure.invalidChannel }
        }
        do {
            let request = try JSONDecoder().decode(VoiceRequest.self, from: data)
            try validate(request)
            return request
        } catch { throw VoiceFailure.invalidChannel }
    }
    private static func validate(_ request: VoiceRequest) throws {
        for path in [request.directory, request.modelDirectory] {
            guard path.hasPrefix("/"), path.utf8.count <= 4096, !path.utf8.contains(0),
                !path.split(separator: "/").contains("..")
            else { throw VoiceFailure.invalidChannel }
        }
    }
    private static func object(_ data: Data, limit: Int) throws -> [String: Any] {
        guard !data.isEmpty, data.count <= limit, String(data: data, encoding: .utf8) != nil else {
            throw VoiceFailure.invalidChannel
        }
        do {
            guard let value = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                throw VoiceFailure.invalidChannel
            }
            return value
        } catch { throw VoiceFailure.invalidChannel }
    }
}
