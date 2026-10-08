/* GTK 4 / Cairo / Pango host. All application logic lives in the Rust library. */
#include <gtk/gtk.h>
#include <pango/pangocairo.h>
#include <math.h>
#include <string.h>
#include "../rusterize.h"

typedef struct { const uint8_t *p, *end; gboolean ok; } Reader;
static uint32_t u32(Reader *r) { uint32_t v=0; if(r->end-r->p<4){r->ok=FALSE;return 0;} memcpy(&v,r->p,4);r->p+=4;return GUINT32_FROM_LE(v); }
static float f32(Reader *r) {uint32_t n=u32(r);float v;memcpy(&v,&n,4);return v;}
static uint64_t u64(Reader *r) {uint64_t lo=u32(r),hi=u32(r);return lo|(hi<<32);}
static const uint8_t *bytes(Reader *r,uint32_t n) {if((size_t)(r->end-r->p)<n){r->ok=FALSE;return NULL;}const uint8_t *p=r->p;r->p+=n;return p;}
static void rgba(Reader *r,double c[4]) {const uint8_t *p=bytes(r,4);for(int i=0;i<4;i++)c[i]=p?p[i]/255.0:0;}
static void source(cairo_t *c,Reader *r) {
    uint32_t kind=u32(r);double a[4],b[4];
    if(kind==0){rgba(r,a);cairo_set_source_rgba(c,a[0],a[1],a[2],a[3]);}
    else if(kind==1){double x=f32(r),y=f32(r),xx=f32(r),yy=f32(r);rgba(r,a);rgba(r,b);cairo_pattern_t *p=cairo_pattern_create_linear(x,y,xx,yy);cairo_pattern_add_color_stop_rgba(p,0,a[0],a[1],a[2],a[3]);cairo_pattern_add_color_stop_rgba(p,1,b[0],b[1],b[2],b[3]);cairo_pattern_set_extend(p,CAIRO_EXTEND_PAD);cairo_set_source(c,p);cairo_pattern_destroy(p);}
    else r->ok=FALSE;
}
static void shape(cairo_t *c,Reader *r) {
    uint32_t kind=u32(r);cairo_new_path(c);
    if(kind<=2) {
        double x=f32(r),y=f32(r),w=f32(r),h=f32(r);
        if(kind==0)cairo_rectangle(c,x,y,w,h);
        else if(kind==1) {
            double radius=f32(r),k=0.5522847498307936*radius;
            cairo_move_to(c,x+radius,y);cairo_line_to(c,x+w-radius,y);
            cairo_curve_to(c,x+w-radius+k,y,x+w,y+radius-k,x+w,y+radius);cairo_line_to(c,x+w,y+h-radius);
            cairo_curve_to(c,x+w,y+h-radius+k,x+w-radius+k,y+h,x+w-radius,y+h);cairo_line_to(c,x+radius,y+h);
            cairo_curve_to(c,x+radius-k,y+h,x,y+h-radius+k,x,y+h-radius);cairo_line_to(c,x,y+radius);
            cairo_curve_to(c,x,y+radius-k,x+radius-k,y,x+radius,y);cairo_close_path(c);
        } else if(w>0 && h>0) {cairo_save(c);cairo_translate(c,x+w/2,y+h/2);cairo_scale(c,w/2,h/2);cairo_arc(c,0,0,1,0,2*G_PI);cairo_restore(c);cairo_close_path(c);}
    } else if(kind==3) {
        uint32_t n=u32(r);for(uint32_t i=0;i<n && r->ok;i++) {
            uint32_t op=u32(r);
            if(op==0 || op==1){double x=f32(r),y=f32(r);if(op==0)cairo_move_to(c,x,y);else cairo_line_to(c,x,y);}
            else if(op==2){double a=f32(r),b=f32(r),d=f32(r),e=f32(r),f=f32(r),g=f32(r);cairo_curve_to(c,a,b,d,e,f,g);}
            else if(op==3)cairo_close_path(c);else r->ok=FALSE;
        }
    } else r->ok=FALSE;
}

