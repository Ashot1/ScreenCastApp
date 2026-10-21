# Feasibility Keychain helper

Swift 6 / macOS 14+ tooling for the isolated cloud probe. It stores one session using the Security framework; Node owns sign-in, refresh, identity validation and the existing active lock. This package is not production native credential integration.

Build from the repository root after helper source changes:

```sh
swift build --package-path tests/feasibility/01-cloud/native-keychain -c release -Xswiftc -warnings-as-errors -Xswiftc -strict-concurrency=complete
swift test --package-path tests/feasibility/01-cloud/native-keychain -Xswiftc -warnings-as-errors -Xswiftc -strict-concurrency=complete
swift format lint --strict --configuration .swift-format --recursive tests/feasibility/01-cloud/native-keychain/Package.swift tests/feasibility/01-cloud/native-keychain/Sources tests/feasibility/01-cloud/native-keychain/Tests
```

Node starts `.build/release/keychain-helper` at a stable absolute path. No build on each login, shell command, `security` CLI or broad ACL adjustment is involved. The system can show an access/unlock prompt; Node allows 120 seconds and waits for process closure after cancellation, overflow or timeout. A rebuilt helper may need fresh access approval.

## Storage and private protocol

`SecItemAdd`, `SecItemCopyMatching`, `SecItemUpdate` and `SecItemDelete` operate on file-based generic-password items with service `com.ambientassistant.feasibility.session.v1`. Node derives account `v1:<sha256>` from host UUID plus the complete verified client ID/issuer/subject. No credential query is broad across services/accounts.

One version-1 JSON request is read from stdin until EOF, in bounded chunks, with a 64 KiB maximum. Operations are `read`, `add`, `update`, `delete`; all require the account, and writes require `session`. The response has `version`, `outcome`, optional `session` and fixed `error_code`/numeric `status_code`. Nothing is written to stderr. Node captures stdout privately and never prints its session data.

Session fields are `access_token`, `refresh_token`, `expires_at_ms` (absolute Unix milliseconds), `granted_scopes`, `nonce` and `registration: {client_id, issuer, subject}`. Keychain data are limited to 48 KiB; tokens to 16 KiB each. Both boundaries check external data. No ID token, email, client secret or browser credentials are stored. The executable accepts no arguments and offers no alternate-Keychain option.

Refresh deliberately uses delete-then-add, not update: delete the previous rotating token before the single network attempt, then persist the verified replacement. A lost response cannot replay the old persisted token. General update remains a tested storage operation.

## Actual native test isolation

The test creates a unique encrypted temporary Keychain containing only synthetic data. An internal test-only initializer targets that Keychain: `kSecUseKeychain` selects the destination for Add; `kSecMatchSearchList: [testKeychain]` scopes read/update/delete. It never calls `SecKeychainSetDefault` or `SecKeychainSetSearchList` and never searches the user's saved sessions. It removes only its exact temporary Keychain reference and directory. The test compares default-Keychain/search-list metadata in memory before creation, after creation, after CRUD and after cleanup; it never prints names or paths.

This distinction comes from the installed Apple SDK `Security.framework/Headers/SecItem.h`: `kSecUseKeychain` is Add-only; `kSecMatchSearchList` supplies a query-local list of `SecKeychainRef` values. Using the Add-only key for reads does not establish isolation. Temporary file creation/deletion requires deprecated `SecKeychainCreate/Delete`; the fixture is explicitly marked as a deprecated context, while all actual record operations and the helper use SecItem APIs. Strict compiler/lint settings remain enabled. Apple’s [StorageManager implementation](https://github.com/apple-oss-distributions/Security/blob/main/OSX/libsecurity_keychain/lib/StorageManager.cpp) also limits automatic list insertion to login/System keychains (`shouldAddToSearchList`), excluding this private `isolated.keychain`.

Four tests cover JSON field names, bounded input, fragmented pipe input and the real Keychain round trip (missing/Add/read/duplicate/update/isolation/delete). This verifies storage in an isolated file, not actual-account access, login-Keychain ACL dialogs or provider refresh. Node separately tests lifecycle and waiting for helper closure.
