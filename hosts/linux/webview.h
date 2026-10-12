/* Included only for the optional webview feature; WebKitGTK 6 uses GTK 4. */
#include <webkit/webkit.h>

typedef struct {
    uint32_t id; WebKitWebView *web; gboolean alive,javascript,failed;
    gint refs; GCancellable *cancel;
} WebEntry;
typedef struct { WebEntry *entry; uint32_t token; } WebScript;
static GHashTable *webviews;
static GQueue web_events=G_QUEUE_INIT;
static GtkWidget *web_layer;
static void web_unref(WebEntry *e) {
    if(--e->refs==0){g_object_unref(e->cancel);g_object_unref(e->web);g_free(e);}
}
static void web_free(gpointer data) {
    WebEntry *e=data;e->alive=FALSE;g_cancellable_cancel(e->cancel);
    g_signal_handlers_disconnect_by_data(e->web,e);webkit_web_view_stop_loading(e->web);
    GtkWidget *parent=gtk_widget_get_parent(GTK_WIDGET(e->web));
    if(parent)gtk_fixed_remove(GTK_FIXED(parent),GTK_WIDGET(e->web));
    web_unref(e);
}
static void web_emit(WebEntry *e,uint32_t kind,uint32_t token,const char *value) {
    if(!e->alive)return;
    GByteArray *out=g_byte_array_new();put_u32(out,kind);put_u32(out,e->id);put_u32(out,token);put_string(out,value?value:"");
    g_queue_push_tail(&web_events,out);gtk_widget_queue_draw(service_app->area);
}
static gboolean web_allowed(const char *url) {
    return url && (g_str_has_prefix(url,"https://")||g_str_has_prefix(url,"http://")||strcmp(url,"about:blank")==0);
}
static gboolean web_policy(WebKitWebView *web,WebKitPolicyDecision *decision,WebKitPolicyDecisionType type,gpointer data) {
    (void)web;WebEntry *e=data;
    if(type==WEBKIT_POLICY_DECISION_TYPE_NEW_WINDOW_ACTION){webkit_policy_decision_ignore(decision);return TRUE;}
    if(type==WEBKIT_POLICY_DECISION_TYPE_NAVIGATION_ACTION) {
        WebKitNavigationAction *action=webkit_navigation_policy_decision_get_navigation_action(WEBKIT_NAVIGATION_POLICY_DECISION(decision));
        const char *uri=webkit_uri_request_get_uri(webkit_navigation_action_get_request(action));
        if(!web_allowed(uri)){webkit_policy_decision_ignore(decision);web_emit(e,3,0,"Only HTTP(S) and about:blank navigation is allowed");return TRUE;}
    }
    return FALSE;
}
static void web_loaded(WebKitWebView *web,WebKitLoadEvent event,gpointer data) {
    WebEntry *e=data;if(event==WEBKIT_LOAD_STARTED)e->failed=FALSE;
    if(event==WEBKIT_LOAD_FINISHED && !e->failed)web_emit(e,2,0,webkit_web_view_get_uri(web));
}
static gboolean web_failed(WebKitWebView *web,WebKitLoadEvent event,const char *uri,GError *error,gpointer data) {
    (void)web;(void)event;(void)uri;WebEntry *e=data;e->failed=TRUE;web_emit(e,3,0,error->message);return TRUE;
}
static void web_script_finished(GObject *source,GAsyncResult *result,gpointer data) {
    WebScript *script=data;GError *error=NULL;
    JSCValue *value=webkit_web_view_evaluate_javascript_finish(WEBKIT_WEB_VIEW(source),result,&error);
    if(error){web_emit(script->entry,5,script->token,error->message);g_error_free(error);}
    else {
        char *json=value?jsc_value_to_json(value,0):NULL;web_emit(script->entry,4,script->token,json?json:"null");g_free(json);
    }
    if(value){g_object_unref(value);}
    web_unref(script->entry);g_free(script);
}
static void web_place(WebEntry *e,Reader *r,gboolean create) {
    double x=f32(r),y=f32(r);int width=(int)round(f32(r)),height=(int)round(f32(r));
    gtk_widget_set_size_request(GTK_WIDGET(e->web),width,height);
    if(create)gtk_fixed_put(GTK_FIXED(web_layer),GTK_WIDGET(e->web),x,y);
    else gtk_fixed_move(GTK_FIXED(web_layer),GTK_WIDGET(e->web),x,y);
}
static void web_purge(uint32_t id) {
    for(GList *it=web_events.head;it;) {
        GList *next=it->next;GByteArray *event=it->data;Reader r={event->data+4,event->data+event->len,TRUE};
        if(u32(&r)==id){g_queue_delete_link(&web_events,it);g_byte_array_unref(event);}it=next;
    }
}
static char *web_command(uint32_t op,Reader *r,GByteArray *out) {
    if(op==21) {
        GByteArray *event=g_queue_pop_head(&web_events);
        if(event){g_byte_array_append(out,event->data,event->len);g_byte_array_unref(event);}else put_u32(out,0);
        return NULL;
    }
    uint32_t id=u32(r),action=u32(r);
    if(!webviews)webviews=g_hash_table_new_full(g_direct_hash,g_direct_equal,NULL,web_free);
    WebEntry *e=g_hash_table_lookup(webviews,GUINT_TO_POINTER(id));
    if(action==1) {
        if(e)return g_strdup("WebView ID already exists");
        if(!web_layer)return g_strdup("WebView container is not ready");
        e=g_new0(WebEntry,1);e->id=id;e->alive=TRUE;e->refs=1;e->cancel=g_cancellable_new();
        e->web=WEBKIT_WEB_VIEW(webkit_web_view_new());g_object_ref_sink(e->web);
        web_place(e,r,TRUE);gboolean visible=u32(r);e->javascript=u32(r);
        WebKitSettings *settings=webkit_web_view_get_settings(e->web);
        webkit_settings_set_enable_javascript(settings,e->javascript);
        webkit_settings_set_enable_javascript_markup(settings,e->javascript);
        webkit_settings_set_allow_file_access_from_file_urls(settings,FALSE);
        webkit_settings_set_allow_universal_access_from_file_urls(settings,FALSE);
        g_signal_connect(e->web,"decide-policy",G_CALLBACK(web_policy),e);
        g_signal_connect(e->web,"load-changed",G_CALLBACK(web_loaded),e);
        g_signal_connect(e->web,"load-failed",G_CALLBACK(web_failed),e);
        g_hash_table_insert(webviews,GUINT_TO_POINTER(id),e);gtk_widget_set_visible(GTK_WIDGET(e->web),visible);
        web_emit(e,1,0,"");return NULL;
    }
    if(!e)return g_strdup("Unknown WebView ID");
    if(action==2){g_hash_table_remove(webviews,GUINT_TO_POINTER(id));web_purge(id);}
    else if(action==3)web_place(e,r,FALSE);
    else if(action==4)gtk_widget_set_visible(GTK_WIDGET(e->web),u32(r)!=0);
    else if(action==5 || action==6) {
        char *value=string(r);if(action==5)webkit_web_view_load_uri(e->web,value);else webkit_web_view_load_html(e->web,value,"about:blank");g_free(value);
    } else if(action==7) {
        if(!e->javascript)return g_strdup("JavaScript is disabled");
        WebScript *script=g_new0(WebScript,1);script->entry=e;script->token=u32(r);e->refs++;
        char *value=string(r);webkit_web_view_evaluate_javascript(e->web,value,-1,NULL,NULL,e->cancel,web_script_finished,script);g_free(value);
    } else if(action==8)gtk_widget_grab_focus(GTK_WIDGET(e->web));
    else if(action==9)webkit_web_view_reload(e->web);
    else if(action==10)webkit_web_view_go_back(e->web);
    else if(action==11)webkit_web_view_go_forward(e->web);
    else return g_strdup("Unknown WebView command");
    return NULL;
}
static void web_close(void) {
    if(webviews){g_hash_table_destroy(webviews);webviews=NULL;}
    g_queue_clear_full(&web_events,(GDestroyNotify)g_byte_array_unref);web_layer=NULL;
}
static gboolean web_window_closing(GtkWindow *window,gpointer data) {
    (void)window;(void)data;web_close();return FALSE;
}
