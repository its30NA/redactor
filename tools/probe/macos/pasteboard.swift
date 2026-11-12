// P6a helper. `pasteboard write <text>`: put text on the general pasteboard together with the
// concealed marker that password managers use, print the changeCount.
// `pasteboard read`: from a second process, print changeCount, the types and the text.
import AppKit

let concealed = NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType")
let board = NSPasteboard.general
let args = CommandLine.arguments

switch args.count > 1 ? args[1] : "" {
case "write" where args.count > 2:
    board.clearContents()
    board.declareTypes([.string, concealed], owner: nil)
    let wroteText = board.setString(args[2], forType: .string)
    let wroteMark = board.setString("", forType: concealed)
    print("changeCount=\(board.changeCount) text=\(wroteText) concealed=\(wroteMark)")
case "read":
    let types = (board.types ?? []).map { $0.rawValue }.joined(separator: ",")
    print("changeCount=\(board.changeCount)")
    print("types=\(types)")
    print("text=\(board.string(forType: .string) ?? "")")
default:
    FileHandle.standardError.write("usage: pasteboard write <text> | read\n".data(using: .utf8)!)
    exit(2)
}
