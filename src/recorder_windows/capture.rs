use anyhow::Result;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use windows_capture::capture::{Context as CaptureContext, GraphicsCaptureApiHandler};
use windows_capture::encoder::*;
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::*;

use crate::recorder_windows::audio::AudioMixer;
use crate::recorder_windows::focus_card::{FocusCard, BACKGROUND};
use crate::recorder_windows::focus_gate::{FocusGate, FocusSettings};
use crate::recorder_windows::segment::{
    decide_scaler, downscale_cpu_bottom_up, max_buffer_segments, prune_old_segments,
    select_segments_for_duration, ScalerChoice, SegmentInfo,
};

#[derive(Clone)]
pub struct SendWrapper<T>(pub T);
unsafe impl<T> Send for SendWrapper<T> {}
unsafe impl<T> Sync for SendWrapper<T> {}

impl<T: Clone> SendWrapper<T> {
    pub fn get_clone(&self) -> T {
        self.0.clone()
    }
}

#[cfg(windows)]
pub fn read_stream_bytes(
    stream: &windows::Storage::Streams::InMemoryRandomAccessStream,
) -> Result<Vec<u8>> {
    use windows::Storage::Streams::DataReader;
    stream.Seek(0).map_err(|e| anyhow::anyhow!("Seek failed: {e}"))?;
    let size = stream.Size().map_err(|e| anyhow::anyhow!("Size failed: {e}"))? as usize;
    if size == 0 {
        return Ok(Vec::new());
    }
    let input_stream = stream
        .GetInputStreamAt(0)
        .map_err(|e| anyhow::anyhow!("GetInputStreamAt failed: {e}"))?;
    let reader = DataReader::CreateDataReader(&input_stream)
        .map_err(|e| anyhow::anyhow!("CreateDataReader failed: {e}"))?;
    let op = reader
        .LoadAsync(size as u32)
        .map_err(|e| anyhow::anyhow!("LoadAsync failed: {e}"))?;
    let _ = op.get().map_err(|e| anyhow::anyhow!("LoadAsync get failed: {e}"))?;
    let mut buffer = vec![0u8; size];
    reader
        .ReadBytes(&mut buffer)
        .map_err(|e| anyhow::anyhow!("ReadBytes failed: {e}"))?;
    Ok(buffer)
}

#[cfg(windows)]
pub fn current_qpc_100ns() -> i64 {
    use windows_sys::Win32::System::Performance::{
        QueryPerformanceCounter, QueryPerformanceFrequency,
    };
    unsafe {
        let mut qpc: i64 = 0;
        let mut freq: i64 = 0;
        QueryPerformanceCounter(&mut qpc);
        QueryPerformanceFrequency(&mut freq);
        if freq > 0 {
            (qpc as i128 * 10_000_000 / freq as i128) as i64
        } else {
            0
        }
    }
}

#[cfg(not(windows))]
pub fn current_qpc_100ns() -> i64 {
    0
}

enum EncoderJob {
    SendFrame {
        surface: SendWrapper<windows::Graphics::DirectX::Direct3D11::IDirect3DSurface>,
        texture: SendWrapper<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D>,
        timestamp: windows::Foundation::TimeSpan,
        width: u32,
        height: u32,
        reply: std::sync::mpsc::SyncSender<Result<()>>,
    },
    SendBuffer {
        buffer: Vec<u8>,
        timestamp: i64,
        reply: std::sync::mpsc::SyncSender<Result<()>>,
    },
    Finish {
        reply: std::sync::mpsc::SyncSender<Result<()>>,
    },
}

/// A wrapper around `VideoEncoder` running on a worker thread with timeouts.
pub struct SegmentEncoderInner {
    job_tx: std::sync::mpsc::SyncSender<EncoderJob>,
    stuck: Arc<AtomicBool>,
    segment_path: PathBuf,
    ram_stream: Option<SendWrapper<windows::Storage::Streams::InMemoryRandomAccessStream>>,
}

unsafe impl Send for SegmentEncoderInner {}
unsafe impl Sync for SegmentEncoderInner {}

impl SegmentEncoderInner {
    pub fn new(
        dev: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
        ctx: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
        mut encoder: VideoEncoder,
        segment_path: PathBuf,
        ram_stream: Option<windows::Storage::Streams::InMemoryRandomAccessStream>,
    ) -> Self {
        let (job_tx, job_rx) = std::sync::mpsc::sync_channel::<EncoderJob>(2);
        let stuck = Arc::new(AtomicBool::new(false));
        let dev_clone = SendWrapper(dev.clone());
        let ctx_clone = SendWrapper(ctx.clone());

        std::thread::Builder::new()
            .name("lapse-seg-encoder".into())
            .spawn(move || {
                let mut dummy_vec = Vec::new();
                while let Ok(job) = job_rx.recv() {
                    match job {
                        EncoderJob::SendFrame {
                            surface,
                            texture,
                            timestamp,
                            width,
                            height,
                            reply,
                        } => {
                            let mut frame = Frame::new(
                                &dev_clone.0,
                                surface.0,
                                texture.0,
                                timestamp,
                                &ctx_clone.0,
                                &mut dummy_vec,
                                width,
                                height,
                                ColorFormat::Bgra8,
                                None,
                            );
                            let res = encoder
                                .send_frame(&mut frame)
                                .map_err(|e| anyhow::anyhow!("send_frame error: {e}"));
                            let _ = reply.send(res);
                        }
                        EncoderJob::SendBuffer {
                            buffer,
                            timestamp,
                            reply,
                        } => {
                            let res = encoder
                                .send_frame_buffer(&buffer, timestamp)
                                .map_err(|e| anyhow::anyhow!("send_frame_buffer error: {e}"));
                            let _ = reply.send(res);
                        }
                        EncoderJob::Finish { reply } => {
                            let res = encoder
                                .finish()
                                .map_err(|e| anyhow::anyhow!("finish error: {e}"));
                            let _ = reply.send(res);
                            return;
                        }
                    }
                }
            })
            .expect("failed to spawn segment encoder thread");

        Self {
            job_tx,
            stuck,
            segment_path,
            ram_stream: ram_stream.map(SendWrapper),
        }
    }

    pub fn send_frame(&self, frame: &mut Frame) -> Result<()> {
        if self.stuck.load(Ordering::Acquire) {
            anyhow::bail!("Encoder is stuck");
        }
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
        let job = EncoderJob::SendFrame {
            surface: SendWrapper(unsafe { frame.as_raw_surface().clone() }),
            texture: SendWrapper(unsafe { frame.as_raw_texture().clone() }),
            timestamp: frame.timestamp(),
            width: frame.width(),
            height: frame.height(),
            reply: reply_tx,
        };
        self.job_tx
            .send(job)
            .map_err(|_| anyhow::anyhow!("Encoder thread stopped"))?;
        match reply_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(res) => res,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                self.stuck.store(true, Ordering::Release);
                anyhow::bail!("send_frame timed out after 2 s");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.stuck.store(true, Ordering::Release);
                anyhow::bail!("Encoder thread disconnected");
            }
        }
    }

    pub fn send_frame_buffer(&self, buffer: &[u8], timestamp: i64) -> Result<()> {
        if self.stuck.load(Ordering::Acquire) {
            anyhow::bail!("Encoder is stuck");
        }
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
        let job = EncoderJob::SendBuffer {
            buffer: buffer.to_vec(),
            timestamp,
            reply: reply_tx,
        };
        self.job_tx
            .send(job)
            .map_err(|_| anyhow::anyhow!("Encoder thread stopped"))?;
        match reply_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(res) => res,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                self.stuck.store(true, Ordering::Release);
                anyhow::bail!("send_frame_buffer timed out after 2 s");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.stuck.store(true, Ordering::Release);
                anyhow::bail!("Encoder thread disconnected");
            }
        }
    }

    pub fn finish(&self) -> Result<()> {
        if self.stuck.load(Ordering::Acquire) {
            return Ok(());
        }
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
        if self.job_tx.send(EncoderJob::Finish { reply: reply_tx }).is_ok() {
            if let Ok(res) = reply_rx.recv_timeout(Duration::from_secs(2)) {
                res?;
                if let Some(ref stream_wrapper) = self.ram_stream {
                    let bytes = read_stream_bytes(&stream_wrapper.0)?;
                    crate::recorder_windows::mem_store::put(&self.segment_path, bytes);
                }
            }
        }
        Ok(())
    }

    pub fn is_stuck(&self) -> bool {
        self.stuck.load(Ordering::Acquire)
    }

    pub fn mark_stuck(&self) {
        self.stuck.store(true, Ordering::Release);
    }
}

#[cfg(windows)]
pub fn trim_working_set() {
    use windows_sys::Win32::System::ProcessStatus::EmptyWorkingSet;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        EmptyWorkingSet(GetCurrentProcess());
    }
}

#[cfg(not(windows))]
pub fn trim_working_set() {}

#[cfg(windows)]
pub fn get_working_set_bytes() -> usize {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        let mut pmc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, cb) != 0 {
            pmc.WorkingSetSize
        } else {
            0
        }
    }
}

#[cfg(not(windows))]
pub fn get_working_set_bytes() -> usize {
    0
}

#[cfg(windows)]
fn create_reusable_texture(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    width: u32,
    height: u32,
) -> Result<(
    windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    windows::Graphics::DirectX::Direct3D11::IDirect3DSurface,
)> {
    use windows::core::Interface;
    use windows::Graphics::DirectX::Direct3D11::IDirect3DSurface;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC,
        D3D11_USAGE_DEFAULT,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
    use windows::Win32::Graphics::Dxgi::IDXGISurface;
    use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11SurfaceFromDXGISurface;

    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT(ColorFormat::Bgra8 as i32),
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };

    let mut texture = None;
    unsafe {
        device
            .CreateTexture2D(&desc, None, Some(&mut texture))
            .map_err(|e| anyhow::anyhow!("CreateTexture2D failed: {e}"))?;
    }
    let texture = texture.ok_or_else(|| anyhow::anyhow!("Texture was None"))?;
    let dxgi_surface: IDXGISurface = texture
        .cast()
        .map_err(|e| anyhow::anyhow!("Cast to IDXGISurface failed: {e}"))?;
    let inspectable = unsafe {
        CreateDirect3D11SurfaceFromDXGISurface(&dxgi_surface)
            .map_err(|e| anyhow::anyhow!("CreateDirect3D11SurfaceFromDXGISurface failed: {e}"))?
    };
    let surface: IDirect3DSurface = inspectable
        .cast()
        .map_err(|e| anyhow::anyhow!("Cast to IDirect3DSurface failed: {e}"))?;

    Ok((texture, surface))
}

