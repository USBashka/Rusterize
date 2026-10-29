package dev.rusterize;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.Intent;
import android.graphics.*;
import android.os.Bundle;
import android.view.View;
import android.view.ViewGroup;
import java.lang.reflect.Field;

/** Device-only regression probes. Pixel readback here is verification, never the compositor path. */
public final class GpuCompositionInstrumentation extends Instrumentation {
    private Throwable failure;
    private String result;
    private static void require(boolean ok,String message) {if(!ok)throw new AssertionError(message);}
    private static void pixel(Bitmap bitmap,int x,int y,int r,int g,int b,int a) {
        int p=bitmap.getPixel(x,y);
        require(Math.abs(Color.red(p)-r)<=3 && Math.abs(Color.green(p)-g)<=3 && Math.abs(Color.blue(p)-b)<=3 && Math.abs(Color.alpha(p)-a)<=3,
                "Pixel "+x+","+y+": "+Integer.toHexString(p)+" != "+r+","+g+","+b+","+a);
    }
    private static RusterizeView find(View view) {
        if(view instanceof RusterizeView)return (RusterizeView)view;
        if(view instanceof ViewGroup)for(int i=0;i<((ViewGroup)view).getChildCount();i++) {
            RusterizeView child=find(((ViewGroup)view).getChildAt(i));if(child!=null)return child;
        }
        return null;
    }
    @Override public void onCreate(Bundle arguments) {super.onCreate(arguments);start();}
    @Override public void onStart() {
        final Activity activity=startActivitySync(new Intent().setClassName(getTargetContext().getPackageName(),"dev.rusterize.MainActivity").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        waitForIdleSync();
        runOnMainSync(new Runnable(){@Override public void run(){try{test(activity);}catch(Throwable error){failure=error;}}});
        Bundle report=new Bundle();
        if(failure!=null){report.putString("stream","ANDROID_GPU_FAILED: "+android.util.Log.getStackTraceString(failure));finish(1,report);}
        else {report.putString("stream",result);finish(0,report);}
    }
    private void test(Activity activity) throws Exception {
        RusterizeView view=find(activity.getWindow().getDecorView());require(view!=null,"No production RusterizeView");
        Field field=RusterizeView.class.getDeclaredField("shaders");field.setAccessible(true);
        NativeShaders host=(NativeShaders)field.get(view);require(host!=null,"Native shader registration missing");
        require(host.directFrames>=2 && host.readbackFrames==0,"Production host did not use the direct GPU path");
        NativeShaders renderer=new NativeShaders();
        RenderNode node=new RenderNode("GPU composition regression");node.setPosition(0,0,128,128);
        Canvas hardware=node.beginRecording();require(hardware.isHardwareAccelerated(),"Probe requires a hardware Canvas");
        android.opengl.EGLContext previousContext=android.opengl.EGL14.eglGetCurrentContext();
        String vertex="#version 300 es\nprecision highp float;out vec2 uv;void main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2));uv=vec2(p.x,1.0-p.y);gl_Position=vec4(p*2.0-1.0,0,1);}";
        String header="#version 300 es\nprecision highp float;in vec2 uv;layout(std140) uniform Params{vec4 info;vec4 data[4];};out vec4 color;";
        renderer.uploadSource(1,vertex,header+"void main(){color=vec4(uv,info.z,data[0].w);}","Params","");
        float[] values=new float[20];values[0]=2;values[1]=2;values[2]=0.25f;values[3]=1;values[7]=0.5f;
        Bitmap a=renderer.render(hardware,1,values,0);require(a.getConfig()==Bitmap.Config.HARDWARE,"GPU output must be a hardware bitmap");
        require(previousContext.equals(android.opengl.EGL14.eglGetCurrentContext()),"Shader renderer changed the caller EGL context");
        Bitmap read=a.copy(Bitmap.Config.ARGB_8888,false);pixel(read,0,0,64,64,64,128);pixel(read,1,1,191,191,64,128);
        require(renderer.render(hardware,1,values,0)==a && renderer.directFrames==1,"Identical draw should retain the immutable output");
        float[] changed=values.clone();changed[2]=0.75f;
        Bitmap b=renderer.render(hardware,1,changed,0);require(a!=b,"Distinct draw needs distinct immutable storage");
        pixel(a.copy(Bitmap.Config.ARGB_8888,false),0,0,64,64,64,128);
        pixel(b.copy(Bitmap.Config.ARGB_8888,false),0,0,64,64,191,128);
        renderer.uploadSource(2,vertex,header+"uniform sampler2D inputTexture;void main(){color=texture(inputTexture,uv);}","Params","inputTexture");
        renderer.uploadImage(3,2,2,new byte[]{(byte)255,0,0,(byte)255,0,(byte)255,0,(byte)128,0,0,(byte)255,64,(byte)255,(byte)255,0,0});
        Bitmap input=renderer.render(hardware,2,values,3).copy(Bitmap.Config.ARGB_8888,false);
        pixel(input,0,0,255,0,0,255);pixel(input,1,0,0,255,0,128);pixel(input,0,1,0,0,255,64);pixel(input,1,1,0,0,0,0);
        renderer.releaseImage(3);boolean rejected=false;
        try{renderer.render(hardware,2,values,3);}catch(IllegalStateException expected){rejected=true;}
        require(rejected,"Released input must not remain usable");
        renderer.releaseSource(2);rejected=false;
        try{renderer.render(hardware,2,values,0);}catch(IllegalStateException expected){rejected=true;}
        require(rejected,"Released shader must not remain usable");
        for(int i=0;i<24;i++){float[] varying=values.clone();varying[2]=i/24.0f;renderer.render(hardware,1,varying,0);}
        // The first bitmap remains valid even after cache eviction and destruction of EGL objects.
        require(renderer.readbackFrames==0 && renderer.directFrames>=26,"Hardware probe unexpectedly read pixels on CPU");
        long direct=renderer.directFrames;
        renderer.close();pixel(a.copy(Bitmap.Config.ARGB_8888,false),0,0,64,64,64,128);
        renderer.uploadSource(1,vertex,header+"void main(){color=vec4(uv,info.z,data[0].w);}","Params","");
        Bitmap resumed=renderer.render(hardware,1,values,0);pixel(resumed.copy(Bitmap.Config.ARGB_8888,false),1,1,191,191,64,128);
        require(renderer.readbackFrames==0,"Context recreation fell back to CPU");
        Canvas software=new Canvas(Bitmap.createBitmap(2,2,Bitmap.Config.ARGB_8888));
        Bitmap fallback=renderer.render(software,1,values,0);require(fallback.getConfig()!=Bitmap.Config.HARDWARE && renderer.readbackFrames==1,"Software Canvas fallback is missing");
        pixel(fallback,0,0,64,64,64,128);pixel(fallback,1,1,191,191,64,128);
        renderer.close();node.endRecording();node.discardDisplayList();
        result="ANDROID_GPU_OK host_direct="+host.directFrames+" host_readback="+host.readbackFrames+" probe_direct="+(direct+1)+" probe_readback_before_software=0 software_fallback=1 orientation=ok alpha=ok input=ok cache=ok resource_release=ok immutable_lifetime=ok context_recreate=ok";
    }
}
