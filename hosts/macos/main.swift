import AppKit
import CoreText

enum WireError: Error { case malformed, native(String) }
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
    mutating func string() throws -> String { let count = try u32(); guard let text = String(data:try take(Int(count)),encoding:.utf8) else { throw WireError.malformed };return text }
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


struct Writer {
    var data = Data()
    mutating func u32(_ value:UInt32) {var value=value.littleEndian;withUnsafeBytes(of:&value){data.append(contentsOf:$0)}}
    mutating func f(_ value:CGFloat) {u32(Float(value).bitPattern)}
    mutating func string(_ text:String) {let bytes=Data(text.utf8);u32(UInt32(bytes.count));data.append(bytes)}
}
struct NativeLine {let start:Int;var end:Int;let top:CGFloat;let height:CGFloat;let baseline:CGFloat}
final class NativeText {
    let text:String
    let storage:NSTextStorage
    let manager=NSLayoutManager()
    let container:NSTextContainer
    var lines:[NativeLine]=[]
    let used:CGRect
    let offsetX:CGFloat
    init(_ r:inout Reader) throws {
        let size=try r.f(),color=try r.color(),family=try r.u32(),flags=try r.u32(),name=try r.string(),width=try r.f(),wrap=try r.u32(),align=try r.u32(),direction=try r.u32()
        text=try r.string()
        let fallback=family==0 ? NSFont.systemFont(ofSize:size) : NSFont(name:family==1 ? "Georgia":"Menlo",size:size) ?? NSFont.systemFont(ofSize:size)
        var font=name.isEmpty ? fallback : NSFont(name:name,size:size) ?? fallback
        if flags&1 != 0 {font=NSFontManager.shared.convert(font,toHaveTrait:.boldFontMask)}
        if flags&2 != 0 {font=NSFontManager.shared.convert(font,toHaveTrait:.italicFontMask)}
        let paragraph=NSMutableParagraphStyle();paragraph.alignment=align==1 ? .center:align==2 ? .right:.left
        paragraph.baseWritingDirection=direction==1 ? .rightToLeft:.leftToRight
        paragraph.lineBreakMode=wrap==0 ? .byClipping:wrap==2 ? .byCharWrapping:.byWordWrapping
        var attributes:[NSAttributedString.Key:Any]=[.font:font,.foregroundColor:NSColor(cgColor:color) ?? NSColor.textColor,.paragraphStyle:paragraph]
        if flags&4 != 0 {attributes[.underlineStyle]=NSUnderlineStyle.single.rawValue};if flags&8 != 0 {attributes[.strikethroughStyle]=NSUnderlineStyle.single.rawValue}
        storage=NSTextStorage(string:text,attributes:attributes)
        let actualWidth=wrap==0 ? max(width,ceil(storage.size().width)):width
        offsetX=(width-actualWidth)*(align==1 ? 0.5:align==2 ? 1:0)
        container=NSTextContainer(size:NSSize(width:actualWidth,height:CGFloat.greatestFiniteMagnitude));container.lineFragmentPadding=0
        storage.addLayoutManager(manager);manager.addTextContainer(container);manager.ensureLayout(for:container)
        let glyphs=manager.glyphRange(for:container);used=manager.usedRect(for:container)
        var utf8Offsets=[Int](repeating:0,count:text.utf16.count+1),unit=0,byte=0
        for scalar in text.unicodeScalars {let units=scalar.value>0xffff ? 2:1;for j in 0..<units {utf8Offsets[unit+j]=byte};unit+=units;byte+=scalar.value<0x80 ? 1:scalar.value<0x800 ? 2:scalar.value<0x10000 ? 3:4;utf8Offsets[unit]=byte}
        manager.enumerateLineFragments(forGlyphRange:glyphs){[self] rect,_,_,range,_ in
            let chars=manager.characterRange(forGlyphRange:range,actualGlyphRange:nil)
            let start=utf8Offsets[chars.location]
            let baseline=rect.minY+manager.location(forGlyphAt:range.location).y
            lines.append(NativeLine(start:start,end:0,top:rect.minY,height:rect.height,baseline:baseline))
        }
        if manager.extraLineFragmentTextContainer != nil || lines.isEmpty {
            let rect=manager.extraLineFragmentRect
            let height=rect.height>0 ? rect.height:manager.defaultLineHeight(for:font)
            lines.append(NativeLine(start:text.utf8.count,end:0,top:rect.minY,height:height,baseline:rect.minY+font.ascender))
        }
        for i in lines.indices {lines[i].end=i+1<lines.count ? lines[i+1].start:text.utf8.count}
    }
    func metrics()->Data {
        var out=Writer();out.f(used.width);out.f(used.width);out.f(max(used.height,lines.last.map{$0.top+$0.height} ?? 0));out.f(lines[0].baseline);out.u32(UInt32(lines.count))
        for line in lines {out.u32(UInt32(line.start));out.u32(UInt32(line.end));out.f(line.top);out.f(line.height);out.f(line.baseline)}
        return out.data
    }
    func draw(_ context:CGContext,at position:CGPoint) {
        NSGraphicsContext.saveGraphicsState();defer {NSGraphicsContext.restoreGraphicsState()}
        NSGraphicsContext.current=NSGraphicsContext(cgContext:context,flipped:true)
        let range=manager.glyphRange(for:container),origin=CGPoint(x:position.x+offsetX,y:position.y);manager.drawBackground(forGlyphRange:range,at:origin);manager.drawGlyphs(forGlyphRange:range,at:origin)
    }
}
weak var nativeView:RusterizeView?
var nativeResponse:UnsafeMutablePointer<UInt8>?
func nativeService(_ data:UnsafePointer<UInt8>?,_ count:Int,_ length:UnsafeMutablePointer<Int>?)->UnsafePointer<UInt8>? {
    var out=Writer();out.u32(0)
    do {
        guard let data=data else {throw WireError.malformed}
        var r=Reader(data:Data(bytes:data,count:count));let op=try r.u32()
        if op==1 {out.data.append(try NativeText(&r).metrics())}
        else if op==2 {out.u32(1023)}
        else {
            guard let view=nativeView,let window=view.window else {throw WireError.native("No active native window")}
            switch op {
            case 3: out.data.append(Data((NSPasteboard.general.string(forType:.string) ?? "").utf8))
            case 4: let text=try r.string();NSPasteboard.general.clearContents();if !NSPasteboard.general.setString(text,forType:.string){throw WireError.native("Clipboard write failed")}
            case 5:
                let cursors:[NSCursor]=[.arrow,.iBeam,.pointingHand,.crosshair,.openHand,.resizeLeftRight,.resizeUpDown,NSCursor(image:NSImage(size:NSSize(width:1,height:1)),hotSpot:.zero)]
                let index=Int(try r.u32());guard index<cursors.count else {throw WireError.malformed};view.serviceCursor=cursors[index];window.invalidateCursorRects(for:view);view.serviceCursor.set()
            case 6:
                let state=try r.u32(),fullscreen=window.styleMask.contains(.fullScreen)
                if (state==3) != fullscreen {window.toggleFullScreen(nil)}
                if state==0 {window.deminiaturize(nil);if window.isZoomed {window.zoom(nil)}}
                else if state==1 {window.miniaturize(nil)}
                else if state==2 && !window.isZoomed {window.zoom(nil)}
            case 7: let w=try r.f(),h=try r.f();window.setContentSize(NSSize(width:w,height:h))
            case 8: let x=try r.f(),y=try r.f();let top=NSScreen.screens.first?.frame.maxY ?? 0;window.setFrameTopLeftPoint(NSPoint(x:x,y:top-y))
            case 9: window.level=try r.u32() != 0 ? .floating:.normal
            case 10:
                let save=try r.u32() != 0,directory=try r.u32() != 0,title=try r.string(),name=try r.string()
                let panel:NSSavePanel
                if save {panel=NSSavePanel()} else {let open=NSOpenPanel();open.canChooseDirectories=directory;open.canChooseFiles = !directory;open.allowsMultipleSelection=false;panel=open}
                panel.title=title;panel.nameFieldStringValue=name
                if panel.runModal() == .OK,let url=panel.url {out.u32(1);out.string(url.path)}else {out.u32(0)}
            case 11: let alert=NSAlert();alert.messageText=try r.string();alert.informativeText=try r.string();alert.runModal()
            case 12: guard let url=URL(string:try r.string()),NSWorkspace.shared.open(url) else {throw WireError.native("Cannot open URI")}
            default: throw WireError.native("Unsupported native operation")
            }
        }
    } catch {out=Writer();out.u32(1);out.data.append(Data(String(describing:error).utf8))}
    nativeResponse?.deallocate();let pointer=UnsafeMutablePointer<UInt8>.allocate(capacity:out.data.count);out.data.copyBytes(to:pointer,count:out.data.count);nativeResponse=pointer;length?.pointee=out.data.count;return UnsafePointer(pointer)
}