#[cfg(windows)]
fn create_cpu_staging_texture(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    width: u32,
    height: u32,
) -> Result<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D> {
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CPU_ACCESS_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};

    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT(ColorFormat::Bgra8 as i32),
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };

    let mut texture = None;
    unsafe {
        device
            .CreateTexture2D(&desc, None, Some(&mut texture))
            .map_err(|e| anyhow::anyhow!("CreateTexture2D CPU staging failed: {e}"))?;
    }
    texture.ok_or_else(|| anyhow::anyhow!("CPU staging texture was None"))
}

#[cfg(windows)]
fn save_texture_to_png(
    context: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    gpu_texture: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    cpu_staging: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    width: u32,
    height: u32,
    path: &Path,
) -> Result<()> {
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    };

    unsafe {
        context.CopyResource(cpu_staging, gpu_texture);
    }

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        context
            .Map(cpu_staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .map_err(|e| anyhow::anyhow!("Map for screenshot failed: {e}"))?;
    }

    if mapped.pData.is_null() {
        unsafe {
            context.Unmap(cpu_staging, 0);
        }
        anyhow::bail!("Mapped pData is null");
    }

    let row_pitch = mapped.RowPitch as usize;
    let w = width as usize;
    let h = height as usize;
    let w4 = w * 4;

    let mut rgba = vec![0u8; w * h * 4];
    let src_slice = unsafe {
        std::slice::from_raw_parts(mapped.pData as *const u8, h * row_pitch)
    };

    for y in 0..h {
        let src_row = &src_slice[y * row_pitch..y * row_pitch + w4];
        let dst_row = &mut rgba[y * w4..(y + 1) * w4];
        for x in 0..w {
            let px = x * 4;
            dst_row[px] = src_row[px + 2];
            dst_row[px + 1] = src_row[px + 1];
            dst_row[px + 2] = src_row[px];
            dst_row[px + 3] = src_row[px + 3];
        }
    }

    unsafe {
        context.Unmap(cpu_staging, 0);
    }

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    image::save_buffer_with_format(
        path,
        &rgba,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|e| anyhow::anyhow!("Failed to write screenshot file: {e}"))?;

    Ok(())
}

#[cfg(windows)]
pub struct D3D11VideoScaler {
    video_context: windows::Win32::Graphics::Direct3D11::ID3D11VideoContext,
    video_processor: windows::Win32::Graphics::Direct3D11::ID3D11VideoProcessor,
    input_view: windows::Win32::Graphics::Direct3D11::ID3D11VideoProcessorInputView,
    output_view: windows::Win32::Graphics::Direct3D11::ID3D11VideoProcessorOutputView,
}

unsafe impl Send for D3D11VideoScaler {}
unsafe impl Sync for D3D11VideoScaler {}

#[cfg(windows)]
impl D3D11VideoScaler {
    pub fn new(
        device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
        device_context: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
        input_texture: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
        output_texture: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
        in_w: u32,
        in_h: u32,
        out_w: u32,
        out_h: u32,
        fps: u32,
    ) -> Result<Self> {
        use windows::core::Interface;
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
            D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
            D3D11_VIDEO_USAGE_OPTIMAL_QUALITY, D3D11_VPIV_DIMENSION_TEXTURE2D,
            D3D11_VPOV_DIMENSION_TEXTURE2D, ID3D11VideoContext, ID3D11VideoDevice,
        };
        use windows::Win32::Graphics::Dxgi::Common::DXGI_RATIONAL;

        let video_device: ID3D11VideoDevice = device
            .cast()
            .map_err(|e| anyhow::anyhow!("Cast to ID3D11VideoDevice failed: {e}"))?;
        let video_context: ID3D11VideoContext = device_context
            .cast()
            .map_err(|e| anyhow::anyhow!("Cast to ID3D11VideoContext failed: {e}"))?;

        let content_desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL {
                Numerator: fps.max(1),
                Denominator: 1,
            },
            InputWidth: in_w,
            InputHeight: in_h,
            OutputFrameRate: DXGI_RATIONAL {
                Numerator: fps.max(1),
                Denominator: 1,
            },
            OutputWidth: out_w,
            OutputHeight: out_h,
            Usage: D3D11_VIDEO_USAGE_OPTIMAL_QUALITY,
        };

        let enumerator = unsafe {
            video_device
                .CreateVideoProcessorEnumerator(&content_desc)
                .map_err(|e| anyhow::anyhow!("CreateVideoProcessorEnumerator failed: {e}"))?
        };

        let video_processor = unsafe {
            video_device
                .CreateVideoProcessor(&enumerator, 0)
                .map_err(|e| anyhow::anyhow!("CreateVideoProcessor failed: {e}"))?
        };

        let mut in_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC::default();
        in_desc.ViewDimension = D3D11_VPIV_DIMENSION_TEXTURE2D;
        let mut input_view = None;
        unsafe {
            video_device
                .CreateVideoProcessorInputView(
                    input_texture,
                    &enumerator,
                    &in_desc,
                    Some(&mut input_view),
                )
                .map_err(|e| anyhow::anyhow!("CreateVideoProcessorInputView failed: {e}"))?;
        }
        let input_view = input_view.ok_or_else(|| anyhow::anyhow!("input_view was None"))?;

        let mut out_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC::default();
        out_desc.ViewDimension = D3D11_VPOV_DIMENSION_TEXTURE2D;
        let mut output_view = None;
        unsafe {
            video_device
                .CreateVideoProcessorOutputView(
                    output_texture,
                    &enumerator,
                    &out_desc,
                    Some(&mut output_view),
                )
                .map_err(|e| anyhow::anyhow!("CreateVideoProcessorOutputView failed: {e}"))?;
        }
        let output_view = output_view.ok_or_else(|| anyhow::anyhow!("output_view was None"))?;

        Ok(Self {
            video_context,
            video_processor,
            input_view,
            output_view,
        })
    }

    pub fn scale(&self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D11::D3D11_VIDEO_PROCESSOR_STREAM;
        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: std::ptr::null_mut(),
            pInputSurface: std::mem::ManuallyDrop::new(Some(self.input_view.clone())),
            ppFutureSurfaces: std::ptr::null_mut(),
            ppPastSurfacesRight: std::ptr::null_mut(),
            pInputSurfaceRight: std::mem::ManuallyDrop::new(None),
            ppFutureSurfacesRight: std::ptr::null_mut(),
        };
        unsafe {
            self.video_context
                .VideoProcessorBlt(&self.video_processor, &self.output_view, 0, &[stream])
                .map_err(|e| anyhow::anyhow!("VideoProcessorBlt failed: {e}"))?;
        }
        Ok(())
    }
}

struct D3D11ScaleState {
    target_width: u32,
    target_height: u32,
    scaler: D3D11VideoScaler,
    scaled_tex: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    scaled_surf: windows::Graphics::DirectX::Direct3D11::IDirect3DSurface,
}
unsafe impl Send for D3D11ScaleState {}

struct CpuScaleState {
    target_width: u32,
    target_height: u32,
    cpu_tex: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    buffer: Vec<u8>,
}
unsafe impl Send for CpuScaleState {}

enum ScaleMode {
    None,
    D3D11(D3D11ScaleState),
    Cpu(CpuScaleState),
}

#[cfg(windows)]
fn setup_scaler(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    device_context: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    staging_tex: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    capture_w: u32,
    capture_h: u32,
    target_w: u32,
    target_h: u32,
    fps: u32,
) -> (ScaleMode, bool) {
    let choice = decide_scaler(capture_w, capture_h, target_w, target_h, true);
    match choice {
        ScalerChoice::None => (ScaleMode::None, true),
        ScalerChoice::D3D11 => {
            let d3d11_res = create_reusable_texture(device, target_w, target_h).and_then(
                |(scaled_tex, scaled_surf)| {
                    let scaler = D3D11VideoScaler::new(
                        device,
                        device_context,
                        staging_tex,
                        &scaled_tex,
                        capture_w,
                        capture_h,
                        target_w,
                        target_h,
                        fps,
                    )?;
                    Ok((scaled_tex, scaled_surf, scaler))
                },
            );
            match d3d11_res {
                Ok((scaled_tex, scaled_surf, scaler)) => (
                    ScaleMode::D3D11(D3D11ScaleState {
                        target_width: target_w,
                        target_height: target_h,
                        scaler,
                        scaled_tex,
                        scaled_surf,
                    }),
                    true,
                ),
                Err(e) => {
                    eprintln!("Warning: Failed to initialize D3D11 video scaler: {e}");
                    let _ = std::io::stderr().flush();
                    match decide_scaler(capture_w, capture_h, target_w, target_h, false) {
                        ScalerChoice::Cpu => match create_cpu_staging_texture(device, capture_w, capture_h) {
                            Ok(cpu_tex) => {
                                let buffer = vec![0u8; (target_w * target_h * 4) as usize];
                                (
                                    ScaleMode::Cpu(CpuScaleState {
                                        target_width: target_w,
                                        target_height: target_h,
                                        cpu_tex,
                                        buffer,
                                    }),
                                    true,
                                )
                            }
                            Err(ce) => {
                                eprintln!("Warning: Failed to initialize CPU downscaler: {ce}");
                                let _ = std::io::stderr().flush();
                                (ScaleMode::None, false)
                            }
                        },
                        _ => (ScaleMode::None, false),
                    }
                }
            }
        }
        ScalerChoice::Cpu => match create_cpu_staging_texture(device, capture_w, capture_h) {
            Ok(cpu_tex) => {
                let buffer = vec![0u8; (target_w * target_h * 4) as usize];
                (
                    ScaleMode::Cpu(CpuScaleState {
                        target_width: target_w,
                        target_height: target_h,
                        cpu_tex,
                        buffer,
                    }),
                    true,
                )
            }
            Err(e) => {
                eprintln!("Warning: Failed to initialize CPU downscaler: {e}");
                let _ = std::io::stderr().flush();
                (ScaleMode::None, false)
            }
        },
    }
}

