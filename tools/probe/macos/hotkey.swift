// P6b helper: register Control+Option+R with Carbon's RegisterEventHotKey (what the
// global-hotkey crate uses), post that key with CGEventPost, and print whether the hot key
// handler ran within 2 s. Exit 0 either way; the printed line carries the result.
import AppKit
import Carbon

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

var eventType = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
let installed = InstallEventHandler(
    GetApplicationEventTarget(),
    { _, _, _ -> OSStatus in
        print("fired=yes")
        exit(0)
    },
    1, &eventType, nil, nil)

var hotKeyRef: EventHotKeyRef?
let hotKeyID = EventHotKeyID(signature: OSType(0x5244_5052), id: 1)  // "RDPR"
let registered = RegisterEventHotKey(
    UInt32(kVK_ANSI_R), UInt32(controlKey | optionKey), hotKeyID,
    GetApplicationEventTarget(), 0, &hotKeyRef)
print("install=\(installed) register=\(registered) axTrusted=\(AXIsProcessTrusted())")

func post(_ down: Bool) {
    let source = CGEventSource(stateID: .hidSystemState)
    guard let event = CGEvent(keyboardEventSource: source, virtualKey: CGKeyCode(kVK_ANSI_R), keyDown: down) else {
        print("event=nil")
        return
    }
    event.flags = [.maskControl, .maskAlternate]
    event.post(tap: .cghidEventTap)
}

DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
    post(true)
    post(false)
}
DispatchQueue.main.asyncAfter(deadline: .now() + 2.5) {
    print("fired=no")
    exit(0)
}
app.run()