static char *string(Reader *r) {uint32_t n=u32(r);const uint8_t *p=bytes(r,n);return p?g_strndup((const char*)p,n):g_strdup("");}
static void put_u32(GByteArray *b,uint32_t v){v=GUINT32_TO_LE(v);g_byte_array_append(b,(const guint8*)&v,4);}
static void put_f32(GByteArray *b,float v){uint32_t n;memcpy(&n,&v,4);put_u32(b,n);}
static void put_string(GByteArray *b,const char *s){put_u32(b,strlen(s));g_byte_array_append(b,(const guint8*)s,strlen(s));}
typedef struct {PangoLayout *layout;char *text;double color[4];double offset_x;} NativeText;
static NativeText native_text(Reader *r) {
    NativeText result={0};double size=f32(r);rgba(r,result.color);uint32_t family=u32(r),flags=u32(r);char *name=string(r);
    double width=f32(r);uint32_t wrap=u32(r),align=u32(r),direction=u32(r);result.text=string(r);
    PangoContext *context=pango_font_map_create_context(pango_cairo_font_map_get_default());
    pango_cairo_context_set_resolution(context,96);
    cairo_font_options_t *options=cairo_font_options_create();cairo_font_options_set_hint_metrics(options,CAIRO_HINT_METRICS_OFF);pango_cairo_context_set_font_options(context,options);cairo_font_options_destroy(options);
    pango_context_set_base_dir(context,direction?PANGO_DIRECTION_RTL:PANGO_DIRECTION_LTR);
    result.layout=pango_layout_new(context);g_object_unref(context);
    PangoFontDescription *font=pango_font_description_new();pango_font_description_set_family(font,*name?name:family==0?"Sans":family==1?"Serif":"Monospace");
    pango_font_description_set_absolute_size(font,size*PANGO_SCALE);pango_font_description_set_weight(font,flags&1?PANGO_WEIGHT_BOLD:PANGO_WEIGHT_NORMAL);pango_font_description_set_style(font,flags&2?PANGO_STYLE_ITALIC:PANGO_STYLE_NORMAL);
    pango_layout_set_font_description(result.layout,font);pango_font_description_free(font);g_free(name);
    pango_layout_set_text(result.layout,result.text,-1);pango_layout_set_auto_dir(result.layout,FALSE);
    pango_layout_set_alignment(result.layout,align==1?PANGO_ALIGN_CENTER:align==2?PANGO_ALIGN_RIGHT:PANGO_ALIGN_LEFT);
    pango_layout_set_width(result.layout,wrap?(int)(width*PANGO_SCALE):-1);pango_layout_set_wrap(result.layout,wrap==2?PANGO_WRAP_CHAR:PANGO_WRAP_WORD_CHAR);
    PangoAttrList *attrs=pango_attr_list_new();if(flags&4)pango_attr_list_insert(attrs,pango_attr_underline_new(PANGO_UNDERLINE_SINGLE));if(flags&8)pango_attr_list_insert(attrs,pango_attr_strikethrough_new(TRUE));pango_layout_set_attributes(result.layout,attrs);pango_attr_list_unref(attrs);
    if(!wrap && align) {PangoRectangle logical;pango_layout_get_extents(result.layout,NULL,&logical);result.offset_x=(width-(double)logical.width/PANGO_SCALE)*(align==1?0.5:1.0);}
    return result;
}
static void native_text_free(NativeText *t){if(t->layout)g_object_unref(t->layout);g_free(t->text);}
static void text_metrics(NativeText *t,GByteArray *out) {
    PangoRectangle logical;pango_layout_get_extents(t->layout,NULL,&logical);
    put_f32(out,(float)logical.width/PANGO_SCALE);put_f32(out,(float)logical.width/PANGO_SCALE);put_f32(out,(float)logical.height/PANGO_SCALE);put_f32(out,(float)pango_layout_get_baseline(t->layout)/PANGO_SCALE);
    int count=pango_layout_get_line_count(t->layout);put_u32(out,count);PangoLayoutIter *iter=pango_layout_get_iter(t->layout);
    for(int i=0;i<count;i++) {
        PangoLayoutLine *line=pango_layout_iter_get_line_readonly(iter);int start=line->start_index,top,bottom,baseline=pango_layout_iter_get_baseline(iter);
        pango_layout_iter_get_line_yrange(iter,&top,&bottom);gboolean has_next=pango_layout_iter_next_line(iter);PangoLayoutLine *next=has_next?pango_layout_iter_get_line_readonly(iter):NULL;
        put_u32(out,start);put_u32(out,next?(uint32_t)next->start_index:(uint32_t)strlen(t->text));put_f32(out,(float)top/PANGO_SCALE);put_f32(out,(float)(bottom-top)/PANGO_SCALE);put_f32(out,(float)baseline/PANGO_SCALE);
    }
    pango_layout_iter_free(iter);
}

