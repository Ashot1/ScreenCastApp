import Foundation
import Testing

@testable import AmbientMacOS

@Test func englishAndRussianStringsHaveTheSameKeys() throws {
    let english = try localizationKeys(try stringsURL("en"))
    let russian = try localizationKeys(try stringsURL("ru"))
    #expect(!english.isEmpty)
    #expect(english.subtracting(russian).sorted() == [])
    #expect(russian.subtracting(english).sorted() == [])
}

private func stringsURL(_ localization: String) throws -> URL {
    try #require(
        Bundle.module.url(
            forResource: "Localizable", withExtension: "strings", subdirectory: nil,
            localization: localization), "\(localization).lproj/Localizable.strings is missing")
}

private func localizationKeys(_ url: URL) throws -> Set<String> {
    let strings = try #require(
        NSDictionary(contentsOf: url) as? [String: String], "\(url.path) is not a strings table")
    return Set(strings.keys)
}
