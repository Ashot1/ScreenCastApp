import AppKit
import Foundation

// A hidden native main-run-loop probe; this is not the future product UI.
let application = NSApplication.shared
application.setActivationPolicy(.prohibited)
let duration = Double(CommandLine.arguments.dropFirst().first ?? "10") ?? 10
var intervals: [Double] = []
var previous = ProcessInfo.processInfo.systemUptime
let timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60.0, repeats: true) { _ in
    let now = ProcessInfo.processInfo.systemUptime
    intervals.append(now - previous)
    previous = now
}
RunLoop.main.run(until: Date().addingTimeInterval(duration))
timer.invalidate()
let sorted = intervals.sorted()
let p95 = sorted.isEmpty ? 0 : sorted[min(sorted.count - 1, Int(Double(sorted.count) * 0.95))]
let output: [String: Any] = [
    "samples": intervals.count, "p95_gap_ms": p95 * 1000,
    "max_gap_ms": (intervals.max() ?? 0) * 1000,
    "over_50ms": intervals.filter { $0 > 0.05 }.count,
]
let data = try JSONSerialization.data(withJSONObject: output, options: .sortedKeys)
FileHandle.standardOutput.write(data)
