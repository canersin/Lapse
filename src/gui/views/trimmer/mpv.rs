//! Runtime-loaded libmpv player for GPU video preview in the trimmer.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use eframe::egui::{self, Rect};
use eframe::egui_glow;
use eframe::glow::{self, HasContext};

// --- FFI Types & Constants (matching mpv client.h, render.h, render_gl.h) ---

pub type MpvHandle = c_void;
pub type MpvRenderContext = c_void;

pub const MPV_FORMAT_DOUBLE: c_int = 5;

/// `mpv_render_context_update` flag: a new video frame must be rendered.
const MPV_RENDER_UPDATE_FRAME: u64 = 1;
const MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;
#[allow(dead_code)]
pub const MPV_RENDER_PARAM_INVALID: c_int = 0;
#[allow(dead_code)]
pub const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
#[allow(dead_code)]
pub const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
#[allow(dead_code)]
pub const MPV_RENDER_PARAM_OPENGL_FBO: c_int = 3;
#[allow(dead_code)]
pub const MPV_RENDER_PARAM_FLIP_Y: c_int = 4;
#[allow(dead_code)]
pub const MPV_RENDER_PARAM_X11_DISPLAY: c_int = 8;
#[allow(dead_code)]
pub const MPV_RENDER_PARAM_WL_DISPLAY: c_int = 9;
/// Lets the decoder ask the render thread for a hardware device (needed for zero-copy VAAPI).
pub const MPV_RENDER_PARAM_ADVANCED_CONTROL: c_int = 10;

#[allow(dead_code)]
pub const MPV_RENDER_API_TYPE_OPENGL: &[u8] = b"opengl\0";

#[repr(C)]
pub struct MpvRenderParam {
    pub type_: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct MpvOpenglInitParams {
    pub get_proc_address: Option<unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *mut c_void>,
    pub get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MpvOpenglFbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}

#[allow(dead_code)]
pub struct MpvFfi {
    _lib: libloading::Library,
    pub create: unsafe extern "C" fn() -> *mut MpvHandle,
    pub initialize: unsafe extern "C" fn(ctx: *mut MpvHandle) -> c_int,
    pub set_option_string: unsafe extern "C" fn(ctx: *mut MpvHandle, name: *const c_char, data: *const c_char) -> c_int,
    pub set_property_string: unsafe extern "C" fn(ctx: *mut MpvHandle, name: *const c_char, data: *const c_char) -> c_int,
    pub get_property: unsafe extern "C" fn(ctx: *mut MpvHandle, name: *const c_char, format: c_int, data: *mut c_void) -> c_int,
    pub command: unsafe extern "C" fn(ctx: *mut MpvHandle, args: *mut *const c_char) -> c_int,
    pub terminate_destroy: unsafe extern "C" fn(ctx: *mut MpvHandle),
    pub free: unsafe extern "C" fn(data: *mut c_void),
    pub error_string: unsafe extern "C" fn(error: c_int) -> *const c_char,
    pub render_context_create: unsafe extern "C" fn(res: *mut *mut MpvRenderContext, mpv: *mut MpvHandle, params: *mut MpvRenderParam) -> c_int,
    pub render_context_render: unsafe extern "C" fn(ctx: *mut MpvRenderContext, params: *mut MpvRenderParam) -> c_int,
    pub render_context_set_update_callback: unsafe extern "C" fn(ctx: *mut MpvRenderContext, callback: unsafe extern "C" fn(*mut c_void), callback_ctx: *mut c_void),
    pub render_context_free: unsafe extern "C" fn(ctx: *mut MpvRenderContext),
    pub render_context_update: unsafe extern "C" fn(ctx: *mut MpvRenderContext) -> u64,
}

impl MpvFfi {
    fn load(lib: libloading::Library) -> Option<Self> {
        // SAFETY: Binding dynamic function symbols from loaded libmpv library.
        unsafe {
            let create = *lib.get::<unsafe extern "C" fn() -> *mut MpvHandle>(b"mpv_create\0").ok()?;
            let initialize = *lib.get::<unsafe extern "C" fn(*mut MpvHandle) -> c_int>(b"mpv_initialize\0").ok()?;
            let set_option_string = *lib.get::<unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int>(b"mpv_set_option_string\0").ok()?;
            let set_property_string = *lib.get::<unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int>(b"mpv_set_property_string\0").ok()?;
            let get_property = *lib.get::<unsafe extern "C" fn(*mut MpvHandle, *const c_char, c_int, *mut c_void) -> c_int>(b"mpv_get_property\0").ok()?;
            let command = *lib.get::<unsafe extern "C" fn(*mut MpvHandle, *mut *const c_char) -> c_int>(b"mpv_command\0").ok()?;
            let terminate_destroy = *lib.get::<unsafe extern "C" fn(*mut MpvHandle)>(b"mpv_terminate_destroy\0").ok()?;
            let free = *lib.get::<unsafe extern "C" fn(*mut c_void)>(b"mpv_free\0").ok()?;
            let error_string = *lib.get::<unsafe extern "C" fn(c_int) -> *const c_char>(b"mpv_error_string\0").ok()?;
            let render_context_create = *lib.get::<unsafe extern "C" fn(*mut *mut MpvRenderContext, *mut MpvHandle, *mut MpvRenderParam) -> c_int>(b"mpv_render_context_create\0").ok()?;
            let render_context_render = *lib.get::<unsafe extern "C" fn(*mut MpvRenderContext, *mut MpvRenderParam) -> c_int>(b"mpv_render_context_render\0").ok()?;
            let render_context_set_update_callback = *lib.get::<unsafe extern "C" fn(*mut MpvRenderContext, unsafe extern "C" fn(*mut c_void), *mut c_void)>(b"mpv_render_context_set_update_callback\0").ok()?;
            let render_context_free = *lib.get::<unsafe extern "C" fn(*mut MpvRenderContext)>(b"mpv_render_context_free\0").ok()?;
            let render_context_update = *lib.get::<unsafe extern "C" fn(*mut MpvRenderContext) -> u64>(b"mpv_render_context_update\0").ok()?;

            Some(Self {
                _lib: lib,
                create,
                initialize,
                set_option_string,
                set_property_string,
                get_property,
                command,
                terminate_destroy,
                free,
                error_string,
                render_context_create,
                render_context_render,
                render_context_set_update_callback,
                render_context_free,
                render_context_update,
            })
        }
    }
}

// --- Dynamic Library Loading ---

pub fn mpv_plugin_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "lapse", "lapse")
        .map(|d| d.data_dir().join("plugins"))
        .unwrap_or_else(|| PathBuf::from("plugins"))
}

