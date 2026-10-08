import AppKit
import CoreText

enum WireError: Error { case malformed }
struct Reader {
    let data: Data
    var offset = 0
    mutating func take(_ count: Int) throws -> Data {
        guard count >= 0, count <= data.count - offset else { throw WireError.malformed }
        defer { offset += count }
        return data.subdata(in: offset ..< offset + count)
    }
    mutating func u32() throws -> UInt32 {
        guard data.count-offset >= 4 else { throw WireError.malformed }
        defer { offset += 4 }
        return data.withUnsafeBytes { UInt32(littleEndian: $0.loadUnaligned(fromByteOffset: offset, as: UInt32.self)) }
    }
    mutating func u64() throws -> UInt64 { let low = try u32(); return UInt64(low) | (UInt64(try u32()) << 32) }
    mutating func f() throws -> CGFloat { CGFloat(Float(bitPattern: try u32())) }
    mutating func point() throws -> CGPoint { let x = try f(); return CGPoint(x: x, y: try f()) }
    mutating func rect() throws -> CGRect { let x = try f(), y = try f(), w = try f(), h = try f(); return CGRect(x:x,y:y,width:w,height:h) }
    mutating func color() throws -> CGColor {
        let bytes = try take(4)
        return CGColor(colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!, components: bytes.map { CGFloat($0)/255 })!
    }
    mutating func shape() throws -> CGPath {
        let kind = try u32(), path = CGMutablePath()
        switch kind {
        case 0: path.addRect(try rect())
        case 1: let bounds = try rect(), radius = try f(); path.addRoundedRect(in: bounds, cornerWidth: radius, cornerHeight: radius)
        case 2: path.addEllipse(in: try rect())
        case 3:
            let n = try u32()
            for _ in 0..<n {
                switch try u32() {
                case 0: path.move(to: try point())
                case 1: path.addLine(to: try point())
                case 2: let a = try point(), b = try point(), end = try point(); path.addCurve(to: end, control1: a, control2: b)
                case 3: path.closeSubpath()
                default: throw WireError.malformed
                }
            }
        default: throw WireError.malformed
        }
        return path
    }
}
enum Brush {
    case solid(CGColor)
    case linear(CGPoint, CGPoint, CGColor, CGColor)
    static func read(_ r: inout Reader) throws -> Brush {
        switch try r.u32() {
        case 0: return .solid(try r.color())
        case 1: let a = try r.point(), b = try r.point(), c = try r.color(), d = try r.color(); return .linear(a,b,c,d)
        default: throw WireError.malformed
        }
    }
    func draw(_ context: CGContext, path: CGPath, width: CGFloat?) {
        context.saveGState(); defer { context.restoreGState() }
        context.addPath(path)
        if let width = width { context.setLineWidth(width) }
        switch self {
        case .solid(let color):
            if width != nil { context.setStrokeColor(color); context.strokePath() }
            else { context.setFillColor(color); context.fillPath(using: .winding) }
        case .linear(let from, let to, let start, let end):
            if width != nil { context.replacePathWithStrokedPath() }
            context.clip(using: .winding)
            if let gradient = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: [start,end] as CFArray, locations: [0,1]) {
                context.drawLinearGradient(gradient,start:from,end:to,options:[.drawsBeforeStartLocation,.drawsAfterEndLocation])
            }
        }
    }
}