struct FrameStoreHolder {
    push_fn: Arc<Mutex<Box<dyn FnMut(&mut Frame) + Send>>>,
    pop_encode_fn: Box<dyn FnMut(&SegmentEncoderInner, i64) -> Result<Option<Duration>> + Send>,
    repeat_fn: Box<dyn FnMut(&SegmentEncoderInner, i64) -> Result<Duration> + Send>,
    has_frame_fn: Box<dyn Fn() -> bool + Send>,
    /// Draws the "not in the game" card when the player is away and a frame is due.
    card_fn: Option<Box<dyn FnMut(bool) + Send>>,
    set_target_resolution_fn: Box<dyn FnMut(u32, u32) -> Result<bool> + Send>,
    save_screenshot_fn: Box<dyn FnMut(&Path) -> Result<()> + Send>,
    scale_state: Arc<Mutex<ScaleMode>>,
    dropped_size_mismatch: Arc<AtomicU64>,
}

fn create_frame_store(
    ctx: &CaptureContext<CaptureFlags>,
    capture_width: u32,
    capture_height: u32,
    target_width: u32,
    target_height: u32,
    fps: u32,
    encoder_busy: Arc<AtomicBool>,
    focus: Option<FocusSettings>,
) -> Result<FrameStoreHolder> {
    let (staging_tex, staging_surf) = create_reusable_texture(&ctx.device, capture_width, capture_height)
        .map_err(|e| anyhow::anyhow!("Failed to create staging texture: {e}"))?;
    let staging_tex = SendWrapper(staging_tex);
    let staging_surf = SendWrapper(staging_surf);

    let (initial_scale_mode, _) = setup_scaler(
        &ctx.device,
        &ctx.device_context,
        &staging_tex.0,
        capture_width,
        capture_height,
        target_width,
        target_height,
        fps,
    );
    let scale_state: Arc<Mutex<ScaleMode>> = Arc::new(Mutex::new(initial_scale_mode));

    let has_frame = Arc::new(AtomicBool::new(false));
    let has_new_frame = Arc::new(AtomicBool::new(false));

    let h1 = has_frame.clone();
    let hn1 = has_new_frame.clone();
    let s_tex1 = SendWrapper(staging_tex.0.clone());
    let ctx1 = SendWrapper(ctx.device_context.clone());
    let eb_push = encoder_busy.clone();
    let dropped_size_mismatch = Arc::new(AtomicU64::new(0));
    let dropped_sm_push = dropped_size_mismatch.clone();

    let cover = focus.and_then(|f| match FocusCard::new(capture_width, capture_height, &f.title) {
        Ok(card) => Some((Arc::new(Mutex::new(FocusGate::new(f.monitor, f.games))), card)),
        Err(e) => {
            eprintln!("Warning: alt-tab card unavailable, recording the screen as it is: {e}");
            let _ = std::io::stderr().flush();
            None
        }
    });
    // The real screen while the card is shown, for screenshots.
    let away_tex: Arc<Mutex<Option<SendWrapper<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D>>>> =
        Arc::new(Mutex::new(None));
    let gate_push = cover.as_ref().map(|(gate, _)| gate.clone());
    let away_push = away_tex.clone();
    let dev_push = SendWrapper(ctx.device.clone());
    {
        use windows::core::Interface;
        // The capture, the card and a segment that is still being finished use the device
        // from different threads.
        if let Ok(mt) = ctx.device_context.cast::<windows::Win32::Graphics::Direct3D11::ID3D11Multithread>() {
            let _ = unsafe { mt.SetMultithreadProtected(true) };
        }
    }

    let push_fn = Arc::new(Mutex::new(Box::new(move |frame: &mut Frame| {
        if frame.width() != capture_width || frame.height() != capture_height {
            dropped_sm_push.fetch_add(1, Ordering::Relaxed);
            eb_push.store(false, Ordering::Release);
            return;
        }
        if let Some(gate) = &gate_push {
            let away = gate.lock().map(|mut g| g.update()).unwrap_or(false);
            if let Ok(mut shot) = away_push.lock() {
                if away {
                    if shot.is_none() {
                        *shot = create_reusable_texture(&dev_push.0, capture_width, capture_height).ok().map(|(t, _)| SendWrapper(t));
                    }
                    if let Some(t) = &*shot {
                        unsafe { ctx1.0.CopyResource(&t.0, frame.as_raw_texture()) };
                    }
                    eb_push.store(false, Ordering::Release);
                    return;
                }
                *shot = None;
            }
        }
        unsafe {
            ctx1.0.CopyResource(&s_tex1.get_clone(), frame.as_raw_texture());
        }
        h1.store(true, Ordering::Release);
        hn1.store(true, Ordering::Release);
    }) as Box<dyn FnMut(&mut Frame) + Send>));

    let ss1 = scale_state.clone();
    let dev_scale = SendWrapper(ctx.device.clone());
    let ctx_scale = SendWrapper(ctx.device_context.clone());
    let s_tex_scale = SendWrapper(staging_tex.0.clone());

    let set_target_resolution_fn = Box::new(move |new_w: u32, new_h: u32| -> Result<bool> {
        let mut guard = match ss1.lock() {
            Ok(g) => g,
            Err(_) => return Ok(false),
        };
        let (new_scale_mode, success) = setup_scaler(
            &dev_scale.0,
            &ctx_scale.0,
            &s_tex_scale.0,
            capture_width,
            capture_height,
            new_w,
            new_h,
            fps,
        );
        *guard = new_scale_mode;
        Ok(success)
    });

    let hn2 = has_new_frame.clone();
    let dev2 = SendWrapper(ctx.device.clone());
    let ctx2 = SendWrapper(ctx.device_context.clone());
    let s_tex2 = SendWrapper(staging_tex.0.clone());
    let s_surf2 = SendWrapper(staging_surf.0.clone());
    let ss2 = scale_state.clone();
    let mut dummy_buf2 = Vec::new();

    let pop_encode_fn = Box::new(
        move |encoder: &SegmentEncoderInner, qpc_ts: i64| -> Result<Option<Duration>> {
            if !hn2.swap(false, Ordering::AcqRel) {
                return Ok(None);
            }
            let ts = windows::Foundation::TimeSpan { Duration: qpc_ts };
            let mut guard = match ss2.lock() {
                Ok(g) => g,
                Err(_) => anyhow::bail!("Failed to lock scale state"),
            };
            let start = Instant::now();
            match &mut *guard {
                ScaleMode::D3D11(ss) => {
                    let _ = ss.scaler.scale();
                    let mut frame = Frame::new(
                        &dev2.0,
                        ss.scaled_surf.clone(),
                        ss.scaled_tex.clone(),
                        ts,
                        &ctx2.0,
                        &mut dummy_buf2,
                        ss.target_width,
                        ss.target_height,
                        ColorFormat::Bgra8,
                        None,
                    );
                    encoder.send_frame(&mut frame)?;
                }
                ScaleMode::Cpu(cpu) => {
                    use windows::Win32::Graphics::Direct3D11::{D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE};
                    unsafe {
                        ctx2.0.CopyResource(&cpu.cpu_tex, &s_tex2.get_clone());
                    }
                    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                    unsafe {
                        ctx2.0
                            .Map(&cpu.cpu_tex, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                            .map_err(|e| anyhow::anyhow!("Map failed: {e}"))?;
                    }
                    let row_pitch = mapped.RowPitch as usize;
                    let src_bytes = unsafe {
                        std::slice::from_raw_parts(
                            mapped.pData as *const u8,
                            (capture_height as usize) * row_pitch,
                        )
                    };
                    downscale_cpu_bottom_up(
                        src_bytes,
                        capture_width,
                        capture_height,
                        row_pitch,
                        &mut cpu.buffer,
                        cpu.target_width,
                        cpu.target_height,
                    );
                    unsafe {
                        ctx2.0.Unmap(&cpu.cpu_tex, 0);
                    }
                    encoder.send_frame_buffer(&cpu.buffer, qpc_ts)?;
                }
                ScaleMode::None => {
                    let mut frame = Frame::new(
                        &dev2.0,
                        s_surf2.get_clone(),
                        s_tex2.get_clone(),
                        ts,
                        &ctx2.0,
                        &mut dummy_buf2,
                        capture_width,
                        capture_height,
                        ColorFormat::Bgra8,
                        None,
                    );
                    encoder.send_frame(&mut frame)?;
                }
            }
            Ok(Some(start.elapsed()))
        },
    );

    let h3 = has_frame.clone();
    let dev3 = SendWrapper(ctx.device.clone());
    let ctx3 = SendWrapper(ctx.device_context.clone());
    let s_tex3 = SendWrapper(staging_tex.0.clone());
    let s_surf3 = SendWrapper(staging_surf.0.clone());
    let ss3 = scale_state.clone();
    let mut dummy_buf3 = Vec::new();

    let repeat_fn = Box::new(
        move |encoder: &SegmentEncoderInner, qpc_ts: i64| -> Result<Duration> {
            if !h3.load(Ordering::Acquire) {
                return Ok(Duration::ZERO);
            }
            let ts = windows::Foundation::TimeSpan { Duration: qpc_ts };
            let mut guard = match ss3.lock() {
                Ok(g) => g,
                Err(_) => anyhow::bail!("Failed to lock scale state"),
            };
            let start = Instant::now();
            match &mut *guard {
                ScaleMode::D3D11(ss) => {
                    let _ = ss.scaler.scale();
                    let mut frame = Frame::new(
                        &dev3.0,
                        ss.scaled_surf.clone(),
                        ss.scaled_tex.clone(),
                        ts,
                        &ctx3.0,
                        &mut dummy_buf3,
                        ss.target_width,
                        ss.target_height,
                        ColorFormat::Bgra8,
                        None,
                    );
                    encoder.send_frame(&mut frame)?;
                }
                ScaleMode::Cpu(cpu) => {
                    encoder.send_frame_buffer(&cpu.buffer, qpc_ts)?;
                }
                ScaleMode::None => {
                    let mut frame = Frame::new(
                        &dev3.0,
                        s_surf3.get_clone(),
                        s_tex3.get_clone(),
                        ts,
                        &ctx3.0,
                        &mut dummy_buf3,
                        capture_width,
                        capture_height,
                        ColorFormat::Bgra8,
                        None,
                    );
                    encoder.send_frame(&mut frame)?;
                }
            }
            Ok(start.elapsed())
        },
    );

    let card_fn = cover.map(|(gate, mut card)| {
        let (h_card, hn_card) = (has_frame.clone(), has_new_frame.clone());
        let ctx_card = SendWrapper(ctx.device_context.clone());
        let tex_card = SendWrapper(staging_tex.0.clone());
        let mut since: Option<Instant> = None;
        Box::new(move |due: bool| {
            if !gate.lock().map(|mut g| g.update()).unwrap_or(false) {
                since = None;
                return;
            }
            if since.is_some() && !due {
                return;
            }
            let entering = since.is_none();
            let start = *since.get_or_insert_with(Instant::now);
            if entering {
                let full: Vec<u8> = BACKGROUND.repeat((capture_width * capture_height) as usize);
                unsafe { ctx_card.0.UpdateSubresource(&tex_card.0, 0, None, full.as_ptr() as *const _, capture_width * 4, 0) };
            }
            let (x, y, w, h) = card.content_rect();
            let area = windows::Win32::Graphics::Direct3D11::D3D11_BOX { left: x, top: y, front: 0, right: x + w, bottom: y + h, back: 1 };
            let pixels = card.draw(start.elapsed().as_secs_f32());
            unsafe { ctx_card.0.UpdateSubresource(&tex_card.0, 0, Some(&area), pixels.as_ptr() as *const _, w * 4, 0) };
            h_card.store(true, Ordering::Release);
            hn_card.store(true, Ordering::Release);
        }) as Box<dyn FnMut(bool) + Send>
    });

    let h_check = has_frame;
    let has_frame_fn = Box::new(move || h_check.load(Ordering::Acquire));

    let dev_shot = SendWrapper(ctx.device.clone());
    let ctx_shot = SendWrapper(ctx.device_context.clone());
    let s_tex_shot = SendWrapper(staging_tex.0.clone());
    let cpu_staging_shot = create_cpu_staging_texture(&ctx.device, capture_width, capture_height).ok();
    let mut cpu_staging_holder = cpu_staging_shot.map(SendWrapper);
    let save_screenshot_fn = Box::new(move |path: &Path| -> Result<()> {
        if cpu_staging_holder.is_none() {
            if let Ok(tex) = create_cpu_staging_texture(&dev_shot.0, capture_width, capture_height) {
                cpu_staging_holder = Some(SendWrapper(tex));
            }
        }
        let staging = match &cpu_staging_holder {
            Some(w) => &w.0,
            None => anyhow::bail!("Failed to allocate CPU staging texture for screenshot"),
        };
        let away = away_tex.lock().ok().and_then(|t| t.as_ref().map(|t| t.0.clone()));
        save_texture_to_png(
            &ctx_shot.0,
            away.as_ref().unwrap_or(&s_tex_shot.0),
            staging,
            capture_width,
            capture_height,
            path,
        )
    });

    Ok(FrameStoreHolder {
        push_fn,
        pop_encode_fn,
        repeat_fn,
        has_frame_fn,
        card_fn,
        set_target_resolution_fn,
        save_screenshot_fn,
        scale_state,
        dropped_size_mismatch,
    })
}

#[derive(Clone)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
    pub codec: String,
    #[allow(dead_code)]
    pub audio_enabled: bool,
}

