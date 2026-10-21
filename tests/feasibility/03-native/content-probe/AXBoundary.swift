import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

func checkedAXElement(_ raw: CFTypeRef?) throws -> AXUIElement {
    guard let raw, CFGetTypeID(raw) == AXUIElementGetTypeID() else {
        throw ContentProbeError.unsupported
    }
    // AX returns erased CF values. Only this checked native boundary converts their representation.
    return unsafeDowncast(raw, to: AXUIElement.self)
}

func frameFromAX(position: CFTypeRef?, size: CFTypeRef?) -> CGRect? {
    guard let position, let size,
        CFGetTypeID(position) == AXValueGetTypeID(), CFGetTypeID(size) == AXValueGetTypeID()
    else { return nil }
    // Type IDs and payload kinds are checked before the C API writes into typed Swift storage.
    let pointValue = unsafeDowncast(position, to: AXValue.self)
    let sizeValue = unsafeDowncast(size, to: AXValue.self)
    guard AXValueGetType(pointValue) == .cgPoint, AXValueGetType(sizeValue) == .cgSize else {
        return nil
    }
    var point = CGPoint.zero
    var dimensions = CGSize.zero
    guard AXValueGetValue(pointValue, .cgPoint, &point),
        AXValueGetValue(sizeValue, .cgSize, &dimensions),
        point.x.isFinite, point.y.isFinite, dimensions.width.isFinite, dimensions.height.isFinite,
        dimensions.width > 0, dimensions.height > 0
    else { return nil }
    return CGRect(origin: point, size: dimensions)
}

@MainActor
func axRead(_ element: AXUIElement, _ attribute: String) throws -> CFTypeRef? {
    guard AXIsProcessTrusted() else { throw ContentProbeError.permissionDenied }
    let timeout = AXUIElementSetMessagingTimeout(element, 0.1)
    guard timeout == .success else { throw ContentProbeError.ax(timeout.rawValue) }
    var raw: CFTypeRef?
    let status = AXUIElementCopyAttributeValue(element, attribute as CFString, &raw)
    // Missing optional attributes are distinct from communication or permission failures.
    if status == .attributeUnsupported || status == .noValue { return nil }
    guard status == .success else { throw ContentProbeError.ax(status.rawValue) }
    return raw
}

@MainActor
func axFrame(_ element: AXUIElement) throws -> CGRect? {
    return try frameFromAX(
        position: axRead(element, kAXPositionAttribute), size: axRead(element, kAXSizeAttribute)
    )
}

@MainActor
func bindTarget(_ request: ContentProbeRequest) throws -> (window: AXUIElement, launchDate: Date) {
    try validateRequest(request)
    guard !request.excludedBundleIDs.contains(request.bundleID) else {
        throw ContentProbeError.sourceExcluded
    }
    guard AXIsProcessTrusted() else { throw ContentProbeError.permissionDenied }
    guard let target = NSRunningApplication(processIdentifier: request.pid), !target.isTerminated,
        target.bundleIdentifier == request.bundleID, let launched = target.launchDate,
        NSWorkspace.shared.frontmostApplication?.processIdentifier == request.pid
    else { throw ContentProbeError.targetChanged }
    let app = AXUIElementCreateApplication(request.pid)
    let window = try checkedAXElement(axRead(app, kAXFocusedWindowAttribute))
    try verifyTarget(request, window: window, launchDate: launched)
    return (window, launched)
}

@MainActor
func verifyTarget(_ request: ContentProbeRequest, window: AXUIElement, launchDate: Date) throws {
    guard AXIsProcessTrusted() else { throw ContentProbeError.permissionDenied }
    guard let target = NSRunningApplication(processIdentifier: request.pid), !target.isTerminated,
        target.bundleIdentifier == request.bundleID, target.launchDate == launchDate,
        NSWorkspace.shared.frontmostApplication?.processIdentifier == request.pid
    else { throw ContentProbeError.targetChanged }
    let current = try checkedAXElement(
        axRead(AXUIElementCreateApplication(request.pid), kAXFocusedWindowAttribute))
    guard CFEqual(window, current) else { throw ContentProbeError.targetChanged }
    let document = try axRead(window, kAXDocumentAttribute) as? String
    try validateSource(request, actualDocument: document.flatMap { URL(string: $0) })
}

