package dev.rusterize;

import android.app.Activity;
import android.content.Context;
import android.content.ClipboardManager;
import android.content.ClipData;
import android.content.Intent;
import android.net.Uri;
import android.text.*;
import java.io.ByteArrayOutputStream;
import java.lang.ref.WeakReference;
import android.graphics.*;
import android.util.LongSparseArray;
import android.view.*;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.HashSet;

/** Thin UI-thread host. Application state and display lists are produced by Rust. */
public final class RusterizeView extends View implements AutoCloseable {
    static { System.loadLibrary("rusterize_app"); }
    private static native long nativeCreate();
    private static native int nativePoll(long id);
    private static native void nativeDraw(long id,Canvas canvas,View view);
    private static WeakReference<RusterizeView> active = new WeakReference<>(null);
    private static native void nativeDestroy(long id);
    private static native int nativeStatus(long id);
    private static native int nativeOption(long id,int option);
    private static native String nativeTitle(long id);
    private static native int nativeEvent(long id, int kind, float x, float y, float dx, float dy, int detail, int flags);
    private static native byte[] nativeFrame(long id, float w, float h, float scale, double seconds);
    private long handle = nativeCreate();
    private boolean suspended;
    private int windowRevision;
    private Integer originalStatus,originalNavigation,originalFlags,originalRootColor;
    private final long origin = System.nanoTime();
    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
    private final Path path = new Path();
    private final RectF rect = new RectF();
    private final Matrix matrix = new Matrix();
    private final float[] matrixValues = new float[9];
    private float shapeRadius;
    private final LongSparseArray<Bitmap> bitmaps = new LongSparseArray<>();
    private final HashSet<Long> usedImages = new HashSet<>();
    public RusterizeView(Context context) {
        super(context); active = new WeakReference<>(this); setFocusable(true); setFocusableInTouchMode(true);
        paint.setStrokeCap(Paint.Cap.BUTT); paint.setStrokeJoin(Paint.Join.MITER); paint.setStrokeMiter(10);
    }
    private float density() { return getResources().getDisplayMetrics().density; }
    private void schedule(int flags) {
        flags = nativePoll(handle);
        if ((flags & 0x80000000) != 0) throw new IllegalStateException("Rusterize native application failed");
        if ((flags & 4) != 0) { if (getContext() instanceof Activity) ((Activity)getContext()).finish(); return; }
        updateChrome();
        if (!suspended && (flags & 3) != 0) postInvalidateOnAnimation();
    }
    @SuppressWarnings("deprecation")
    private void updateChrome() {
        int revision=nativeOption(handle,8);
        if(windowRevision==revision || !(getContext() instanceof Activity))return;
        windowRevision=revision;
        Activity activity=(Activity)getContext();activity.setTitle(nativeTitle(handle));
        Window window=activity.getWindow();
        if(originalStatus==null){originalStatus=window.getStatusBarColor();originalNavigation=window.getNavigationBarColor();originalFlags=window.getDecorView().getSystemUiVisibility();}
        View root=getParent() instanceof View?(View)getParent():null;
        if(root!=null && originalRootColor==null){android.graphics.drawable.Drawable drawable=root.getBackground();originalRootColor=drawable instanceof android.graphics.drawable.ColorDrawable?((android.graphics.drawable.ColorDrawable)drawable).getColor():Color.TRANSPARENT;}
        int background=nativeOption(handle,4),foreground=nativeOption(handle,5);
        if(background!=-1){window.setStatusBarColor(0xff000000|background);window.setNavigationBarColor(0xff000000|background);if(root!=null)root.setBackgroundColor(0xff000000|background);}
        else {window.setStatusBarColor(originalStatus);window.setNavigationBarColor(originalNavigation);if(root!=null && originalRootColor!=null)root.setBackgroundColor(originalRootColor);}
        int flags=window.getDecorView().getSystemUiVisibility();
        int masks=View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR|View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
        if(foreground==-1) flags=(flags&~masks)|(originalFlags&masks);
        else {boolean dark=((foreground>>16&255)*299+(foreground>>8&255)*587+(foreground&255)*114)<128000;flags=dark?(flags|masks):(flags&~masks);}
        window.getDecorView().setSystemUiVisibility(flags);
    }
    private void event(int kind, float x, float y, float dx, float dy, int detail, int flags) {
        if (handle != 0) schedule(nativeEvent(handle, kind, x, y, dx, dy, detail, flags));
    }
    public void resume() { suspended = false; event(11,0,0,0,0,0,0); }
    public void suspend() { suspended = true; event(10,0,0,0,0,0,0); }
    @Override public void close() {
        if (handle != 0) { nativeDestroy(handle); handle = 0; }
        // Hardware display lists may still reference a bitmap until the next render pass.
        bitmaps.clear();
    }
    @Override protected void onDraw(Canvas canvas) {
        if (handle == 0 || suspended) return;
        float scale = density();
        byte[] data = nativeFrame(handle, getWidth()/scale, getHeight()/scale, scale, (System.nanoTime()-origin)/1e9);
        int saved = canvas.save();
        try {
            canvas.drawColor(Color.TRANSPARENT, PorterDuff.Mode.CLEAR);
            canvas.scale(scale, scale);
            int commands = canvas.save();
            replay(canvas, ByteBuffer.wrap(data).order(ByteOrder.LITTLE_ENDIAN));
            canvas.restoreToCount(commands);
            nativeDraw(handle,canvas,this);
        } finally { canvas.restoreToCount(saved); }
        schedule(nativeStatus(handle));
    }
    private static int rgba(ByteBuffer b) {
        int r = b.get() & 255, g = b.get() & 255, blue = b.get() & 255, a = b.get() & 255;
        return Color.argb(a,r,g,blue);
    }
    private RectF rectangle(ByteBuffer b) {
        float x=b.getFloat(),y=b.getFloat(),w=b.getFloat(),h=b.getFloat(); rect.set(x,y,x+w,y+h); return rect;
    }
    private int shape(ByteBuffer b) {
        int kind=b.getInt(); path.rewind(); path.setFillType(Path.FillType.WINDING);
        switch (kind) {
            case 0: path.addRect(rectangle(b),Path.Direction.CW); break;
            case 1: rectangle(b); shapeRadius=b.getFloat(); path.addRoundRect(rect,shapeRadius,shapeRadius,Path.Direction.CW); break;
            case 2: path.addOval(rectangle(b),Path.Direction.CW); break;
            case 3:
                int n=b.getInt();
                for (int i=0;i<n;i++) {
                    int opcode=b.getInt();switch(opcode) {
                        case 0: path.moveTo(b.getFloat(),b.getFloat()); break;
                        case 1: path.lineTo(b.getFloat(),b.getFloat()); break;
                        case 2: path.cubicTo(b.getFloat(),b.getFloat(),b.getFloat(),b.getFloat(),b.getFloat(),b.getFloat()); break;
                        case 3: path.close(); break;
                        default: throw new IllegalStateException("Unknown path segment");
                    }
                } break;
            default: throw new IllegalStateException("Unknown shape");
        }
        return kind;
    }
    private void brush(ByteBuffer b) {
        paint.setShader(null); paint.setAlpha(255);
        int kind=b.getInt();
        if (kind==0) paint.setColor(rgba(b));
        else if (kind==1) {
            float x0=b.getFloat(),y0=b.getFloat(),x1=b.getFloat(),y1=b.getFloat();
            paint.setColor(Color.WHITE);
            paint.setShader(new LinearGradient(x0,y0,x1,y1,rgba(b),rgba(b),Shader.TileMode.CLAMP));
        } else throw new IllegalStateException("Unknown paint");
    }
    private void drawShape(Canvas c,int kind) {
        if(kind==0)c.drawRect(rect,paint);
        else if(kind==1)c.drawRoundRect(rect,shapeRadius,shapeRadius,paint);
        else if(kind==2)c.drawOval(rect,paint);
        else c.drawPath(path,paint);
    }
    private void replay(Canvas c, ByteBuffer b) {
        if (b.getInt()!=0x32305a52) throw new IllegalStateException("Unsupported Rusterize protocol");
        int count=b.getInt(),depth=0; usedImages.clear();
        for (int i=0;i<count;i++) {
            int opcode=b.getInt();switch (opcode) {
                case 1: c.drawColor(rgba(b),PorterDuff.Mode.SRC); break;
                case 2: c.save(); depth++; break;
                case 3: if(depth--<=0)throw new IllegalStateException("Unbalanced restore");c.restore();break;
                case 4:
                    float a=b.getFloat(),bb=b.getFloat(),cc=b.getFloat(),d=b.getFloat(),e=b.getFloat(),f=b.getFloat();
                    matrixValues[0]=a;matrixValues[1]=cc;matrixValues[2]=e;matrixValues[3]=bb;matrixValues[4]=d;matrixValues[5]=f;matrixValues[6]=0;matrixValues[7]=0;matrixValues[8]=1;
                    matrix.setValues(matrixValues);c.concat(matrix);break;
                case 5: shape(b);c.clipPath(path);break;
                case 6: {int kind=shape(b);brush(b);paint.setStyle(Paint.Style.FILL);drawShape(c,kind);break;}
                case 7: {int kind=shape(b);brush(b);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth(b.getFloat());drawShape(c,kind);break;}
                case 8:
                case 10: {
                    float x=b.getFloat(),y=b.getFloat(); NativeText t=new NativeText(b);
                    int save=c.save();c.translate(x+t.offsetX,y-(opcode==8?t.layout.getLineBaseline(0):0));t.layout.draw(c);c.restoreToCount(save);break;
                }
                case 9:
                    long id=b.getLong();int width=b.getInt(),height=b.getInt();rectangle(b);float opacity=b.getFloat();int len=b.getInt();
                    if(len!=(long)width*height*4)throw new IllegalStateException("Invalid image dimensions");
                    Bitmap bitmap=bitmaps.get(id);
                    if(bitmap==null) {
                        int[] pixels=new int[width*height];for(int p=0;p<pixels.length;p++)pixels[p]=rgba(b);
                        bitmap=Bitmap.createBitmap(pixels,width,height,Bitmap.Config.ARGB_8888);bitmaps.put(id,bitmap);
                    } else b.position(b.position()+len);
                    usedImages.add(id);paint.setShader(null);paint.setColor(Color.WHITE);paint.setAlpha(Math.round(opacity*255));
                    c.drawBitmap(bitmap,null,rect,paint);break;
                default:throw new IllegalStateException("Unknown drawing command");
            }
        }
        if(depth!=0 || b.hasRemaining())throw new IllegalStateException("Invalid display-list length");
        for(int i=bitmaps.size()-1;i>=0;i--)if(!usedImages.contains(bitmaps.keyAt(i)))bitmaps.removeAt(i);
    }