typedef struct {cairo_surface_t *surface;uint8_t *data;uint64_t generation;} Image;
typedef struct {
    uint64_t id,generation;GtkWidget *window,*area;GHashTable *images,*keys;guint tick;gint64 origin;
    GtkCssProvider *chrome;uint32_t window_revision;
    double pointer_x,pointer_y;uint32_t buttons;gboolean failed;
} App;

static App *service_app;
typedef struct {GMainLoop *loop;char *text;GError *error;int response;} NativeWait;
static void clipboard_ready(GObject *source,GAsyncResult *result,gpointer data){NativeWait *w=data;w->text=gdk_clipboard_read_text_finish(GDK_CLIPBOARD(source),result,&w->error);g_main_loop_quit(w->loop);}
static void dialog_ready(gpointer dialog,int response,gpointer data){(void)dialog;NativeWait *w=data;w->response=response;g_main_loop_quit(w->loop);}
G_GNUC_BEGIN_IGNORE_DEPRECATIONS
static const uint8_t *native_service(const uint8_t *data,size_t len,size_t *size) {
    static GByteArray *previous=NULL;GByteArray *out=g_byte_array_new();Reader r={data,data+len,TRUE};uint32_t op=u32(&r);char *error=NULL;put_u32(out,0);
    App *app=service_app;
    if(op==1){NativeText text=native_text(&r);if(r.ok)text_metrics(&text,out);native_text_free(&text);}
    else if(op==2)put_u32(out,1|2|4|8|16|128|256|512);
    else if(!app || !app->window)error=g_strdup("No active native window");
    else if(op==3) {
        NativeWait wait={0};wait.loop=g_main_loop_new(NULL,FALSE);
        gdk_clipboard_read_text_async(gtk_widget_get_clipboard(app->window),NULL,clipboard_ready,&wait);g_main_loop_run(wait.loop);g_main_loop_unref(wait.loop);
        if(wait.error){error=g_strdup(wait.error->message);g_error_free(wait.error);}else if(wait.text)g_byte_array_append(out,(const guint8*)wait.text,strlen(wait.text));g_free(wait.text);
    } else if(op==4){char *text=string(&r);gdk_clipboard_set_text(gtk_widget_get_clipboard(app->window),text);g_free(text);}
    else if(op==5){const char *names[]={"default","text","pointer","crosshair","move","ew-resize","ns-resize","none"};uint32_t cursor=u32(&r);if(cursor<8)gtk_widget_set_cursor_from_name(app->area,names[cursor]);else r.ok=FALSE;}
    else if(op==6){uint32_t state=u32(&r);GtkWindow *window=GTK_WINDOW(app->window);if(state!=3)gtk_window_unfullscreen(window);if(state==0){gtk_window_unmaximize(window);gtk_window_unminimize(window);}else if(state==1)gtk_window_minimize(window);else if(state==2)gtk_window_maximize(window);else if(state==3)gtk_window_fullscreen(window);else r.ok=FALSE;}
    else if(op==7){int w=(int)f32(&r),h=(int)f32(&r);gtk_window_set_default_size(GTK_WINDOW(app->window),w,h);}
    else if(op==10) {
        gboolean save=u32(&r),directory=u32(&r);char *title=string(&r),*name=string(&r);
        GtkFileChooserNative *dialog=gtk_file_chooser_native_new(title,GTK_WINDOW(app->window),directory?GTK_FILE_CHOOSER_ACTION_SELECT_FOLDER:save?GTK_FILE_CHOOSER_ACTION_SAVE:GTK_FILE_CHOOSER_ACTION_OPEN,NULL,NULL);
        if(save && *name)gtk_file_chooser_set_current_name(GTK_FILE_CHOOSER(dialog),name);
        NativeWait wait={0};wait.loop=g_main_loop_new(NULL,FALSE);g_signal_connect(dialog,"response",G_CALLBACK(dialog_ready),&wait);gtk_native_dialog_show(GTK_NATIVE_DIALOG(dialog));g_main_loop_run(wait.loop);g_main_loop_unref(wait.loop);
        if(wait.response==GTK_RESPONSE_ACCEPT){GFile *file=gtk_file_chooser_get_file(GTK_FILE_CHOOSER(dialog));char *path=file?g_file_get_path(file):NULL;if(path){put_u32(out,1);put_string(out,path);}else error=g_strdup("The selected file has no local filesystem path");g_free(path);if(file)g_object_unref(file);}else put_u32(out,0);
        gtk_native_dialog_destroy(GTK_NATIVE_DIALOG(dialog));g_object_unref(dialog);g_free(title);g_free(name);
    } else if(op==11) {
        char *title=string(&r),*message=string(&r);GtkWidget *dialog=gtk_message_dialog_new(GTK_WINDOW(app->window),GTK_DIALOG_MODAL,GTK_MESSAGE_INFO,GTK_BUTTONS_OK,"%s",message);gtk_window_set_title(GTK_WINDOW(dialog),title);
        NativeWait wait={0};wait.loop=g_main_loop_new(NULL,FALSE);g_signal_connect(dialog,"response",G_CALLBACK(dialog_ready),&wait);gtk_window_present(GTK_WINDOW(dialog));g_main_loop_run(wait.loop);g_main_loop_unref(wait.loop);gtk_window_destroy(GTK_WINDOW(dialog));g_free(title);g_free(message);
    } else if(op==12){char *uri=string(&r);GError *e=NULL;if(!g_app_info_launch_default_for_uri(uri,NULL,&e)){error=g_strdup(e->message);g_error_free(e);}g_free(uri);}
    else error=g_strdup("Native operation unavailable on GTK/Wayland");
    if(!r.ok && !error)error=g_strdup("Malformed native request");
    if(error){g_byte_array_set_size(out,0);put_u32(out,1);g_byte_array_append(out,(const guint8*)error,strlen(error));g_free(error);}
    if(previous){g_byte_array_unref(previous);}
    previous=out;*size=out->len;return out->data;
}