final class RusterizeView: NSView {
    private var handle = rusterize_create()
    private let origin = ProcessInfo.processInfo.systemUptime
    private var timer: Timer?
    private var images: [UInt64: CGImage] = [:]
    private var tracking: NSTrackingArea?
    private var windowRevision:UInt32 = 0
    var initialSize:NSSize {NSSize(width:CGFloat(Float(bitPattern:rusterize_window_option(handle,0))),height:CGFloat(Float(bitPattern:rusterize_window_option(handle,1))))}
    var minimumSize:NSSize {NSSize(width:CGFloat(Float(bitPattern:rusterize_window_option(handle,2))),height:CGFloat(Float(bitPattern:rusterize_window_option(handle,3))))}
    var resizable:Bool {rusterize_window_option(handle,7) != 0}
    var serviceCursor: NSCursor = .arrow
    override func resetCursorRects() {addCursorRect(bounds,cursor:serviceCursor)}
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override init(frame: NSRect) { super.init(frame:frame); wantsLayer = false; nativeView = self }
    required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
    deinit { timer?.invalidate(); rusterize_destroy(handle) }
    func event(_ kind: UInt32, _ x: CGFloat = 0, _ y: CGFloat = 0, _ dx: CGFloat = 0, _ dy: CGFloat = 0, _ detail: UInt32 = 0, _ flags: UInt32 = 0) {
        _ = rusterize_event(handle,kind,Float(x),Float(y),Float(dx),Float(dy),detail,flags); schedule()
    }
    private func schedule() {
        let status = rusterize_poll_native(handle)
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
        guard try r.u32() == 0x32305a52 else { throw WireError.malformed }
        let commands = try r.u32()
        var depth = 0, used = Set<UInt64>()
        context.saveGState()
        defer { for _ in 0..<depth { context.restoreGState() }; context.restoreGState() }
        context.clear(bounds); context.setLineCap(.butt);context.setLineJoin(.miter);context.setMiterLimit(10)
        for _ in 0..<commands {
            let opcode = try r.u32()
            switch opcode {
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
            case 8,10:
                let position = try r.point(), text = try NativeText(&r)
                text.draw(context,at:CGPoint(x:position.x,y:position.y-(opcode == 8 ? text.lines[0].baseline : 0)))
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
        context.restoreGState();context.saveGState()
        context.saveGState();defer {context.restoreGState()}
        let contextPointer=Unmanaged.passUnretained(context).toOpaque(),viewPointer=Unmanaged.passUnretained(self).toOpaque()
        let windowPointer=window.map{Unmanaged.passUnretained($0).toOpaque()}
        if rusterize_native_draw(handle,contextPointer,viewPointer,windowPointer) & 0x80000000 != 0 {throw WireError.native("Native draw failed")}

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
rusterize_set_native_service(nativeService)
let application = NSApplication.shared
if CommandLine.arguments.count == 3 && CommandLine.arguments[1] == "--snapshot" {
    do {try RusterizeView(frame:NSRect(x:0,y:0,width:960,height:680)).snapshot(to:CommandLine.arguments[2]);exit(0)}
    catch {FileHandle.standardError.write(Data("\(error)\n".utf8));exit(1)}
}
let delegate = Delegate()
application.delegate = delegate
application.setActivationPolicy(.regular)
application.run()