#[derive(Clone)]
pub enum SessionMode {
    Replay {
        replay_seconds: u32,
        buffer_dir: PathBuf,
        replay_storage: String,
        low_memory_guard: bool,
    },
    Continuous,
}

pub struct SaveRequest {
    pub requested_seconds: u32,
    pub game: Option<String>,
    pub label: Option<String>,
    pub response_tx: Option<std::sync::mpsc::SyncSender<std::result::Result<PathBuf, String>>>,
}

pub struct ScreenshotRequest {
    pub path: PathBuf,
    pub response_tx: Option<std::sync::mpsc::SyncSender<std::result::Result<PathBuf, String>>>,
}

pub struct SharedCaptureState {
    pub save_request: Mutex<Option<SaveRequest>>,
    pub segments: Mutex<Vec<SegmentInfo>>,
    pub current_segment_index: AtomicU64,
    pub last_saved: Mutex<Option<String>>,
    pub save_counter: AtomicU64,
    pub base_save_path: PathBuf,
    pub markers: Mutex<Vec<(SystemTime, crate::markers::MarkKind)>>,
    pub notify: AtomicBool,
    pub save_sound: bool,
    pub save_sound_path: String,
    pub screenshot_request: Mutex<Option<ScreenshotRequest>>,
    pub last_error: Mutex<Option<String>>,
}

impl SharedCaptureState {
    pub fn new(base_save_path: PathBuf, save_sound: bool, save_sound_path: String) -> Self {
        Self {
            save_request: Mutex::new(None),
            segments: Mutex::new(Vec::new()),
            current_segment_index: AtomicU64::new(1),
            last_saved: Mutex::new(None),
            save_counter: AtomicU64::new(0),
            base_save_path,
            markers: Mutex::new(Vec::new()),
            notify: AtomicBool::new(true),
            save_sound,
            save_sound_path,
            screenshot_request: Mutex::new(None),
            last_error: Mutex::new(None),
        }
    }
}

pub struct CaptureFlags {
    pub capture_width: u32,
    pub capture_height: u32,
    pub initial_encoder: VideoEncoder,
    pub initial_ram_stream: Option<windows::Storage::Streams::InMemoryRandomAccessStream>,
    pub initial_segment_path: PathBuf,
    pub encoder_config: EncoderConfig,
    pub mode: SessionMode,
    pub shared: Arc<SharedCaptureState>,
    pub audio_mixer: Option<Arc<AudioMixer>>,
    /// Games only mode: cover the picture while the player is away from the game.
    pub focus: Option<FocusSettings>,
}

pub struct CaptureHandler {
    push_fn: Arc<Mutex<Box<dyn FnMut(&mut Frame) + Send>>>,
    encode_thread: Option<std::thread::JoinHandle<()>>,
    stop_signal: Arc<AtomicBool>,
    encoder_busy: Arc<AtomicBool>,
    capture_frames_arrived: Arc<AtomicU64>,
    dropped_encoder_busy: Arc<AtomicU64>,
    dropped_push_lock: Arc<AtomicU64>,
}

