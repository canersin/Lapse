//! gpu-screen-recorder plugin that draws the "not in the game" card over the picture.
//! build.rs compiles this file on its own into a shared library which Lapse carries along.
//! What to draw comes through a file shared with the daemon, see `shared.rs`.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicU32, Ordering::*};

#[path = "shared.rs"]
mod shared;
use shared::*;

extern "C" {
    fn open(path: *const c_char, flags: c_int, ...) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn mmap(addr: *mut c_void, len: usize, prot: c_int, flags: c_int, fd: c_int, off: i64) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> c_int;
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(lib: *mut c_void, name: *const c_char) -> *mut c_void;
}

#[repr(C)]
pub struct DrawParams {
    width: u32,
    height: u32,
}

#[repr(C)]
pub struct InitParams {
    width: u32,
    height: u32,
    fps: u32,
    color_depth: c_int,
    graphics_api: c_int,
}

#[repr(C)]
pub struct InitReturn {
    name: *const c_char,
    version: u32,
    userdata: *mut c_void,
    draw: Option<unsafe extern "C" fn(*const DrawParams, *mut c_void)>,
    is_damaged: Option<unsafe extern "C" fn(*mut c_void) -> bool>,
    clear_damage: Option<unsafe extern "C" fn(*mut c_void)>,
}