fn try_load() -> Option<Arc<MpvFfi>> {
    #[cfg(windows)]
    const NAMES: &[&str] = &["libmpv-2.dll"];
    #[cfg(not(windows))]
    const NAMES: &[&str] = &["libmpv.so.2", "libmpv.so"];

    let plugin_dir = mpv_plugin_dir();
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));

    // 1. First try <data dir>/plugins/
    for name in NAMES {
        let path = plugin_dir.join(name);
        if path.exists() {
            // SAFETY: Loading dynamic library from explicit user plugins folder.
            if let Ok(lib) = unsafe { libloading::Library::new(&path) } {
                if let Some(ffi) = MpvFfi::load(lib) {
                    return Some(Arc::new(ffi));
                }
            }
        }
    }

    // 2. Next to the executable
    if let Some(dir) = &exe_dir {
        for name in NAMES {
            let path = dir.join(name);
            if path.exists() {
                // SAFETY: Loading dynamic library located in application binary folder.
                if let Ok(lib) = unsafe { libloading::Library::new(&path) } {
                    if let Some(ffi) = MpvFfi::load(lib) {
                        return Some(Arc::new(ffi));
                    }
                }
            }
        }
    }

    // 3. System search path
    for name in NAMES {
        // SAFETY: Loading dynamic library using system search path resolution.
        if let Ok(lib) = unsafe { libloading::Library::new(name) } {
            if let Some(ffi) = MpvFfi::load(lib) {
                return Some(Arc::new(ffi));
            }
        }
    }

    None
}

pub fn get_mpv() -> Option<Arc<MpvFfi>> {
    static MPV: OnceLock<Option<Arc<MpvFfi>>> = OnceLock::new();
    MPV.get_or_init(try_load).clone()
}

pub fn mpv_available() -> bool {
    get_mpv().is_some()
}

// --- Platform Native Display ---

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub enum NativeDisplay {
    None,
    Wayland(usize),
    X11(usize),
}

#[allow(dead_code)]
static NATIVE_DISPLAY: OnceLock<NativeDisplay> = OnceLock::new();

#[allow(dead_code)]
pub fn set_native_display(nd: NativeDisplay) {
    let _ = NATIVE_DISPLAY.set(nd);
}

#[allow(dead_code)]
pub fn get_native_display() -> NativeDisplay {
    NATIVE_DISPLAY.get().copied().unwrap_or(NativeDisplay::None)
}

// --- OpenGL Procedure Address Resolver ---

#[cfg(not(windows))]
struct GlProcLoader {
    egl_get_proc_address: Option<unsafe extern "C" fn(*const c_char) -> *mut c_void>,
    glx_get_proc_address: Option<unsafe extern "C" fn(*const c_char) -> *mut c_void>,
    lib_egl: Option<libloading::Library>,
    lib_gl: Option<libloading::Library>,
}

#[cfg(not(windows))]
impl GlProcLoader {
    fn new() -> Self {
        // SAFETY: Loading libEGL shared library for eglGetProcAddress symbol resolution.
        let lib_egl = unsafe {
            libloading::Library::new("libEGL.so.1")
                .or_else(|_| libloading::Library::new("libEGL.so"))
                .ok()
        };
        let egl_get_proc_address = lib_egl.as_ref().and_then(|lib| {
            // SAFETY: Resolving eglGetProcAddress function pointer from libEGL.
            unsafe {
                lib.get::<unsafe extern "C" fn(*const c_char) -> *mut c_void>(b"eglGetProcAddress\0")
                    .ok()
                    .map(|sym| *sym)
            }
        });

        // SAFETY: Loading libGL shared library for glXGetProcAddressARB fallback.
        let lib_gl = unsafe {
            libloading::Library::new("libGL.so.1")
                .or_else(|_| libloading::Library::new("libGL.so"))
                .ok()
        };
        let glx_get_proc_address = lib_gl.as_ref().and_then(|lib| {
            // SAFETY: Resolving glXGetProcAddressARB function pointer from libGL.
            unsafe {
                lib.get::<unsafe extern "C" fn(*const c_char) -> *mut c_void>(b"glXGetProcAddressARB\0")
                    .or_else(|_| lib.get(b"glXGetProcAddress\0"))
                    .ok()
                    .map(|sym| *sym)
            }
        });

        Self {
            egl_get_proc_address,
            glx_get_proc_address,
            lib_egl,
            lib_gl,
        }
    }

    fn get_proc(&self, name: *const c_char) -> *mut c_void {
        if let Some(proc) = self.egl_get_proc_address {
            // SAFETY: Calling eglGetProcAddress to query GL function pointer.
            let ptr = unsafe { proc(name) };
            if !ptr.is_null() {
                return ptr;
            }
        }

        if let Some(proc) = self.glx_get_proc_address {
            // SAFETY: Calling glXGetProcAddressARB to query GL function pointer.
            let ptr = unsafe { proc(name) };
            if !ptr.is_null() {
                return ptr;
            }
        }

        if let Some(lib) = &self.lib_egl {
            // SAFETY: Querying direct symbol by CStr from loaded libEGL library.
            if let Ok(c_str) = unsafe { CStr::from_ptr(name) }.to_str() {
                if let Ok(sym) = unsafe { lib.get::<*mut c_void>(c_str.as_bytes()) } {
                    return *sym;
                }
            }
        }
        if let Some(lib) = &self.lib_gl {
            // SAFETY: Querying direct symbol by CStr from loaded libGL library.
            if let Ok(c_str) = unsafe { CStr::from_ptr(name) }.to_str() {
                if let Ok(sym) = unsafe { lib.get::<*mut c_void>(c_str.as_bytes()) } {
                    return *sym;
                }
            }
        }

        // SAFETY: Fallback dynamic symbol lookup via RTLD_DEFAULT for core GL functions.
        unsafe { libc::dlsym(libc::RTLD_DEFAULT, name) }
    }
}