impl GraphicsCaptureApiHandler for CaptureHandler {
    type Flags = CaptureFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(mut ctx: CaptureContext<Self::Flags>) -> std::result::Result<Self, Self::Error> {
        let capture_width = ctx.flags.capture_width;
        let capture_height = ctx.flags.capture_height;
        let enc_width = ctx.flags.encoder_config.width;
        let enc_height = ctx.flags.encoder_config.height;
        let fps = ctx.flags.encoder_config.fps;

        let stop_signal = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_signal.clone();
        let encoder_busy = Arc::new(AtomicBool::new(false));
        let encoder_busy_clone = encoder_busy.clone();

        let capture_frames_arrived = Arc::new(AtomicU64::new(0));
        let dropped_encoder_busy = Arc::new(AtomicU64::new(0));
        let dropped_push_lock = Arc::new(AtomicU64::new(0));

        let focus = ctx.flags.focus.take();
        let frame_store = create_frame_store(
            &ctx,
            capture_width,
            capture_height,
            enc_width,
            enc_height,
            fps,
            encoder_busy.clone(),
            focus,
        )?;
        let flags = ctx.flags;

        let push_fn = frame_store.push_fn;
        let mut pop_encode_fn = frame_store.pop_encode_fn;
        let mut repeat_fn = frame_store.repeat_fn;
        let has_frame_fn = frame_store.has_frame_fn;
        let mut card_fn = frame_store.card_fn;
        let mut set_target_resolution_fn = frame_store.set_target_resolution_fn;
        let mut save_screenshot_fn = frame_store.save_screenshot_fn;
        let scale_state_holder = frame_store.scale_state;
        let dropped_size_mismatch = frame_store.dropped_size_mismatch;

        let shared = flags.shared;
        let shared_clone = shared.clone();
        let encoder_config = flags.encoder_config;
        let mode = flags.mode;
        let initial_encoder = flags.initial_encoder;
        let initial_segment_path = flags.initial_segment_path;
        let audio_mixer = flags.audio_mixer;

        let dev_worker = SendWrapper(ctx.device.clone());
        let ctx_worker = SendWrapper(ctx.device_context.clone());
        let dev_w = dev_worker.clone();
        let ctx_w = ctx_worker.clone();

        let frame_interval = Duration::from_nanos(1_000_000_000 / fps.max(1) as u64);

        let initial_is_ram = flags.initial_ram_stream.is_some();
        let initial_segment_encoder = Arc::new(SegmentEncoderInner::new(
            &dev_worker.0,
            &ctx_worker.0,
            initial_encoder,
            initial_segment_path.clone(),
            flags.initial_ram_stream,
        ));
        let initial_start_qpc = current_qpc_100ns();
        if let Some(ref mixer) = audio_mixer {
            let _ = mixer
                .rotate_segment(Some(initial_segment_path.with_extension("m4a")), initial_start_qpc, initial_is_ram)
                .recv_timeout(Duration::from_millis(500));
        }

        let capture_frames_counter = capture_frames_arrived.clone();
        let dropped_eb = dropped_encoder_busy.clone();
        let dropped_pl = dropped_push_lock.clone();
        let dropped_sm = dropped_size_mismatch.clone();
        let scale_state_for_guard = scale_state_holder.clone();
        let last_encode_err = Arc::new(Mutex::new(None::<String>));
        let last_encode_err_clone = last_encode_err.clone();

        let encode_thread = std::thread::spawn(move || {
            let mut current_encoder = initial_segment_encoder;
            let mut current_segment_path = initial_segment_path;
            let mut current_seg_start_qpc = initial_start_qpc;
            let mut current_is_ram = initial_is_ram;
            let mut segment_start = Instant::now();
            let mut frames_in_segment = 0u64;
            let mut last_sent_qpc = initial_start_qpc;
            let mut last_frame_instant = Instant::now();
            let target_interval = frame_interval;
            let mut last_mem_log = Instant::now();

            let mut perf_window_start = Instant::now();
            let mut perf_window_frames = 0u64;
            let mut auto_scaled = encoder_config.height <= 1080 && capture_height > 1080;
            let mut current_encoder_config = encoder_config.clone();
            let mut logged_zero_frames = false;
            let mut prepared: Option<std::sync::mpsc::Receiver<PreparedSegment>> = None;
            let mut finishing: Option<std::thread::JoinHandle<()>> = None;

            let check_zero_frames_guard = |frames: u64,
                                           cfg: &EncoderConfig,
                                           logged: &mut bool| {
                if frames == 0 {
                    let cap_arrived = capture_frames_counter.load(Ordering::Relaxed);
                    if cap_arrived > 0 && !*logged {
                        *logged = true;
                        let reason = if dropped_sm.load(Ordering::Relaxed) > 0 {
                            format!(
                                "capture frame size mismatch (expected {}x{})",
                                capture_width, capture_height
                            )
                        } else if dropped_pl.load(Ordering::Relaxed) > 0 {
                            "push_fn lock failed".to_string()
                        } else if dropped_eb.load(Ordering::Relaxed) > 0 {
                            format!(
                                "dropped by encoder_busy guard ({} frames)",
                                dropped_eb.load(Ordering::Relaxed)
                            )
                        } else if let Ok(guard) = last_encode_err_clone.lock() {
                            if let Some(ref err) = *guard {
                                format!("encoder error: {err}")
                            } else {
                                let mode_desc = match scale_state_for_guard.lock() {
                                    Ok(g) => match &*g {
                                        ScaleMode::None => format!(
                                            "ScaleMode::None (capture {}x{}, encoder {}x{})",
                                            capture_width, capture_height, cfg.width, cfg.height
                                        ),
                                        ScaleMode::D3D11(_) => "ScaleMode::D3D11".to_string(),
                                        ScaleMode::Cpu(_) => "ScaleMode::Cpu".to_string(),
                                    },
                                    Err(_) => "ScaleMode lock error".to_string(),
                                };
                                format!("no frames encoded by encoder loop (scale_mode: {mode_desc})")
                            }
                        } else {
                            "no frames encoded by encoder loop".to_string()
                        };
                        eprintln!(
                            "Warning: Segment ended with 0 frames while {} capture frames arrived: {}",
                            cap_arrived, reason
                        );
                        let _ = std::io::stderr().flush();
                    }
                }
            };

            while !stop_clone.load(Ordering::Relaxed) {
                if last_mem_log.elapsed() >= Duration::from_secs(30) {
                    let ws_bytes = get_working_set_bytes();
                    let ws_mb = ws_bytes as f64 / (1024.0 * 1024.0);
                    eprintln!("Memory working set: {:.1} MB", ws_mb);
                    let _ = std::io::stderr().flush();
                    last_mem_log = Instant::now();
                }

                if !auto_scaled && perf_window_start.elapsed() >= Duration::from_secs(2) {
                    let elapsed_secs = perf_window_start.elapsed().as_secs_f64();
                    let avg_fps = perf_window_frames as f64 / elapsed_secs;
                    let target_fps = current_encoder_config.fps as f64;
                    if avg_fps < target_fps * 0.8 && current_encoder_config.height > 1080 {
                        let scale = 1080.0 / current_encoder_config.height as f64;
                        let new_h = 1080;
                        let new_w = ((current_encoder_config.width as f64 * scale).round() as u32) & !1;
                        auto_scaled = true;
                        eprintln!(
                            "Encoder underperforming ({:.1} fps < {:.1} fps target): downscaling to {}x{} immediately",
                            avg_fps, target_fps * 0.8, new_w, new_h
                        );
                        let _ = std::io::stderr().flush();

                        crate::recorder_windows::segment::set_cached_downscale(
                            capture_width,
                            capture_height,
                            "Media Foundation",
                            new_w,
                            new_h,
                        );

                        if let SessionMode::Replay {
                            ref buffer_dir,
                            ref replay_storage,
                            low_memory_guard,
                            ..
                        } = &mode {
                            let _ = current_encoder.finish();
                            eprintln!(
                                "Abandoning invalid segment (underperforming): {}",
                                current_segment_path.display()
                            );
                            let _ = std::io::stderr().flush();
                            let _ = crate::recorder_windows::mem_store::remove(&current_segment_path);
                            let _ = std::fs::remove_file(&current_segment_path);
                            let _ = crate::recorder_windows::mem_store::remove(current_segment_path.with_extension("m4a"));
                            let _ = std::fs::remove_file(current_segment_path.with_extension("m4a"));

                            match set_target_resolution_fn(new_w, new_h) {
                                Ok(true) => {
                                    current_encoder_config.width = new_w;
                                    current_encoder_config.height = new_h;
                                    trim_working_set();
                                    let ws_mb = get_working_set_bytes() as f64 / (1024.0 * 1024.0);
                                    eprintln!("Memory working set: {:.1} MB", ws_mb);
                                    let _ = std::io::stderr().flush();
                                }
                                _ => {
                                    eprintln!(
                                        "Downscaling failed, keeping native resolution {}x{}",
                                        capture_width, capture_height
                                    );
                                    let _ = std::io::stderr().flush();
                                }
                            }

                            let next_is_ram = crate::recorder_windows::segment::decide_replay_storage(
                                replay_storage,
                                *low_memory_guard,
                                current_is_ram,
                                crate::config::available_ram_mb(),
                                crate::config::total_ram_mb(),
                            );
                            if current_is_ram && !next_is_ram && replay_storage == "ram" && *low_memory_guard {
                                eprintln!("Low memory detected; spilling replay buffer to disk");
                                let _ = std::io::stderr().flush();
                                let _ = crate::recorder_windows::mem_store::spill_all();
                                crate::utils::send_notification(
                                    "Lapse",
                                    "Memory is running low, so the replay buffer moved to disk. Clips are still saved normally.",
                                    "dialog-warning",
                                );
                            }

                            let next_idx = shared_clone.current_segment_index.fetch_add(1, Ordering::SeqCst) + 1;
                            let next_path = buffer_dir.join(format!("seg_{}.mp4", next_idx));
                            let next_audio_path = buffer_dir.join(format!("seg_{}.m4a", next_idx));
                            let next_start_qpc = current_qpc_100ns();

                            if let Some(ref mixer) = audio_mixer {
                                let _ = mixer.rotate_segment(Some(next_audio_path), next_start_qpc, next_is_ram);
                            }

                            match create_encoder_for_target(&current_encoder_config, &next_path, next_is_ram) {
                                Ok((next_enc, next_ram_stream)) => {
                                    eprintln!("Segment rotated to downscaled resolution: {}", next_path.display());
                                    let _ = std::io::stderr().flush();

                                    current_encoder = Arc::new(SegmentEncoderInner::new(&dev_w.0, &ctx_w.0, next_enc, next_path.clone(), next_ram_stream));
                                    current_segment_path = next_path;
                                    current_seg_start_qpc = next_start_qpc;
                                    current_is_ram = next_is_ram;
                                    segment_start = Instant::now();
                                    frames_in_segment = 0;
                                    last_sent_qpc = next_start_qpc;

                                    if has_frame_fn() {
                                        let _ = (repeat_fn)(&current_encoder, next_start_qpc);
                                        frames_in_segment += 1;
                                        last_frame_instant = Instant::now();
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Failed to start next replay segment: {e}");
                                    let _ = std::io::stderr().flush();
                                    segment_start = Instant::now();
                                }
                            }
                        }
                    } else {
                        auto_scaled = true;
                    }
                    perf_window_start = Instant::now();
                    perf_window_frames = 0;
                }

                if let Ok(mut req) = shared_clone.screenshot_request.lock() {
                    if let Some(req_val) = req.take() {
                        if has_frame_fn() {
                            let res = (save_screenshot_fn)(&req_val.path);
                            match res {
                                Ok(()) => {
                                    eprintln!("Screenshot: {}", req_val.path.display());
                                    let _ = std::io::stderr().flush();
                                    if shared_clone.save_sound {
                                        crate::sound::play_save_sound(&shared_clone.save_sound_path);
                                    }
                                    if shared_clone.notify.load(Ordering::Relaxed) {
                                        let name = req_val
                                            .path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .to_string();
                                        crate::utils::send_notification(
                                            "Lapse - Screenshot",
                                            &format!("Saved {}", name),
                                            "media-record",
                                        );
                                    }
                                    if let Some(tx) = req_val.response_tx {
                                        let _ = tx.send(Ok(req_val.path));
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Screenshot error: {e}");
                                    let _ = std::io::stderr().flush();
                                    if let Some(tx) = req_val.response_tx {
                                        let _ = tx.send(Err(e.to_string()));
                                    }
                                }
                            }
                        } else {
                            *req = Some(req_val);
                        }
                    }
                }

                let save_req = shared_clone.save_request.lock().ok().and_then(|mut r| r.take());
                let rotate_due = segment_start.elapsed() >= Duration::from_secs(10);
                let encoder_stuck = current_encoder.is_stuck();
                let should_rotate = match &mode {
                    SessionMode::Replay { .. } => save_req.is_some() || rotate_due || encoder_stuck,
                    SessionMode::Continuous => encoder_stuck,
                };

                if matches!(mode, SessionMode::Replay { .. }) && prepared.is_none() && !should_rotate && segment_start.elapsed() >= PREPARE_AFTER {
                    if let SessionMode::Replay { ref buffer_dir, ref replay_storage, low_memory_guard, .. } = &mode {
                        let is_ram = crate::recorder_windows::segment::decide_replay_storage(
                            replay_storage,
                            *low_memory_guard,
                            current_is_ram,
                            crate::config::available_ram_mb(),
                            crate::config::total_ram_mb(),
                        );
                        let idx = shared_clone.current_segment_index.fetch_add(1, Ordering::SeqCst) + 1;
                        let path = buffer_dir.join(format!("seg_{}.mp4", idx));
                        prepared = Some(prepare_segment(&dev_w, &ctx_w, &current_encoder_config, path, is_ram));
                    }
                }

                if should_rotate {
                    let mut audio_done = None;
                    let mut ended = None;
                    if let SessionMode::Replay {
                        replay_seconds,
                        ref buffer_dir,
                        ref replay_storage,
                        low_memory_guard,
                    } = &mode {
                        let next_start_qpc = current_qpc_100ns();

                        if encoder_stuck || frames_in_segment == 0 {
                            check_zero_frames_guard(
                                frames_in_segment,
                                &current_encoder_config,
                                &mut logged_zero_frames,
                            );
                            eprintln!(
                                "Abandoning invalid segment (stuck={}, frames={}): {}",
                                encoder_stuck,
                                frames_in_segment,
                                current_segment_path.display()
                            );
                            let _ = std::io::stderr().flush();
                            let _ = current_encoder.finish();
                            let _ = crate::recorder_windows::mem_store::remove(&current_segment_path);
                            let _ = std::fs::remove_file(&current_segment_path);
                            let _ = crate::recorder_windows::mem_store::remove(current_segment_path.with_extension("m4a"));
                            let _ = std::fs::remove_file(current_segment_path.with_extension("m4a"));
                        } else {
                            let now_qpc = current_qpc_100ns();
                            if now_qpc > last_sent_qpc + 500_000 && has_frame_fn() {
                                let _ = (repeat_fn)(&current_encoder, now_qpc);
                            }

                            ended = Some((current_encoder.clone(), current_segment_path.clone(), segment_start.elapsed().as_secs_f64(), current_seg_start_qpc));
                        }

                        let next_is_ram = crate::recorder_windows::segment::decide_replay_storage(
                            replay_storage,
                            *low_memory_guard,
                            current_is_ram,
                            crate::config::available_ram_mb(),
                            crate::config::total_ram_mb(),
                        );
                        if current_is_ram && !next_is_ram && replay_storage == "ram" && *low_memory_guard {
                            eprintln!("Low memory detected; spilling replay buffer to disk");
                            let _ = std::io::stderr().flush();
                            let _ = crate::recorder_windows::mem_store::spill_all();
                            crate::utils::send_notification(
                                "Lapse",
                                "Memory is running low, so the replay buffer moved to disk. Clips are still saved normally.",
                                "dialog-warning",
                            );
                        }

                        let size = (current_encoder_config.width, current_encoder_config.height);
                        let ready = match prepared.take().map(|rx| (rx.try_recv(), rx)) {
                            Some((Ok(p), _)) if p.size == size && p.is_ram == next_is_ram => Some(p),
                            Some((Ok(p), _)) => {
                                std::thread::spawn(move || {
                                    drop(p.encoder);
                                    let _ = std::fs::remove_file(&p.path);
                                });
                                None
                            }
                            Some((Err(_), rx)) => {
                                discard_prepared(rx);
                                None
                            }
                            None => None,
                        };
                        let next = match ready {
                            Some(p) => Ok((p.encoder, p.path)),
                            None => {
                                let idx = shared_clone.current_segment_index.fetch_add(1, Ordering::SeqCst) + 1;
                                let path = buffer_dir.join(format!("seg_{}.mp4", idx));
                                create_encoder_for_target(&current_encoder_config, &path, next_is_ram).map(|(enc, stream)| {
                                    (Arc::new(SegmentEncoderInner::new(&dev_w.0, &ctx_w.0, enc, path.clone(), stream)), path)
                                })
                            }
                        };

                        match next {
                            Ok((next_encoder, next_path)) => {
                                eprintln!("Segment rotated: {}", next_path.display());
                                let _ = std::io::stderr().flush();

                                if let Some(ref mixer) = audio_mixer {
                                    audio_done = Some(mixer.rotate_segment(Some(next_path.with_extension("m4a")), next_start_qpc, next_is_ram));
                                }

                                current_encoder = next_encoder;
                                current_segment_path = next_path;
                                current_seg_start_qpc = next_start_qpc;
                                current_is_ram = next_is_ram;
                                segment_start = Instant::now();
                                frames_in_segment = 0;
                                last_sent_qpc = next_start_qpc;

                                if has_frame_fn() {
                                    let _ = (repeat_fn)(&current_encoder, next_start_qpc);
                                    frames_in_segment += 1;
                                    last_frame_instant = Instant::now();
                                }
                            }
                            Err(e) => {
                                eprintln!("Failed to start next replay segment: {e}");
                                let _ = std::io::stderr().flush();
                                segment_start = Instant::now();
                            }
                        }

                        // Finishing takes a few frames' time, so the recording goes on meanwhile.
                        if let Some((encoder, path, duration, start_qpc)) = ended {
                            let shared = shared_clone.clone();
                            let max_segs = max_buffer_segments(*replay_seconds);
                            let previous = finishing.take();
                            finishing = Some(std::thread::spawn(move || {
                                if let Some(previous) = previous {
                                    let _ = previous.join();
                                }
                                let finish_start = Instant::now();
                                let _ = encoder.finish();
                                eprintln!("Finish segment: {} ms", finish_start.elapsed().as_millis());
                                let _ = std::io::stderr().flush();
                                if let Ok(mut segs) = shared.segments.lock() {
                                    segs.push(SegmentInfo { path, duration_secs: duration, start_qpc });
                                    prune_old_segments(&mut segs, max_segs);
                                }
                            }));
                        }
                    } else if encoder_stuck {
                        eprintln!("Continuous encoder stuck, restarting");
                        let _ = std::io::stderr().flush();
                        if let Ok((next_enc, ram_stream)) = create_encoder_for_target(&current_encoder_config, &current_segment_path, false) {
                            current_encoder = Arc::new(SegmentEncoderInner::new(&dev_w.0, &ctx_w.0, next_enc, current_segment_path.clone(), ram_stream));
                        }
                    }

                    if let Some(req) = save_req {
                        let sc = shared_clone.clone();
                        // Waits for the segment that just ended; the next one queues behind it.
                        let (done_tx, done_rx) = std::sync::mpsc::sync_channel::<()>(1);
                        let previous = finishing.take();
                        finishing = Some(std::thread::spawn(move || {
                            if let Some(previous) = previous {
                                let _ = previous.join();
                            }
                            let _ = done_tx.send(());
                        }));
                        std::thread::spawn(move || {
                            let _ = done_rx.recv_timeout(Duration::from_secs(3));
                            // The last segment's audio is still on its way out of the encoder.
                            if let Some(done) = audio_done {
                                let _ = done.recv_timeout(Duration::from_millis(1000));
                            }
                            handle_save_replay_task(sc, req);
                        });
                    }
                }

                if let Some(card) = &mut card_fn {
                    card(last_frame_instant.elapsed() >= target_interval);
                }

                let now_qpc = current_qpc_100ns();
                // A segment begins with its first picture, so that one counts from the segment's
                // start like the audio does. (Only the very first comes noticeably later.)
                let frame_qpc = if frames_in_segment == 0 {
                    current_seg_start_qpc
                } else {
                    now_qpc.max(current_seg_start_qpc).max(last_sent_qpc + 10_000)
                };

                match pop_encode_fn(&current_encoder, frame_qpc) {
                    Ok(Some(encode_time)) => {
                        frames_in_segment += 1;
                        perf_window_frames += 1;
                        last_sent_qpc = frame_qpc;
                        last_frame_instant = Instant::now();

                        encoder_busy_clone.store(false, Ordering::Release);

                        if encode_time < target_interval {
                            std::thread::sleep(target_interval - encode_time);
                        }
                    }
                    Ok(None) => {
                        if last_frame_instant.elapsed() >= target_interval && has_frame_fn() {
                            encoder_busy_clone.store(true, Ordering::Release);
                            let now_qpc = current_qpc_100ns();
                            let repeat_qpc = now_qpc.max(current_seg_start_qpc).max(last_sent_qpc + 10_000);
                            match (repeat_fn)(&current_encoder, repeat_qpc) {
                                Ok(encode_time) => {
                                    frames_in_segment += 1;
                                    perf_window_frames += 1;
                                    last_sent_qpc = repeat_qpc;
                                    last_frame_instant = Instant::now();

                                    encoder_busy_clone.store(false, Ordering::Release);

                                    if encode_time < target_interval {
                                        std::thread::sleep(target_interval - encode_time);
                                    }
                                }
                                Err(e) => {
                                    encoder_busy_clone.store(false, Ordering::Release);
                                    current_encoder.mark_stuck();
                                    if let Ok(mut err_g) = last_encode_err_clone.lock() {
                                        *err_g = Some(e.to_string());
                                    }
                                    eprintln!("Repeat encode error: {e}");
                                    let _ = std::io::stderr().flush();
                                    std::thread::sleep(Duration::from_millis(1));
                                }
                            }
                        } else {
                            encoder_busy_clone.store(false, Ordering::Release);
                            let elapsed = last_frame_instant.elapsed();
                            let sleep_time = if elapsed < target_interval {
                                (target_interval - elapsed).min(Duration::from_millis(5)).max(Duration::from_millis(1))
                            } else {
                                Duration::from_millis(1)
                            };
                            std::thread::sleep(sleep_time);
                        }
                    }
                    Err(e) => {
                        encoder_busy_clone.store(false, Ordering::Release);
                        current_encoder.mark_stuck();
                        if let Ok(mut err_g) = last_encode_err_clone.lock() {
                            *err_g = Some(e.to_string());
                        }
                        eprintln!("Encode error: {e}");
                        let _ = std::io::stderr().flush();
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            }

            if let Some(ref mixer) = audio_mixer {
                let _ = mixer.rotate_segment(None, 0, false).recv_timeout(Duration::from_millis(1000));
            }
            if let Some(p) = prepared.take() {
                discard_prepared(p);
            }
            if let Some(f) = finishing.take() {
                let _ = f.join();
            }
            let duration = segment_start.elapsed().as_secs_f64();
            if !current_encoder.is_stuck() && frames_in_segment > 0 {
                let _ = current_encoder.finish();
                if let SessionMode::Replay { replay_seconds, .. } = &mode {
                    if let Ok(mut segs) = shared_clone.segments.lock() {
                        segs.push(SegmentInfo {
                            path: current_segment_path,
                            duration_secs: duration,
                            start_qpc: current_seg_start_qpc,
                        });
                        let max_segs = max_buffer_segments(*replay_seconds);
                        prune_old_segments(&mut segs, max_segs);
                    }
                }
            } else {
                check_zero_frames_guard(
                    frames_in_segment,
                    &current_encoder_config,
                    &mut logged_zero_frames,
                );
                let _ = crate::recorder_windows::mem_store::remove(&current_segment_path);
                let _ = std::fs::remove_file(&current_segment_path);
                let _ = crate::recorder_windows::mem_store::remove(current_segment_path.with_extension("m4a"));
                let _ = std::fs::remove_file(current_segment_path.with_extension("m4a"));
            }

            if let Ok(mut req) = shared_clone.screenshot_request.lock() {
                if let Some(req_val) = req.take() {
                    if let Some(tx) = req_val.response_tx {
                        let _ = tx.send(Err("Recording stopped before screenshot could be taken".to_string()));
                    }
                }
            }
        });

        Ok(Self {
            push_fn,
            encode_thread: Some(encode_thread),
            stop_signal,
            encoder_busy,
            capture_frames_arrived,
            dropped_encoder_busy,
            dropped_push_lock,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> std::result::Result<(), Self::Error> {
        self.capture_frames_arrived.fetch_add(1, Ordering::Relaxed);

        if self.encoder_busy.load(Ordering::Acquire) {
            self.dropped_encoder_busy.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }

        self.encoder_busy.store(true, Ordering::Release);
        match self.push_fn.lock() {
            Ok(mut push) => push(frame),
            Err(_) => {
                self.encoder_busy.store(false, Ordering::Release);
                self.dropped_push_lock.fetch_add(1, Ordering::Relaxed);
            }
        }

        Ok(())
    }

    fn on_closed(&mut self) -> std::result::Result<(), Self::Error> {
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(th) = self.encode_thread.take() {
            let _ = th.join();
        }
        crate::recorder_windows::mem_store::clear();
        Ok(())
    }
}

impl Drop for CaptureHandler {
    fn drop(&mut self) {
        let _ = self.on_closed();
    }
}

#[cfg(windows)]
#[link(name = "ole32")]
extern "system" {
    fn CoTaskMemFree(pv: *const std::ffi::c_void);
}

#[cfg(windows)]
pub fn hardware_encoder_available(subtype: &windows::core::GUID) -> bool {
    use windows::Win32::Media::MediaFoundation::{
        MFTEnumEx, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
        MFMediaType_Video, MFT_REGISTER_TYPE_INFO, IMFActivate,
    };

    let output_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: *subtype,
    };

    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count: u32 = 0;

    let res = unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            None,
            Some(&output_type),
            &mut activates,
            &mut count,
        )
    };

    if res.is_err() {
        return false;
    }

    if count > 0 && !activates.is_null() {
        unsafe {
            for i in 0..count as usize {
                let act = std::ptr::read(activates.add(i));
                drop(act);
            }
            CoTaskMemFree(activates as *const _);
        }
        true
    } else {
        if !activates.is_null() {
            unsafe {
                CoTaskMemFree(activates as *const _);
            }
        }
        false
    }
}

#[cfg(windows)]
pub fn hevc_supported() -> bool {
    use std::sync::OnceLock;
    use windows::Win32::Media::MediaFoundation::MFVideoFormat_HEVC;
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| hardware_encoder_available(&MFVideoFormat_HEVC))
}

/// AV1 needs both a card with an AV1 encoder (RTX 40+, RX 7000+, Intel Arc) and Windows 11
/// 24H2+, whose media pipeline is the first to accept AV1.
#[cfg(windows)]
pub fn av1_supported() -> bool {
    use std::sync::OnceLock;
    use windows::Win32::Media::MediaFoundation::MFVideoFormat_AV1;
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        windows::Media::MediaProperties::MediaEncodingSubtypes::Av1().is_ok()
            && hardware_encoder_available(&MFVideoFormat_AV1)
    })
}

/// Set once an H.265/AV1 encoder failed to start; later segments then stay on H.264 so one
/// buffer doesn't keep switching codecs.
static CODEC_FAILED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
fn video_subtype(codec: &str) -> VideoSettingsSubType {
    if CODEC_FAILED.load(Ordering::Relaxed) {
        VideoSettingsSubType::H264
    } else if codec.starts_with("hevc") && hevc_supported() {
        VideoSettingsSubType::HEVC
    } else if codec.starts_with("av1") && av1_supported() {
        VideoSettingsSubType::AV1
    } else {
        VideoSettingsSubType::H264
    }
}

/// Creates a video encoder using windows-capture.
/// Note: windows-capture does not expose GOP or keyframe interval settings on VideoSettingsBuilder.
/// Keyframes rely on segment starts (each segment starts with an IDR frame) and Media Foundation defaults.
pub fn create_video_encoder(config: &EncoderConfig, output_path: &Path) -> Result<VideoEncoder> {
    let video_settings = VideoSettingsBuilder::new(config.width, config.height)
        .frame_rate(config.fps)
        .bitrate(config.bitrate)
        .sub_type(video_subtype(&config.codec));

    let audio_settings = AudioSettingsBuilder::default().disabled(true);

    let container_settings = ContainerSettingsBuilder::new();

    VideoEncoder::new(video_settings, audio_settings, container_settings, output_path)
        .map_err(|e| anyhow::anyhow!("Failed to create video encoder: {e}"))
}

pub fn create_video_encoder_ram(
    config: &EncoderConfig,
) -> Result<(VideoEncoder, windows::Storage::Streams::InMemoryRandomAccessStream)> {
    use windows::Storage::Streams::InMemoryRandomAccessStream;
    use windows::core::Interface;

    let stream = InMemoryRandomAccessStream::new()
        .map_err(|e| anyhow::anyhow!("Failed to create InMemoryRandomAccessStream: {e}"))?;

    let video_settings = VideoSettingsBuilder::new(config.width, config.height)
        .frame_rate(config.fps)
        .bitrate(config.bitrate)
        .sub_type(video_subtype(&config.codec));

    let audio_settings = AudioSettingsBuilder::default().disabled(true);

    let container_settings = ContainerSettingsBuilder::new();

    let encoder = VideoEncoder::new_from_stream(
        video_settings,
        audio_settings,
        container_settings,
        stream.clone().cast()?,
    )
    .map_err(|e| anyhow::anyhow!("Failed to create RAM video encoder: {e}"))?;

    Ok((encoder, stream))
}

/// How far into a segment the next one's encoder is prepared.
#[cfg(windows)]
const PREPARE_AFTER: Duration = Duration::from_secs(8);

/// The next replay segment's encoder, started before it is needed: starting one takes
/// long enough to lose frames.
#[cfg(windows)]
struct PreparedSegment {
    encoder: Arc<SegmentEncoderInner>,
    path: PathBuf,
    is_ram: bool,
    size: (u32, u32),
}

/// Starts preparing on a thread of its own. Unlike `create_encoder_for_target` a failure
/// here (say, no free hardware session) means nothing yet.
#[cfg(windows)]
fn prepare_segment(
    dev: &SendWrapper<windows::Win32::Graphics::Direct3D11::ID3D11Device>,
    ctx: &SendWrapper<windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext>,
    config: &EncoderConfig,
    path: PathBuf,
    is_ram: bool,
) -> std::sync::mpsc::Receiver<PreparedSegment> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let (dev, ctx) = (SendWrapper(dev.0.clone()), SendWrapper(ctx.0.clone()));
    let config = config.clone();
    std::thread::spawn(move || {
        let (dev, ctx) = (dev, ctx);
        let created = if is_ram {
            create_video_encoder_ram(&config).map(|(enc, stream)| (enc, Some(stream)))
        } else {
            create_video_encoder(&config, &path).map(|enc| (enc, None))
        };
        if let Ok((enc, stream)) = created {
            let encoder = Arc::new(SegmentEncoderInner::new(&dev.0, &ctx.0, enc, path.clone(), stream));
            let _ = tx.send(PreparedSegment { encoder, path, is_ram, size: (config.width, config.height) });
        }
    });
    rx
}