macro_rules! gl {
    ($($field:ident = $name:literal ($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        struct Gl { $($field: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)* }
        impl Gl {
            unsafe fn load(api: c_int) -> Option<Self> {
                let lib = dlopen(if api == 0 { c"libGLESv2.so.2" } else { c"libGL.so.1" }.as_ptr(), 1);
                if lib.is_null() {
                    return None;
                }
                $(let $field = dlsym(lib, concat!($name, "\0").as_ptr().cast());
                if $field.is_null() {
                    return None;
                })*
                Some(Self { $($field: std::mem::transmute::<*mut c_void, unsafe extern "C" fn($($arg),*) $(-> $ret)?>($field),)* })
            }
        }
    };
}

gl! {
    get_integer = "glGetIntegerv"(u32, *mut c_int);
    get_float = "glGetFloatv"(u32, *mut f32);
    is_enabled = "glIsEnabled"(u32) -> u8;
    enable = "glEnable"(u32);
    disable = "glDisable"(u32);
    viewport = "glViewport"(c_int, c_int, c_int, c_int);
    clear_color = "glClearColor"(f32, f32, f32, f32);
    clear = "glClear"(u32);
    create_shader = "glCreateShader"(u32) -> u32;
    shader_source = "glShaderSource"(u32, c_int, *const *const c_char, *const c_int);
    compile_shader = "glCompileShader"(u32);
    delete_shader = "glDeleteShader"(u32);
    create_program = "glCreateProgram"() -> u32;
    attach_shader = "glAttachShader"(u32, u32);
    bind_attrib = "glBindAttribLocation"(u32, u32, *const c_char);
    link_program = "glLinkProgram"(u32);
    get_program = "glGetProgramiv"(u32, u32, *mut c_int);
    use_program = "glUseProgram"(u32);
    gen_textures = "glGenTextures"(c_int, *mut u32);
    bind_texture = "glBindTexture"(u32, u32);
    active_texture = "glActiveTexture"(u32);
    tex_parameter = "glTexParameteri"(u32, u32, c_int);
    tex_image = "glTexImage2D"(u32, c_int, c_int, c_int, c_int, c_int, u32, u32, *const c_void);
    tex_sub_image = "glTexSubImage2D"(u32, c_int, c_int, c_int, c_int, c_int, u32, u32, *const c_void);
    pixel_store = "glPixelStorei"(u32, c_int);
    gen_buffers = "glGenBuffers"(c_int, *mut u32);
    bind_buffer = "glBindBuffer"(u32, u32);
    buffer_data = "glBufferData"(u32, isize, *const c_void, u32);
    gen_vertex_arrays = "glGenVertexArrays"(c_int, *mut u32);
    bind_vertex_array = "glBindVertexArray"(u32);
    enable_attrib = "glEnableVertexAttribArray"(u32);
    attrib_pointer = "glVertexAttribPointer"(u32, c_int, u32, u8, c_int, *const c_void);
    draw_arrays = "glDrawArrays"(u32, c_int, c_int);
}

const GL_FLOAT: u32 = 0x1406;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_RGBA: u32 = 0x1908;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_TEXTURE0: u32 = 0x84C0;
const GL_ARRAY_BUFFER: u32 = 0x8892;
const GL_STATIC_DRAW: u32 = 0x88E4;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;
const GL_SCISSOR_TEST: u32 = 0x0C11;
const GL_BLEND: u32 = 0x0BE2;

struct Card {
    map: *mut u8,
    gl: Gl,
    program: u32,
    texture: u32,
    vertex_array: u32,
    texture_size: (u32, u32),
    /// Sequence number of the picture in the texture, 0: none yet.
    uploaded: u32,
    shown: bool,
}

impl Card {
    fn field(&self, i: usize) -> &AtomicU32 {
        unsafe { &*(self.map as *const AtomicU32).add(i) }
    }

    unsafe fn new(params: &InitParams) -> Option<Self> {
        let path = CString::new(std::env::var(ENV).ok()?).ok()?;
        let fd = open(path.as_ptr(), 2 | 0o2000000); // O_RDWR | O_CLOEXEC
        if fd < 0 {
            return None;
        }
        let map = mmap(std::ptr::null_mut(), FILE_SIZE, 3, 1, fd, 0);
        close(fd);
        if map as isize == -1 {
            return None;
        }
        let gl = Gl::load(params.graphics_api)?;
        let program = program(&gl, params.graphics_api)?;
        let (mut texture, mut buffer, mut vertex_array) = (0, 0, 0);
        (gl.gen_textures)(1, &mut texture);
        (gl.gen_buffers)(1, &mut buffer);
        (gl.gen_vertex_arrays)(1, &mut vertex_array);

        let saved = Saved::new(&gl);
        (gl.bind_vertex_array)(vertex_array);
        (gl.bind_buffer)(GL_ARRAY_BUFFER, buffer);
        let corners: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        (gl.buffer_data)(GL_ARRAY_BUFFER, 32, corners.as_ptr().cast(), GL_STATIC_DRAW);
        (gl.enable_attrib)(0);
        (gl.attrib_pointer)(0, 2, GL_FLOAT, 0, 0, std::ptr::null());
        (gl.active_texture)(GL_TEXTURE0);
        (gl.bind_texture)(GL_TEXTURE_2D, texture);
        for (name, value) in [(0x2801, 0x2600), (0x2800, 0x2600), (0x2802, 0x812F), (0x2803, 0x812F)] {
            (gl.tex_parameter)(GL_TEXTURE_2D, name, value); // nearest, clamp to edge
        }
        saved.restore(&gl);

        let card = Self { map: map.cast(), gl, program, texture, vertex_array, texture_size: (0, 0), uploaded: 0, shown: false };
        if card.field(MAGIC_AT).load(Acquire) != MAGIC {
            return None;
        }
        card.field(FRAME_H).store(params.height, Relaxed);
        card.field(FRAME_W).store(params.width, Release);
        Some(card)
    }

    /// Uploads the newest picture if the texture doesn't hold it yet.
    unsafe fn upload(&mut self) {
        let seq = self.field(SEQ).load(Acquire);
        let (w, h) = (self.field(CARD_W).load(Relaxed), self.field(CARD_H).load(Relaxed));
        if seq == 0 || seq == self.uploaded || w == 0 || h == 0 || (w as usize) * (h as usize) * 4 > BUFFER_SIZE {
            return;
        }
        // The daemon fills the buffer that isn't the newest one.
        let pixels = self.map.add(HEADER_SIZE + (seq & 1) as usize * BUFFER_SIZE).cast::<c_void>();
        if self.texture_size != (w, h) {
            (self.gl.tex_image)(GL_TEXTURE_2D, 0, GL_RGBA as c_int, w as c_int, h as c_int, 0, GL_RGBA, GL_UNSIGNED_BYTE, pixels);
            self.texture_size = (w, h);
        } else {
            (self.gl.tex_sub_image)(GL_TEXTURE_2D, 0, 0, 0, w as c_int, h as c_int, GL_RGBA, GL_UNSIGNED_BYTE, pixels);
        }
        self.uploaded = seq;
    }

    unsafe fn draw(&mut self, params: &DrawParams) {
        self.shown = self.field(AWAY).load(Acquire) != 0;
        if !self.shown {
            return;
        }
        let saved = Saved::new(&self.gl);
        (self.gl.disable)(GL_SCISSOR_TEST);
        (self.gl.disable)(GL_BLEND);
        let bg = self.field(BACKGROUND).load(Relaxed);
        let part = |shift: u32| ((bg >> shift) & 0xFF) as f32 / 255.0;
        (self.gl.clear_color)(part(16), part(8), part(0), 1.0);
        (self.gl.clear)(GL_COLOR_BUFFER_BIT);

        (self.gl.active_texture)(GL_TEXTURE0);
        (self.gl.bind_texture)(GL_TEXTURE_2D, self.texture);
        (self.gl.pixel_store)(0x0CF5, 4); // GL_UNPACK_ALIGNMENT
        self.upload();
        if self.uploaded != 0 {
            let (w, h) = self.texture_size;
            let x = (params.width.saturating_sub(w) / 2) & !1;
            let y = (params.height.saturating_sub(h) / 2) & !1;
            (self.gl.viewport)(x as c_int, y as c_int, w as c_int, h as c_int);
            (self.gl.use_program)(self.program);
            (self.gl.bind_vertex_array)(self.vertex_array);
            (self.gl.draw_arrays)(5, 0, 4); // GL_TRIANGLE_STRIP
        }
        saved.restore(&self.gl);
    }

    fn is_damaged(&self) -> bool {
        let away = self.field(AWAY).load(Acquire) != 0;
        away != self.shown || (away && self.field(SEQ).load(Acquire) != self.uploaded)
    }
}

/// The state of gpu-screen-recorder's own rendering.
struct Saved {
    program: c_int,
    texture: c_int,
    unit: c_int,
    buffer: c_int,
    vertex_array: c_int,
    alignment: c_int,
    viewport: [c_int; 4],
    clear: [f32; 4],
    scissor: bool,
    blend: bool,
}

impl Saved {
    unsafe fn new(gl: &Gl) -> Self {
        let get = |name: u32| {
            let mut v = 0;
            (gl.get_integer)(name, &mut v);
            v
        };
        let unit = get(0x84E0);
        (gl.active_texture)(GL_TEXTURE0);
        let mut s = Self {
            program: get(0x8B8D),
            texture: get(0x8069),
            unit,
            buffer: get(0x8894),
            vertex_array: get(0x85B5),
            alignment: get(0x0CF5),
            viewport: [0; 4],
            clear: [0.0; 4],
            scissor: (gl.is_enabled)(GL_SCISSOR_TEST) != 0,
            blend: (gl.is_enabled)(GL_BLEND) != 0,
        };
        (gl.get_integer)(0x0BA2, s.viewport.as_mut_ptr());
        (gl.get_float)(0x0C22, s.clear.as_mut_ptr());
        s
    }

    unsafe fn restore(self, gl: &Gl) {
        (gl.bind_vertex_array)(self.vertex_array as u32);
        (gl.bind_buffer)(GL_ARRAY_BUFFER, self.buffer as u32);
        (gl.use_program)(self.program as u32);
        (gl.pixel_store)(0x0CF5, self.alignment);
        (gl.active_texture)(GL_TEXTURE0);
        (gl.bind_texture)(GL_TEXTURE_2D, self.texture as u32);
        (gl.active_texture)(self.unit as u32);
        let [x, y, w, h] = self.viewport;
        (gl.viewport)(x, y, w, h);
        let [r, g, b, a] = self.clear;
        (gl.clear_color)(r, g, b, a);
        if self.scissor {
            (gl.enable)(GL_SCISSOR_TEST);
        }
        if self.blend {
            (gl.enable)(GL_BLEND);
        }
    }
}

unsafe fn program(gl: &Gl, api: c_int) -> Option<u32> {
    let version = if api == 0 { "#version 100\nprecision mediump float;\n" } else { "#version 120\n" };
    let stage = |kind: u32, body: &str| {
        let source = CString::new(format!("{version}{body}")).unwrap_or_default();
        let shader = (gl.create_shader)(kind);
        (gl.shader_source)(shader, 1, &source.as_ptr(), std::ptr::null());
        (gl.compile_shader)(shader);
        shader
    };
    let vertex = stage(
        0x8B31,
        "attribute vec2 corner;\nvarying vec2 uv;\nvoid main() { uv = corner; gl_Position = vec4(corner * 2.0 - 1.0, 0.0, 1.0); }\n",
    );
    let fragment = stage(
        0x8B30,
        "varying vec2 uv;\nuniform sampler2D card;\nvoid main() { gl_FragColor = vec4(texture2D(card, uv).bgr, 1.0); }\n",
    );
    let program = (gl.create_program)();
    (gl.attach_shader)(program, vertex);
    (gl.attach_shader)(program, fragment);
    (gl.bind_attrib)(program, 0, c"corner".as_ptr());
    (gl.link_program)(program);
    (gl.delete_shader)(vertex);
    (gl.delete_shader)(fragment);
    let mut linked = 0;
    (gl.get_program)(program, 0x8B82, &mut linked);
    (linked != 0).then_some(program)
}

unsafe extern "C" fn draw(params: *const DrawParams, card: *mut c_void) {
    (*card.cast::<Card>()).draw(&*params);
}

unsafe extern "C" fn is_damaged(card: *mut c_void) -> bool {
    (*card.cast::<Card>()).is_damaged()
}

unsafe extern "C" fn clear_damage(_card: *mut c_void) {}

/// # Safety
/// Called by gpu-screen-recorder with its GL context current.
#[no_mangle]
pub unsafe extern "C" fn gsr_plugin_init(params: *const InitParams, ret: *mut InitReturn) -> bool {
    const NAME: &CStr = c"lapse";
    (*ret).name = NAME.as_ptr();
    (*ret).version = 1;
    // Without the card the recording still has to work, so a failure only leaves it out.
    if let Some(card) = Card::new(&*params) {
        (*ret).userdata = Box::into_raw(Box::new(card)).cast();
        (*ret).draw = Some(draw);
        (*ret).is_damaged = Some(is_damaged);
        (*ret).clear_damage = Some(clear_damage);
    } else {
        eprintln!("lapse plugin: the focus card is unavailable");
    }
    true
}

/// # Safety
/// `card` is the userdata `gsr_plugin_init` returned.
#[no_mangle]
pub unsafe extern "C" fn gsr_plugin_deinit(card: *mut c_void) {
    if !card.is_null() {
        let card = Box::from_raw(card.cast::<Card>());
        munmap(card.map.cast(), FILE_SIZE);
    }
}
