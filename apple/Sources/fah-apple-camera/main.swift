#if os(macOS)
import AppKit
import FAHApple
import Foundation

final class Preview: NSView {
    var image: NSImage?
    var points: [[[Double]]] = []
    override var isFlipped: Bool { true }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.black.setFill(); bounds.fill()
        guard let image else { return }
        let scale = min(bounds.width / image.size.width, bounds.height / image.size.height)
        let size = NSSize(width: image.size.width * scale, height: image.size.height * scale)
        let rect = NSRect(x: (bounds.width-size.width)/2, y: (bounds.height-size.height)/2, width: size.width, height: size.height)
        image.draw(in: rect, from: .zero, operation: .copy, fraction: 1, respectFlipped: true, hints: nil)
        NSColor.systemGreen.setStroke(); NSColor.white.setFill()
        for hand in points where hand.count == 21 {
            let mapped = hand.map { NSPoint(x: rect.minX + $0[0]*scale, y: rect.minY + $0[1]*scale) }
            let path = NSBezierPath(); path.lineWidth = 2
            for start in [1,5,9,13,17] {
                path.move(to: mapped[0]); path.line(to: mapped[start])
                for index in (start+1)...(start+3) { path.line(to: mapped[index]) }
            }
            path.stroke()
            for p in mapped { NSBezierPath(ovalIn: NSRect(x:p.x-3,y:p.y-3,width:6,height:6)).fill() }
        }
    }
}

final class Viewer: NSObject, NSApplicationDelegate {
    let models: URL
    let window = NSWindow(contentRect: NSRect(x:0,y:0,width:960,height:760), styleMask: [.titled,.closable,.resizable,.miniaturizable], backing:.buffered, defer:false)
    let preview = Preview()
    let status = NSTextField(labelWithString: "Preparing Metal…")
    let toggle = NSButton(title: "Start camera", target:nil, action:nil)
    var camera: CameraSource?
    var generation = 0
    var running = false
    let logURL: URL?
    init(models: URL, logURL: URL?) { self.models = models; self.logURL = logURL; super.init() }
    func applicationDidFinishLaunching(_ notification: Notification) {
        window.title = "FAH · Metal camera"
        let bar = NSStackView(views: [toggle,status]); bar.spacing = 16
        let root = NSStackView(views: [bar,preview]); root.orientation = .vertical
        root.edgeInsets = NSEdgeInsets(top:12,left:12,bottom:12,right:12)
        preview.widthAnchor.constraint(greaterThanOrEqualToConstant:640).isActive = true
        preview.heightAnchor.constraint(greaterThanOrEqualToConstant:480).isActive = true
        preview.setContentHuggingPriority(.defaultLow, for:.vertical)
        window.contentView = root; window.center(); window.makeKeyAndOrderFront(nil)
        toggle.target=self; toggle.action = #selector(changeCapture)
        NSApp.activate(ignoringOtherApps:true)
        start()
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
    func applicationWillTerminate(_ notification: Notification) { camera?.stop() }
    @objc func changeCapture() {
        if running {
            generation += 1; camera?.stop(); camera=nil; running=false
            preview.image=nil; preview.points=[]; preview.needsDisplay=true
            toggle.title="Start camera"; status.stringValue="Stopped"
        } else { start() }
    }
    func start() {
        generation += 1
        let token = generation
        running=true; toggle.title="Stop camera"; status.stringValue="Waiting for camera permission…"
        CameraSource.requestAccess { allowed in
            guard allowed else { DispatchQueue.main.async { self.fail("Camera permission denied", token:token) }; return }
            DispatchQueue.global(qos:.userInitiated).async {
                do {
                    let tracker = try TrackingSession(models:self.models)
                    let source = try CameraSource()
                    DispatchQueue.main.async {
                        guard self.generation == token else { return }
                        self.camera=source; self.status.stringValue=source.name
                        var count=0
                        source.start { buffer in
                            do {
                                let frame = try FrameInput.rgba(from:buffer)
                                let result = try tracker.process(rgba:frame.data,width:frame.width,height:frame.height,stride:frame.stride,reset:count==0)
                                let json = try JSONSerialization.jsonObject(with:result) as! [String:Any]
                                let hands = json["hands"] as? [[String:Any]] ?? []
                                let points = hands.compactMap { $0["joints2d"] as? [[Double]] }
                                let bitmap = NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:frame.width,pixelsHigh:frame.height,bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:frame.stride,bitsPerPixel:32)!
                                frame.data.withUnsafeBytes { bitmap.bitmapData!.update(from:$0.bindMemory(to:UInt8.self).baseAddress!,count:frame.data.count) }
                                let image = NSImage(size:NSSize(width:frame.width,height:frame.height)); image.addRepresentation(bitmap)
                                count += 1
                                if let path=self.logURL, count <= 300 {
                                    // Write numeric inference results only; do not record camera images.
                                    if !FileManager.default.fileExists(atPath:path.path) { FileManager.default.createFile(atPath:path.path,contents:nil) }
                                    let file=try FileHandle(forWritingTo:path); defer { try? file.close() }
                                    try file.seekToEnd(); try file.write(contentsOf:result); try file.write(contentsOf:Data([10]))
                                }
                                let text=String(format:"Metal · %d hands · %.1f ms · frame %d",hands.count,json["total_ms"] as? Double ?? 0,count)
                                DispatchQueue.main.async {
                                    guard self.generation == token else { return }
                                    self.preview.image=image; self.preview.points=points; self.preview.needsDisplay=true
                                    self.status.stringValue=text
                                }
                            } catch { source.stop(); DispatchQueue.main.async { self.fail(String(describing:error),token:token) } }
                        }
                    }
                } catch { DispatchQueue.main.async { self.fail(String(describing:error),token:token) } }
            }
        }
    }
    func fail(_ message:String, token:Int) {
        guard generation == token else { return }
        status.stringValue=message; running=false; toggle.title="Start camera"
        FileHandle.standardError.write(Data((message+"\n").utf8))
    }
}
let args=CommandLine.arguments
func argument(_ flag:String) -> String? { guard let index=args.firstIndex(of:flag), index+1<args.count else { return nil }; return args[index+1] }
guard let models=argument("--models") else { fatalError("Use --models DIRECTORY [--log FILE]") }
let app=NSApplication.shared
app.setActivationPolicy(.regular)
let viewer=Viewer(models:URL(fileURLWithPath:models),logURL:argument("--log").map { URL(fileURLWithPath:$0) })
app.delegate=viewer
app.run()
#else
import Foundation
print("fah-apple-camera is a macOS viewer; use CameraSource from an iOS app.")
#endif