    private static String string(ByteBuffer b) {byte[] data=new byte[b.getInt()];b.get(data);return new String(data,StandardCharsets.UTF_8);}
    private static void integer(ByteArrayOutputStream out,int v) {out.write(v);out.write(v>>>8);out.write(v>>>16);out.write(v>>>24);}
    private static void real(ByteArrayOutputStream out,float v) {integer(out,Float.floatToIntBits(v));}
    private static void raw(ByteArrayOutputStream out,byte[] data) {out.write(data,0,data.length);}
    private static final class NativeText {
        final String text;
        final StaticLayout layout;
        final float offsetX;
        NativeText(ByteBuffer b) {
            float size=b.getFloat();int color=rgba(b),family=b.getInt(),flags=b.getInt();String font=string(b);
            float width=b.getFloat();int wrap=b.getInt(),align=b.getInt(),direction=b.getInt();text=string(b);
            if(wrap==2)throw new UnsupportedOperationException("Character wrapping is not exposed by Android StaticLayout");
            TextPaint p=new TextPaint(Paint.ANTI_ALIAS_FLAG|Paint.SUBPIXEL_TEXT_FLAG);
            p.setColor(color);p.setTextSize(size);p.setTypeface(Typeface.create(font.isEmpty()?(family==0?"sans-serif":family==1?"serif":"monospace"):font,flags&3));
            p.setUnderlineText((flags&4)!=0);p.setStrikeThruText((flags&8)!=0);
            int actualWidth=(int)Math.ceil(wrap==0?Math.max(width,Layout.getDesiredWidth(text,p)):width);
            offsetX=(width-actualWidth)*(align==1?0.5f:align==2?1f:0f);
            Layout.Alignment alignment=align==1?Layout.Alignment.ALIGN_CENTER:((align==2) != (direction==1)?Layout.Alignment.ALIGN_OPPOSITE:Layout.Alignment.ALIGN_NORMAL);
            layout=StaticLayout.Builder.obtain(text,0,text.length(),p,actualWidth).setAlignment(alignment)
                .setTextDirection(direction==1?TextDirectionHeuristics.RTL:TextDirectionHeuristics.LTR)
                .setIncludePad(false).setBreakStrategy(Layout.BREAK_STRATEGY_SIMPLE).setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE).build();
        }
        byte[] metrics() {
            ByteArrayOutputStream out=new ByteArrayOutputStream();float width=0,full=0;
            for(int i=0;i<layout.getLineCount();i++){width=Math.max(width,layout.getLineMax(i));full=Math.max(full,layout.getLineWidth(i));}
            real(out,width);real(out,full);real(out,layout.getHeight());real(out,layout.getLineBaseline(0));integer(out,layout.getLineCount());
            int[] bytes=new int[text.length()+1];int offset=0;
            for(int i=0;i<text.length();){int ch=text.codePointAt(i),units=Character.charCount(ch);for(int j=0;j<units;j++)bytes[i+j]=offset;offset+=ch<0x80?1:ch<0x800?2:ch<0x10000?3:4;i+=units;bytes[i]=offset;}
            for(int i=0;i<layout.getLineCount();i++) {
                integer(out,bytes[layout.getLineStart(i)]);integer(out,bytes[layout.getLineEnd(i)]);
                real(out,layout.getLineTop(i));real(out,layout.getLineBottom(i)-layout.getLineTop(i));real(out,layout.getLineBaseline(i));
            }
            return out.toByteArray();
        }
    }
    public static byte[] nativeService(byte[] data) {
        ByteArrayOutputStream out=new ByteArrayOutputStream();
        try {
            ByteBuffer b=ByteBuffer.wrap(data).order(ByteOrder.LITTLE_ENDIAN);int op=b.getInt();
            integer(out,0);
            if(op==1){raw(out,new NativeText(b).metrics());return out.toByteArray();}
            if(op==2){integer(out,1|2|4|256|512);return out.toByteArray();}
            RusterizeView view=active.get();if(view==null)throw new IllegalStateException("No active native view");
            Context context=view.getContext();
            switch(op) {
                case 3: {
                    ClipboardManager clipboard=(ClipboardManager)context.getSystemService(Context.CLIPBOARD_SERVICE);
                    ClipData clip=clipboard.getPrimaryClip();CharSequence value=clip==null||clip.getItemCount()==0?"":clip.getItemAt(0).coerceToText(context);
                    raw(out,(value==null?"":value.toString()).getBytes(StandardCharsets.UTF_8));break;
                }
                case 4: ((ClipboardManager)context.getSystemService(Context.CLIPBOARD_SERVICE)).setPrimaryClip(ClipData.newPlainText("",string(b)));break;
                case 5: {int[] types={PointerIcon.TYPE_ARROW,PointerIcon.TYPE_TEXT,PointerIcon.TYPE_HAND,PointerIcon.TYPE_CROSSHAIR,PointerIcon.TYPE_ALL_SCROLL,PointerIcon.TYPE_HORIZONTAL_DOUBLE_ARROW,PointerIcon.TYPE_VERTICAL_DOUBLE_ARROW,PointerIcon.TYPE_NULL};view.setPointerIcon(PointerIcon.getSystemIcon(context,types[b.getInt()]));break;}
                case 11: new android.app.AlertDialog.Builder(context).setTitle(string(b)).setMessage(string(b)).setPositiveButton(android.R.string.ok,null).show();break;
                case 12: context.startActivity(new Intent(Intent.ACTION_VIEW,Uri.parse(string(b))));break;
                default: throw new UnsupportedOperationException("Native operation unavailable on Android: "+op);
            }
        } catch(Exception error) {out.reset();integer(out,1);raw(out,error.toString().getBytes(StandardCharsets.UTF_8));}
        return out.toByteArray();
    }

    @Override public boolean onTouchEvent(MotionEvent e) {
        float scale=density();int action=e.getActionMasked(),index=e.getActionIndex();
        if(action==MotionEvent.ACTION_DOWN) {requestFocus();getParent().requestDisallowInterceptTouchEvent(true);}
        if(action==MotionEvent.ACTION_MOVE || action==MotionEvent.ACTION_CANCEL) {
            for(int i=0;i<e.getPointerCount();i++) event(action==MotionEvent.ACTION_MOVE?2:4,e.getX(i)/scale,e.getY(i)/scale,0,0,e.getPointerId(i),action==MotionEvent.ACTION_MOVE?1:0);
        } else {
            int kind=(action==MotionEvent.ACTION_DOWN||action==MotionEvent.ACTION_POINTER_DOWN)?1:3;
            event(kind,e.getX(index)/scale,e.getY(index)/scale,0,0,e.getPointerId(index),kind==1?1:0);
            if(action==MotionEvent.ACTION_UP)performClick();
        }
        return true;
    }
    @Override public boolean performClick() {super.performClick();return true;}
    @Override public boolean onGenericMotionEvent(MotionEvent e) {
        if(e.getActionMasked()==MotionEvent.ACTION_SCROLL) {event(5,e.getX()/density(),e.getY()/density(),e.getAxisValue(MotionEvent.AXIS_HSCROLL)*40,-e.getAxisValue(MotionEvent.AXIS_VSCROLL)*40,0,0);return true;}
        if(e.getActionMasked()==MotionEvent.ACTION_HOVER_MOVE) {event(2,e.getX()/density(),e.getY()/density(),0,0,0,0);return true;}
        return super.onGenericMotionEvent(e);
    }
    private static int key(int key) {
        switch(key) {
            case KeyEvent.KEYCODE_ENTER:return 13;case KeyEvent.KEYCODE_SPACE:return 32;case KeyEvent.KEYCODE_ESCAPE:return 27;
            case KeyEvent.KEYCODE_TAB:return 9;case KeyEvent.KEYCODE_DEL:return 8;case KeyEvent.KEYCODE_FORWARD_DEL:return 46;
            case KeyEvent.KEYCODE_DPAD_LEFT:return 37;case KeyEvent.KEYCODE_DPAD_UP:return 38;case KeyEvent.KEYCODE_DPAD_RIGHT:return 39;case KeyEvent.KEYCODE_DPAD_DOWN:return 40;
            case KeyEvent.KEYCODE_MOVE_HOME:return 36;case KeyEvent.KEYCODE_MOVE_END:return 35;default:return 0x10000+key;
        }
    }
    @Override public boolean onKeyDown(int code,KeyEvent e) {
        if(code==KeyEvent.KEYCODE_BACK)return super.onKeyDown(code,e);
        int mods=(e.isShiftPressed()?1:0)|(e.isCtrlPressed()?2:0)|(e.isAltPressed()?4:0)|(e.isMetaPressed()?8:0)|(e.getRepeatCount()>0?16:0);
        event(6,0,0,0,0,key(code),mods);
        int ch=e.getUnicodeChar();if(ch!=0 && !e.isCtrlPressed() && !e.isAltPressed())event(8,0,0,0,0,ch,0);
        return true;
    }
    @Override public boolean onKeyUp(int code,KeyEvent e) {
        if(code==KeyEvent.KEYCODE_BACK)return super.onKeyUp(code,e);
        event(7,0,0,0,0,key(code),(e.isShiftPressed()?1:0)|(e.isCtrlPressed()?2:0)|(e.isAltPressed()?4:0)|(e.isMetaPressed()?8:0));return true;
    }
    @Override protected void onFocusChanged(boolean focused,int direction,Rect previous) {super.onFocusChanged(focused,direction,previous);event(9,0,0,0,0,focused?1:0,0);}
}