@MainActor
private func axChildren(_ element: AXUIElement, capacity: Int) throws -> (
    elements: [AXUIElement], lost: Bool
) {
    guard AXIsProcessTrusted() else { throw ContentProbeError.permissionDenied }
    var count = 0
    let countStatus = AXUIElementGetAttributeValueCount(
        element, kAXChildrenAttribute as CFString, &count)
    if countStatus == .attributeUnsupported || countStatus == .noValue { return ([], false) }
    guard countStatus == .success, count >= 0 else {
        throw ContentProbeError.ax(countStatus.rawValue)
    }
    let take = min(count, max(0, capacity))
    guard take > 0 else { return ([], count > 0) }
    var values: CFArray?
    let status = AXUIElementCopyAttributeValues(
        element, kAXChildrenAttribute as CFString, 0, take, &values)
    guard status == .success else { throw ContentProbeError.ax(status.rawValue) }
    guard let objects = values as? [AnyObject], objects.count <= take else {
        throw ContentProbeError.unsupported
    }
    return (try objects.map { try checkedAXElement($0) }, count > take)
}

@MainActor
private func axText(_ element: AXUIElement, role: String, snapshot: inout AXSnapshot) throws
    -> String?
{
    if try axRead(element, kAXSubroleAttribute) as? String == kAXSecureTextFieldSubrole {
        snapshot.limitations.insert(.unavailableText)
        return nil
    }
    if role == kAXStaticTextRole { return try axRead(element, kAXValueAttribute) as? String }
    guard let count = try axRead(element, kAXNumberOfCharactersAttribute) as? NSNumber,
        count.intValue >= 0
    else {
        snapshot.limitations.insert(.unavailableText)
        return nil
    }
    guard count.intValue > 0 else { return nil }
    var range = CFRange(location: 0, length: min(2_048, count.intValue))
    guard let parameter = AXValueCreate(.cfRange, &range) else {
        throw ContentProbeError.unsupported
    }
    var raw: CFTypeRef?
    let status = AXUIElementCopyParameterizedAttributeValue(
        element, kAXStringForRangeParameterizedAttribute as CFString, parameter, &raw
    )
    if status == .attributeUnsupported || status == .noValue {
        snapshot.limitations.insert(.unavailableText)
        return nil
    }
    guard status == .success else { throw ContentProbeError.ax(status.rawValue) }
    guard let text = raw as? String else { throw ContentProbeError.unsupported }
    if count.intValue > range.length { snapshot.limitations.insert(.byteLimit) }
    return text
}

