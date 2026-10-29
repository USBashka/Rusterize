/* GLES composition stays in GtkGLArea's context. Cairo supplies only ordinary
 * drawing layers and coverage masks; shader pixels never pass through Cairo. */
typedef struct {GLuint program;} GpuShader;
typedef struct Gpu {
    GLuint composite,vao,uniforms,framebuffer,output,layer,mask,white;
    GLint target,max_size;
    int width,height,output_width,output_height;
    double scale;
    guint shader_draws;
    cairo_surface_t *surface;
} Gpu;

static gboolean gpu_ok(const char *operation) {
    GLenum error=glGetError();
    if(error==GL_NO_ERROR)return TRUE;
    g_printerr("Rusterize GPU %s: GL error 0x%x\n",operation,error);return FALSE;
}
static GLuint gpu_program(const char *vertex,const char *fragment) {
    GLuint program=glCreateProgram();const char *sources[]={vertex,fragment};
    const GLenum kinds[]={GL_VERTEX_SHADER,GL_FRAGMENT_SHADER};
    for(int i=0;i<2;i++) {
        GLuint shader=glCreateShader(kinds[i]);glShaderSource(shader,1,&sources[i],NULL);glCompileShader(shader);
        GLint ok=0;glGetShaderiv(shader,GL_COMPILE_STATUS,&ok);
        if(!ok){char log[4096];glGetShaderInfoLog(shader,sizeof log,NULL,log);g_printerr("Rusterize GPU shader: %s\n",log);glDeleteShader(shader);glDeleteProgram(program);return 0;}
        glAttachShader(program,shader);glDeleteShader(shader);
    }
    glLinkProgram(program);GLint ok=0;glGetProgramiv(program,GL_LINK_STATUS,&ok);
    if(!ok){char log[4096];glGetProgramInfoLog(program,sizeof log,NULL,log);g_printerr("Rusterize GPU link: %s\n",log);glDeleteProgram(program);return 0;}
    return program;
}
static void gpu_shader_free(gpointer ptr) {
    GpuShader *shader=ptr;
    if(shader->program && gdk_gl_context_get_current())glDeleteProgram(shader->program);
    g_free(shader);
}
static GLuint gpu_texture(void) {
    GLuint texture=0;glGenTextures(1,&texture);glBindTexture(GL_TEXTURE_2D,texture);
    glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_LINEAR);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_CLAMP_TO_EDGE);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_CLAMP_TO_EDGE);
    return texture;
}
static gboolean gpu_init(App *app) {
    if(app->gpu)return TRUE;
    Gpu *gpu=g_new0(Gpu,1);app->gpu=gpu;
    static const char *vertex=
        "#version 300 es\nprecision highp float;out vec2 uv;"
        "void main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2));"
        "gl_Position=vec4(p*2.0-1.0,0,1);uv=vec2(p.x,1.0-p.y);}";
    static const char *fragment=
        "#version 300 es\nprecision highp float;in vec2 uv;out vec4 color;"
        "uniform sampler2D image;uniform sampler2D mask;uniform int mode;"
        "uniform mat3 inverseTransform;uniform vec4 bounds;uniform vec2 viewport;"
        "void main(){if(mode==0){color=texture(image,uv).bgra;}else{"
        "vec2 p=(inverseTransform*vec3(uv*viewport,1)).xy;"
        "vec2 q=clamp((p-bounds.xy)/bounds.zw,vec2(0),vec2(1));"
        "vec4 c=texture(image,vec2(q.x,1.0-q.y));"
        "float coverage=texture(mask,uv).a;"
        "color=vec4(c.rgb*c.a,c.a)*coverage;}}";
    gpu->composite=gpu_program(vertex,fragment);if(!gpu->composite)return FALSE;
    glGenVertexArrays(1,&gpu->vao);glGenBuffers(1,&gpu->uniforms);glGenFramebuffers(1,&gpu->framebuffer);
    gpu->output=gpu_texture();gpu->layer=gpu_texture();gpu->mask=gpu_texture();gpu->white=gpu_texture();
    const guint8 white[]={255,255,255,255};glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA8,1,1,0,GL_RGBA,GL_UNSIGNED_BYTE,white);
    glGetIntegerv(GL_MAX_TEXTURE_SIZE,&gpu->max_size);
    glUseProgram(gpu->composite);glUniform1i(glGetUniformLocation(gpu->composite,"image"),0);glUniform1i(glGetUniformLocation(gpu->composite,"mask"),1);
    g_print("RUSTERIZE_GPU_COMPOSITION gtk-gles %s / %s\n",glGetString(GL_RENDERER),glGetString(GL_VERSION));
    return gpu_ok("initialization");
}
static void gpu_destroy(App *app) {
    Gpu *gpu=app->gpu;if(!gpu)return;
    g_hash_table_remove_all(app->shaders);g_hash_table_remove_all(app->images);
    if(gdk_gl_context_get_current()) {
        glDeleteProgram(gpu->composite);glDeleteVertexArrays(1,&gpu->vao);glDeleteBuffers(1,&gpu->uniforms);glDeleteFramebuffers(1,&gpu->framebuffer);
        GLuint textures[]={gpu->output,gpu->layer,gpu->mask,gpu->white};glDeleteTextures(4,textures);
    }
    if(gpu->surface)cairo_surface_destroy(gpu->surface);
    g_free(gpu);app->gpu=NULL;
}
static gboolean gpu_begin(App *app,int width,int height,double scale) {
    if(!gpu_init(app))return FALSE;
    Gpu *gpu=app->gpu;
    gpu->shader_draws=0;
    if(width<=0 || height<=0 || width>gpu->max_size || height>gpu->max_size)return FALSE;
    glGetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING,&gpu->target);
    if(gpu->width!=width || gpu->height!=height || gpu->scale!=scale) {
        if(gpu->surface)cairo_surface_destroy(gpu->surface);
        gpu->surface=cairo_image_surface_create(CAIRO_FORMAT_ARGB32,width,height);
        if(cairo_surface_status(gpu->surface)!=CAIRO_STATUS_SUCCESS)return FALSE;
        cairo_surface_set_device_scale(gpu->surface,scale,scale);
        gpu->width=width;gpu->height=height;gpu->scale=scale;
        for(int i=0;i<2;i++){glBindTexture(GL_TEXTURE_2D,i?gpu->mask:gpu->layer);glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA8,width,height,0,GL_RGBA,GL_UNSIGNED_BYTE,NULL);}
    }
    glBindVertexArray(gpu->vao);glViewport(0,0,width,height);
    glDisable(GL_DEPTH_TEST);glDisable(GL_STENCIL_TEST);glDisable(GL_SCISSOR_TEST);glDisable(GL_CULL_FACE);glDisable(GL_DITHER);
    glColorMask(GL_TRUE,GL_TRUE,GL_TRUE,GL_TRUE);glClearColor(0,0,0,0);glClear(GL_COLOR_BUFFER_BIT);
    return gpu_ok("frame begin");
}
static void gpu_clear_layer(Gpu *gpu) {
    cairo_surface_flush(gpu->surface);
    memset(cairo_image_surface_get_data(gpu->surface),0,(size_t)cairo_image_surface_get_stride(gpu->surface)*gpu->height);
    cairo_surface_mark_dirty(gpu->surface);
}
static void gpu_upload_layer(Gpu *gpu,GLuint texture) {
    cairo_surface_flush(gpu->surface);glActiveTexture(GL_TEXTURE0);glBindTexture(GL_TEXTURE_2D,texture);
    glPixelStorei(GL_UNPACK_ALIGNMENT,4);glPixelStorei(GL_UNPACK_ROW_LENGTH,0);
    glTexSubImage2D(GL_TEXTURE_2D,0,0,0,gpu->width,gpu->height,GL_RGBA,GL_UNSIGNED_BYTE,cairo_image_surface_get_data(gpu->surface));
}
static void gpu_composite_state(Gpu *gpu) {
    glBindFramebuffer(GL_FRAMEBUFFER,(GLuint)gpu->target);glViewport(0,0,gpu->width,gpu->height);
    glDisable(GL_DEPTH_TEST);glDisable(GL_STENCIL_TEST);glDisable(GL_SCISSOR_TEST);glDisable(GL_CULL_FACE);glDisable(GL_DITHER);glColorMask(GL_TRUE,GL_TRUE,GL_TRUE,GL_TRUE);
    glBindVertexArray(gpu->vao);glUseProgram(gpu->composite);glEnable(GL_BLEND);glBlendEquation(GL_FUNC_ADD);glBlendFunc(GL_ONE,GL_ONE_MINUS_SRC_ALPHA);
}
static gboolean gpu_flush_layer(App *app) {
    Gpu *gpu=app->gpu;gpu_upload_layer(gpu,gpu->layer);gpu_composite_state(gpu);
    glUniform1i(glGetUniformLocation(gpu->composite,"mode"),0);glDrawArrays(GL_TRIANGLES,0,3);
    gpu_clear_layer(gpu);return gpu_ok("Cairo layer");
}
static gboolean gpu_shader_upload(App *app,Reader *reader,uint64_t id) {
    char *vertex=string(reader),*fragment=string(reader),*block=string(reader),*sampler=string(reader);
    GLuint program=reader->ok?gpu_program(vertex,fragment):0;
    if(program) {
        glUseProgram(program);
        if(*block){GLuint index=glGetUniformBlockIndex(program,block);if(index!=GL_INVALID_INDEX)glUniformBlockBinding(program,index,0);}
        if(*sampler)glUniform1i(glGetUniformLocation(program,sampler),0);
        GpuShader *shader=g_new0(GpuShader,1);shader->program=program;
        uint64_t *key=g_new(uint64_t,1);*key=id;g_hash_table_replace(app->shaders,key,shader);
    }
    g_free(vertex);g_free(fragment);g_free(block);g_free(sampler);return program && gpu_ok("resource upload");
}
static gboolean gpu_shader_draw(App *app,cairo_t *context,Reader *reader) {
    uint64_t id=u64(reader);float values[20];for(int i=0;i<20;i++)values[i]=f32(reader);
    uint64_t input=u64(reader);float bounds[4];for(int i=0;i<4;i++)bounds[i]=f32(reader);
    Gpu *gpu=app->gpu;GpuShader *shader=g_hash_table_lookup(app->shaders,&id);Image *image=input?g_hash_table_lookup(app->images,&input):NULL;
    if(!reader->ok || !shader || (input && !image))return FALSE;
    for(int i=0;i<20;i++)if(!isfinite(values[i]))return FALSE;
    for(int i=0;i<4;i++)if(!isfinite(bounds[i]))return FALSE;
    if(values[0]<1 || values[1]<1 || values[0]>MIN(gpu->max_size,4096) || values[1]>MIN(gpu->max_size,4096) || values[0]*values[1]>4*1024*1024 || values[3]<=0 || bounds[2]<=0 || bounds[3]<=0)return FALSE;
    int width=(int)values[0],height=(int)values[1];
    if(!gpu_flush_layer(app))return FALSE;
    cairo_matrix_t inverse;cairo_get_matrix(context,&inverse);
    if(cairo_matrix_invert(&inverse)!=CAIRO_STATUS_SUCCESS)return FALSE;
    cairo_save(context);cairo_new_path(context);cairo_rectangle(context,bounds[0],bounds[1],bounds[2],bounds[3]);cairo_set_source_rgba(context,1,1,1,1);cairo_fill(context);cairo_restore(context);
    gpu_upload_layer(gpu,gpu->mask);gpu_clear_layer(gpu);
    glActiveTexture(GL_TEXTURE0);
    if(image && !image->texture) {
        int iw=cairo_image_surface_get_width(image->surface),ih=cairo_image_surface_get_height(image->surface);
        if(iw>gpu->max_size || ih>gpu->max_size)return FALSE;
        image->texture=gpu_texture();glPixelStorei(GL_UNPACK_ALIGNMENT,1);
        glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA8,iw,ih,0,GL_RGBA,GL_UNSIGNED_BYTE,g_bytes_get_data(image->rgba,NULL));
    }
    if(gpu->output_width!=width || gpu->output_height!=height) {
        glBindTexture(GL_TEXTURE_2D,gpu->output);glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA8,width,height,0,GL_RGBA,GL_UNSIGNED_BYTE,NULL);
        gpu->output_width=width;gpu->output_height=height;
    }
    glBindFramebuffer(GL_FRAMEBUFFER,gpu->framebuffer);glFramebufferTexture2D(GL_FRAMEBUFFER,GL_COLOR_ATTACHMENT0,GL_TEXTURE_2D,gpu->output,0);
    if(glCheckFramebufferStatus(GL_FRAMEBUFFER)!=GL_FRAMEBUFFER_COMPLETE)return FALSE;
    glUseProgram(shader->program);glBindVertexArray(gpu->vao);glBindBuffer(GL_UNIFORM_BUFFER,gpu->uniforms);glBufferData(GL_UNIFORM_BUFFER,sizeof values,values,GL_DYNAMIC_DRAW);glBindBufferBase(GL_UNIFORM_BUFFER,0,gpu->uniforms);
    glBindTexture(GL_TEXTURE_2D,image?image->texture:gpu->white);glDisable(GL_BLEND);glViewport(0,0,width,height);glClearColor(0,0,0,0);glClear(GL_COLOR_BUFFER_BIT);glDrawArrays(GL_TRIANGLES,0,3);
    gpu_composite_state(gpu);
    const float matrix[]={(float)inverse.xx,(float)inverse.yx,0,(float)inverse.xy,(float)inverse.yy,0,(float)inverse.x0,(float)inverse.y0,1};
    glUniform1i(glGetUniformLocation(gpu->composite,"mode"),1);glUniformMatrix3fv(glGetUniformLocation(gpu->composite,"inverseTransform"),1,GL_FALSE,matrix);glUniform4fv(glGetUniformLocation(gpu->composite,"bounds"),1,bounds);
    glUniform2f(glGetUniformLocation(gpu->composite,"viewport"),(float)(gpu->width/gpu->scale),(float)(gpu->height/gpu->scale));
    glActiveTexture(GL_TEXTURE0);glBindTexture(GL_TEXTURE_2D,gpu->output);glActiveTexture(GL_TEXTURE1);glBindTexture(GL_TEXTURE_2D,gpu->mask);glDrawArrays(GL_TRIANGLES,0,3);glActiveTexture(GL_TEXTURE0);
    gpu->shader_draws++;
    return gpu_ok("shader composition");
}