#[cfg(windows)]
struct GlProcLoader {
    lib_opengl32: Option<libloading::Library>,
    wgl_get_proc_address: Option<unsafe extern "system" fn(*const c_char) -> *mut c_void>,
}

#[cfg(windows)]
impl GlProcLoader {
    fn new() -> Self {
        // SAFETY: Loading system opengl32.dll for WGL function resolution.
        let lib_opengl32 = unsafe { libloading::Library::new("opengl32.dll").ok() };
        let wgl_get_proc_address = lib_opengl32.as_ref().and_then(|lib| {
            // SAFETY: Resolving wglGetProcAddress symbol from opengl32.dll.
            unsafe {
                lib.get::<unsafe extern "system" fn(*const c_char) -> *mut c_void>(b"wglGetProcAddress\0")
                    .ok()
                    .map(|sym| *sym)
            }
        });
        Self {
            lib_opengl32,
            wgl_get_proc_address,
        }
    }

    fn get_proc(&self, name: *const c_char) -> *mut c_void {
        if let Some(wgl_proc) = self.wgl_get_proc_address {
            // SAFETY: Invoking wglGetProcAddress to resolve extension pointer.
            let ptr = unsafe { wgl_proc(name) };
            let val = ptr as usize;
            if val > 3 && val != usize::MAX {
                return ptr;
            }
        }

        if let Some(lib) = &self.lib_opengl32 {
            // SAFETY: Fallback GetProcAddress on opengl32.dll for core OpenGL 1.1 symbols.
            if let Ok(c_str) = unsafe { CStr::from_ptr(name) }.to_str() {
                if let Ok(sym) = unsafe { lib.get::<*mut c_void>(c_str.as_bytes()) } {
                    return *sym;
                }
            }
        }

        std::ptr::null_mut()
    }
}

static GL_LOADER: OnceLock<GlProcLoader> = OnceLock::new();

// SAFETY: Extern C callback provided to libmpv to query OpenGL procedures on current thread context.
unsafe extern "C" fn mpv_get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let loader = GL_LOADER.get_or_init(GlProcLoader::new);
    loader.get_proc(name)
}

// SAFETY: Update callback invoked by libmpv when a new video frame is ready to render.
unsafe extern "C" fn mpv_update_callback(ctx_ptr: *mut c_void) {
    if !ctx_ptr.is_null() {
        let ctx = &*(ctx_ptr as *const egui::Context);
        ctx.request_repaint();
    }
}

// --- GL Resource Cleanup Queue ---

struct GlCleanup {
    ffi: Arc<MpvFfi>,
    render_context: Option<*mut MpvRenderContext>,
    mpv_handle: *mut MpvHandle,
    ctx_ptr: Option<*mut egui::Context>,
    fbo: Option<glow::NativeFramebuffer>,
    texture: Option<glow::NativeTexture>,
    /// The `time-pos` poller; joined after the render context is gone (the core may be
    /// waiting on it until then) and before the handle it uses is destroyed.
    pos_thread: Option<std::thread::JoinHandle<()>>,
}

// SAFETY: GlCleanup holds raw handles to be freed on the GUI thread where GL context is current.
unsafe impl Send for GlCleanup {}

static GL_CLEANUP_QUEUE: Mutex<Vec<GlCleanup>> = Mutex::new(Vec::new());

pub fn has_pending_gl_cleanup() -> bool {
    GL_CLEANUP_QUEUE.lock().map(|q| !q.is_empty()).unwrap_or(false)
}

pub fn drain_gl_cleanup(gl: &glow::Context) {
    let items = {
        let mut q = match GL_CLEANUP_QUEUE.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        std::mem::take(&mut *q)
    };

    for item in items {
        unsafe {
            if let Some(fbo) = item.fbo {
                // SAFETY: Deleting OpenGL framebuffer object on GUI thread with active context.
                gl.delete_framebuffer(fbo);
            }
            if let Some(tex) = item.texture {
                // SAFETY: Deleting OpenGL texture object on GUI thread with active context.
                gl.delete_texture(tex);
            }
            if let Some(r_ctx) = item.render_context {
                // SAFETY: Releasing libmpv render context with current OpenGL context.
                (item.ffi.render_context_free)(r_ctx);
            }
            if let Some(ctx_ptr) = item.ctx_ptr {
                // SAFETY: Reclaiming heap allocation for update callback Context pointer.
                drop(Box::from_raw(ctx_ptr));
            }
            if let Some(t) = item.pos_thread {
                let _ = t.join();
            }
            if !item.mpv_handle.is_null() {
                // SAFETY: Terminating and destroying libmpv core after render context is freed.
                (item.ffi.terminate_destroy)(item.mpv_handle);
            }
        }
    }
}

// --- Render State ---

struct MpvRenderState {
    ffi: Arc<MpvFfi>,
    mpv_handle: *mut MpvHandle,
    render_context: Option<*mut MpvRenderContext>,
    ctx_ptr: Option<*mut egui::Context>,
    fbo: Option<glow::NativeFramebuffer>,
    texture: Option<glow::NativeTexture>,
    fbo_size: (u32, u32),
    orig_width: usize,
    orig_height: usize,
    failed: Arc<AtomicBool>,
    pending_load: Option<CString>,
}

// SAFETY: MpvRenderState is guarded by a Mutex and only rendered on the GUI thread.
unsafe impl Send for MpvRenderState {}
// SAFETY: Mutex synchronization guarantees exclusive access to MpvRenderState.
unsafe impl Sync for MpvRenderState {}

