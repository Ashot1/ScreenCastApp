import Foundation

enum ChatGPTFailure: Error, Equatable, Sendable {
    case storage, corrupt, invalid, busy, cancelled, expired, network, denied, response, identity
    case browser, registrationUnknown, closed
}
struct ChatGPTCredentials: Codable, Sendable {
    var access: String
    var refresh: String
    var scopes: [String]
    var expiresAt: Int64
}
struct ChatGPTEnvelope: Codable, Sendable {
    var version = 1
    var host: String
    var registration: UInt32 = 0
    var clientID: String?
    var issuer: String?
    var subject: String?
    var hint: String?
    var nonce: String?
    var credentials: ChatGPTCredentials?

    func validate() throws {
        guard version == 1, host.hasPrefix("urn:uuid:"),
            let uuid = UUID(uuidString: String(host.dropFirst(9))),
            host.lowercased() == "urn:uuid:" + uuid.uuidString.lowercased(), registration <= 3
        else { throw ChatGPTFailure.corrupt }
        if registration < 2 {
            guard clientID == nil else { throw ChatGPTFailure.corrupt }
        } else {
            guard let clientID, Self.validClientID(clientID) else { throw ChatGPTFailure.corrupt }
        }
        if registration == 3 {
            guard issuer == "https://auth.openai.com", let subject, Self.bounded(subject, 1024)
            else { throw ChatGPTFailure.corrupt }
        } else {
            guard issuer == nil, subject == nil, hint == nil, nonce == nil, credentials == nil
            else { throw ChatGPTFailure.corrupt }
        }
        if let hint {
            guard Self.bounded(hint, 16_384), let nonce, Self.bounded(nonce, 1024)
            else { throw ChatGPTFailure.corrupt }
        } else if nonce != nil {
            throw ChatGPTFailure.corrupt
        }
        if let c = credentials {
            guard hint != nil, c.expiresAt > 0, Self.bounded(c.access, 16_384),
                Self.bounded(c.refresh, 16_384), Self.validScopes(c.scopes)
            else { throw ChatGPTFailure.corrupt }
        }
    }
    static func bounded(_ s: String, _ limit: Int) -> Bool {
        !s.isEmpty && s.utf8.count <= limit && !s.utf8.contains(0)
    }
    static func validClientID(_ s: String) -> Bool {
        bounded(s, 512) && s != "dynamic_agent_client"
            && s.utf8.allSatisfy {
                (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0)
                    || $0 == 45 || $0 == 95
            }
    }
    static func validScopes(_ scopes: [String]) -> Bool {
        !scopes.isEmpty && scopes.count <= 64
            && scopes.reduce(0, { $0 + $1.utf8.count + 1 }) <= 4096
            && scopes.allSatisfy {
                !$0.isEmpty
                    && $0.utf8.allSatisfy {
                        $0 == 0x21 || (0x23...0x5b).contains($0) || (0x5d...0x7e).contains($0)
                    }
            }
            && ["openid", "resource.invoke", "chatgpt.tokens.use.direct"].allSatisfy(
                scopes.contains)
    }
    func encoded() throws -> Data {
        try validate()
        let data = try JSONEncoder().encode(self)
        guard data.count <= 65_536 else { throw ChatGPTFailure.corrupt }
        return data
    }
    static func decode(_ data: Data) throws -> Self {
        guard data.count <= 65_536 else { throw ChatGPTFailure.corrupt }
        do {
            let value = try JSONDecoder().decode(Self.self, from: data)
            try value.validate()
            return value
        } catch { throw ChatGPTFailure.corrupt }
    }
    mutating func localSignOut() {
        credentials = nil
        hint = nil
        nonce = nil
    }
}
protocol ChatGPTCredentialStore: Sendable {
    func load() throws -> ChatGPTEnvelope?
    func save(_ envelope: ChatGPTEnvelope) throws
}
