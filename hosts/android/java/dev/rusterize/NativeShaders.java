package dev.rusterize;

import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.hardware.HardwareBuffer;
import android.opengl.*;
import android.os.Build;
import android.util.Log;
import android.util.LongSparseArray;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.Map;

/** ES3 to an immutable hardware bitmap; the system Canvas retains its own GPU reference. */
final class NativeShaders implements AutoCloseable {
    static boolean enabled;
    private static native long attachHardwareBuffer(HardwareBuffer buffer);
    private static native void releaseHardwareBuffer(long image);
    private EGLDisplay display = EGL14.EGL_NO_DISPLAY;
    private EGLContext context = EGL14.EGL_NO_CONTEXT;
    private EGLSurface surface = EGL14.EGL_NO_SURFACE;
    private int framebuffer, uniforms, vertexArray, white;
    private boolean sharingUnavailable;
    private final LongSparseArray<Source> sources = new LongSparseArray<>();
    private final LongSparseArray<Input> inputs = new LongSparseArray<>();
    private final LinkedHashMap<Key,Bitmap> outputs = new LinkedHashMap<>(16,0.75f,true);
    private long outputBytes;
    long directFrames, readbackFrames;
    private static final class Source {
        final String vertex, fragment, block, sampler;
        int program;
        Source(String v,String f,String b,String s) {vertex=v;fragment=f;block=b;sampler=s;}
    }
    private static final class Input {
        final int width,height;
        byte[] rgba;
        int texture;
        Input(int w,int h,byte[] pixels) {width=w;height=h;rgba=pixels;}
    }
    private static final class Key {
        final long source,image;
        final float[] values;
        final boolean hardware;
        Key(long s,long i,float[] v,boolean h) {source=s;image=i;values=v.clone();hardware=h;}
        @Override public boolean equals(Object other) {
            if(!(other instanceof Key))return false;
            Key k=(Key)other;
            return source==k.source && image==k.image && hardware==k.hardware && Arrays.equals(values,k.values);
        }
        @Override public int hashCode() {return Arrays.hashCode(values) ^ (int)source ^ (int)image ^ (hardware?1:0);}
    }
    private final class Current implements AutoCloseable {
        final EGLDisplay oldDisplay=EGL14.eglGetCurrentDisplay();
        final EGLContext oldContext=EGL14.eglGetCurrentContext();
        final EGLSurface oldDraw=EGL14.eglGetCurrentSurface(EGL14.EGL_DRAW);
        final EGLSurface oldRead=EGL14.eglGetCurrentSurface(EGL14.EGL_READ);
        Current() {
            try {
                ensureContext();
                require(EGL14.eglMakeCurrent(display,surface,surface,context),"activate shader context");
            } catch(RuntimeException error) {close();throw error;}
        }
        @Override public void close() {
            EGL14.eglMakeCurrent(display,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_CONTEXT);
            if(!oldDisplay.equals(EGL14.EGL_NO_DISPLAY))EGL14.eglMakeCurrent(oldDisplay,oldDraw,oldRead,oldContext);
        }
    }
    void uploadImage(long id,int width,int height,byte[] rgba) {
        releaseImage(id);
        inputs.put(id,new Input(width,height,rgba));
    }
    void releaseImage(long id) {
        Input old=inputs.get(id);
        if(old!=null && old.texture!=0)try(Current current=new Current()){GLES30.glDeleteTextures(1,new int[]{old.texture},0);}
        inputs.remove(id);
        clearOutputs();
    }
    void uploadSource(long id,String vertex,String fragment,String block,String sampler) {
        releaseSource(id);
        sources.put(id,new Source(vertex,fragment,block,sampler));
    }
    void releaseSource(long id) {
        Source old=sources.get(id);
        if(old!=null && old.program!=0)try(Current current=new Current()){GLES30.glDeleteProgram(old.program);}
        sources.remove(id);
        clearOutputs();
    }
    private void clearOutputs() {outputs.clear();outputBytes=0;}
    Bitmap render(Canvas canvas,long sourceId,float[] values,long imageId) {
        int width=(int)values[0],height=(int)values[1];
        if(width<=0 || height<=0 || width>4096 || height>4096 || (long)width*height>4*1024*1024)throw new IllegalStateException("Invalid shader dimensions");
        for(float value:values)if(Float.isNaN(value)||Float.isInfinite(value))throw new IllegalStateException("Invalid shader parameters");
        if(values[3]<=0)throw new IllegalStateException("Invalid shader scale");
        Source source=sources.get(sourceId);
        if(source==null)throw new IllegalStateException("Unknown shader resource");
        if(imageId!=0 && inputs.get(imageId)==null)throw new IllegalStateException("Unknown shader image");
        boolean hardware=Build.VERSION.SDK_INT>=29 && canvas.isHardwareAccelerated() && !sharingUnavailable;
        Key key=new Key(sourceId,imageId,values,hardware);
        Bitmap cached=outputs.get(key);
        if(cached!=null)return cached;
        Bitmap result;
        try(Current current=new Current()) {
            if(source.program==0)source.program=program(source);
            int[] limit=new int[1];GLES30.glGetIntegerv(GLES30.GL_MAX_TEXTURE_SIZE,limit,0);
            if(width>limit[0]||height>limit[0])throw new IllegalStateException("Shader exceeds device texture limit");
            if(hardware) {
                try { result=hardware(source,values,imageId,width,height); }
                catch(RuntimeException error) {
                    sharingUnavailable=true;
                    while(GLES30.glGetError()!=GLES30.GL_NO_ERROR) { }
                    Log.w("RusterizeShaders","HardwareBuffer sharing unavailable; using readback",error);
                    result=readback(source,values,imageId,width,height);
                    key=new Key(sourceId,imageId,values,false);
                }
            } else result=readback(source,values,imageId,width,height);
        }
        outputs.put(key,result);outputBytes+=(long)width*height*4;
        while(outputs.size()>16 || outputBytes>64L*1024*1024) {
            Map.Entry<Key,Bitmap> first=outputs.entrySet().iterator().next();
            outputBytes-=(long)first.getValue().getWidth()*first.getValue().getHeight()*4;
            outputs.remove(first.getKey());
        }
        return result;
    }
    private Bitmap hardware(Source source,float[] values,long imageId,int width,int height) {
        HardwareBuffer buffer=HardwareBuffer.create(width,height,HardwareBuffer.RGBA_8888,1,
                HardwareBuffer.USAGE_GPU_COLOR_OUTPUT|HardwareBuffer.USAGE_GPU_SAMPLED_IMAGE);
        int texture=texture();long image=0;
        try {
            image=attachHardwareBuffer(buffer);
            if(image==0)throw new IllegalStateException("EGLImage creation unavailable");
            check();
            draw(source,values,imageId,texture,true);
            // Bitmap/RenderThread imports a different context. Complete writes before publishing it.
            GLES30.glFinish();check();
            Bitmap bitmap=Bitmap.wrapHardwareBuffer(buffer,null);
            if(bitmap==null)throw new IllegalStateException("Cannot wrap shader HardwareBuffer");
            directFrames++;
            if(directFrames==1)Log.i("RusterizeShaders","DIRECT_GPU HardwareBuffer/EGLImage; readback=0");
            return bitmap;
        } finally {
            GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,framebuffer);
            GLES30.glFramebufferTexture2D(GLES30.GL_FRAMEBUFFER,GLES30.GL_COLOR_ATTACHMENT0,GLES30.GL_TEXTURE_2D,0,0);
            GLES30.glDeleteTextures(1,new int[]{texture},0);
            if(image!=0)releaseHardwareBuffer(image);
            // wrapHardwareBuffer owns a reference; never mutate or recycle the returned bitmap.
            buffer.close();
        }
    }
    private Bitmap readback(Source source,float[] values,long imageId,int width,int height) {
        int texture=texture();
        try {
            GLES30.glTexImage2D(GLES30.GL_TEXTURE_2D,0,GLES30.GL_RGBA8,width,height,0,GLES30.GL_RGBA,GLES30.GL_UNSIGNED_BYTE,null);
            draw(source,values,imageId,texture,false);
            ByteBuffer bytes=ByteBuffer.allocateDirect(width*height*4);
            GLES30.glPixelStorei(GLES30.GL_PACK_ALIGNMENT,1);
            GLES30.glReadPixels(0,0,width,height,GLES30.GL_RGBA,GLES30.GL_UNSIGNED_BYTE,bytes);
            check();
            int[] pixels=new int[width*height];
            for(int i=0;i<pixels.length;i++)pixels[i]=Color.argb(bytes.get(i*4+3)&255,bytes.get(i*4)&255,bytes.get(i*4+1)&255,bytes.get(i*4+2)&255);
            readbackFrames++;
            return Bitmap.createBitmap(pixels,width,height,Bitmap.Config.ARGB_8888);
        } finally {GLES30.glDeleteTextures(1,new int[]{texture},0);}
    }
    private void draw(Source source,float[] values,long imageId,int output,boolean premultiply) {
        GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,framebuffer);
        GLES30.glFramebufferTexture2D(GLES30.GL_FRAMEBUFFER,GLES30.GL_COLOR_ATTACHMENT0,GLES30.GL_TEXTURE_2D,output,0);
        if(GLES30.glCheckFramebufferStatus(GLES30.GL_FRAMEBUFFER)!=GLES30.GL_FRAMEBUFFER_COMPLETE)throw new IllegalStateException("Incomplete shader framebuffer");
        GLES30.glUseProgram(source.program);GLES30.glBindVertexArray(vertexArray);
        ByteBuffer bytes=ByteBuffer.allocateDirect(80).order(ByteOrder.nativeOrder());
        for(float value:values)bytes.putFloat(value);bytes.flip();
        GLES30.glBindBuffer(GLES30.GL_UNIFORM_BUFFER,uniforms);
        GLES30.glBufferData(GLES30.GL_UNIFORM_BUFFER,80,bytes,GLES30.GL_DYNAMIC_DRAW);
        GLES30.glBindBufferBase(GLES30.GL_UNIFORM_BUFFER,0,uniforms);
        GLES30.glActiveTexture(GLES30.GL_TEXTURE0);
        GLES30.glBindTexture(GLES30.GL_TEXTURE_2D,inputTexture(imageId));
        GLES30.glViewport(0,0,(int)values[0],(int)values[1]);
        GLES30.glDisable(GLES30.GL_SCISSOR_TEST);GLES30.glDisable(GLES30.GL_DEPTH_TEST);GLES30.glDisable(GLES30.GL_CULL_FACE);GLES30.glDisable(GLES30.GL_DITHER);
        if(premultiply) {
            GLES30.glEnable(GLES30.GL_BLEND);
            GLES30.glBlendFuncSeparate(GLES30.GL_SRC_ALPHA,GLES30.GL_ZERO,GLES30.GL_ONE,GLES30.GL_ZERO);
        } else GLES30.glDisable(GLES30.GL_BLEND);
        GLES30.glClearColor(0,0,0,0);GLES30.glClear(GLES30.GL_COLOR_BUFFER_BIT);
        GLES30.glDrawArrays(GLES30.GL_TRIANGLES,0,3);check();
    }
    private int inputTexture(long id) {
        if(id==0)return white;
        Input input=inputs.get(id);
        if(input.texture==0) {
            input.texture=texture();
            ByteBuffer pixels=ByteBuffer.allocateDirect(input.rgba.length);pixels.put(input.rgba).flip();
            GLES30.glPixelStorei(GLES30.GL_UNPACK_ALIGNMENT,1);
            GLES30.glTexImage2D(GLES30.GL_TEXTURE_2D,0,GLES30.GL_RGBA8,input.width,input.height,0,GLES30.GL_RGBA,GLES30.GL_UNSIGNED_BYTE,pixels);
            check();input.rgba=null;
        }
        return input.texture;
    }
    private static int texture() {
        int[] id=new int[1];GLES30.glGenTextures(1,id,0);GLES30.glBindTexture(GLES30.GL_TEXTURE_2D,id[0]);
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D,GLES30.GL_TEXTURE_MIN_FILTER,GLES30.GL_LINEAR);
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D,GLES30.GL_TEXTURE_MAG_FILTER,GLES30.GL_LINEAR);
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D,GLES30.GL_TEXTURE_WRAP_S,GLES30.GL_CLAMP_TO_EDGE);
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D,GLES30.GL_TEXTURE_WRAP_T,GLES30.GL_CLAMP_TO_EDGE);
        return id[0];
    }
    private int program(Source source) {
        // AHardwareBuffer rows start at the top; GL framebuffer coordinates start at the bottom.
        int newline=source.vertex.indexOf('\n');
        if(newline<0)throw new IllegalStateException("Missing GLSL version");
        String vertex=source.vertex.substring(0,newline+1)+"#define main rusterize_original_main\n"+source.vertex.substring(newline+1)
                +"\n#undef main\nvoid main(){ rusterize_original_main(); gl_Position.y=-gl_Position.y; }\n";
        int program=GLES30.glCreateProgram(),v=0,f=0;
        try {
            v=stage(GLES30.GL_VERTEX_SHADER,vertex);f=stage(GLES30.GL_FRAGMENT_SHADER,source.fragment);
            GLES30.glAttachShader(program,v);GLES30.glAttachShader(program,f);GLES30.glLinkProgram(program);
            int[] ok=new int[1];GLES30.glGetProgramiv(program,GLES30.GL_LINK_STATUS,ok,0);
            if(ok[0]==0)throw new IllegalStateException(GLES30.glGetProgramInfoLog(program));
            GLES30.glUseProgram(program);
            if(!source.block.isEmpty()) {int block=GLES30.glGetUniformBlockIndex(program,source.block);if(block!=-1)GLES30.glUniformBlockBinding(program,block,0);}
            if(!source.sampler.isEmpty())GLES30.glUniform1i(GLES30.glGetUniformLocation(program,source.sampler),0);
            check();return program;
        } catch(RuntimeException error){GLES30.glDeleteProgram(program);throw error;}
        finally {if(v!=0)GLES30.glDeleteShader(v);if(f!=0)GLES30.glDeleteShader(f);}
    }
    private static int stage(int kind,String text) {
        int shader=GLES30.glCreateShader(kind);GLES30.glShaderSource(shader,text);GLES30.glCompileShader(shader);
        int[] ok=new int[1];GLES30.glGetShaderiv(shader,GLES30.GL_COMPILE_STATUS,ok,0);
        if(ok[0]==0){String error=GLES30.glGetShaderInfoLog(shader);GLES30.glDeleteShader(shader);throw new IllegalStateException(error);}
        return shader;
    }
    private void ensureContext() {
        if(!context.equals(EGL14.EGL_NO_CONTEXT))return;
        display=EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY);
        require(EGL14.eglInitialize(display,new int[2],0,new int[2],1),"initialize shader EGL");
        EGLConfig[] configs=new EGLConfig[1];int[] count=new int[1];
        require(EGL14.eglChooseConfig(display,new int[]{EGL14.EGL_SURFACE_TYPE,EGL14.EGL_PBUFFER_BIT,EGL14.EGL_RENDERABLE_TYPE,0x40,
                EGL14.EGL_RED_SIZE,8,EGL14.EGL_GREEN_SIZE,8,EGL14.EGL_BLUE_SIZE,8,EGL14.EGL_ALPHA_SIZE,8,EGL14.EGL_NONE},0,configs,0,1,count,0)&&count[0]>0,"choose ES3 configuration");
        context=EGL14.eglCreateContext(display,configs[0],EGL14.EGL_NO_CONTEXT,new int[]{EGL14.EGL_CONTEXT_CLIENT_VERSION,3,EGL14.EGL_NONE},0);
        require(!context.equals(EGL14.EGL_NO_CONTEXT),"create shader context");
        surface=EGL14.eglCreatePbufferSurface(display,configs[0],new int[]{EGL14.EGL_WIDTH,1,EGL14.EGL_HEIGHT,1,EGL14.EGL_NONE},0);
        require(!surface.equals(EGL14.EGL_NO_SURFACE),"create shader surface");
        require(EGL14.eglMakeCurrent(display,surface,surface,context),"initialize shader objects");
        int[] id=new int[1];GLES30.glGenFramebuffers(1,id,0);framebuffer=id[0];
        GLES30.glGenBuffers(1,id,0);uniforms=id[0];GLES30.glGenVertexArrays(1,id,0);vertexArray=id[0];
        white=texture();ByteBuffer pixel=ByteBuffer.allocateDirect(4);pixel.putInt(-1).flip();
        GLES30.glTexImage2D(GLES30.GL_TEXTURE_2D,0,GLES30.GL_RGBA8,1,1,0,GLES30.GL_RGBA,GLES30.GL_UNSIGNED_BYTE,pixel);check();
    }
    private static void require(boolean value,String action) {if(!value)throw new IllegalStateException("Cannot "+action+": EGL "+Integer.toHexString(EGL14.eglGetError()));}
    private static void check() {int error=GLES30.glGetError();if(error!=GLES30.GL_NO_ERROR)throw new IllegalStateException("Shader GL error "+Integer.toHexString(error));}
    @Override public void close() {
        clearOutputs();inputs.clear();sources.clear();
        if(!context.equals(EGL14.EGL_NO_CONTEXT)) {
            if(EGL14.eglGetCurrentContext().equals(context))EGL14.eglMakeCurrent(display,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_CONTEXT);
            EGL14.eglDestroyContext(display,context);
        }
        if(!surface.equals(EGL14.EGL_NO_SURFACE))EGL14.eglDestroySurface(display,surface);
        context=EGL14.EGL_NO_CONTEXT;surface=EGL14.EGL_NO_SURFACE;display=EGL14.EGL_NO_DISPLAY;
        sharingUnavailable=false;
    }
}