impl MpvRenderState {
    /// egui keeps drawing into whatever framebuffer is bound when the callback returns,
    /// so the bindings, viewport and scissor are saved before anything else touches them
    /// (creating the FBO binds it) and restored on every path out.
    fn render(&mut self, info: egui::PaintCallbackInfo, gl: &glow::Context, ctx: &egui::Context) {
        // SAFETY: plain state queries on the GUI thread with egui's context current.
        let (prev_draw_fbo, prev_read_fbo, prev_scissor, prev_viewport) = unsafe {
            let mut vp = [0i32; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut vp);
            (
                gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING),
                gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING),
                gl.is_enabled(glow::SCISSOR_TEST),
                vp,
            )
        };
        self.render_inner(info, gl, ctx, prev_draw_fbo);
        let fb = |id: i32| NonZeroU32::new(id as u32).map(glow::NativeFramebuffer);
        // SAFETY: restoring the state saved above on the same context.
        unsafe {
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, fb(prev_draw_fbo));
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, fb(prev_read_fbo));
            gl.viewport(prev_viewport[0], prev_viewport[1], prev_viewport[2], prev_viewport[3]);
            if prev_scissor {
                gl.enable(glow::SCISSOR_TEST);
            } else {
                gl.disable(glow::SCISSOR_TEST);
            }
        }
    }

    fn render_inner(&mut self, info: egui::PaintCallbackInfo, gl: &glow::Context, ctx: &egui::Context, prev_draw_fbo: i32) {
        drain_gl_cleanup(gl);

        if self.failed.load(Ordering::Acquire) {
            return;
        }

        // 1. Lazily create mpv render context
        if self.render_context.is_none() {
            let advanced_control: c_int = 1;
            let init_params = MpvOpenglInitParams {
                get_proc_address: Some(mpv_get_proc_address),
                get_proc_address_ctx: std::ptr::null_mut(),
            };

            let mut params = vec![
                MpvRenderParam {
                    type_: MPV_RENDER_PARAM_API_TYPE,
                    data: MPV_RENDER_API_TYPE_OPENGL.as_ptr() as *mut c_void,
                },
                MpvRenderParam {
                    type_: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                    data: &init_params as *const _ as *mut c_void,
                },
                MpvRenderParam {
                    type_: MPV_RENDER_PARAM_ADVANCED_CONTROL,
                    data: &advanced_control as *const c_int as *mut c_void,
                },
            ];

            #[cfg(not(windows))]
            match get_native_display() {
                NativeDisplay::Wayland(ptr) => {
                    params.push(MpvRenderParam {
                        type_: MPV_RENDER_PARAM_WL_DISPLAY,
                        data: ptr as *mut c_void,
                    });
                }
                NativeDisplay::X11(ptr) => {
                    params.push(MpvRenderParam {
                        type_: MPV_RENDER_PARAM_X11_DISPLAY,
                        data: ptr as *mut c_void,
                    });
                }
                NativeDisplay::None => {}
            }

            params.push(MpvRenderParam {
                type_: MPV_RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            });

            let mut r_ctx: *mut MpvRenderContext = std::ptr::null_mut();
            // SAFETY: Calling mpv_render_context_create with valid OpenGL parameters and handle.
            let err = unsafe { (self.ffi.render_context_create)(&mut r_ctx, self.mpv_handle, params.as_mut_ptr()) };
            if err < 0 || r_ctx.is_null() {
                eprintln!("Trimmer: mpv_render_context_create failed ({err})");
                self.failed.store(true, Ordering::Release);
                return;
            }

            let ctx_box = Box::into_raw(Box::new(ctx.clone()));
            self.ctx_ptr = Some(ctx_box);
            // SAFETY: Setting libmpv render update callback with heap-allocated Context pointer.
            unsafe {
                (self.ffi.render_context_set_update_callback)(r_ctx, mpv_update_callback, ctx_box as *mut c_void);
            }
            self.render_context = Some(r_ctx);

            if let Some(path_c) = self.pending_load.take() {
                let cmd = [b"loadfile\0".as_ptr() as *const c_char, path_c.as_ptr(), std::ptr::null()];
                // SAFETY: loadfile on the live handle; the argument strings outlive the call.
                if unsafe { (self.ffi.command)(self.mpv_handle, cmd.as_ptr() as *mut _) } < 0 {
                    eprintln!("Trimmer: mpv could not load the clip");
                    self.failed.store(true, Ordering::Release);
                    return;
                }
            }
        }

        let Some(r_ctx) = self.render_context else { return };

        // 2. Physical pixel size capped at the video's own dimensions
        let viewport_px = info.viewport_in_pixels();
        let target_w = (viewport_px.width_px.max(1) as u32).min(self.orig_width as u32).max(1);
        let target_h = (viewport_px.height_px.max(1) as u32).min(self.orig_height as u32).max(1);

        // 3. (Re)allocate FBO and texture if size changed
        let resized = self.fbo.is_none() || self.fbo_size != (target_w, target_h);
        if resized {
            unsafe {
                if let Some(old_fbo) = self.fbo.take() {
                    // SAFETY: Re-allocating FBO: deleting obsolete framebuffer.
                    gl.delete_framebuffer(old_fbo);
                }
                if let Some(old_tex) = self.texture.take() {
                    // SAFETY: Re-allocating FBO: deleting obsolete texture.
                    gl.delete_texture(old_tex);
                }

                // SAFETY: Generating new 2D texture for video render target.
                let Ok(tex) = gl.create_texture() else {
                    self.failed.store(true, Ordering::Release);
                    return;
                };
                gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    target_w as i32,
                    target_h as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    None,
                );

                // SAFETY: Generating and binding OpenGL framebuffer object.
                let Ok(fbo) = gl.create_framebuffer() else {
                    gl.delete_texture(tex);
                    self.failed.store(true, Ordering::Release);
                    return;
                };
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(tex),
                    0,
                );

                if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                    gl.delete_framebuffer(fbo);
                    gl.delete_texture(tex);
                    self.failed.store(true, Ordering::Release);
                    return;
                }

                self.fbo = Some(fbo);
                self.texture = Some(tex);
                self.fbo_size = (target_w, target_h);
            }
        }

        let Some(fbo) = self.fbo else { return };


        // ADVANCED_CONTROL requires acknowledging every update on the render thread.
        // SAFETY: live render context on the GUI thread.
        let flags = unsafe { (self.ffi.render_context_update)(r_ctx) };
        // Scrolling or hovering repaints the window many times a second; only ask mpv for a
        // frame when it has a new one or our target changed, otherwise reuse the FBO.
        let needs_render = resized || flags & MPV_RENDER_UPDATE_FRAME != 0;

        // 5. Render mpv frame into our FBO
        let mut mpv_fbo = MpvOpenglFbo {
            fbo: fbo.0.get() as c_int,
            w: target_w as c_int,
            h: target_h as c_int,
            internal_format: glow::RGBA8 as c_int,
        };
        // mpv draws top row first; flipping matches GL's bottom-up FBO for the blit.
        let mut flip_y: c_int = 1;
        // By default mpv sleeps in render() until the frame's display time, which stalls
        // the whole UI for most of every frame; the app's frame cap paces us instead.
        let mut block: c_int = 0;
        let mut render_params = [
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_OPENGL_FBO,
                data: &mut mpv_fbo as *mut _ as *mut c_void,
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_FLIP_Y,
                data: &mut flip_y as *mut _ as *mut c_void,
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                data: &mut block as *mut _ as *mut c_void,
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];

        if needs_render {
            // SAFETY: Invoking mpv_render_context_render with bound FBO parameters.
            unsafe {
                (self.ffi.render_context_render)(r_ctx, render_params.as_mut_ptr());
            }
        }

        // 6. Blit FBO into destination framebuffer at callback viewport
        unsafe {
            // SAFETY: Blitting rendered FBO into destination viewport with linear filter.
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(fbo));
            gl.bind_framebuffer(
                glow::DRAW_FRAMEBUFFER,
                if prev_draw_fbo != 0 {
                    NonZeroU32::new(prev_draw_fbo as u32).map(glow::NativeFramebuffer)
                } else {
                    None
                },
            );

            gl.disable(glow::SCISSOR_TEST);
            gl.blit_framebuffer(
                0,
                0,
                target_w as i32,
                target_h as i32,
                viewport_px.left_px,
                viewport_px.from_bottom_px,
                viewport_px.left_px + viewport_px.width_px,
                viewport_px.from_bottom_px + viewport_px.height_px,
                glow::COLOR_BUFFER_BIT,
                glow::LINEAR,
            );

        }
    }
}