/* Readback is deliberately restricted to an explicitly requested PNG export. */
static gboolean gpu_snapshot(App *app,const char *path) {
    Gpu *gpu=app->gpu;size_t size=(size_t)gpu->width*gpu->height*4;guint8 *pixels=g_try_malloc(size);if(!pixels)return FALSE;
    glBindFramebuffer(GL_FRAMEBUFFER,(GLuint)gpu->target);glPixelStorei(GL_PACK_ALIGNMENT,1);glReadPixels(0,0,gpu->width,gpu->height,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
    gboolean ok=gpu_ok("snapshot");cairo_surface_t *surface=cairo_image_surface_create(CAIRO_FORMAT_ARGB32,gpu->width,gpu->height);guint8 *target=cairo_image_surface_get_data(surface);int stride=cairo_image_surface_get_stride(surface);
    if(cairo_surface_status(surface)!=CAIRO_STATUS_SUCCESS)ok=FALSE;
    if(ok)for(int y=0;y<gpu->height;y++)for(int x=0;x<gpu->width;x++) {
        guint8 *p=pixels+((size_t)(gpu->height-1-y)*gpu->width+x)*4;uint32_t argb=((uint32_t)p[3]<<24)|((uint32_t)p[0]<<16)|((uint32_t)p[1]<<8)|p[2];memcpy(target+(size_t)y*stride+x*4,&argb,4);
    }
    cairo_surface_mark_dirty(surface);if(ok)ok=cairo_surface_write_to_png(surface,path)==CAIRO_STATUS_SUCCESS;
    cairo_surface_destroy(surface);g_free(pixels);return ok;
}