final class RusterizeView: NSView {
    private var handle = rusterize_create()
    private let origin = ProcessInfo.processInfo.systemUptime
    private var timer: Timer?
    private var images: [UInt64: CGImage] = [:]
    private var textCache: [String: CTLine] = [:]
    private var tracking: NSTrackingArea?
    private var windowRevision:UInt32 = 0
    var initialSize:NSSize {NSSize(width:CGFloat(Float(bitPattern:rusterize_window_option(handle,0))),height:CGFloat(Float(bitPattern:rusterize_window_option(handle,1))))}
    var minimumSize:NSSize {NSSize(width:CGFloat(Float(bitPattern:rusterize_window_option(handle,2))),height:CGFloat(Float(bitPattern:rusterize_window_option(handle,3))))}
    var resizable:Bool {rusterize_window_option(handle,7) != 0}
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override init(frame: NSRect) { super.init(frame:frame); wantsLayer = false }
    required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
    deinit { timer?.invalidate(); rusterize_destroy(handle) }
    func event(_ kind: UInt32, _ x: CGFloat = 0, _ y: CGFloat = 0, _ dx: CGFloat = 0, _ dy: CGFloat = 0, _ detail: UInt32 = 0, _ flags: UInt32 = 0) {
        _ = rusterize_event(handle,kind,Float(x),Float(y),Float(dx),Float(dy),detail,flags); schedule()
    }
    private func schedule() {
        let status = rusterize_status(handle)
        if status & 0x80000000 != 0 { fail(String(cString:rusterize_error(handle))); return }
        if status & 4 != 0 { window?.close(); return }
        updateChrome()
        if status & 1 != 0 { needsDisplay = true }
        if status & 2 != 0 {
            if timer == nil {
                let created = Timer(timeInterval:1.0/60.0,repeats:true) { [weak self] _ in self?.needsDisplay = true }
                timer = created; RunLoop.main.add(created,forMode:.common)
            }
        } else { timer?.invalidate(); timer = nil }
    }
    func updateChrome() {
        guard let window = window else {return}
        let revision = rusterize_window_option(handle,8)
        if revision == windowRevision {return};windowRevision = revision
        window.title = String(cString:rusterize_title(handle))
        let bg = rusterize_window_option(handle,4),fg = rusterize_window_option(handle,5)
        window.titlebarAppearsTransparent = bg != UInt32.max
        window.backgroundColor = bg == UInt32.max ? NSColor.windowBackgroundColor : NSColor(srgbRed:CGFloat((bg>>16)&255)/255,green:CGFloat((bg>>8)&255)/255,blue:CGFloat(bg&255)/255,alpha:1)
        if fg == UInt32.max {window.appearance = nil}
        else {let dark = ((fg>>16&255)*299+(fg>>8&255)*587+(fg&255)*114)<128000;window.appearance = NSAppearance(named:dark ? .aqua:.darkAqua)}
    }
    private func fail(_ message:String) {
        FileHandle.standardError.write(Data(("Rusterize: " + message + "\n").utf8))
        exit(1)
    }
    func snapshot(to path:String) throws {
        let status = rusterize_frame(handle,960,680,1,0)
        guard status & 0x80000000 == 0,let pointer = rusterize_data(handle),let context = CGContext(data:nil,width:960,height:680,bitsPerComponent:8,bytesPerRow:960*4,space:CGColorSpace(name:CGColorSpace.sRGB)!,bitmapInfo:CGImageAlphaInfo.premultipliedLast.rawValue) else {throw WireError.malformed}
        context.translateBy(x:0,y:680);context.scaleBy(x:1,y:-1)
        try replay(context,Data(bytes:pointer,count:rusterize_len(handle)))
        guard let image = context.makeImage(),let png = NSBitmapImageRep(cgImage:image).representation(using:.png,properties:[:]) else {throw WireError.malformed}
        try png.write(to:URL(fileURLWithPath:path))
    }
    override func draw(_ dirtyRect:NSRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        let scale = window?.backingScaleFactor ?? 1
        let status = rusterize_frame(handle,Float(bounds.width),Float(bounds.height),Float(scale),ProcessInfo.processInfo.systemUptime-origin)
        if status & 0x80000000 != 0 { fail(String(cString:rusterize_error(handle))); return }
        let count = rusterize_len(handle)
        guard let pointer = rusterize_data(handle), count >= 8 else { fail("Empty display list"); return }
        do { try replay(context,Data(bytes:pointer,count:count)) } catch { fail("Invalid display list: \(error)") }
        schedule()
    }
    private func replay(_ context:CGContext,_ data:Data) throws {
        var r = Reader(data:data)
        guard try r.u32() == 0x31305a52 else { throw WireError.malformed }
        let commands = try r.u32()
        var depth = 0, used = Set<UInt64>()
        context.saveGState()
        defer { for _ in 0..<depth { context.restoreGState() }; context.restoreGState() }
        context.clear(bounds); context.setLineCap(.butt);context.setLineJoin(.miter);context.setMiterLimit(10)
        for _ in 0..<commands {
            switch try r.u32() {
            case 1:
                let color = try r.color(); context.saveGState();context.setBlendMode(.copy);context.setFillColor(color);context.fill(bounds);context.restoreGState()
            case 2: context.saveGState(); depth += 1
            case 3: guard depth > 0 else {throw WireError.malformed}; context.restoreGState();depth -= 1
            case 4:
                let a = try r.f(),b = try r.f(),c = try r.f(),d = try r.f(),e = try r.f(),f = try r.f()
                context.concatenate(CGAffineTransform(a:a,b:b,c:c,d:d,tx:e,ty:f))
            case 5: context.addPath(try r.shape());context.clip(using:.winding)
            case 6: let path = try r.shape(),brush = try Brush.read(&r);brush.draw(context,path:path,width:nil)
            case 7: let path = try r.shape(),brush = try Brush.read(&r),width = try r.f();brush.draw(context,path:path,width:width)
            case 8:
                let point = try r.point(),size = try r.f(),color = try r.color(),family = try r.u32(),bold = try r.u32(),length = try r.u32()
                guard family < 3, let string = String(data:try r.take(Int(length)),encoding:.utf8) else {throw WireError.malformed}
                let cacheKey = "\(family):\(bold):\(size):\(string)"
                var line = textCache[cacheKey]
                if line == nil {
                    let fontName = family == 0 ? ".AppleSystemUIFont" : family == 1 ? "Georgia" : "Menlo"
                    var font = CTFontCreateWithName(fontName as CFString,size,nil)
                    if bold != 0, let heavy = CTFontCreateCopyWithSymbolicTraits(font,size,nil,.boldTrait,.boldTrait) { font = heavy }
                    let attributes: [NSAttributedString.Key: Any] = [NSAttributedString.Key(kCTFontAttributeName as String):font,NSAttributedString.Key(kCTForegroundColorFromContextAttributeName as String):true]
                    line = CTLineCreateWithAttributedString(NSAttributedString(string:string,attributes:attributes))
                    if textCache.count >= 256 {textCache.removeAll(keepingCapacity:true)}
                    textCache[cacheKey] = line
                }
                context.saveGState();context.translateBy(x:point.x,y:point.y);context.scaleBy(x:1,y:-1);context.textMatrix = .identity;context.textPosition = .zero;context.setFillColor(color);CTLineDraw(line!,context);context.restoreGState()
            case 9:
                let id = try r.u64(),w = try r.u32(),h = try r.u32(),destination = try r.rect(),opacity = try r.f(),length = try r.u32()
                guard w > 0,h > 0,UInt64(w)*UInt64(h)*4 == UInt64(length) else {throw WireError.malformed}
                let pixels = try r.take(Int(length))
                if images[id] == nil {
                    var rgba = [UInt8](pixels)
                    for i in stride(from:0,to:rgba.count,by:4) {let a = UInt32(rgba[i+3]);for c in 0..<3 {rgba[i+c] = UInt8((UInt32(rgba[i+c])*a+127)/255)}}
                    guard let provider = CGDataProvider(data:Data(rgba) as CFData), let image = CGImage(width:Int(w),height:Int(h),bitsPerComponent:8,bitsPerPixel:32,bytesPerRow:Int(w)*4,space:CGColorSpace(name:CGColorSpace.sRGB)!,bitmapInfo:CGBitmapInfo(rawValue:CGImageAlphaInfo.premultipliedLast.rawValue),provider:provider,decode:nil,shouldInterpolate:true,intent:.defaultIntent) else {throw WireError.malformed}
                    images[id] = image
                }
                used.insert(id)
                context.saveGState();context.setAlpha(opacity);context.translateBy(x:destination.minX,y:destination.maxY);context.scaleBy(x:1,y:-1);context.draw(images[id]!,in:CGRect(origin:.zero,size:destination.size));context.restoreGState()
            default: throw WireError.malformed
            }
        }
        guard depth == 0,r.offset == data.count else {throw WireError.malformed}
        images = images.filter { used.contains($0.key) }
    }
    override func updateTrackingAreas() {
        super.updateTrackingAreas();if let old = tracking {removeTrackingArea(old)}
        let area = NSTrackingArea(rect:.zero,options:[.mouseMoved,.activeInKeyWindow,.inVisibleRect],owner:self,userInfo:nil);addTrackingArea(area);tracking = area
    }
    private func pointer(_ e:NSEvent,_ phase:UInt32) {
        let p = convert(e.locationInWindow,from:nil);event(phase,p.x,p.y,0,0,0,UInt32(NSEvent.pressedMouseButtons)&7)
    }
    override func mouseDown(with e:NSEvent) {window?.makeFirstResponder(self);pointer(e,1)}
    override func mouseUp(with e:NSEvent) {pointer(e,3)}
    override func mouseMoved(with e:NSEvent) {pointer(e,2)}
    override func mouseDragged(with e:NSEvent) {pointer(e,2)}
    override func rightMouseDown(with e:NSEvent) {pointer(e,1)}
    override func rightMouseUp(with e:NSEvent) {pointer(e,3)}
    override func rightMouseDragged(with e:NSEvent) {pointer(e,2)}
    override func otherMouseDown(with e:NSEvent) {pointer(e,1)}
    override func otherMouseUp(with e:NSEvent) {pointer(e,3)}
    override func otherMouseDragged(with e:NSEvent) {pointer(e,2)}
    override func scrollWheel(with e:NSEvent) {let p = convert(e.locationInWindow,from:nil),factor:CGFloat = e.hasPreciseScrollingDeltas ? 1:40;event(5,p.x,p.y,-e.scrollingDeltaX*factor,-e.scrollingDeltaY*factor)}
    private func key(_ code:UInt16)->UInt32 {
        switch code {case 36,76:return 13;case 49:return 32;case 53:return 27;case 48:return 9;case 51:return 8;case 117:return 46;case 123:return 37;case 126:return 38;case 124:return 39;case 125:return 40;case 115:return 36;case 119:return 35;default:return 0x10000+UInt32(code)}
    }
    private func modifiers(_ e:NSEvent)->UInt32 {
        var result:UInt32 = 0
        if e.modifierFlags.contains(.shift){result |= 1};if e.modifierFlags.contains(.control){result |= 2};if e.modifierFlags.contains(.option){result |= 4};if e.modifierFlags.contains(.command){result |= 8};if e.isARepeat {result |= 16};return result
    }
    override func keyDown(with e:NSEvent) {event(6,0,0,0,0,key(e.keyCode),modifiers(e));if !e.modifierFlags.contains(.command) && !e.modifierFlags.contains(.control) {for scalar in (e.characters ?? "").unicodeScalars {event(8,0,0,0,0,scalar.value)}}}
    override func keyUp(with e:NSEvent) {event(7,0,0,0,0,key(e.keyCode),modifiers(e))}
}
final class Delegate:NSObject,NSApplicationDelegate,NSWindowDelegate {
    var window:NSWindow!
    var view:RusterizeView!
    func applicationDidFinishLaunching(_ notification:Notification) {
        view = RusterizeView(frame:NSRect(x:0,y:0,width:960,height:680))
        view.setFrameSize(view.initialSize)
        window = NSWindow(contentRect:view.frame,styleMask:[.titled,.closable,.miniaturizable,.resizable],backing:.buffered,defer:false)
        window.contentMinSize = view.minimumSize;window.contentView = view;if !view.resizable {window.styleMask.remove(.resizable)};view.updateChrome();window.delegate = self;window.isReleasedWhenClosed = false;window.center();window.makeKeyAndOrderFront(nil);window.makeFirstResponder(view)
        let menu = NSMenu(),item = NSMenuItem(),appMenu = NSMenu();appMenu.addItem(withTitle:"Завершить Rusterize",action:#selector(NSApplication.terminate(_:)),keyEquivalent:"q");item.submenu = appMenu;menu.addItem(item);NSApplication.shared.mainMenu = menu;NSApplication.shared.activate(ignoringOtherApps:true)
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender:NSApplication)->Bool {true}
    func windowDidBecomeKey(_ n:Notification){view.event(9,0,0,0,0,1)}
    func windowDidResignKey(_ n:Notification){view.event(9)}
    func windowDidMiniaturize(_ n:Notification){view.event(10)}
    func windowDidDeminiaturize(_ n:Notification){view.event(11)}
    func applicationDidHide(_ n:Notification){view.event(10)}
    func applicationDidUnhide(_ n:Notification){view.event(11)}
}
let application = NSApplication.shared
if CommandLine.arguments.count == 3 && CommandLine.arguments[1] == "--snapshot" {
    do {try RusterizeView(frame:NSRect(x:0,y:0,width:960,height:680)).snapshot(to:CommandLine.arguments[2]);exit(0)}
    catch {FileHandle.standardError.write(Data("\(error)\n".utf8));exit(1)}
}
let delegate = Delegate()
application.delegate = delegate
application.setActivationPolicy(.regular)
application.run()
