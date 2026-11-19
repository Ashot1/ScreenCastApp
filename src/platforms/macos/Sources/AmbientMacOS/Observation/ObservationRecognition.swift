import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import Vision

enum ObservationRecognition {
    static func recognize(_ image: CGImage) async throws -> [ObservationLine] {
        var request = RecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.recognitionLanguages = [
            Locale.Language(identifier: "ru-RU"), Locale.Language(identifier: "en-US"),
        ]
        let supported = Set(
            request.supportedRecognitionLanguages.compactMap { $0.languageCode?.identifier })
        guard supported.contains("ru"), supported.contains("en") else {
            throw ModelCheckFailure.native
        }
        let observations = try await request.perform(on: image)
        return observations.compactMap { observation in
            guard let text = observation.topCandidates(1).first else { return nil }
            return line(text.string) { range in text.boundingBox(for: range)?.boundingBox.cgRect }
        }
    }
    static func line(_ text: String, bounds: (Range<String.Index>) -> CGRect?) -> ObservationLine {
        var words: [ObservationWord] = []
        for word in text.split(whereSeparator: \.isWhitespace) {
            guard let rect = bounds(word.startIndex..<word.endIndex) else {
                // Core joins words for anchors; a missing word must not invent new adjacency.
                return .init(text: text, words: [])
            }
            words.append(.init(text: String(word), rect: rect))
        }
        return .init(text: text, words: words)
    }
    static func warmUp() async throws {
        // Vision exposes no warm-up operation; a throwaway local image loads the same request/model.
        guard
            let context = CGContext(
                data: nil, width: 128, height: 64, bitsPerComponent: 8,
                bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue),
            let image = context.makeImage()
        else { throw ModelCheckFailure.native }
        _ = try await recognize(image)
    }
    static func jpeg(_ image: CGImage) throws -> Data {
        let scale = min(1, 1280 / Double(max(image.width, image.height)))
        let width = max(1, Int(Double(image.width) * scale))
        let height = max(1, Int(Double(image.height) * scale))
        guard
            let context = CGContext(
                data: nil, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
        else { throw ModelCheckFailure.native }
        context.interpolationQuality = .high
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        guard let resized = context.makeImage() else { throw ModelCheckFailure.native }
        for quality in [0.8, 0.6, 0.4, 0.2] {
            let data = NSMutableData()
            guard
                let destination = CGImageDestinationCreateWithData(
                    data, UTType.jpeg.identifier as CFString, 1, nil)
            else { throw ModelCheckFailure.native }
            CGImageDestinationAddImage(
                destination, resized,
                [kCGImageDestinationLossyCompressionQuality: quality] as CFDictionary)
            guard CGImageDestinationFinalize(destination) else { throw ModelCheckFailure.native }
            if data.length <= 786432 { return data as Data }
        }
        // An image that cannot fit the ABI bound is skipped, never truncated or written to disk.
        throw ModelCheckFailure.limit
    }
}
