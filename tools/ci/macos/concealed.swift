// The pasteboard as a password manager uses it, for tools/ci/macos-smoke.sh.
// Built with `swiftc -o concealed concealed.swift`.
//
//   concealed copy <text>   copies <text> with the marker password managers set
//                           (org.nspasteboard.ConcealedType), declared next to the text the way
//                           most of them do it; prints the new change count
//   concealed count         prints the pasteboard's change count (it rises with every copy)
//
// Exit 1 when the pasteboard refused the text (another copy took it meanwhile).
import AppKit

let concealed = NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType")
let pasteboard = NSPasteboard.general
let args = Array(CommandLine.arguments.dropFirst())

switch args.first {
case "copy" where args.count == 2:
    pasteboard.declareTypes([.string, concealed], owner: nil)
    let textSet = pasteboard.setString(args[1], forType: .string)
    let markerSet = pasteboard.setString("", forType: concealed)
    print(pasteboard.changeCount)
    exit(textSet && markerSet ? 0 : 1)
case "count" where args.count == 1:
    print(pasteboard.changeCount)
default:
    FileHandle.standardError.write(Data("usage: concealed copy <text> | concealed count\n".utf8))
    exit(2)
}
