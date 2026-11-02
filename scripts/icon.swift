import AppKit
// Vector artwork rendered at native icon sizes; no bundled raster or design dependency.
let directory = "target/AppIcon.iconset"
try FileManager.default.createDirectory(atPath: directory, withIntermediateDirectories: true)
for size in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let pixels = size * scale
        let image = NSImage(size: NSSize(width: pixels, height: pixels))
        image.lockFocus()
        let p = CGFloat(pixels)
        NSColor(srgbRed: 0.94, green: 0.96, blue: 0.90, alpha: 1).setFill()
        NSBezierPath(roundedRect: NSRect(x:p*0.04,y:p*0.04,width:p*0.92,height:p*0.92), xRadius:p*0.21,yRadius:p*0.21).fill()
        NSColor(srgbRed:0.18,green:0.42,blue:0.34,alpha:1).setStroke()
        for (x, direction) in [(0.37, 1.0), (0.63, -1.0)] {
            let tip = CGPoint(x:p*x,y:p*(0.5+direction*0.24))
            let path = NSBezierPath()
            path.lineWidth = p*0.065
            path.lineCapStyle = .round
            path.lineJoinStyle = .round
            path.move(to: CGPoint(x:p*x,y:p*(0.5-direction*0.24)))
            path.line(to:tip)
            path.move(to:CGPoint(x:tip.x-p*0.11,y:tip.y-direction*p*0.11))
            path.line(to:tip)
            path.line(to:CGPoint(x:tip.x+p*0.11,y:tip.y-direction*p*0.11))
            path.stroke()
        }
        image.unlockFocus()
        let bitmap = NSBitmapImageRep(data:image.tiffRepresentation!)!
        let png = bitmap.representation(using:.png,properties:[:])!
        let suffix = scale == 2 ? "@2x" : ""
        try png.write(to:URL(fileURLWithPath:"\(directory)/icon_\(size)x\(size)\(suffix).png"))
    }
}
