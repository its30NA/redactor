// Posts a distributed notification by name to the apps of this session, for
// tools/ci/macos-smoke.sh (S7 posts com.apple.screenIsLocked: the app reacts as on a real lock;
// nothing is locked). Built with `swiftc -o notify notify.swift`.
//
//   notify <name>
import Foundation

let args = Array(CommandLine.arguments.dropFirst())
guard args.count == 1, !args[0].isEmpty else {
    FileHandle.standardError.write(Data("usage: notify <name>\n".utf8))
    exit(2)
}
DistributedNotificationCenter.default().postNotificationName(
    NSNotification.Name(args[0]), object: nil, userInfo: nil, deliverImmediately: true)
// Give the notification server a moment before this process goes away.
RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.3))
print("posted \(args[0])")
