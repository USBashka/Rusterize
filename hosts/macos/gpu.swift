import AppKit
import OpenGL.GL3

// A sibling of native child views: NSOpenGLView itself cannot contain subviews.
final class GPUCanvasView: NSOpenGLView {
    weak var owner: RusterizeView?
    private var compositor: MacGPU?

    override init(frame: NSRect) {
        let attributes: [NSOpenGLPixelFormatAttribute] = [
            UInt32(NSOpenGLPFAOpenGLProfile), UInt32(NSOpenGLProfileVersion4_1Core),
            UInt32(NSOpenGLPFADoubleBuffer), UInt32(NSOpenGLPFAColorSize), 32,
            UInt32(NSOpenGLPFAAlphaSize), 8, 0
        ]
        guard let format = NSOpenGLPixelFormat(attributes: attributes) else {
            fatalError("Rusterize requires an OpenGL 4.1 pixel format")
        }
        super.init(frame: frame, pixelFormat: format)!
        wantsBestResolutionOpenGLSurface = true
        autoresizingMask = [.width, .height]
    }
    required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func draw(_ dirtyRect: NSRect) { owner?.drawGPU(self) }
    func current() throws -> MacGPU {
        guard let context = openGLContext else { throw WireError.native("Missing OpenGL view context") }
        context.makeCurrentContext()
        if compositor == nil { compositor = try MacGPU() }
        return compositor!
    }
    func resetResources() throws { try current().resetResources() }
    deinit { openGLContext?.makeCurrentContext(); compositor = nil }
}

// CoreGraphics supplies text, paths and exact clip coverage. Shader pixels stay
// in GPU textures; only ordinary Canvas segments and coverage masks are uploaded.
final class MacGPU {
    private var vao: GLuint = 0
    private var uniforms: GLuint = 0
    private var framebuffers = [GLuint](repeating: 0, count: 2)
    private var textures = [GLuint](repeating: 0, count: 4)
    private var programs: [UInt64: GLuint] = [:]
    private var images: [UInt64: GLuint] = [:]
    private var imageData: [UInt64: (width: Int, height: Int, rgba: Data)] = [:]
    private var composition: GLuint = 0
    private var width = 0, height = 0
    private var shaderWidth = 0, shaderHeight = 0
    private var maxTexture = 0
    private(set) var context: CGContext?
    private(set) var shaderDraws = 0
    private(set) var snapshotReadbacks = 0
    private(set) var resourceUploads = 0
    private var destinationFramebuffer: GLuint = 0

    init() throws {
        var limit: GLint = 0
        glGetIntegerv(GLenum(GL_MAX_TEXTURE_SIZE), &limit); maxTexture = Int(limit)
        glGenVertexArrays(1, &vao); glGenBuffers(1, &uniforms)
        glGenFramebuffers(2, &framebuffers); glGenTextures(4, &textures)
        composition = try Self.program(vertex: """
        #version 330 core
        void main() {
            vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
            gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
        }
        """, fragment: """
        #version 330 core
        uniform sampler2D pixels;
        uniform sampler2D coverage;
        uniform vec2 viewportSize;
        uniform vec3 inverseX;
        uniform vec3 inverseY;
        uniform vec4 destination;
        uniform int shaderLayer;
        out vec4 color;
        void main() {
            vec2 topUV = vec2(gl_FragCoord.x, viewportSize.y-gl_FragCoord.y)/viewportSize;
            if (shaderLayer == 0) {
                color = texture(pixels, topUV);
            } else {
                vec3 p = vec3(gl_FragCoord.xy, 1.0);
                vec2 local = vec2(dot(inverseX,p), dot(inverseY,p));
                vec2 uv = (local-destination.xy)/destination.zw;
                vec4 c = texture(pixels, vec2(uv.x,1.0-uv.y));
                float a = c.a * texture(coverage,topUV).a;
                color = vec4(c.rgb*a,a);
            }
        }
        """)
        glUseProgram(composition)
        glUniform1i(glGetUniformLocation(composition, "pixels"), 0)
        glUniform1i(glGetUniformLocation(composition, "coverage"), 1)
        try check()
    }