/// Ends an encoder that never got a frame and removes its file.
#[cfg(windows)]
fn discard_prepared(prepared: std::sync::mpsc::Receiver<PreparedSegment>) {
    std::thread::spawn(move || {
        if let Ok(p) = prepared.recv() {
            drop(p.encoder);
            let _ = std::fs::remove_file(&p.path);
        }
    });
}

pub fn create_encoder_for_target(
    config: &EncoderConfig,
    path: &Path,
    is_ram: bool,
) -> Result<(VideoEncoder, Option<windows::Storage::Streams::InMemoryRandomAccessStream>)> {
    let create = |config: &EncoderConfig| -> Result<(VideoEncoder, Option<windows::Storage::Streams::InMemoryRandomAccessStream>)> {
        if is_ram {
            let (enc, stream) = create_video_encoder_ram(config)?;
            Ok((enc, Some(stream)))
        } else {
            Ok((create_video_encoder(config, path)?, None))
        }
    };
    match create(config) {
        Err(e) if video_subtype(&config.codec) != VideoSettingsSubType::H264 => {
            eprintln!("Warning: {} encoder failed to start, using H.264: {e}", config.codec);
            CODEC_FAILED.store(true, Ordering::Relaxed);
            create(config)
        }
        r => r,
    }
}

pub fn resolve_monitor(monitor_choice: &str) -> Result<Monitor> {
    if !monitor_choice.is_empty()
        && !monitor_choice.eq_ignore_ascii_case("primary")
        && !monitor_choice.eq_ignore_ascii_case("focused")
    {
        if let Ok(monitors) = Monitor::enumerate() {
            for m in monitors {
                if let Ok(name) = m.name() {
                    if name.eq_ignore_ascii_case(monitor_choice) {
                        return Ok(m);
                    }
                }
                if let Ok(dev) = m.device_name() {
                    if dev.eq_ignore_ascii_case(monitor_choice) {
                        return Ok(m);
                    }
                }
            }
        }
    }
    Monitor::primary().map_err(|e| anyhow::anyhow!("Failed to find primary monitor: {e}"))
}

