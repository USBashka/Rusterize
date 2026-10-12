package dev.rusterize;

import android.annotation.SuppressLint;
import android.graphics.Bitmap;
import android.net.Uri;
import android.view.View;
import android.webkit.*;
import android.widget.FrameLayout;
import java.io.ByteArrayOutputStream;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.HashMap;

final class NativeWebViews implements AutoCloseable {
    static final int CAPABILITY = 1024;
    private final View owner;
    private final Runnable wake;
    private final HashMap<Integer, Entry> views = new HashMap<>();
    private final ArrayDeque<byte[]> events = new ArrayDeque<>();
    private boolean closed;
    private static final class Entry {
        final int id;
        final WebView web;
        final boolean javascript;
        boolean failed;
        Entry(int id, WebView web, boolean javascript) { this.id=id; this.web=web; this.javascript=javascript; }
    }
    NativeWebViews(View owner, Runnable wake) { this.owner=owner; this.wake=wake; }
    private static void integer(ByteArrayOutputStream out, int n) { out.write(n); out.write(n>>>8); out.write(n>>>16); out.write(n>>>24); }
    private static String string(ByteBuffer b) { byte[] data=new byte[b.getInt()]; b.get(data); return new String(data, StandardCharsets.UTF_8); }
    private static boolean allowed(String url) { return url.startsWith("https://") || url.startsWith("http://") || url.equals("about:blank"); }
    private void emit(Entry e, int kind, int token, String text) {
        if(closed || views.get(e.id)!=e)return;
        ByteArrayOutputStream out=new ByteArrayOutputStream(); integer(out,kind); integer(out,e.id); integer(out,token);
        byte[] data=text.getBytes(StandardCharsets.UTF_8); integer(out,data.length); out.write(data,0,data.length);
        events.add(out.toByteArray()); owner.post(wake);
    }
    private FrameLayout root() { return (FrameLayout)owner.getParent(); }
    private FrameLayout.LayoutParams bounds(ByteBuffer b) {
        float scale=owner.getResources().getDisplayMetrics().density;
        int x=Math.round(b.getFloat()*scale),y=Math.round(b.getFloat()*scale);
        FrameLayout.LayoutParams p=new FrameLayout.LayoutParams(Math.round(b.getFloat()*scale),Math.round(b.getFloat()*scale));
        p.leftMargin=x; p.topMargin=y; p.gravity=android.view.Gravity.TOP|android.view.Gravity.LEFT; return p;
    }
    @SuppressLint("SetJavaScriptEnabled")
    byte[] command(int op, ByteBuffer b) {
        if(op==21)return events.isEmpty()?new byte[4]:events.remove();
        int id=b.getInt(),action=b.getInt();
        if(action==1) {
            if(views.containsKey(id))throw new IllegalStateException("WebView ID already exists");
            FrameLayout.LayoutParams frame=bounds(b); boolean visible=b.getInt()!=0,javascript=b.getInt()!=0;
            WebView web=new WebView(owner.getContext()); Entry e=new Entry(id,web,javascript);
            WebSettings settings=web.getSettings(); settings.setJavaScriptEnabled(javascript);
            settings.setAllowFileAccess(false); settings.setAllowContentAccess(false);
            settings.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
            CookieManager.getInstance().setAcceptThirdPartyCookies(web,false);
            web.setWebViewClient(new WebViewClient() {
                @Override public boolean shouldOverrideUrlLoading(WebView v, WebResourceRequest request) {
                    String url=request.getUrl().toString();
                    if(!allowed(url)){emit(e,3,0,"Only HTTP(S) and about:blank navigation is allowed");return true;} return false;
                }
                @Override public void onPageStarted(WebView v,String url,Bitmap icon) { e.failed=false; }
                @Override public void onPageFinished(WebView v,String url) { if(!e.failed)emit(e,2,0,url); }
                @Override public void onReceivedError(WebView v,WebResourceRequest request,WebResourceError error) {
                    if(request.isForMainFrame()){e.failed=true;emit(e,3,0,error.getDescription().toString());}
                }
                @Override public void onReceivedSslError(WebView v,SslErrorHandler handler,android.net.http.SslError error) {
                    handler.cancel(); e.failed=true; emit(e,3,0,"TLS certificate validation failed");
                }
            });
            web.setVisibility(visible?View.VISIBLE:View.INVISIBLE);
            views.put(id,e); root().addView(web,frame); emit(e,1,0,""); return new byte[0];
        }
        Entry e=views.get(id); if(e==null)throw new IllegalStateException("Unknown WebView ID");
        switch(action) {
            case 2:
                views.remove(id);
                java.util.Iterator<byte[]> iterator=events.iterator();
                while(iterator.hasNext())if(ByteBuffer.wrap(iterator.next()).order(java.nio.ByteOrder.LITTLE_ENDIAN).getInt(4)==id)iterator.remove();
                root().removeView(e.web); e.web.stopLoading(); e.web.destroy(); break;
            case 3: e.web.setLayoutParams(bounds(b)); break;
            case 4: e.web.setVisibility(b.getInt()!=0?View.VISIBLE:View.INVISIBLE); break;
            case 5: e.web.loadUrl(string(b)); break;
            case 6: e.web.loadDataWithBaseURL("about:blank",string(b),"text/html","UTF-8",null); break;
            case 7:
                if(!e.javascript)throw new IllegalStateException("JavaScript is disabled");
                final int token=b.getInt(); e.web.evaluateJavascript(string(b),new ValueCallback<String>() {
                    @Override public void onReceiveValue(String json) { emit(e,4,token,json==null?"null":json); }
                }); break;
            case 8: e.web.requestFocus(); break;
            case 9: e.web.reload(); break;
            case 10: if(e.web.canGoBack())e.web.goBack(); break;
            case 11: if(e.web.canGoForward())e.web.goForward(); break;
            default: throw new IllegalArgumentException("Unknown WebView command");
        }
        return new byte[0];
    }
    void resume() { for(Entry e:views.values())e.web.onResume(); }
    void suspend() { for(Entry e:views.values())e.web.onPause(); }
    @Override public void close() {
        closed=true;
        for(Entry e:views.values()){root().removeView(e.web);e.web.stopLoading();e.web.destroy();}
        views.clear(); events.clear();
    }
}