G_GNUC_END_IGNORE_DEPRECATIONS
static float window_float(App *app,uint32_t option){uint32_t bits=rusterize_window_option(app->id,option);float result;memcpy(&result,&bits,4);return result;}
static void update_chrome(App *app) {
    uint32_t revision=rusterize_window_option(app->id,8);if(revision==app->window_revision)return;app->window_revision=revision;
    gtk_window_set_title(GTK_WINDOW(app->window),rusterize_title(app->id));
    uint32_t bg=rusterize_window_option(app->id,4),fg=rusterize_window_option(app->id,5);
    GString *css=g_string_new("headerbar.rusterize-chrome {");
    if(bg!=UINT32_MAX)g_string_append_printf(css,"background-image:none;background-color:#%06x;",bg);
    if(fg!=UINT32_MAX)g_string_append_printf(css,"color:#%06x;",fg);
    g_string_append(css,"}");
    if(fg!=UINT32_MAX)g_string_append_printf(css,"headerbar.rusterize-chrome label,headerbar.rusterize-chrome button{color:#%06x;}",fg);
    #if GTK_CHECK_VERSION(4,12,0)
    gtk_css_provider_load_from_string(app->chrome,css->str);
    #else
    gtk_css_provider_load_from_data(app->chrome,css->str,-1);
    #endif
    g_string_free(css,TRUE);
}
static void image_free(gpointer ptr) {Image *i=ptr;cairo_surface_destroy(i->surface);g_free(i->data);g_free(i);}
static gboolean image_unused(gpointer key,gpointer value,gpointer data) {(void)key;return ((Image*)value)->generation!=*(uint64_t*)data;}
static gboolean replay(App *app,cairo_t *c,const uint8_t *data,size_t len) {
    if(len<8 || memcmp(data,"RZ02",4)!=0)return FALSE;
    Reader r={data+4,data+len,TRUE};uint32_t count=u32(&r);int depth=0;app->generation++;
    cairo_save(c);cairo_set_operator(c,CAIRO_OPERATOR_SOURCE);cairo_set_source_rgba(c,0,0,0,0);cairo_paint(c);cairo_set_operator(c,CAIRO_OPERATOR_OVER);
    cairo_set_line_cap(c,CAIRO_LINE_CAP_BUTT);cairo_set_line_join(c,CAIRO_LINE_JOIN_MITER);cairo_set_miter_limit(c,10);cairo_set_fill_rule(c,CAIRO_FILL_RULE_WINDING);
    for(uint32_t i=0;i<count && r.ok;i++) {
        uint32_t op=u32(&r);
        if(op==1){double v[4];rgba(&r,v);cairo_save(c);cairo_set_operator(c,CAIRO_OPERATOR_SOURCE);cairo_set_source_rgba(c,v[0],v[1],v[2],v[3]);cairo_paint(c);cairo_restore(c);}
        else if(op==2){cairo_save(c);depth++;}
        else if(op==3){if(depth<=0){r.ok=FALSE;break;}cairo_restore(c);depth--;}
        else if(op==4){cairo_matrix_t m;m.xx=f32(&r);m.yx=f32(&r);m.xy=f32(&r);m.yy=f32(&r);m.x0=f32(&r);m.y0=f32(&r);cairo_transform(c,&m);}
        else if(op>=5 && op<=7){shape(c,&r);if(op==5)cairo_clip(c);else {source(c,&r);if(op==6)cairo_fill(c);else {cairo_set_line_width(c,f32(&r));cairo_stroke(c);}}}
        else if(op==8 || op==10) {
            double x=f32(&r),y=f32(&r);NativeText text=native_text(&r);
            if(!r.ok){native_text_free(&text);break;}
            cairo_set_source_rgba(c,text.color[0],text.color[1],text.color[2],text.color[3]);
            cairo_move_to(c,x+text.offset_x,y-(op==8?(double)pango_layout_get_baseline(text.layout)/PANGO_SCALE:0));
            pango_cairo_show_layout(c,text.layout);native_text_free(&text);
        } else if(op==9) {
            uint64_t id=u64(&r);uint32_t w=u32(&r),h=u32(&r);double x=f32(&r),y=f32(&r),dw=f32(&r),dh=f32(&r),opacity=f32(&r);uint32_t n=u32(&r);const uint8_t *pixels=bytes(&r,n);
            if(!pixels || !w || !h || w>G_MAXINT/4 || (uint64_t)w*h*4!=n){r.ok=FALSE;break;}
            Image *image=g_hash_table_lookup(app->images,&id);
            if(!image) {
                image=g_new0(Image,1);int stride=cairo_format_stride_for_width(CAIRO_FORMAT_ARGB32,w);image->data=g_malloc0((size_t)stride*h);
                for(uint32_t yy=0;yy<h;yy++)for(uint32_t xx=0;xx<w;xx++){
                    const uint8_t *p=pixels+((size_t)yy*w+xx)*4;uint32_t a=p[3];uint32_t argb=(a<<24)|(((p[0]*a+127)/255)<<16)|(((p[1]*a+127)/255)<<8)|((p[2]*a+127)/255);memcpy(image->data+(size_t)yy*stride+xx*4,&argb,4);
                }
                image->surface=cairo_image_surface_create_for_data(image->data,CAIRO_FORMAT_ARGB32,w,h,stride);uint64_t *key=g_new(uint64_t,1);*key=id;g_hash_table_insert(app->images,key,image);
            }
            image->generation=app->generation;
            if(dw>0 && dh>0){cairo_save(c);cairo_rectangle(c,x,y,dw,dh);cairo_clip(c);cairo_translate(c,x,y);cairo_scale(c,dw/w,dh/h);cairo_set_source_surface(c,image->surface,0,0);cairo_pattern_set_filter(cairo_get_source(c),CAIRO_FILTER_BILINEAR);cairo_pattern_set_extend(cairo_get_source(c),CAIRO_EXTEND_PAD);cairo_paint_with_alpha(c,opacity);cairo_restore(c);}
        } else r.ok=FALSE;
    }
    gboolean balanced=depth==0;while(depth-->0)cairo_restore(c);cairo_restore(c);
    if(r.ok && balanced && r.p==r.end) {
        cairo_save(c);
        if(rusterize_native_draw(app->id,c,app->area,app->window)&RUSTERIZE_FAILED)r.ok=FALSE;
        cairo_restore(c);
    }
    g_hash_table_foreach_remove(app->images,image_unused,&app->generation);
    return r.ok && balanced && r.p==r.end && cairo_status(c)==CAIRO_STATUS_SUCCESS;
}
static void schedule(App *app);
static void event(App *app,uint32_t kind,float x,float y,float dx,float dy,uint32_t detail,uint32_t flags) {rusterize_event(app->id,kind,x,y,dx,dy,detail,flags);schedule(app);}
static gboolean tick(GtkWidget *widget,GdkFrameClock *clock,gpointer ptr) {
    (void)clock;App *app=ptr;uint32_t flags=rusterize_status(app->id);
    if(!(flags&RUSTERIZE_ANIMATE)){app->tick=0;return G_SOURCE_REMOVE;}
    gtk_widget_queue_draw(widget);return G_SOURCE_CONTINUE;
}
static void schedule(App *app) {
    rusterize_poll_native(app->id);
    uint32_t flags=rusterize_status(app->id);
    if(flags&RUSTERIZE_FAILED){app->failed=TRUE;g_printerr("Rusterize: %s\n",rusterize_error(app->id));gtk_window_close(GTK_WINDOW(app->window));return;}
    if(flags&RUSTERIZE_EXIT){gtk_window_close(GTK_WINDOW(app->window));return;}
    update_chrome(app);
    if(flags&RUSTERIZE_REDRAW)gtk_widget_queue_draw(app->area);
    if(flags&RUSTERIZE_ANIMATE){if(!app->tick)app->tick=gtk_widget_add_tick_callback(app->area,tick,app,NULL);}
    else if(app->tick){gtk_widget_remove_tick_callback(app->area,app->tick);app->tick=0;}
}
static void draw(GtkDrawingArea *area,cairo_t *c,int w,int h,gpointer ptr) {
    App *app=ptr;uint32_t flags=rusterize_frame(app->id,w,h,gtk_widget_get_scale_factor(GTK_WIDGET(area)),(g_get_monotonic_time()-app->origin)/1e6);
    if(flags&RUSTERIZE_FAILED){schedule(app);return;}
    if(!replay(app,c,rusterize_data(app->id),rusterize_len(app->id))){app->failed=TRUE;g_printerr("Invalid Rusterize display list\n");gtk_window_close(GTK_WINDOW(app->window));return;}
    schedule(app);
}
static uint32_t mouse_button(guint b){return b==1?1:b==3?2:b==2?4:0;}
static void pressed(GtkGestureClick *gesture,int n,double x,double y,gpointer ptr){(void)n;App *a=ptr;uint32_t b=mouse_button(gtk_gesture_single_get_current_button(GTK_GESTURE_SINGLE(gesture)));a->buttons|=b;gtk_widget_grab_focus(a->area);event(a,1,x,y,0,0,0,a->buttons);}
static void released(GtkGestureClick *gesture,int n,double x,double y,gpointer ptr){(void)n;App *a=ptr;a->buttons&=~mouse_button(gtk_gesture_single_get_current_button(GTK_GESTURE_SINGLE(gesture)));event(a,3,x,y,0,0,0,a->buttons);}
static void cancel(GtkGesture *gesture,GdkEventSequence *seq,gpointer ptr){(void)gesture;(void)seq;App *a=ptr;a->buttons=0;event(a,4,a->pointer_x,a->pointer_y,0,0,0,0);}
static void motion(GtkEventControllerMotion *ctrl,double x,double y,gpointer ptr){(void)ctrl;App *a=ptr;a->pointer_x=x;a->pointer_y=y;event(a,2,x,y,0,0,0,a->buttons);}
static gboolean scroll(GtkEventControllerScroll *ctrl,double dx,double dy,gpointer ptr){(void)ctrl;App *a=ptr;event(a,5,a->pointer_x,a->pointer_y,dx*40,dy*40,0,0);return TRUE;}
static uint32_t modifiers(GdkModifierType s){return ((s&GDK_SHIFT_MASK)?1:0)|((s&GDK_CONTROL_MASK)?2:0)|((s&GDK_ALT_MASK)?4:0)|((s&GDK_META_MASK)?8:0);}
static uint32_t key(guint k){switch(k){case GDK_KEY_Return:case GDK_KEY_KP_Enter:return 13;case GDK_KEY_space:return 32;case GDK_KEY_Escape:return 27;case GDK_KEY_Tab:case GDK_KEY_ISO_Left_Tab:return 9;case GDK_KEY_BackSpace:return 8;case GDK_KEY_Delete:return 46;case GDK_KEY_Left:return 37;case GDK_KEY_Up:return 38;case GDK_KEY_Right:return 39;case GDK_KEY_Down:return 40;case GDK_KEY_Home:return 36;case GDK_KEY_End:return 35;default:return 0x10000+k;}}
static gboolean key_down(GtkEventControllerKey *ctrl,guint val,guint code,GdkModifierType mods,gpointer ptr){(void)ctrl;App *a=ptr;gboolean repeat=g_hash_table_contains(a->keys,GUINT_TO_POINTER(code));g_hash_table_add(a->keys,GUINT_TO_POINTER(code));event(a,6,0,0,0,0,key(val),modifiers(mods)|(repeat?16:0));gunichar ch=gdk_keyval_to_unicode(val);if(ch && !(mods&(GDK_CONTROL_MASK|GDK_ALT_MASK)))event(a,8,0,0,0,0,ch,0);return TRUE;}
static void key_up(GtkEventControllerKey *ctrl,guint val,guint code,GdkModifierType mods,gpointer ptr){(void)ctrl;App *a=ptr;g_hash_table_remove(a->keys,GUINT_TO_POINTER(code));event(a,7,0,0,0,0,key(val),modifiers(mods));}
static void focus_in(GtkEventControllerFocus *ctrl,gpointer ptr){(void)ctrl;event(ptr,9,0,0,0,0,1,0);}
static void focus_out(GtkEventControllerFocus *ctrl,gpointer ptr){(void)ctrl;App *a=ptr;g_hash_table_remove_all(a->keys);event(a,9,0,0,0,0,0,0);}
static void mapped(GtkWidget *widget,gpointer ptr){(void)widget;event(ptr,11,0,0,0,0,0,0);}
static void unmapped(GtkWidget *widget,gpointer ptr){(void)widget;event(ptr,10,0,0,0,0,0,0);}
static void activate(GtkApplication *gtk,gpointer ptr) {
    App *app=ptr;
    app->window=gtk_application_window_new(gtk);gtk_window_set_title(GTK_WINDOW(app->window),rusterize_title(app->id));gtk_window_set_default_size(GTK_WINDOW(app->window),window_float(app,0),window_float(app,1));gtk_window_set_resizable(GTK_WINDOW(app->window),rusterize_window_option(app->id,7)!=0);
    GtkWidget *header=gtk_header_bar_new();gtk_widget_add_css_class(header,"rusterize-chrome");gtk_window_set_titlebar(GTK_WINDOW(app->window),header);
    app->chrome=gtk_css_provider_new();gtk_style_context_add_provider_for_display(gtk_widget_get_display(app->window),GTK_STYLE_PROVIDER(app->chrome),GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);update_chrome(app);
    app->area=gtk_drawing_area_new();gtk_widget_set_size_request(app->area,window_float(app,2),window_float(app,3));gtk_widget_set_focusable(app->area,TRUE);gtk_drawing_area_set_draw_func(GTK_DRAWING_AREA(app->area),draw,app,NULL);gtk_window_set_child(GTK_WINDOW(app->window),app->area);
    GtkGesture *click=gtk_gesture_click_new();gtk_gesture_single_set_button(GTK_GESTURE_SINGLE(click),0);g_signal_connect(click,"pressed",G_CALLBACK(pressed),app);g_signal_connect(click,"released",G_CALLBACK(released),app);g_signal_connect(click,"cancel",G_CALLBACK(cancel),app);gtk_widget_add_controller(app->area,GTK_EVENT_CONTROLLER(click));
    GtkEventController *move=gtk_event_controller_motion_new();g_signal_connect(move,"motion",G_CALLBACK(motion),app);gtk_widget_add_controller(app->area,move);
    GtkEventController *wheel=gtk_event_controller_scroll_new(GTK_EVENT_CONTROLLER_SCROLL_BOTH_AXES);g_signal_connect(wheel,"scroll",G_CALLBACK(scroll),app);gtk_widget_add_controller(app->area,wheel);
    GtkEventController *keys=gtk_event_controller_key_new();g_signal_connect(keys,"key-pressed",G_CALLBACK(key_down),app);g_signal_connect(keys,"key-released",G_CALLBACK(key_up),app);gtk_widget_add_controller(app->area,keys);
    GtkEventController *focus=gtk_event_controller_focus_new();g_signal_connect(focus,"enter",G_CALLBACK(focus_in),app);g_signal_connect(focus,"leave",G_CALLBACK(focus_out),app);gtk_widget_add_controller(app->area,focus);
    g_signal_connect(app->area,"map",G_CALLBACK(mapped),app);g_signal_connect(app->area,"unmap",G_CALLBACK(unmapped),app);
    gtk_window_present(GTK_WINDOW(app->window));gtk_widget_grab_focus(app->area);
}
int main(int argc,char **argv) {
    App app={0};service_app=&app;rusterize_set_native_service(native_service);app.id=rusterize_create();app.origin=g_get_monotonic_time();app.images=g_hash_table_new_full(g_int64_hash,g_int64_equal,g_free,image_free);app.keys=g_hash_table_new(g_direct_hash,g_direct_equal);
    if(rusterize_status(app.id)&RUSTERIZE_FAILED){g_printerr("%s\n",rusterize_error(app.id));rusterize_destroy(app.id);g_hash_table_destroy(app.images);g_hash_table_destroy(app.keys);return 1;}
    if(argc==3 && strcmp(argv[1],"--snapshot")==0){
        cairo_surface_t *surface=cairo_image_surface_create(CAIRO_FORMAT_ARGB32,960,680);cairo_t *context=cairo_create(surface);
        uint32_t status=rusterize_frame(app.id,960,680,1,0);gboolean ok=!(status&RUSTERIZE_FAILED)&&replay(&app,context,rusterize_data(app.id),rusterize_len(app.id));
        if(ok)ok=cairo_surface_write_to_png(surface,argv[2])==CAIRO_STATUS_SUCCESS;
        cairo_destroy(context);cairo_surface_destroy(surface);g_hash_table_destroy(app.images);g_hash_table_destroy(app.keys);rusterize_destroy(app.id);return ok?0:1;
    }
    GtkApplication *gtk=gtk_application_new("dev.rusterize.app",G_APPLICATION_NON_UNIQUE);g_signal_connect(gtk,"activate",G_CALLBACK(activate),&app);
    int code=g_application_run(G_APPLICATION(gtk),argc,argv);
    if(app.chrome){GdkDisplay *display=gdk_display_get_default();if(display)gtk_style_context_remove_provider_for_display(display,GTK_STYLE_PROVIDER(app.chrome));g_object_unref(app.chrome);}
    g_object_unref(gtk);g_hash_table_destroy(app.images);g_hash_table_destroy(app.keys);rusterize_destroy(app.id);return app.failed?1:code;
}