pub fn handle_save_replay_task(shared: Arc<SharedCaptureState>, req: SaveRequest) {
    eprintln!("Save requested: {}s", req.requested_seconds);
    let _ = std::io::stderr().flush();
    let saved_at = SystemTime::now();
    let segments_snapshot: Vec<SegmentInfo> = {
        let segs = shared.segments.lock().unwrap();
        segs.iter()
            .filter(|s| s.path.is_file() || crate::recorder_windows::mem_store::exists(&s.path))
            .cloned()
            .collect()
    };

    let selected = select_segments_for_duration(&segments_snapshot, req.requested_seconds as f64);
    if selected.is_empty() {
        let err_msg = "No segments available to save replay".to_string();
        eprintln!("{err_msg}");
        let _ = std::io::stderr().flush();
        if let Some(tx) = req.response_tx {
            let _ = tx.send(Err(err_msg));
        }
        return;
    }

    let segment_paths: Vec<PathBuf> = selected.into_iter().map(|s| s.path).collect();
    eprintln!(
        "Segments chosen: {} segments ({:?})",
        segment_paths.len(),
        segment_paths
    );
    let _ = std::io::stderr().flush();

    let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let base_filename = match &req.game {
        Some(game) => crate::clip_index::game_file_name(game, &format!("{}.mp4", timestamp)),
        None => format!("Replay_{}.mp4", timestamp),
    };
    let final_filename = match &req.label {
        Some(label) => labelled_name_str(&base_filename, label),
        None => base_filename,
    };

    let target_dir = match &req.game {
        Some(game_name) => {
            let d = shared.base_save_path.join(game_name);
            let _ = std::fs::create_dir_all(&d);
            d
        }
        None => {
            let _ = std::fs::create_dir_all(&shared.base_save_path);
            shared.base_save_path.clone()
        }
    };
    let final_path = crate::clip_index::free_path(target_dir.join(&final_filename));

    let remux_start = Instant::now();
    let remux_res = crate::recorder_windows::remux::join_segments(
        &segment_paths,
        &final_path,
        Some(req.requested_seconds as f64),
    );
    let remux_ms = remux_start.elapsed().as_millis();
    eprintln!("Remux: {} ms", remux_ms);
    let _ = std::io::stderr().flush();

    if let Err(e) = remux_res {
        let err_msg = format!("Failed to remux replay segments: {e}");
        eprintln!("{err_msg}");
        let _ = std::io::stderr().flush();
        if let Some(tx) = req.response_tx {
            let _ = tx.send(Err(err_msg));
        }
        return;
    }

    if let Some(tx) = req.response_tx {
        let _ = tx.send(Ok(final_path.clone()));
    }

    let file_size = std::fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);
    eprintln!(
        "Output written: {} ({} bytes)",
        final_path.display(),
        file_size
    );
    let _ = std::io::stderr().flush();

    if shared.save_sound {
        crate::sound::play_save_sound(&shared.save_sound_path);
    }

    let final_name = final_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let shown = match &req.game {
        Some(g) => format!("{}/{}", g, final_name),
        None => final_name,
    };

    if shared.notify.load(Ordering::Relaxed) {
        crate::utils::send_clip_notification(
            "Lapse - Replay Saved",
            &format!("Saved to {}", shown),
            final_path.clone(),
        );
    }

    if let Ok(marks) = shared.markers.lock() {
        let duration = crate::clip_index::probe_duration(&final_path);
        let offsets = crate::markers::offsets_in_clip(&marks, saved_at, duration);
        crate::markers::save(&shared.base_save_path, &final_path, &offsets);
    }

    if let Ok(mut last) = shared.last_saved.lock() {
        *last = Some(final_path.to_string_lossy().to_string());
    }
    shared.save_counter.fetch_add(1, Ordering::SeqCst);
}

