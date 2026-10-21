import Foundation

@MainActor
func runProbe(
    arguments: [String],
    accessibility: Bool,
    screenCapture: Bool,
    listenEvents: Bool,
    windowProbe: (Int32, String) -> Int32,
    emit: (String) -> Void
) -> Int32 {
    emit("accessibility=\(accessibility)")
    emit("screenCapture=\(screenCapture)")
    emit("listenEvents=\(listenEvents)")

    if arguments.isEmpty || arguments == ["--preflight"] {
        emit("mode=preflight")
        return accessibility && screenCapture && listenEvents ? 0 : 2
    }

    guard arguments.count == 4,
        arguments[0] == "--window",
        let pid = Int32(arguments[1]), pid > 0,
        arguments[2] == "--expected-bundle-id",
        !arguments[3].isEmpty,
        arguments[3].rangeOfCharacter(from: .whitespacesAndNewlines) == nil
    else {
        // Never echo unchecked target arguments into the diagnostic log.
        emit("result=invalid-arguments")
        return 64
    }

    guard accessibility else {
        emit("result=blocked-accessibility")
        return 2
    }
    // Capture and event access are diagnostic only: this probe never uses either capability.
    emit("mode=window-count")
    return windowProbe(pid, arguments[3])
}
