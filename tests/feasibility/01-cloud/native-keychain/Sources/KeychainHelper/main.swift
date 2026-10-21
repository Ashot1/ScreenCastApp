import Foundation
import KeychainStore

private enum Operation: String, Decodable {
    case read
    case add
    case update
    case delete
}

private struct Request: Decodable {
    let version: Int
    let operation: Operation
    let account: String
    let session: HelperSession?
}

private struct Response: Encodable {
    let version = 1
    let outcome: String
    let session: HelperSession?
    let errorCode: String?
    let statusCode: Int32?

    enum CodingKeys: String, CodingKey {
        case version
        case outcome
        case session
        case errorCode = "error_code"
        case statusCode = "status_code"
    }
}

private func validRegistration(_ registration: HelperRegistration) -> Bool {
    registration.issuer == "https://auth.openai.com" && registration.clientID.count <= 512
        && registration.clientID.range(of: "^[A-Za-z0-9_-]+$", options: .regularExpression) != nil
        && registration.clientID != "dynamic_agent_client" && !registration.subject.isEmpty
        && registration.subject.utf8.count <= 1024
}

private func validSession(_ session: HelperSession) -> Bool {
    !session.accessToken.isEmpty && session.accessToken.utf8.count <= 16_384
        && !session.refreshToken.isEmpty && session.refreshToken.utf8.count <= 16_384
        && session.expiresAtMilliseconds > 0 && session.grantedScopes.count <= 128
        && session.grantedScopes.allSatisfy {
            $0.range(of: "^[A-Za-z0-9:._/-]{1,128}$", options: .regularExpression) != nil
        } && !session.nonce.isEmpty && session.nonce.utf8.count <= 1024
        && validRegistration(session.registration)
}

private func emit(_ response: Response) {
    guard let data = try? JSONEncoder().encode(response), data.count <= 64 * 1024 else {
        FileHandle.standardOutput.write(
            Data(
                "{\"version\":1,\"outcome\":\"error\",\"error_code\":\"response_too_large\"}\n".utf8
            ))
        return
    }
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([0x0A]))
}

private func run() {
    let input: Data
    do {
        input = try HelperInput.read(from: FileHandle.standardInput)
    } catch {
        emit(
            Response(outcome: "error", session: nil, errorCode: "invalid_request", statusCode: nil))
        return
    }
    guard !input.isEmpty, input.count <= 64 * 1024,
        let request = try? JSONDecoder().decode(Request.self, from: input), request.version == 1,
        request.account.range(of: "^v1:[a-f0-9]{64}$", options: .regularExpression) != nil
    else {
        emit(
            Response(outcome: "error", session: nil, errorCode: "invalid_request", statusCode: nil))
        return
    }

    do {
        guard CommandLine.arguments.count == 1 else {
            emit(
                Response(
                    outcome: "error", session: nil, errorCode: "invalid_request", statusCode: nil))
            return
        }
        let store = KeychainStore()

        switch request.operation {
        case .read:
            guard request.session == nil else { throw KeychainStoreError(status: errSecParam) }
            let session: HelperSession? = try store.read(
                account: request.account, as: HelperSession.self)
            if let session, !validSession(session) {
                throw KeychainStoreError(status: errSecDecode)
            }
            emit(
                Response(
                    outcome: session == nil ? "not_found" : "found", session: session,
                    errorCode: nil, statusCode: nil))
        case .add:
            guard let session = request.session, validSession(session) else {
                throw KeychainStoreError(status: errSecParam)
            }
            try store.add(account: request.account, session: session)
            emit(Response(outcome: "stored", session: nil, errorCode: nil, statusCode: nil))
        case .update:
            guard let session = request.session, validSession(session) else {
                throw KeychainStoreError(status: errSecParam)
            }
            try store.update(account: request.account, session: session)
            emit(Response(outcome: "stored", session: nil, errorCode: nil, statusCode: nil))
        case .delete:
            guard request.session == nil else { throw KeychainStoreError(status: errSecParam) }
            try store.delete(account: request.account)
            emit(Response(outcome: "deleted", session: nil, errorCode: nil, statusCode: nil))
        }
    } catch let error as KeychainStoreError {
        emit(
            Response(
                outcome: "error", session: nil, errorCode: "keychain_failure",
                statusCode: error.status))
    } catch {
        emit(
            Response(outcome: "error", session: nil, errorCode: "invalid_session", statusCode: nil))
    }
}

run()