fn labelled_name_str(filename: &str, label: &str) -> String {
    let clean: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    let stem = filename.strip_suffix(".mp4").unwrap_or(filename);
    format!("{}_{}.mp4", stem, clean.trim_matches('_'))
}

pub fn take_one_shot_screenshot(
    monitor: Monitor,
    path: PathBuf,
    sound: bool,
    sound_path: String,
    notify: bool,
) -> Result<()> {
    struct ShotHandler {
        path: PathBuf,
        device: SendWrapper<windows::Win32::Graphics::Direct3D11::ID3D11Device>,
        context: SendWrapper<windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext>,
    }
    impl GraphicsCaptureApiHandler for ShotHandler {
        type Flags = PathBuf;
        type Error = Box<dyn std::error::Error + Send + Sync>;
        fn new(ctx: CaptureContext<Self::Flags>) -> std::result::Result<Self, Self::Error> {
            Ok(Self {
                path: ctx.flags,
                device: SendWrapper(ctx.device),
                context: SendWrapper(ctx.device_context),
            })
        }
        fn on_frame_arrived(
            &mut self,
            frame: &mut Frame,
            capture_control: InternalCaptureControl,
        ) -> std::result::Result<(), Self::Error> {
            capture_control.stop();
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            // Not frame.buffer(): its memory is unmapped before it can be read.
            let (width, height) = (frame.width(), frame.height());
            let staging = create_cpu_staging_texture(&self.device.0, width, height)?;
            let texture = unsafe { frame.as_raw_texture() };
            save_texture_to_png(&self.context.0, texture, &staging, width, height, &self.path)?;
            Ok(())
        }
    }
    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::Default,
        border_draw_setting(),
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        path.clone(),
    );
    let res = ShotHandler::start(settings);
    match res {
        Ok(_) => {
            eprintln!("Screenshot: {}", path.display());
            let _ = std::io::stderr().flush();
            if sound {
                crate::sound::play_save_sound(&sound_path);
            }
            if notify {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                crate::utils::send_notification(
                    "Lapse - Screenshot",
                    &format!("Saved {}", name),
                    "media-record",
                );
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("Screenshot error: {e:?}");
            let _ = std::io::stderr().flush();
            Err(anyhow::anyhow!("Screenshot failed: {e:?}"))
        }
    }
}

/// Caps capture callbacks at the recording frame rate where Windows supports it (Windows 11
/// 24H2+). Older Windows delivers every monitor refresh instead; the encoder thread paces
/// itself, so extra frames are only dropped.
#[cfg(windows)]
pub fn update_interval_setting(fps: u32) -> MinimumUpdateIntervalSettings {
    use windows_capture::graphics_capture_api::GraphicsCaptureApi;
    if GraphicsCaptureApi::is_minimum_update_interval_supported().unwrap_or(false) {
        MinimumUpdateIntervalSettings::Custom(Duration::from_nanos(1_000_000_000 / fps.max(1) as u64))
    } else {
        MinimumUpdateIntervalSettings::Default
    }
}

/// Determines the border draw setting by requesting borderless access once on Windows 11.
/// Falls back to Default on Windows 10 or when access is denied or unavailable.
#[cfg(windows)]
pub fn border_draw_setting() -> DrawBorderSettings {
    use std::sync::OnceLock;
    static BORDER_SETTING: OnceLock<DrawBorderSettings> = OnceLock::new();

    *BORDER_SETTING.get_or_init(request_borderless_access)
}

/// Whether this Windows version lets apps turn the yellow capture border off.
#[cfg(windows)]
pub fn border_removable() -> bool {
    static REMOVABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *REMOVABLE.get_or_init(|| {
        windows_capture::graphics_capture_api::GraphicsCaptureApi::is_border_settings_supported().unwrap_or(false)
    })
}

#[cfg(windows)]
fn request_borderless_access() -> DrawBorderSettings {
    use windows::Graphics::Capture::{GraphicsCaptureAccess, GraphicsCaptureAccessKind};
    use windows::Security::Authorization::AppCapabilityAccess::AppCapabilityAccessStatus;

    // Windows 10 always draws the border; asking for none there would fail the capture.
    if !border_removable() {
        eprintln!("Borderless capture unsupported on this Windows version; the yellow border stays");
        let _ = std::io::stderr().flush();
        return DrawBorderSettings::Default;
    }

    let res = std::panic::catch_unwind(|| -> windows::core::Result<bool> {
        let op = GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless)?;
        let status = op.get()?;
        Ok(status == AppCapabilityAccessStatus::Allowed)
    });

    match res {
        Ok(Ok(true)) => {
            eprintln!("Borderless capture access granted");
            let _ = std::io::stderr().flush();
            DrawBorderSettings::WithoutBorder
        }
        Ok(Ok(false)) => {
            eprintln!("Borderless capture access denied; falling back to default border");
            let _ = std::io::stderr().flush();
            DrawBorderSettings::Default
        }
        Ok(Err(e)) => {
            eprintln!("Borderless capture access request failed ({e}); falling back to default border");
            let _ = std::io::stderr().flush();
            DrawBorderSettings::Default
        }
        Err(_) => {
            eprintln!("Borderless capture access panic; falling back to default border");
            let _ = std::io::stderr().flush();
            DrawBorderSettings::Default
        }
    }
}