    deinit {
        resetResources()
        glDeleteProgram(composition); glDeleteVertexArrays(1, &vao)
        glDeleteBuffers(1, &uniforms); glDeleteFramebuffers(2, &framebuffers)
        glDeleteTextures(4, &textures)
    }
    func resetResources() {
        for program in programs.values { glDeleteProgram(program) }
        for var image in images.values { glDeleteTextures(1, &image) }
        programs.removeAll(); images.removeAll(); imageData.removeAll()
    }
    func releaseSource(_ id: UInt64) {
        if let program = programs.removeValue(forKey: id) { glDeleteProgram(program) }
    }
    func releaseImage(_ id: UInt64) {
        if var texture = images.removeValue(forKey: id) { glDeleteTextures(1, &texture) }
        imageData.removeValue(forKey: id)
    }
    func uploadSource(_ id: UInt64, _ sources: [String]) throws {
        guard sources.count == 4 else { throw WireError.malformed }
        let program = try Self.program(vertex: sources[0], fragment: sources[1])
        releaseSource(id); programs[id] = program
        resourceUploads += 1
        glUseProgram(program)
        if !sources[2].isEmpty {
            let index = glGetUniformBlockIndex(program, sources[2])
            if index != GLuint(GL_INVALID_INDEX) { glUniformBlockBinding(program, index, 0) }
        }
        if !sources[3].isEmpty { glUniform1i(glGetUniformLocation(program, sources[3]), 0) }
        try check()
    }
    func uploadImage(_ id: UInt64, width: Int, height: Int, rgba: Data) throws {
        guard width > 0, height > 0, rgba.count == width * height * 4 else { throw WireError.malformed }
        releaseImage(id)
        imageData[id] = (width, height, rgba)
        if id != 0 { resourceUploads += 1 }
    }
    private func imageTexture(_ id: UInt64) throws -> GLuint {
        if let texture = images[id] { return texture }
        guard let data = imageData[id], data.width <= maxTexture, data.height <= maxTexture else {
            throw WireError.native("Unknown or oversized shader image")
        }
        var texture: GLuint = 0; glGenTextures(1, &texture); images[id] = texture
        data.rgba.withUnsafeBytes { upload(texture, data.width, data.height, $0.baseAddress) }
        try check(); return texture
    }
    private func upload(_ texture: GLuint, _ width: Int, _ height: Int, _ pixels: UnsafeRawPointer?) {
        glActiveTexture(GLenum(GL_TEXTURE0)); glBindTexture(GLenum(GL_TEXTURE_2D), texture)
        glTexParameteri(GLenum(GL_TEXTURE_2D), GLenum(GL_TEXTURE_MIN_FILTER), GL_LINEAR)
        glTexParameteri(GLenum(GL_TEXTURE_2D), GLenum(GL_TEXTURE_MAG_FILTER), GL_LINEAR)
        glTexParameteri(GLenum(GL_TEXTURE_2D), GLenum(GL_TEXTURE_WRAP_S), GL_CLAMP_TO_EDGE)
        glTexParameteri(GLenum(GL_TEXTURE_2D), GLenum(GL_TEXTURE_WRAP_T), GL_CLAMP_TO_EDGE)
        glPixelStorei(GLenum(GL_UNPACK_ALIGNMENT), 1)
        glPixelStorei(GLenum(GL_UNPACK_ROW_LENGTH), 0)
        glTexImage2D(GLenum(GL_TEXTURE_2D), 0, GL_RGBA8, GLsizei(width), GLsizei(height), 0,
                     GLenum(GL_RGBA), GLenum(GL_UNSIGNED_BYTE), pixels)
    }
    private func attach(_ framebuffer: GLuint, _ texture: GLuint) throws {
        glBindFramebuffer(GLenum(GL_FRAMEBUFFER), framebuffer)
        glFramebufferTexture2D(GLenum(GL_FRAMEBUFFER), GLenum(GL_COLOR_ATTACHMENT0), GLenum(GL_TEXTURE_2D), texture, 0)
        guard glCheckFramebufferStatus(GLenum(GL_FRAMEBUFFER)) == GLenum(GL_FRAMEBUFFER_COMPLETE) else {
            throw WireError.native("Incomplete GPU composition framebuffer")
        }
    }
    func begin(width: Int, height: Int, scale: CGFloat) throws -> CGContext {
        guard width > 0, height > 0, width <= maxTexture, height <= maxTexture,
              scale.isFinite, scale > 0 else { throw WireError.malformed }
        shaderDraws = 0; resourceUploads = 0
        var target: GLint = 0; glGetIntegerv(GLenum(GL_DRAW_FRAMEBUFFER_BINDING), &target)
        destinationFramebuffer = GLuint(target)
        if self.width != width || self.height != height || context == nil {
            self.width = width; self.height = height
            guard let context = CGContext(data: nil, width: width, height: height,
                    bitsPerComponent: 8, bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue) else {
                throw WireError.native("Cannot allocate native Canvas segment")
            }
            self.context = context
            upload(textures[0], width, height, nil)
        }
        let context = self.context!
        context.resetClip(); context.concatenate(context.ctm.inverted())
        context.translateBy(x: 0, y: CGFloat(height)); context.scaleBy(x: scale, y: -scale)
        clearCanvas()
        try attach(framebuffers[0], textures[0])
        glViewport(0, 0, GLsizei(width), GLsizei(height))
        glDisable(GLenum(GL_SCISSOR_TEST)); glDisable(GLenum(GL_DEPTH_TEST)); glDisable(GLenum(GL_STENCIL_TEST))
        glDisable(GLenum(GL_CULL_FACE)); glColorMask(GLboolean(GL_TRUE), GLboolean(GL_TRUE), GLboolean(GL_TRUE), GLboolean(GL_TRUE))
        glDisable(GLenum(GL_DITHER)); glDisable(GLenum(GL_FRAMEBUFFER_SRGB))
        glClearColor(0, 0, 0, 0); glClear(GLbitfield(GL_COLOR_BUFFER_BIT))
        try check(); return context
    }
    private func clearCanvas() {
        if let pixels = context?.data { memset(pixels, 0, width * height * 4) }
    }
    func flushCanvas() throws {
        guard let context = context else { throw WireError.malformed }
        context.flush(); upload(textures[2], width, height, context.data)
        try compose(texture: textures[2], shader: false)
        clearCanvas()
    }
    private func compose(texture: GLuint, shader: Bool, inverse: CGAffineTransform = .identity, destination: CGRect = .zero) throws {
        try attach(framebuffers[0], textures[0])
        glViewport(0, 0, GLsizei(width), GLsizei(height)); glUseProgram(composition)
        glDisable(GLenum(GL_SCISSOR_TEST)); glDisable(GLenum(GL_DEPTH_TEST)); glDisable(GLenum(GL_STENCIL_TEST))
        glDisable(GLenum(GL_CULL_FACE)); glDisable(GLenum(GL_FRAMEBUFFER_SRGB))
        glColorMask(GLboolean(GL_TRUE), GLboolean(GL_TRUE), GLboolean(GL_TRUE), GLboolean(GL_TRUE))
        glBindVertexArray(vao); glEnable(GLenum(GL_BLEND)); glBlendEquation(GLenum(GL_FUNC_ADD))
        glBlendFunc(GLenum(GL_ONE), GLenum(GL_ONE_MINUS_SRC_ALPHA))
        glActiveTexture(GLenum(GL_TEXTURE0)); glBindTexture(GLenum(GL_TEXTURE_2D), texture)
        glActiveTexture(GLenum(GL_TEXTURE1)); glBindTexture(GLenum(GL_TEXTURE_2D), textures[3])
        glUniform2f(glGetUniformLocation(composition, "viewportSize"), Float(width), Float(height))
        glUniform1i(glGetUniformLocation(composition, "shaderLayer"), shader ? 1 : 0)
        glUniform3f(glGetUniformLocation(composition, "inverseX"), Float(inverse.a), Float(inverse.c), Float(inverse.tx))
        glUniform3f(glGetUniformLocation(composition, "inverseY"), Float(inverse.b), Float(inverse.d), Float(inverse.ty))
        glUniform4f(glGetUniformLocation(composition, "destination"), Float(destination.minX), Float(destination.minY), Float(destination.width), Float(destination.height))
        glDrawArrays(GLenum(GL_TRIANGLES), 0, 3); try check()
    }
    func shader(_ id: UInt64, values: [Float], image: UInt64, destination: CGRect) throws {
        guard let program = programs[id], let context = context, values.count == 20,
              values.allSatisfy({ $0.isFinite }), values[0] >= 1, values[1] >= 1,
              values[0] <= 4096, values[1] <= 4096, values[0] * values[1] <= 4_194_304,
              values[0].rounded() == values[0], values[1].rounded() == values[1], values[3] > 0,
              destination.width > 0, destination.height > 0 else { throw WireError.malformed }
        let sw = Int(values[0]), sh = Int(values[1])
        guard sw <= maxTexture, sh <= maxTexture else { throw WireError.malformed }
        let determinant = context.ctm.a * context.ctm.d - context.ctm.b * context.ctm.c
        if abs(determinant) < 1e-12 { return }
        try flushCanvas()
        context.saveGState(); context.setBlendMode(.copy)
        context.setFillColor(CGColor(gray: 1, alpha: 1)); context.fill(destination); context.restoreGState()
        context.flush(); upload(textures[3], width, height, context.data); clearCanvas()
        if shaderWidth != sw || shaderHeight != sh {
            upload(textures[1], sw, sh, nil); shaderWidth = sw; shaderHeight = sh
        }
        try attach(framebuffers[1], textures[1])
        glViewport(0, 0, GLsizei(sw), GLsizei(sh)); glDisable(GLenum(GL_BLEND))
        glUseProgram(program); glBindVertexArray(vao)
        glBindBuffer(GLenum(GL_UNIFORM_BUFFER), uniforms)
        values.withUnsafeBufferPointer { glBufferData(GLenum(GL_UNIFORM_BUFFER), $0.count * MemoryLayout<Float>.stride, $0.baseAddress, GLenum(GL_DYNAMIC_DRAW)) }
        glBindBufferBase(GLenum(GL_UNIFORM_BUFFER), 0, uniforms)
        if image == 0 && images[0] == nil { try uploadImage(0, width: 1, height: 1, rgba: Data([255, 255, 255, 255])) }
        let input = try imageTexture(image)
        glActiveTexture(GLenum(GL_TEXTURE0)); glBindTexture(GLenum(GL_TEXTURE_2D), input)
        glClearColor(0, 0, 0, 0); glClear(GLbitfield(GL_COLOR_BUFFER_BIT))
        glDrawArrays(GLenum(GL_TRIANGLES), 0, 3); try check(); shaderDraws += 1
        try compose(texture: textures[1], shader: true, inverse: context.ctm.inverted(), destination: destination)
    }
    func present() throws {
        try flushCanvas()
        glBindFramebuffer(GLenum(GL_READ_FRAMEBUFFER), framebuffers[0])
        glBindFramebuffer(GLenum(GL_DRAW_FRAMEBUFFER), destinationFramebuffer)
        glDisable(GLenum(GL_BLEND))
        glBlitFramebuffer(0, 0, GLint(width), GLint(height), 0, 0, GLint(width), GLint(height),
                          GLbitfield(GL_COLOR_BUFFER_BIT), GLenum(GL_NEAREST))
        try check()
    }
    // Explicit export only: the display path never calls glReadPixels.
    func snapshot() throws -> CGImage {
        try flushCanvas()
        glBindFramebuffer(GLenum(GL_READ_FRAMEBUFFER), framebuffers[0])
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        glPixelStorei(GLenum(GL_PACK_ALIGNMENT), 1)
        pixels.withUnsafeMutableBufferPointer { glReadPixels(0, 0, GLsizei(width), GLsizei(height), GLenum(GL_RGBA), GLenum(GL_UNSIGNED_BYTE), $0.baseAddress) }
        snapshotReadbacks += 1; try check()
        let stride = width * 4
        for y in 0 ..< height / 2 {
            let opposite = height - 1 - y
            for x in 0 ..< stride { pixels.swapAt(y * stride + x, opposite * stride + x) }
        }
        guard let provider = CGDataProvider(data: Data(pixels) as CFData), let image = CGImage(width: width, height: height,
                bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: stride, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGBitmapInfo(rawValue: CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue),
                provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent) else { throw WireError.malformed }
        return image
    }
    private static func program(vertex: String, fragment: String) throws -> GLuint {
        let program = glCreateProgram()
        var stages: [GLuint] = []
        do {
            for (kind, source) in [(GLenum(GL_VERTEX_SHADER), vertex), (GLenum(GL_FRAGMENT_SHADER), fragment)] {
                let stage = glCreateShader(kind); stages.append(stage)
                source.withCString { pointer in
                    var pointer: UnsafePointer<GLchar>? = pointer
                    glShaderSource(stage, 1, &pointer, nil)
                }
                glCompileShader(stage)
                var ok: GLint = 0; glGetShaderiv(stage, GLenum(GL_COMPILE_STATUS), &ok)
                if ok == 0 {
                    var length: GLint = 0; glGetShaderiv(stage, GLenum(GL_INFO_LOG_LENGTH), &length)
                    var message = [GLchar](repeating: 0, count: max(1, Int(length)))
                    glGetShaderInfoLog(stage, GLsizei(message.count), nil, &message)
                    throw WireError.native("OpenGL compilation: " + String(cString: message))
                }
                glAttachShader(program, stage)
            }
            glLinkProgram(program)
            var ok: GLint = 0; glGetProgramiv(program, GLenum(GL_LINK_STATUS), &ok)
            if ok == 0 {
                var length: GLint = 0; glGetProgramiv(program, GLenum(GL_INFO_LOG_LENGTH), &length)
                var message = [GLchar](repeating: 0, count: max(1, Int(length)))
                glGetProgramInfoLog(program, GLsizei(message.count), nil, &message)
                throw WireError.native("OpenGL link: " + String(cString: message))
            }
        } catch {
            for stage in stages { glDeleteShader(stage) }; glDeleteProgram(program); throw error
        }
        for stage in stages { glDeleteShader(stage) }
        return program
    }
    private func check() throws {
        let error = glGetError()
        if error != GLenum(GL_NO_ERROR) { throw WireError.native("GPU composition OpenGL error: \(error)") }
    }
}