@MainActor
func readAXSnapshot(
    _ request: ContentProbeRequest, window: AXUIElement, launchDate: Date
) throws -> (snapshot: AXSnapshot, navigationTarget: AXUIElement?) {
    try verifyTarget(request, window: window, launchDate: launchDate)
    var snapshot = AXSnapshot()
    snapshot.windowFrame = try axFrame(window)
    if snapshot.windowFrame == nil { snapshot.limitations.insert(.geometryUnavailable) }
    let deadline = ContinuousClock.now.advanced(by: .milliseconds(1_500))
    var stack = [(window, 0)]
    var visited: [AXUIElement] = []
    var candidate: AXUIElement?
    while let (node, depth) = stack.popLast() {
        if ContinuousClock.now >= deadline {
            snapshot.limitations.insert(.timeLimit)
            break
        }
        if snapshot.nodesVisited >= 64 {
            snapshot.limitations.insert(.nodeLimit)
            break
        }
        if visited.contains(where: { CFEqual($0, node) }) { continue }
        visited.append(node)
        snapshot.nodesVisited += 1
        do {
            let role = try axRead(node, kAXRoleAttribute) as? String
            if role == kAXStaticTextRole || role == kAXTextAreaRole || role == kAXTextFieldRole {
                // Re-check the document before each content read; a changed source discards the whole snapshot.
                try verifyTarget(request, window: window, launchDate: launchDate)
                // Window metadata does not establish the source of split panes or embedded documents.
                // Never inherit permission for text whose own document identity is unavailable.
                if let document = try axRead(node, kAXDocumentAttribute) as? String,
                    let documentURL = URL(string: document)
                {
                    try validateSource(request, actualDocument: documentURL)
                    if let text = try axText(node, role: role ?? "", snapshot: &snapshot) {
                        let frame = try axFrame(node)
                        if frame == nil { snapshot.limitations.insert(.geometryUnavailable) }
                        appendEvidence(text, frame: frame, to: &snapshot)
                    }
                } else {
                    snapshot.limitations.insert(.unavailableText)
                }
            }
            if candidate == nil,
                (request.navigation == .scrollDown && role == kAXScrollBarRole)
                    || (request.navigation == .expand && role == kAXDisclosureTriangleRole)
            {
                candidate = node
            }
            if depth < 8 {
                let children = try axChildren(
                    node, capacity: 64 - snapshot.nodesVisited - stack.count)
                if children.lost { snapshot.limitations.insert(.nodeLimit) }
                stack.append(contentsOf: children.elements.reversed().map { ($0, depth + 1) })
            } else {
                snapshot.limitations.insert(.depthLimit)
            }
        } catch let error as ContentProbeError {
            switch error {
            case .permissionDenied, .targetChanged, .sourceUnknown, .sourceExcluded, .sourceChanged:
                throw error
            default:
                snapshot.limitations.insert(.readError)
            }
        }
        if snapshot.limitations.contains(.byteLimit) { break }
    }
    try verifyTarget(request, window: window, launchDate: launchDate)
    return (snapshot, candidate)
}

@MainActor
func navigateAX(
    _ request: ContentProbeRequest, node: AXUIElement, window: AXUIElement,
    launchDate: Date, expectedWindowFrame: CGRect?, authorize: () -> Bool
) throws -> Bool {
    try verifyTarget(request, window: window, launchDate: launchDate)
    guard let expectedWindowFrame, try axFrame(window) == expectedWindowFrame,
        CFEqual(try checkedAXElement(axRead(node, kAXWindowAttribute)), window)
    else { throw ContentProbeError.targetChanged }
    let document = try axRead(node, kAXDocumentAttribute) as? String
    try validateSource(request, actualDocument: document.flatMap { URL(string: $0) })
    let role = try axRead(node, kAXRoleAttribute) as? String
    if request.navigation == .scrollDown, role == kAXScrollBarRole,
        try axRead(node, kAXOrientationAttribute) as? String == kAXVerticalOrientationValue
    {
        var names: CFArray?
        let status = AXUIElementCopyActionNames(node, &names)
        guard status == .success else { throw ContentProbeError.ax(status.rawValue) }
        guard let actions = names as? [String],
            actions.contains(kAXIncrementAction)
        else {
            throw ContentProbeError.unsupported
        }
        guard authorize(), AXIsProcessTrusted() else { return false }
        let action = AXUIElementPerformAction(node, kAXIncrementAction as CFString)
        guard action == .success else { throw ContentProbeError.ax(action.rawValue) }
    } else if request.navigation == .expand, role == kAXDisclosureTriangleRole,
        try axRead(node, kAXExpandedAttribute) as? Bool == false
    {
        var settable = DarwinBoolean(false)
        let status = AXUIElementIsAttributeSettable(
            node, kAXExpandedAttribute as CFString, &settable)
        guard status == .success else { throw ContentProbeError.ax(status.rawValue) }
        guard settable.boolValue else { throw ContentProbeError.unsupported }
        guard authorize(), AXIsProcessTrusted() else { return false }
        let action = AXUIElementSetAttributeValue(
            node, kAXExpandedAttribute as CFString, kCFBooleanTrue)
        guard action == .success else { throw ContentProbeError.ax(action.rawValue) }
    } else {
        throw ContentProbeError.unsupported
    }
    // A timeout or stop request cannot retract an AX operation already sent to another process.
    try verifyTarget(request, window: window, launchDate: launchDate)
    return true
}