// --- MpvPlayer Public Interface ---

#[derive(Clone, Debug, PartialEq)]
pub struct SoundOverlayConfig {
    pub path: PathBuf,
    pub start: f64,
    pub end: f64,
    pub volume: f32,
}

pub struct MpvPlayer {
    ffi: Arc<MpvFfi>,
    handle: *mut MpvHandle,
    render_state: Arc<Mutex<MpvRenderState>>,
    failed: Arc<AtomicBool>,
    external_audio_tracks: Mutex<Vec<i64>>,
    /// Latest `time-pos` (f64 bits, NaN = unknown), read by `pos_thread`.
    time_pos: Arc<AtomicU64>,
    pos_stop: Arc<AtomicBool>,
    pos_thread: Option<std::thread::JoinHandle<()>>,
}

/// The raw handle, moved to the position thread.
struct SendHandle(*mut MpvHandle);
// SAFETY: libmpv client API calls are thread-safe; the handle outlives the thread (joined in Drop).
unsafe impl Send for SendHandle {}

// SAFETY: MpvPlayer client commands and properties are thread-safe in libmpv.
unsafe impl Send for MpvPlayer {}
// SAFETY: Mutex protects shared render state across threads.
unsafe impl Sync for MpvPlayer {}

impl MpvPlayer {
    pub fn new(path: &Path, audio_tracks: usize, orig_width: usize, orig_height: usize) -> Option<Self> {
        let ffi = get_mpv()?;

        // SAFETY: Creating new libmpv core instance.
        let handle = unsafe { (ffi.create)() };
        if handle.is_null() {
            return None;
        }

        let options = [
            ("vo", "libmpv"),
            // auto-safe tries CUDA first on mixed AMD/NVIDIA machines and, when that card
            // cannot decode the codec, ends up in copy-back mode; VAAPI first keeps zero copy.
            ("hwdec", if cfg!(target_os = "linux") { "vaapi,auto-safe" } else { "auto-safe" }),
            ("keep-open", "always"),
            ("pause", "yes"),
            ("idle", "yes"),
            ("osc", "no"),
            ("input-default-bindings", "no"),
            ("terminal", "no"),
            ("hr-seek", "yes"),
            ("audio-display", "no"),
            ("msg-level", "all=warn"),
        ];

        for (opt, val) in options {
            let opt_c = CString::new(opt).ok()?;
            let val_c = CString::new(val).ok()?;
            // SAFETY: Setting core configuration option strings before initialization.
            unsafe {
                (ffi.set_option_string)(handle, opt_c.as_ptr(), val_c.as_ptr());
            }
        }

        // LAPSE_MPV_LOG=<file> writes mpv's own verbose log, for diagnosing playback.
        if let Some(log) = std::env::var_os("LAPSE_MPV_LOG").and_then(|p| CString::new(p.to_string_lossy().as_bytes()).ok()) {
            // SAFETY: Setting an option string before initialization.
            unsafe {
                (ffi.set_option_string)(handle, b"log-file\0".as_ptr() as *const c_char, log.as_ptr());
            }
        }

        // SAFETY: Initializing mpv core after setting options.
        let res = unsafe { (ffi.initialize)(handle) };
        if res < 0 {
            // SAFETY: Destroying failed mpv handle on init error.
            unsafe { (ffi.terminate_destroy)(handle) };
            return None;
        }

        // Audio mixing when video has multiple audio tracks
        if audio_tracks > 1 {
            let inputs: String = (1..=audio_tracks).map(|i| format!("[aid{i}]")).collect();
            // Audio only: routing the video through lavfi-complex as well makes mpv fall
            // back from zero-copy VAAPI to copying every frame through the CPU.
            let complex = format!("{inputs}amix=inputs={audio_tracks}:normalize=0[ao]");
            let prop_c = CString::new("lavfi-complex").ok()?;
            let val_c = CString::new(complex).ok()?;
            // SAFETY: Setting lavfi-complex property for multi-track audio downmixing.
            unsafe {
                (ffi.set_property_string)(handle, prop_c.as_ptr(), val_c.as_ptr());
            }
        }

        // Load the file in paused state
        // vo=libmpv fails the load when no render context exists yet, so the file is
        // loaded by the first paint callback right after it creates one.
        let pending_load = Some(CString::new(path.to_string_lossy().as_bytes()).ok()?);

        let failed = Arc::new(AtomicBool::new(false));
        let render_state = Arc::new(Mutex::new(MpvRenderState {
            ffi: Arc::clone(&ffi),
            mpv_handle: handle,
            render_context: None,
            ctx_ptr: None,
            fbo: None,
            texture: None,
            fbo_size: (0, 0),
            orig_width,
            orig_height,
            failed: Arc::clone(&failed),
            pending_load,
        }));

        // A synchronous mpv_get_property waits for the mpv core, and the core can be waiting
        // for our render callback: polling it on the GUI thread froze the UI for ~200 ms at
        // a time. Read the position here instead.
        let time_pos = Arc::new(AtomicU64::new(f64::NAN.to_bits()));
        let pos_stop = Arc::new(AtomicBool::new(false));
        let pos_thread = {
            let (ffi, time_pos, stop, h) = (Arc::clone(&ffi), Arc::clone(&time_pos), Arc::clone(&pos_stop), SendHandle(handle));
            std::thread::Builder::new()
                .name("mpv-pos".into())
                .spawn(move || {
                    let h = h;
                    let prop = c"time-pos";
                    while !stop.load(Ordering::Acquire) {
                        let mut pos: f64 = 0.0;
                        // SAFETY: the handle stays alive until this thread is joined.
                        let res = unsafe { (ffi.get_property)(h.0, prop.as_ptr(), MPV_FORMAT_DOUBLE, &mut pos as *mut _ as *mut c_void) };
                        let v = if res >= 0 && pos.is_finite() { pos } else { f64::NAN };
                        time_pos.store(v.to_bits(), Ordering::Release);
                        std::thread::sleep(std::time::Duration::from_millis(8));
                    }
                })
                .ok()
        };

        Some(Self {
            ffi,
            handle,
            render_state,
            failed,
            external_audio_tracks: Mutex::new(Vec::new()),
            time_pos,
            pos_stop,
            pos_thread,
        })
    }

