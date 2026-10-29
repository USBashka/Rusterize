package dev.rusterize;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;
import android.provider.DocumentsContract;
import java.io.*;
import java.nio.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

final class NativeFiles {
    static final int PICKER = 18522;
    static final int MAX = 32 * 1024 * 1024;
    private static Context application;
    private static final Set<String> freshExports = Collections.synchronizedSet(new HashSet<String>());
    private final RusterizeView view;
    private final ArrayList<Result> results = new ArrayList<>();
    private long app, request;
    private boolean waiting, save, closed;
    private static final class Result { long app,id; int status; byte[] bytes; Result(long a,long i,int s,byte[] b){app=a;id=i;status=s;bytes=b;} }
    NativeFiles(RusterizeView view){this.view=view;application=view.getContext().getApplicationContext();}
    private static String string(ByteBuffer b){int n=b.getInt();if(n<0||n>b.remaining()||n>1024*1024)throw new IllegalArgumentException("Invalid string");byte[] data=new byte[n];b.get(data);return new String(data,StandardCharsets.UTF_8);}
    private static void integer(ByteArrayOutputStream out,int value){out.write(value);out.write(value>>>8);out.write(value>>>16);out.write(value>>>24);}
    private static void longValue(ByteArrayOutputStream out,long value){integer(out,(int)value);integer(out,(int)(value>>>32));}
    private static void raw(ByteArrayOutputStream out,byte[] bytes){out.write(bytes,0,bytes.length);}
    private static void string(ByteArrayOutputStream out,String value){byte[] b=value.getBytes(StandardCharsets.UTF_8);integer(out,b.length);raw(out,b);}
    byte[] command(int op,ByteBuffer b){
        if(op==60){
            if(closed||waiting)throw new IllegalStateException("A document picker is already active or the view was destroyed");
            long owner=b.getLong(),id=b.getLong();if(b.getInt()!=10)throw new IllegalArgumentException("Expected document picker request");
            boolean saving=b.getInt()!=0,directory=b.getInt()!=0;String title=string(b),name=string(b);
            if(saving&&directory)throw new IllegalArgumentException("Cannot save a directory");
            if(!(view.getContext() instanceof Activity))throw new IllegalStateException("No activity for document picker");
            Intent intent=new Intent(directory?Intent.ACTION_OPEN_DOCUMENT_TREE:saving?Intent.ACTION_CREATE_DOCUMENT:Intent.ACTION_OPEN_DOCUMENT);
            if(!directory){intent.addCategory(Intent.CATEGORY_OPENABLE);intent.setType("*/*");}
            if(saving)intent.putExtra(Intent.EXTRA_TITLE,name.isEmpty()?"document":name);
            intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_WRITE_URI_PERMISSION|Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
            ((Activity)view.getContext()).startActivityForResult(intent,PICKER);
            app=owner;request=id;save=saving;waiting=true;return new byte[0];
        }
        long owner=b.getLong();
        if(op==63){if(waiting&&app==owner)waiting=false;for(int i=results.size()-1;i>=0;i--)if(results.get(i).app==owner)results.remove(i);return new byte[0];}
        ByteArrayOutputStream out=new ByteArrayOutputStream();
        for(int i=0;i<results.size();i++)if(results.get(i).app==owner){Result r=results.remove(i);integer(out,1);longValue(out,r.id);integer(out,r.status);raw(out,r.bytes);return out.toByteArray();}
        integer(out,0);return out.toByteArray();
    }
    void result(int code,Intent data){
        if(!waiting||closed)return;waiting=false;ByteArrayOutputStream out=new ByteArrayOutputStream();
        try{
            if(code!=Activity.RESULT_OK||data==null||data.getData()==null)integer(out,0);
            else{
                Uri uri=data.getData();if(!"content".equals(uri.getScheme()))throw new SecurityException("Document provider did not return a content URI");
                // Access lasts for this selection; callers store application data in filesDir.
                if(save)freshExports.add(uri.toString());
                integer(out,1);string(out,uri.toString());
            }
            results.add(new Result(app,request,0,out.toByteArray()));
        }catch(Exception error){results.add(new Result(app,request,1,error.toString().getBytes(StandardCharsets.UTF_8)));}
        view.post(new Runnable(){@Override public void run(){view.wakeServices();}});
    }
    static byte[] execute(byte[] data){
        ByteArrayOutputStream out=new ByteArrayOutputStream();
        try{
            ByteBuffer b=ByteBuffer.wrap(data).order(ByteOrder.LITTLE_ENDIAN);int op=b.getInt(),kind=b.getInt();String location=string(b);
            if(application==null||kind!=1||!location.startsWith("content://"))throw new IllegalArgumentException("Expected Android content URI");
            Uri uri=Uri.parse(location);integer(out,0);
            if(op==40){
                try(InputStream stream=application.getContentResolver().openInputStream(uri)){
                    if(stream==null)throw new IOException("Document provider returned no stream");
                    byte[] block=new byte[32768];int total=0,n;
                    while((n=stream.read(block))!=-1){total+=n;if(total>MAX)throw new IOException("File exceeds 32 MiB");out.write(block,0,n);}
                }
            }else if(op==41){
                int length=b.getInt();if(length<0||length>MAX||length!=b.remaining())throw new IOException("Invalid export size (maximum 32 MiB)");
                if(!freshExports.remove(location))throw new IOException("This provider cannot guarantee atomic replacement; select a new export document");
                try(OutputStream stream=application.getContentResolver().openOutputStream(uri,"wt")){
                    if(stream==null)throw new IOException("Document provider returned no output stream");
                    byte[] block=new byte[32768];while(b.hasRemaining()){int n=Math.min(b.remaining(),block.length);b.get(block,0,n);stream.write(block,0,n);}stream.flush();
                }catch(Exception failure){try{DocumentsContract.deleteDocument(application.getContentResolver(),uri);}catch(Exception ignored){}throw failure;}
            }else if(op==44){freshExports.remove(location);}
            else throw new IllegalArgumentException("Unknown document operation");
        }catch(Exception error){out.reset();integer(out,1);raw(out,error.toString().getBytes(StandardCharsets.UTF_8));}
        return out.toByteArray();
    }
    void close(){closed=true;waiting=false;results.clear();freshExports.clear();}
}
