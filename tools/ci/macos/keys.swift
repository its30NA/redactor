// Posts key presses and mouse clicks the way a keyboard and a mouse would (CGEventPost at the
// HID level), for tools/ci/macos-smoke.sh. Built with `swiftc -o keys keys.swift`.
//
//   keys key <name> [cmd] [shift] [ctrl] [alt]   press and release one key with modifiers held,
//                                                 e.g. `keys key v cmd`, `keys key r ctrl alt`
//   keys rightclick <x> <y>                       right-click at a point (global coordinates in
//   keys leftclick <x> <y>                        points, origin at the top left of the main display)
//
// Exit 0 when the events were posted, 2 on bad arguments or when no event could be made. Whether
// they arrive anywhere depends on the runner's permissions: the smoke's key probe checks that.
import AppKit

/// US-layout virtual key codes (HIToolbox Events.h, kVK_ANSI_* and friends).
let keyCodes: [String: CGKeyCode] = [
    "a": 0x00, "s": 0x01, "d": 0x02, "f": 0x03, "h": 0x04, "g": 0x05, "z": 0x06, "x": 0x07,
    "c": 0x08, "v": 0x09, "b": 0x0B, "q": 0x0C, "w": 0x0D, "e": 0x0E, "r": 0x0F, "y": 0x10,
    "t": 0x11, "1": 0x12, "2": 0x13, "3": 0x14, "o": 0x1F, "j": 0x26, "k": 0x28,
    "return": 0x24, "space": 0x31, "escape": 0x35,
]

/// Modifier name -> (its own key code, its flag).
let modifiers: [String: (CGKeyCode, CGEventFlags)] = [
    "cmd": (0x37, .maskCommand),
    "shift": (0x38, .maskShift),
    "alt": (0x3A, .maskAlternate),
    "ctrl": (0x3B, .maskControl),
]

func die(_ message: String) -> Never {
    FileHandle.standardError.write(Data("keys: \(message)\n".utf8))
    exit(2)
}

func pause() { usleep(30_000) }

func post(_ event: CGEvent?) {
    guard let event else { die("could not make the event") }
    event.post(tap: .cghidEventTap)
    pause()
}

func pressKey(_ name: String, _ held: [String]) {
    guard let code = keyCodes[name.lowercased()] else { die("unknown key \(name)") }
    var mods: [(CGKeyCode, CGEventFlags)] = []
    for m in held {
        guard let mod = modifiers[m.lowercased()] else { die("unknown modifier \(m)") }
        mods.append(mod)
    }
    let source = CGEventSource(stateID: .hidSystemState)
    // The modifiers go down one by one, each event carrying the flags held so far, then the key,
    // then the modifiers come up in reverse order: what a keyboard sends.
    var flags: CGEventFlags = []
    for (modCode, flag) in mods {
        flags.insert(flag)
        let down = CGEvent(keyboardEventSource: source, virtualKey: modCode, keyDown: true)
        down?.flags = flags
        post(down)
    }
    for isDown in [true, false] {
        let event = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: isDown)
        event?.flags = flags
        post(event)
    }
    for (modCode, flag) in mods.reversed() {
        flags.remove(flag)
        let up = CGEvent(keyboardEventSource: source, virtualKey: modCode, keyDown: false)
        up?.flags = flags
        post(up)
    }
}

func click(_ x: Double, _ y: Double, right: Bool) {
    let source = CGEventSource(stateID: .hidSystemState)
    let point = CGPoint(x: x, y: y)
    let button: CGMouseButton = right ? .right : .left
    post(CGEvent(mouseEventSource: source, mouseType: .mouseMoved, mouseCursorPosition: point, mouseButton: button))
    post(CGEvent(
        mouseEventSource: source, mouseType: right ? .rightMouseDown : .leftMouseDown,
        mouseCursorPosition: point, mouseButton: button))
    post(CGEvent(
        mouseEventSource: source, mouseType: right ? .rightMouseUp : .leftMouseUp,
        mouseCursorPosition: point, mouseButton: button))
}

let args = Array(CommandLine.arguments.dropFirst())
switch args.first {
case "key" where args.count >= 2:
    pressKey(args[1], Array(args.dropFirst(2)))
case "rightclick" where args.count == 3, "leftclick" where args.count == 3:
    guard let x = Double(args[1]), let y = Double(args[2]) else { die("x and y must be numbers") }
    click(x, y, right: args[0] == "rightclick")
default:
    die("usage: keys key <name> [cmd] [shift] [ctrl] [alt] | keys rightclick|leftclick <x> <y>")
}