    pub fn command(&self, args: &[&str]) -> bool {
        let mut cstrings = Vec::new();
        for arg in args {
            let Ok(c) = CString::new(*arg) else { return false };
            cstrings.push(c);
        }
        let mut ptrs: Vec<*const c_char> = cstrings.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        // SAFETY: Dispatching command to libmpv with valid null-terminated C string pointers.
        unsafe {
            (self.ffi.command)(self.handle, ptrs.as_ptr() as *mut _) >= 0
        }
    }

    pub fn get_property_i64(&self, name: &str) -> Option<i64> {
        let mut val: i64 = 0;
        let prop = CString::new(name).ok()?;
        const MPV_FORMAT_INT64: c_int = 4;
        // SAFETY: Querying 64-bit integer property from active libmpv instance.
        let res = unsafe {
            (self.ffi.get_property)(self.handle, prop.as_ptr(), MPV_FORMAT_INT64, &mut val as *mut _ as *mut c_void)
        };
        if res >= 0 {
            Some(val)
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn update_audio_overlays(&self, audio_tracks: usize, sound_overlays: &[SoundOverlayConfig]) {
        self.update_audio_filters(audio_tracks, &crate::export::AudioMix::default(), sound_overlays);
    }

    pub fn update_audio_filters(
        &self,
        audio_tracks: usize,
        audio: &crate::export::AudioMix,
        sound_overlays: &[SoundOverlayConfig],
    ) {
        let mut prev_tracks = self.external_audio_tracks.lock().unwrap_or_else(|e| e.into_inner());
        for id in prev_tracks.drain(..) {
            self.command(&["audio-remove", &id.to_string()]);
        }

        if !audio.tracks.is_empty() {
            let active: Vec<(usize, &crate::export::TrackSetting)> = audio
                .tracks
                .iter()
                .enumerate()
                .take(audio_tracks)
                .filter(|(_, t)| t.enabled)
                .collect();

            if sound_overlays.is_empty() {
                if active.is_empty() {
                    self.set_property_str("lavfi-complex", "anullsrc=channel_layout=stereo:sample_rate=48000[ao]");
                } else if active.len() == 1 {
                    let (idx, track) = active[0];
                    let aid = idx + 1;
                    if (track.volume - 1.0).abs() >= 0.01 {
                        if track.volume > 1.0 {
                            self.set_property_str("lavfi-complex", &format!("[aid{aid}]volume={:.2},alimiter=limit=0.95[ao]", track.volume));
                        } else {
                            self.set_property_str("lavfi-complex", &format!("[aid{aid}]volume={:.2}[ao]", track.volume));
                        }
                    } else if audio_tracks == 1 && aid == 1 {
                        self.set_property_str("lavfi-complex", "");
                    } else {
                        self.set_property_str("lavfi-complex", &format!("[aid{aid}]anull[ao]"));
                    }
                } else {
                    let mut parts = Vec::new();
                    let mut inputs = Vec::new();
                    for (idx, track) in &active {
                        let aid = idx + 1;
                        if (track.volume - 1.0).abs() >= 0.01 {
                            let tag = if *idx == 1 && audio_tracks == 2 && track.name == "mic" {
                                "[mic]".to_string()
                            } else {
                                format!("[a_{idx}]")
                            };
                            if track.volume > 1.0 {
                                parts.push(format!("[aid{aid}]volume={:.2},alimiter=limit=0.95{tag}", track.volume));
                            } else {
                                parts.push(format!("[aid{aid}]volume={:.2}{tag}", track.volume));
                            }
                            inputs.push(tag);
                        } else {
                            inputs.push(format!("[aid{aid}]"));
                        }
                    }
                    parts.push(format!("{}amix=inputs={}:normalize=0[ao]", inputs.join(""), inputs.len()));
                    self.set_property_str("lavfi-complex", &parts.join(";"));
                }
                return;
            }

            let mut aid_list = Vec::new();
            for overlay in sound_overlays {
                let path_str = overlay.path.to_string_lossy();
                self.command(&["audio-add", &path_str, "auto"]);
                let count = self.get_property_i64("track-list/count").unwrap_or(0);
                let id = if count > 0 {
                    self.get_property_i64(&format!("track-list/{}/id", count - 1)).unwrap_or(count)
                } else {
                    count
                };
                prev_tracks.push(id);
                aid_list.push(id);
            }

            let mut parts = Vec::new();
            if active.is_empty() {
                parts.push("anullsrc=channel_layout=stereo:sample_rate=48000[abase]".to_string());
            } else if active.len() == 1 {
                let (idx, track) = active[0];
                let aid = idx + 1;
                if (track.volume - 1.0).abs() >= 0.01 {
                    if track.volume > 1.0 {
                        parts.push(format!("[aid{aid}]volume={:.2},alimiter=limit=0.95[abase]", track.volume));
                    } else {
                        parts.push(format!("[aid{aid}]volume={:.2}[abase]", track.volume));
                    }
                } else {
                    parts.push(format!("[aid{aid}]anull[abase]"));
                }
            } else {
                let mut inputs = Vec::new();
                for (idx, track) in &active {
                    let aid = idx + 1;
                    if (track.volume - 1.0).abs() >= 0.01 {
                        let tag = if *idx == 1 && audio_tracks == 2 && track.name == "mic" {
                            "[mic]".to_string()
                        } else {
                            format!("[a_{idx}]")
                        };
                        if track.volume > 1.0 {
                            parts.push(format!("[aid{aid}]volume={:.2},alimiter=limit=0.95{tag}", track.volume));
                        } else {
                            parts.push(format!("[aid{aid}]volume={:.2}{tag}", track.volume));
                        }
                        inputs.push(tag);
                    } else {
                        inputs.push(format!("[aid{aid}]"));
                    }
                }
                parts.push(format!("{}amix=inputs={}:normalize=0[abase]", inputs.join(""), inputs.len()));
            }

            let mut amix_inputs = vec!["[abase]".to_string()];
            for (k, (&aid, overlay)) in aid_list.iter().zip(sound_overlays.iter()).enumerate() {
                let ov_label = format!("[a_ov{}]", k);
                let filter_str = if (overlay.volume - 1.0).abs() >= 0.01 {
                    format!("[aid{}]volume={:.2}{}", aid, overlay.volume, ov_label)
                } else {
                    format!("[aid{}]{}", aid, ov_label)
                };
                parts.push(filter_str);
                amix_inputs.push(ov_label);
            }

            if amix_inputs.len() > 1 {
                parts.push(format!("{}amix=inputs={}:normalize=0[ao]", amix_inputs.join(""), amix_inputs.len()));
            } else {
                parts.push("[abase]anull[ao]".to_string());
            }

            self.set_property_str("lavfi-complex", &parts.join(";"));
            return;
        }

        let mic_track = audio_tracks > 1;
        let game = audio.game && audio_tracks > 0;
        let mic = mic_track && audio.mic;

        if sound_overlays.is_empty() {
            if audio_tracks > 1 {
                if game && mic {
                    let mut parts = Vec::new();
                    if (audio.mic_volume - 1.0).abs() >= 0.01 {
                        if audio.mic_volume > 1.0 {
                            parts.push(format!("[aid2]volume={:.2},alimiter=limit=0.95[mic]", audio.mic_volume));
                        } else {
                            parts.push(format!("[aid2]volume={:.2}[mic]", audio.mic_volume));
                        }
                        parts.push("[aid1][mic]amix=inputs=2:normalize=0[ao]".to_string());
                    } else {
                        parts.push("[aid1][aid2]amix=inputs=2:normalize=0[ao]".to_string());
                    }
                    self.set_property_str("lavfi-complex", &parts.join(";"));
                } else if game {
                    self.set_property_str("lavfi-complex", "[aid1]anull[ao]");
                } else if mic {
                    if (audio.mic_volume - 1.0).abs() >= 0.01 {
                        if audio.mic_volume > 1.0 {
                            self.set_property_str("lavfi-complex", &format!("[aid2]volume={:.2},alimiter=limit=0.95[ao]", audio.mic_volume));
                        } else {
                            self.set_property_str("lavfi-complex", &format!("[aid2]volume={:.2}[ao]", audio.mic_volume));
                        }
                    } else {
                        self.set_property_str("lavfi-complex", "[aid2]anull[ao]");
                    }
                } else {
                    self.set_property_str("lavfi-complex", "anullsrc=channel_layout=stereo:sample_rate=48000[ao]");
                }
            } else if audio_tracks == 1 {
                if !game {
                    self.set_property_str("lavfi-complex", "anullsrc=channel_layout=stereo:sample_rate=48000[ao]");
                } else {
                    self.set_property_str("lavfi-complex", "");
                }
            } else {
                self.set_property_str("lavfi-complex", "");
            }
            return;
        }

        let mut aid_list = Vec::new();
        for overlay in sound_overlays {
            let path_str = overlay.path.to_string_lossy();
            self.command(&["audio-add", &path_str, "auto"]);
            let count = self.get_property_i64("track-list/count").unwrap_or(0);
            let id = if count > 0 {
                self.get_property_i64(&format!("track-list/{}/id", count - 1)).unwrap_or(count)
            } else {
                count
            };
            prev_tracks.push(id);
            aid_list.push(id);
        }

        let mut parts = Vec::new();
        if audio_tracks > 1 {
            if game && mic {
                if (audio.mic_volume - 1.0).abs() >= 0.01 {
                    if audio.mic_volume > 1.0 {
                        parts.push(format!("[aid2]volume={:.2},alimiter=limit=0.95[mic]", audio.mic_volume));
                    } else {
                        parts.push(format!("[aid2]volume={:.2}[mic]", audio.mic_volume));
                    }
                    parts.push("[aid1][mic]amix=inputs=2:normalize=0[abase]".to_string());
                } else {
                    parts.push("[aid1][aid2]amix=inputs=2:normalize=0[abase]".to_string());
                }
            } else if game {
                parts.push("[aid1]anull[abase]".to_string());
            } else if mic {
                if (audio.mic_volume - 1.0).abs() >= 0.01 {
                    if audio.mic_volume > 1.0 {
                        parts.push(format!("[aid2]volume={:.2},alimiter=limit=0.95[abase]", audio.mic_volume));
                    } else {
                        parts.push(format!("[aid2]volume={:.2}[abase]", audio.mic_volume));
                    }
                } else {
                    parts.push("[aid2]anull[abase]".to_string());
                }
            } else {
                parts.push("anullsrc=channel_layout=stereo:sample_rate=48000[abase]".to_string());
            }
        } else if audio_tracks == 1 {
            if game {
                parts.push("[aid1]anull[abase]".to_string());
            } else {
                parts.push("anullsrc=channel_layout=stereo:sample_rate=48000[abase]".to_string());
            }
        } else {
            parts.push("anullsrc=channel_layout=stereo:sample_rate=48000[abase]".to_string());
        }

        let mut amix_inputs = vec!["[abase]".to_string()];
        for (k, (overlay, aid)) in sound_overlays.iter().zip(aid_list.iter()).enumerate() {
            let delay_ms = (overlay.start * 1000.0).round() as u64;
            let len = (overlay.end - overlay.start).max(0.0);
            let ov_label = format!("[a_ov{}]", k);
            parts.push(format!(
                "[aid{}]atrim=0:{:.3},volume={:.2},adelay={}:all=1{}",
                aid, len, overlay.volume, delay_ms, ov_label
            ));
            amix_inputs.push(ov_label);
        }

        parts.push(format!(
            "{}amix=inputs={}:normalize=0:duration=first[ao]",
            amix_inputs.join(""),
            amix_inputs.len()
        ));

        self.set_property_str("lavfi-complex", &parts.join(";"));
    }

    fn set_property_str(&self, name: &str, value: &str) {
        if let (Ok(name_c), Ok(val_c)) = (CString::new(name), CString::new(value)) {
            // SAFETY: Setting mpv string property on active instance.
            unsafe {
                (self.ffi.set_property_string)(self.handle, name_c.as_ptr(), val_c.as_ptr());
            }
        }
    }

    pub fn play(&self) {
        self.set_property_str("pause", "no");
    }

    pub fn pause(&self) {
        self.set_property_str("pause", "yes");
    }

    pub fn seek(&self, secs: f64) {
        let secs_str = format!("{secs:.3}");
        if let Ok(secs_c) = CString::new(secs_str) {
            let cmd = [
                b"seek\0".as_ptr() as *const c_char,
                secs_c.as_ptr(),
                b"absolute+exact\0".as_ptr() as *const c_char,
                std::ptr::null(),
            ];
            // SAFETY: Dispatching exact absolute seek command to libmpv.
            unsafe {
                (self.ffi.command)(self.handle, cmd.as_ptr() as *mut _);
            }
        }
    }

    pub fn get_time_pos(&self) -> Option<f64> {
        let pos = f64::from_bits(self.time_pos.load(Ordering::Acquire));
        pos.is_finite().then_some(pos)
    }

    pub fn set_volume(&self, vol: f32, muted: bool) {
        self.set_property_str("mute", if muted { "yes" } else { "no" });
        self.set_property_str("volume", &format!("{:.2}", (vol * 100.0).clamp(0.0, 100.0)));
    }

    pub fn has_failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    pub fn paint(&self, ui: &egui::Ui, rect: Rect) {
        let render_state = Arc::clone(&self.render_state);
        let ctx = ui.ctx().clone();

        ui.painter().add(egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                if let Ok(mut state) = render_state.lock() {
                    state.render(info, painter.gl(), &ctx);
                }
            })),
        });
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        self.pos_stop.store(true, Ordering::Release);
        // The core is only destroyed on the next GL frame; silence it now so audio does
        // not keep playing after the trimmer closes.
        let stop = [b"stop\0".as_ptr() as *const c_char, std::ptr::null()];
        // SAFETY: the handle stays valid until the queued cleanup destroys it.
        unsafe {
            (self.ffi.command)(self.handle, stop.as_ptr() as *mut _);
        }
        let mut guard = self.render_state.lock().unwrap_or_else(|e| e.into_inner());
        let cleanup = GlCleanup {
            ffi: Arc::clone(&self.ffi),
            render_context: guard.render_context.take(),
            mpv_handle: self.handle,
            ctx_ptr: guard.ctx_ptr.take(),
            fbo: guard.fbo.take(),
            texture: guard.texture.take(),
            pos_thread: self.pos_thread.take(),
        };

        if let Ok(mut q) = GL_CLEANUP_QUEUE.lock() {
            q.push(cleanup);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mpv_plugin_dir_not_empty() {
        let dir = mpv_plugin_dir();
        assert!(!dir.as_os_str().is_empty());
        assert!(dir.ends_with("plugins"));
    }

    #[test]
    fn test_mpv_available_call() {
        // Should not panic, returns bool depending on system environment
        let _ = mpv_available();
    }
}
