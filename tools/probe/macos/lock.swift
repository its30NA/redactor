// P6c helper. `lock listen`: wait up to 5 s for com.apple.screenIsLocked on the distributed
// notification centre, print "ready" once listening and "received=yes|no" at the end.
// `lock post`: post that notification from a second process.
import Foundation

let name = Notification.Name("com.apple.screenIsLocked")
let centre = DistributedNotificationCenter.default()
let args = CommandLine.arguments
setvbuf(stdout, nil, _IOLBF, 0)

switch args.count > 1 ? args[1] : "" {
case "listen":
    let token = centre.addObserver(forName: name, object: nil, queue: .main) { _ in
        print("received=yes")
        exit(0)
    }
    _ = token
    print("ready")
    DispatchQueue.main.asyncAfter(deadline: .now() + 5) {
        print("received=no")
        exit(0)
    }
    RunLoop.main.run()
case "post":
    centre.postNotificationName(name, object: nil, userInfo: nil, deliverImmediately: true)
    print("posted")
default:
    FileHandle.standardError.write("usage: lock listen | post\n".data(using: .utf8)!)
    exit(2)
}
