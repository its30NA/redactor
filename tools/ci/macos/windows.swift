// The windows on screen, and bringing an app to the front, for tools/ci/macos-smoke.sh.
// Built with `swiftc -o windows windows.swift`.
//
//   windows list                 one line per window:
//                                <owner>\t<layer>\t<width>x<height>\t<onscreen>\t<pid>\t<x>,<y>
//                                (points; onscreen is true or false; x,y is the top left corner)
//   windows activate <pid>       bring that app to the front; prints `front` or `not-front`
//                                (exit 0 or 1). When asking is not enough (macOS 14 and later only
//                                let the frontmost app hand over), it clicks the left end of the
//                                app's largest window's title bar, as a person would.
//   windows frontmost            the pid of the frontmost app
//
// Owner names, layers and bounds need no permission; window titles would (Screen Recording), so
// they are not read.
import AppKit

struct Window {
    let owner: String
    let layer: Int
    let bounds: CGRect
    let onscreen: Bool
    let pid: pid_t
}

func windows() -> [Window] {
    guard let info = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] else {
        return []
    }
    return info.map { w in
        var bounds = CGRect.zero
        if let dict = w[kCGWindowBounds as String] as? NSDictionary,
           let rect = CGRect(dictionaryRepresentation: dict as CFDictionary) {
            bounds = rect
        }
        return Window(
            owner: w[kCGWindowOwnerName as String] as? String ?? "?",
            layer: w[kCGWindowLayer as String] as? Int ?? -1,
            bounds: bounds,
            onscreen: w[kCGWindowIsOnscreen as String] as? Bool ?? false,
            pid: pid_t(w[kCGWindowOwnerPID as String] as? Int ?? -1))
    }
}

func frontmostPid() -> pid_t {
    // The workspace learns about activations through the run loop.
    RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.3))
    return NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1
}

func clickTitleBar(of pid: pid_t) -> Bool {
    let mine = windows().filter { $0.pid == pid && $0.layer == 0 && $0.onscreen }
    guard let w = mine.max(by: { $0.bounds.width * $0.bounds.height < $1.bounds.width * $1.bounds.height })
    else { return false }
    // Past the traffic lights (78 pt), on the empty title bar; 22 pt down is the bar's middle.
    let point = CGPoint(x: w.bounds.minX + 120, y: w.bounds.minY + 22)
    let source = CGEventSource(stateID: .hidSystemState)
    for type in [CGEventType.mouseMoved, .leftMouseDown, .leftMouseUp] {
        CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: point, mouseButton: .left)?
            .post(tap: .cghidEventTap)
        usleep(50_000)
    }
    return true
}

func activate(_ pid: pid_t) -> Bool {
    guard let app = NSRunningApplication(processIdentifier: pid) else { return false }
    if #available(macOS 14.0, *) {
        _ = app.activate()
    } else {
        _ = app.activate(options: [.activateIgnoringOtherApps])
    }
    if frontmostPid() == pid { return true }
    if clickTitleBar(of: pid), frontmostPid() == pid { return true }
    return frontmostPid() == pid
}

let args = Array(CommandLine.arguments.dropFirst())
switch args.first {
case "list":
    for w in windows() {
        print("\(w.owner)\t\(w.layer)\t\(Int(w.bounds.width))x\(Int(w.bounds.height))\t\(w.onscreen)\t\(w.pid)\t\(Int(w.bounds.minX)),\(Int(w.bounds.minY))")
    }
case "activate" where args.count == 2:
    guard let pid = pid_t(args[1]) else {
        FileHandle.standardError.write(Data("windows: the pid must be a number\n".utf8))
        exit(2)
    }
    if activate(pid) {
        print("front")
    } else {
        print("not-front")
        exit(1)
    }
case "frontmost":
    print(frontmostPid())
default:
    FileHandle.standardError.write(Data("usage: windows list | windows activate <pid> | windows frontmost\n".utf8))
    exit(2)
}
