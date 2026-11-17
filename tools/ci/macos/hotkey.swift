// Holds a Carbon hotkey (RegisterEventHotKey, the API the app's global hotkey uses) and says
// whether it fired, for the key probe of tools/ci/macos-smoke.sh: it proves that this machine
// hands posted keys to the hotkey matcher at all, before any check relies on it.
// Built with `swiftc -o hotkey hotkey.swift`.
//
//   hotkey <seconds>   holds Ctrl+Option+J; prints `ready` once registered, then `fired` (exit 0)
//                      or `timeout` after <seconds> (exit 1); `not-registered <status>` (exit 2)
import AppKit
import Carbon

let args = Array(CommandLine.arguments.dropFirst())
guard args.count == 1, let seconds = Double(args[0]), seconds > 0 else {
    FileHandle.standardError.write(Data("usage: hotkey <seconds>\n".utf8))
    exit(2)
}

func say(_ line: String) {
    print(line)
    fflush(stdout)
}

var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
let installed = InstallEventHandler(
    GetApplicationEventTarget(),
    { _, _, _ in
        say("fired")
        exit(0)
    },
    1, &spec, nil, nil)
guard installed == noErr else {
    say("not-registered handler \(installed)")
    exit(2)
}

var ref: EventHotKeyRef?
// 'RDCT', 1: any id will do, there is only this one.
let id = EventHotKeyID(signature: OSType(0x5244_4354), id: 1)
let status = RegisterEventHotKey(
    UInt32(kVK_ANSI_J), UInt32(controlKey | optionKey), id, GetApplicationEventTarget(), 0, &ref)
guard status == noErr else {
    say("not-registered \(status)")
    exit(2)
}

let app = NSApplication.shared
// No Dock icon and no menu bar of its own (a background app, as hotkey utilities are).
app.setActivationPolicy(.accessory)
DispatchQueue.main.asyncAfter(deadline: .now() + seconds) {
    say("timeout")
    exit(1)
}
say("ready")
app.run()
