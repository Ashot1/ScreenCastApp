import Foundation

var failures = 0

@MainActor
func check(_ condition: Bool, _ description: String) {
    if !condition {
        print("FAIL: \(description)")
        failures += 1
    }
}

let request = ["--window", "123", "--expected-bundle-id", "dev.ambient.fixture"]

var inspected = false
var output: [String] = []
let denied = runProbe(
    arguments: request,
    accessibility: false,
    screenCapture: true,
    listenEvents: true,
    windowProbe: { _, _ in
        inspected = true
        return 0
    },
    emit: { output.append($0) }
)
check(denied == 2, "AX denial must return blocked, even with other permissions")
check(!inspected, "AX denial must prevent the window read")
check(output.contains("accessibility=false"), "report actual denied permission")

inspected = false
let preflight = runProbe(
    arguments: [],
    accessibility: true,
    screenCapture: false,
    listenEvents: true,
    windowProbe: { _, _ in
        inspected = true
        return 0
    },
    emit: { _ in }
)
check(preflight == 2, "preflight must expose incomplete permissions")
check(!inspected, "preflight must never read a window")

for invalid in [
    ["--window", "0", "--expected-bundle-id", "dev.ambient.fixture"],
    ["--window", "-1", "--expected-bundle-id", "dev.ambient.fixture"],
    ["--window", "123", "--expected-bundle-id", ""],
    ["--window", "123"],
    ["--unexpected", "private-input-must-not-be-logged"],
] {
    inspected = false
    output = []
    let invalidResult = runProbe(
        arguments: invalid,
        accessibility: true,
        screenCapture: true,
        listenEvents: true,
        windowProbe: { _, _ in
            inspected = true
            return 0
        },
        emit: { output.append($0) }
    )
    check(invalidResult == 64, "invalid target arguments must be rejected")
    check(!inspected, "invalid arguments must never reach the window read")
    check(
        !output.joined().contains("private-input-must-not-be-logged"),
        "invalid input must not leak to diagnostics"
    )
}

var target: Int32 = 0
let windowResult = runProbe(
    arguments: request,
    accessibility: true,
    screenCapture: false,
    listenEvents: false,
    windowProbe: { pid, bundle in
        target = pid
        check(bundle == "dev.ambient.fixture", "keep the explicitly requested target")
        return 4
    },
    emit: { _ in }
)
check(target == 123, "AX-only probe must not require capture or input access")
check(windowResult == 4, "a failed real AX operation must not become success")

if failures == 0 {
    print("PASS: permission gate, preflight isolation, invalid targets and AX failure propagation")
}
exit(failures == 0 ? 0 : 1)
