// P6d helper: `imagecheck <png>` prints the image size and the brightest sampled pixel
// (0-255), so the caller can tell a real screenshot from a black one.
import AppKit

guard CommandLine.arguments.count > 1,
    let image = NSImage(contentsOfFile: CommandLine.arguments[1]),
    let tiff = image.tiffRepresentation,
    let bitmap = NSBitmapImageRep(data: tiff)
else {
    print("unreadable")
    exit(0)
}

var brightest = 0.0
let step = max(1, min(bitmap.pixelsWide, bitmap.pixelsHigh) / 64)
for y in stride(from: 0, to: bitmap.pixelsHigh, by: step) {
    for x in stride(from: 0, to: bitmap.pixelsWide, by: step) {
        if let colour = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB) {
            brightest = max(brightest, Double(colour.redComponent + colour.greenComponent + colour.blueComponent) / 3)
        }
    }
}
print("size=\(bitmap.pixelsWide)x\(bitmap.pixelsHigh) brightest=\(Int(brightest * 255))")
