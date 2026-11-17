//! Clip export jobs (trim + optional re-encode) run with ffmpeg in the background.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

/// Discord's free upload limit is 10 MiB; aim a little under it.
pub const DISCORD_LIMIT_BYTES: u64 = 10 * 1024 * 1024;
const DISCORD_TARGET_BYTES: f64 = 9.4 * 1024.0 * 1024.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportPreset {
    /// Stream copy: instant and lossless, but the cut snaps to the previous keyframe.
    Fast,
    /// Re-encode at high quality: frame-accurate cut points.
    Precise,
    /// Re-encode to fit Discord's 10 MB limit.
    Discord,
    /// 9:16 for TikTok / Shorts / Reels: a vertical slice of the picture.
    Vertical,
    Gif,
    Mp3,
}

impl ExportPreset {
    pub const ALL: [ExportPreset; 6] = [Self::Fast, Self::Precise, Self::Discord, Self::Vertical, Self::Gif, Self::Mp3];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fast => "Fast",
            Self::Precise => "Precise",
            Self::Discord => "Discord",
            Self::Vertical => "Vertical",
            Self::Gif => "GIF",
            Self::Mp3 => "MP3",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Fast => "Instant, original quality. Cut points may shift up to ~2 s to the nearest keyframe.",
            Self::Precise => "Exact cut points, near-original quality. Takes a little longer.",
            Self::Discord => "Shrinks the clip to under 10 MB so it uploads to Discord without Nitro.",
            Self::Vertical => "9:16 video (1080x1920) for Shorts, TikTok and Reels. Move the frame to pick which part of the picture is kept.",
            Self::Gif => "Animated GIF (480p, 15 fps, no sound). Best for short moments.",
            Self::Mp3 => "Audio only.",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Gif => "gif",
            Self::Mp3 => "mp3",
            _ => "mp4",
        }
    }

    pub fn has_audio_option(self) -> bool {
        !matches!(self, Self::Gif | Self::Mp3)
    }
}

/// Which audio ends up in the export. Track 0 is game/desktop audio, track 1 the
/// microphone (when it was recorded as a separate track).
#[derive(Clone, Debug, PartialEq)]
pub struct AudioMix {
    pub game: bool,
    pub mic: bool,
    /// Microphone gain, 1.0 = unchanged.
    pub mic_volume: f32,
    /// EBU R128 loudness normalization, so every clip is about as loud.
    pub normalize: bool,
}

impl Default for AudioMix {
    fn default() -> Self {
        Self { game: true, mic: true, mic_volume: 1.0, normalize: false }
    }
}

impl AudioMix {
    pub fn muted() -> Self {
        Self { game: false, mic: false, ..Self::default() }
    }
}

#[derive(Clone, Debug)]
pub struct ExportRequest {
    pub input: PathBuf,
    pub start: f64,
    pub end: f64,
    pub preset: ExportPreset,
    pub audio: AudioMix,
    pub output: PathBuf,
    /// Vertical: where the 9:16 frame sits, 0 = left edge, 1 = right edge.
    pub crop_x: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExportState {
    Running { fraction: f32, pass: u8 },
    Done { path: PathBuf, bytes: u64 },
    Failed(String),
    Cancelled,
}

pub struct ExportJob {
    state: Arc<Mutex<ExportState>>,
    child: Arc<Mutex<Option<Child>>>,
}

impl ExportJob {
    pub fn state(&self) -> ExportState {
        self.state.lock().map(|s| s.clone()).unwrap_or(ExportState::Failed("lock poisoned".into()))
    }

    pub fn is_running(&self) -> bool {
        matches!(self.state(), ExportState::Running { .. })
    }

    pub fn cancel(&self) {
        if let Ok(mut s) = self.state.lock() {
            *s = ExportState::Cancelled;
        }
        if let Ok(mut c) = self.child.lock() {
            if let Some(child) = c.as_mut() {
                let _ = child.kill();
            }
        }
    }
}

/// `clip_trim_143005.mp4` next to the input.
pub fn default_output(input: &Path, preset: ExportPreset) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("clip");
    let suffix = match preset {
        ExportPreset::Discord => "discord",
        ExportPreset::Vertical => "vertical",
        ExportPreset::Gif => "gif",
        ExportPreset::Mp3 => "audio",
        _ => "trim",
    };
    let time = chrono::Local::now().format("%H%M%S");
    input
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{}_{}_{}.{}", stem, suffix, time, preset.extension()))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum H264Encoder {
    X264,
    Vaapi,
}

pub(crate) fn h264_encoder() -> H264Encoder {
    static ENC: OnceLock<H264Encoder> = OnceLock::new();
    *ENC.get_or_init(|| {
        let list = Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        if list.contains("libx264") || !list.contains("h264_vaapi") {
            H264Encoder::X264
        } else {
            H264Encoder::Vaapi
        }
    })
}

pub fn audio_stream_count(input: &Path) -> usize {
    Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "a", "-show_entries", "stream=index", "-of", "csv=p=0"])
        .arg(input)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(1)
}

/// Discord: bitrate budget for the whole clip, then resolution/fps that suit it.
pub fn discord_plan(duration: f64, has_audio: bool, scale: f64) -> (u32, u32, u32) {
    let audio_kbps = if has_audio { 96.0 } else { 0.0 };
    let total_kbps = DISCORD_TARGET_BYTES * 8.0 / duration.max(0.5) / 1000.0 * scale;
    let video_kbps = (total_kbps - audio_kbps).max(120.0);
    let (height, fps) = if video_kbps >= 4500.0 {
        (1080, 60)
    } else if video_kbps >= 2500.0 {
        (1080, 30)
    } else if video_kbps >= 1200.0 {
        (720, 30)
    } else {
        (480, 30)
    };
    (video_kbps as u32, height, fps)
}

pub(crate) fn video_encode_args(args: &mut Vec<String>, filters: &mut Vec<String>, quality: Option<u32>, bitrate_kbps: Option<u32>) {
    match h264_encoder() {
        H264Encoder::X264 => {
            args.extend(["-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p"].map(String::from));
            if let Some(crf) = quality {
                args.extend(["-crf".into(), crf.to_string()]);
            }
            if let Some(k) = bitrate_kbps {
                args.extend([
                    "-b:v".into(), format!("{}k", k),
                    "-maxrate".into(), format!("{}k", k * 12 / 10),
                    "-bufsize".into(), format!("{}k", k * 2),
                ]);
            }
        }
        H264Encoder::Vaapi => {
            filters.push("format=nv12".into());
            filters.push("hwupload".into());
            args.extend(["-c:v", "h264_vaapi"].map(String::from));
            if let Some(q) = quality {
                args.extend(["-qp".into(), (q + 2).to_string()]);
            }
            if let Some(k) = bitrate_kbps {
                args.extend(["-b:v".into(), format!("{}k", k), "-maxrate".into(), format!("{}k", k * 12 / 10)]);
            }
        }
    }
}

/// How the audio of an export is produced.
#[derive(Debug, PartialEq)]
enum AudioOut {
    None,
    /// Keep every track as it is (separate tracks stay separate).
    AllTracks,
    /// Only the game/desktop track, untouched (microphone dropped).
    GameTrack,
    /// One track from a filter graph ending in `[aout]`.
    Filter(String),
}

/// `single_track`: the target can only hold one audio track (Discord, MP3), so the
/// game and microphone tracks are mixed together.
fn plan_audio(mix: &AudioMix, tracks: usize, single_track: bool) -> AudioOut {
    let mic_track = tracks > 1;
    let game = mix.game;
    let mic = mic_track && mix.mic;
    if tracks == 0 || (!game && !mic) {
        return AudioOut::None;
    }
    let untouched = (mic || !mic_track) && (mix.mic_volume - 1.0).abs() < 0.01 && !mix.normalize;
    if game && untouched && (!single_track || tracks == 1) {
        return AudioOut::AllTracks;
    }
    if game && !mic && !mix.normalize {
        return AudioOut::GameTrack;
    }

    let mut parts = Vec::new();
    let mut inputs = Vec::new();
    if game {
        inputs.push("[0:a:0]".to_string());
    }
    if mic {
        if (mix.mic_volume - 1.0).abs() >= 0.01 {
            parts.push(format!("[0:a:1]volume={:.2}[mic]", mix.mic_volume));
            inputs.push("[mic]".into());
        } else {
            inputs.push("[0:a:1]".into());
        }
    }
    let mut chain = if inputs.len() > 1 {
        format!("{}amix=inputs={}:normalize=0", inputs.join(""), inputs.len())
    } else {
        format!("{}anull", inputs[0])
    };
    if mix.normalize {
        chain.push_str(",loudnorm=I=-16:TP=-1.5:LRA=11");
    }
    parts.push(format!("{}[aout]", chain));
    AudioOut::Filter(parts.join(";"))
}

/// Adds the audio maps/filters for `out` with the given encoder arguments.
fn push_audio(args: &mut Vec<String>, out: &AudioOut, codec: &[&str]) {
    match out {
        AudioOut::None => args.push("-an".into()),
        AudioOut::AllTracks => {
            args.extend(["-map", "0:a?"].map(String::from));
            args.extend(codec.iter().map(|s| s.to_string()));
        }
        AudioOut::GameTrack => {
            args.extend(["-map", "0:a:0"].map(String::from));
            args.extend(codec.iter().map(|s| s.to_string()));
        }
        AudioOut::Filter(graph) => {
            args.extend(["-filter_complex".into(), graph.clone(), "-map".into(), "[aout]".into()]);
            args.extend(codec.iter().map(|s| s.to_string()));
        }
    }
}

/// Builds the ffmpeg argument list. `scale` < 1 shrinks the Discord bitrate on a retry.
pub fn build_args(req: &ExportRequest, audio_tracks: usize, scale: f64, output: &Path) -> Vec<String> {
    let duration = (req.end - req.start).max(0.05);
    let mut args: Vec<String> = ["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-progress", "pipe:1", "-nostats"]
        .map(String::from)
        .to_vec();
    let vaapi = h264_encoder() == H264Encoder::Vaapi
        && matches!(req.preset, ExportPreset::Precise | ExportPreset::Discord | ExportPreset::Vertical);
    if vaapi {
        args.extend(["-vaapi_device".into(), "/dev/dri/renderD128".into()]);
    }
    args.extend(["-ss".into(), format!("{:.3}", req.start), "-t".into(), format!("{:.3}", duration), "-i".into()]);
    args.push(req.input.to_string_lossy().to_string());

    let mut filters: Vec<String> = Vec::new();
    let single_track = matches!(req.preset, ExportPreset::Discord | ExportPreset::Vertical | ExportPreset::Mp3);
    let audio = plan_audio(&req.audio, audio_tracks, single_track);

    match req.preset {
        ExportPreset::Fast => {
            args.extend(["-map", "0:v:0", "-c:v", "copy", "-avoid_negative_ts", "make_zero"].map(String::from));
            // Untouched audio is copied too; mixed/normalized audio has to be encoded.
            let copy = matches!(audio, AudioOut::AllTracks | AudioOut::GameTrack);
            let codec: &[&str] = if copy { &["-c:a", "copy"] } else { &["-c:a", "aac", "-b:a", "192k"] };
            push_audio(&mut args, &audio, codec);
        }
        ExportPreset::Precise => {
            args.extend(["-map", "0:v:0"].map(String::from));
            push_audio(&mut args, &audio, &["-c:a", "aac", "-b:a", "192k"]);
            video_encode_args(&mut args, &mut filters, Some(18), None);
        }
        ExportPreset::Discord => {
            let (video_kbps, height, fps) = discord_plan(duration, audio != AudioOut::None, scale);
            // Scale before any hwupload filter added by the encoder.
            filters.push(format!("scale=-2:'min({},ih)'", height));
            filters.push(format!("fps={}", fps));
            video_encode_args(&mut args, &mut filters, None, Some(video_kbps));
            args.extend(["-map", "0:v:0"].map(String::from));
            push_audio(&mut args, &audio, &["-c:a", "aac", "-b:a", "96k"]);
        }
        ExportPreset::Vertical => {
            filters.push(vertical_crop(req.crop_x));
            filters.push("scale=1080:1920:flags=lanczos".into());
            video_encode_args(&mut args, &mut filters, Some(20), None);
            args.extend(["-map", "0:v:0"].map(String::from));
            push_audio(&mut args, &audio, &["-c:a", "aac", "-b:a", "192k"]);
        }
        ExportPreset::Gif => {
            args.extend([
                "-filter_complex".into(),
                "[0:v]fps=15,scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4".into(),
                "-loop".into(),
                "0".into(),
            ]);
        }
        ExportPreset::Mp3 => {
            args.push("-vn".into());
            push_audio(&mut args, &audio, &["-c:a", "libmp3lame", "-q:a", "2"]);
        }
    }

    if !filters.is_empty() {
        args.extend(["-vf".into(), filters.join(",")]);
    }
    if req.preset.extension() == "mp4" {
        args.extend(["-movflags", "+faststart"].map(String::from));
    }
    args.push(output.to_string_lossy().to_string());
    args
}

/// A full-height 9:16 slice of the picture; `x` 0..1 moves it from left to right.
fn vertical_crop(x: f32) -> String {
    let x = x.clamp(0.0, 1.0);
    format!("crop=trunc(ih*9/16/2)*2:ih:(iw-trunc(ih*9/16/2)*2)*{:.3}:0", x)
}

/// Hidden temp name in the same folder, so the clip library ignores half-written files.
fn temp_output(output: &Path) -> PathBuf {
    let name = output.file_name().unwrap_or_default().to_string_lossy();
    output.with_file_name(format!(".{}.part.{}", name, output.extension().and_then(|e| e.to_str()).unwrap_or("mp4")))
}

pub fn start(req: ExportRequest) -> ExportJob {
    let duration = (req.end - req.start).max(0.05);
    let output = req.output.clone();
    let discord = req.preset == ExportPreset::Discord;
    run_job(duration, output, move |tmp| {
        let tracks = audio_stream_count(&req.input);
        // Discord exports retry with a smaller budget if the first attempt is too big.
        let scales: &[f64] = if discord { &[1.0, 0.85, 0.7] } else { &[1.0] };
        scales.iter().map(|scale| build_args(&req, tracks, *scale, tmp)).collect()
    }, move |bytes| !discord || bytes <= DISCORD_LIMIT_BYTES)
}

/// Runs ffmpeg in the background with progress, cancel and a hidden temp file.
/// `attempts` builds one argument list per attempt (on the worker thread, so probing
/// the input never blocks the UI); the next one runs when `good_enough` rejects a result.
pub(crate) fn run_job(
    duration: f64,
    output: PathBuf,
    attempts: impl FnOnce(&Path) -> Vec<Vec<String>> + Send + 'static,
    good_enough: impl Fn(u64) -> bool + Send + 'static,
) -> ExportJob {
    let state = Arc::new(Mutex::new(ExportState::Running { fraction: 0.0, pass: 1 }));
    let child_slot: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let job = ExportJob { state: Arc::clone(&state), child: Arc::clone(&child_slot) };

    std::thread::spawn(move || {
        let tmp = temp_output(&output);
        let attempts = attempts(&tmp);
        let count = attempts.len();

        for (pass, args) in attempts.into_iter().enumerate() {
            if matches!(*state.lock().unwrap(), ExportState::Cancelled) {
                let _ = std::fs::remove_file(&tmp);
                return;
            }
            let spawned = Command::new("ffmpeg")
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            let mut child = match spawned {
                Ok(c) => c,
                Err(e) => {
                    *state.lock().unwrap() = ExportState::Failed(format!("ffmpeg could not start: {}", e));
                    return;
                }
            };
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            *child_slot.lock().unwrap() = Some(child);

            let err_text = Arc::new(Mutex::new(String::new()));
            if let Some(err) = stderr {
                let err_text = Arc::clone(&err_text);
                std::thread::spawn(move || {
                    for line in BufReader::new(err).lines().map_while(|l| l.ok()) {
                        if let Ok(mut t) = err_text.lock() {
                            *t = line;
                        }
                    }
                });
            }
            if let Some(out) = stdout {
                for line in BufReader::new(out).lines().map_while(|l| l.ok()) {
                    // out_time_us=1234567 (older ffmpeg names the same value out_time_ms)
                    let us = line
                        .strip_prefix("out_time_us=")
                        .or_else(|| line.strip_prefix("out_time_ms="))
                        .and_then(|v| v.parse::<f64>().ok());
                    if let Some(us) = us {
                        if let Ok(mut s) = state.lock() {
                            if matches!(*s, ExportState::Running { .. }) {
                                *s = ExportState::Running {
                                    fraction: ((us / 1e6) / duration).clamp(0.0, 1.0) as f32,
                                    pass: pass as u8 + 1,
                                };
                            }
                        }
                    }
                }
            }
            let status = child_slot.lock().unwrap().take().and_then(|mut c| c.wait().ok());

            if matches!(*state.lock().unwrap(), ExportState::Cancelled) {
                let _ = std::fs::remove_file(&tmp);
                return;
            }
            if !status.map(|s| s.success()).unwrap_or(false) {
                let _ = std::fs::remove_file(&tmp);
                let msg = err_text.lock().map(|t| t.clone()).unwrap_or_default();
                *state.lock().unwrap() = ExportState::Failed(if msg.is_empty() { "ffmpeg failed".into() } else { msg });
                return;
            }

            let bytes = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
            if !good_enough(bytes) && pass + 1 < count {
                continue;
            }
            if std::fs::rename(&tmp, &output).is_err() {
                *state.lock().unwrap() = ExportState::Failed("could not move the exported file into place".into());
                return;
            }
            crate::clip_index::index().refresh();
            *state.lock().unwrap() = ExportState::Done { path: output.clone(), bytes };
            return;
        }
    });
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(preset: ExportPreset) -> ExportRequest {
        ExportRequest {
            input: "/v/in.mp4".into(),
            start: 1.5,
            end: 11.5,
            preset,
            audio: AudioMix::default(),
            output: "/v/out.mp4".into(),
            crop_x: 0.5,
        }
    }

    #[test]
    fn vertical_export_crops_to_9_16() {
        let args = build_args(&req(ExportPreset::Vertical), 2, 1.0, Path::new("/v/out.mp4"));
        let vf = args.iter().position(|a| a == "-vf").map(|i| args[i + 1].clone()).unwrap();
        assert!(vf.starts_with("crop=trunc(ih*9/16/2)*2:ih:(iw-trunc(ih*9/16/2)*2)*0.500:0,scale=1080:1920"), "{}", vf);
    }

    #[test]
    fn discord_budget_fits_limit() {
        let (kbps, h, _) = discord_plan(30.0, true, 1.0);
        let total_bytes = (kbps as f64 + 96.0) * 1000.0 / 8.0 * 30.0;
        assert!(total_bytes < DISCORD_LIMIT_BYTES as f64, "{}", total_bytes);
        assert_eq!(h, 1080);
        assert_eq!(discord_plan(300.0, true, 1.0).1, 480);
    }

    #[test]
    fn fast_export_copies_all_audio_tracks() {
        let args = build_args(&req(ExportPreset::Fast), 2, 1.0, Path::new("/v/out.mp4"));
        let joined = args.join(" ");
        assert!(joined.contains("-ss 1.500 -t 10.000 -i /v/in.mp4"));
        assert!(joined.contains("-c:v copy"));
        assert!(joined.contains("-map 0:a? -c:a copy"));
    }

    #[test]
    fn muted_export_drops_audio() {
        let mut r = req(ExportPreset::Precise);
        r.audio = AudioMix::muted();
        let joined = build_args(&r, 1, 1.0, Path::new("/v/out.mp4")).join(" ");
        assert!(!joined.contains("0:a"));
    }

    #[test]
    fn audio_mix_choices() {
        let mix = |game, mic, vol, normalize| AudioMix { game, mic, mic_volume: vol, normalize };
        assert_eq!(plan_audio(&mix(true, true, 1.0, false), 2, false), AudioOut::AllTracks);
        assert_eq!(plan_audio(&mix(true, false, 1.0, false), 2, false), AudioOut::GameTrack);
        assert_eq!(plan_audio(&mix(false, false, 1.0, false), 2, false), AudioOut::None);
        assert_eq!(plan_audio(&mix(false, true, 1.0, false), 2, false), AudioOut::Filter("[0:a:1]anull[aout]".into()));
        assert_eq!(
            plan_audio(&mix(true, true, 1.5, true), 2, false),
            AudioOut::Filter("[0:a:1]volume=1.50[mic];[0:a:0][mic]amix=inputs=2:normalize=0,loudnorm=I=-16:TP=-1.5:LRA=11[aout]".into())
        );
        // Discord/MP3 hold one track: game + mic are mixed.
        assert_eq!(plan_audio(&mix(true, true, 1.0, false), 2, true), AudioOut::Filter("[0:a:0][0:a:1]amix=inputs=2:normalize=0[aout]".into()));
        // A clip with only one track ignores the microphone switch.
        assert_eq!(plan_audio(&mix(true, false, 1.0, false), 1, false), AudioOut::AllTracks);
    }

    #[test]
    fn temp_file_is_hidden() {
        let t = temp_output(Path::new("/v/clip_trim.mp4"));
        assert_eq!(t, PathBuf::from("/v/.clip_trim.mp4.part.mp4"));
    }

    /// Runs every preset through real ffmpeg; `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn exports_real_video() {
        let dir = std::env::temp_dir().join(format!("lapse-export-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let input = dir.join("in.mp4");
        let ok = Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=60:duration=20",
                   "-f", "lavfi", "-i", "sine=f=440:d=20", "-f", "lavfi", "-i", "sine=f=880:d=20",
                   "-map", "0", "-map", "1", "-map", "2", "-c:v", "libx264", "-preset", "ultrafast", "-g", "120",
                   "-c:a", "aac"])
            .arg(&input)
            .status()
            .unwrap()
            .success();
        assert!(ok);

        for preset in ExportPreset::ALL {
            let out = dir.join(format!("out.{}", preset.extension()));
            let job = start(ExportRequest { input: input.clone(), start: 3.0, end: 9.0, preset, audio: AudioMix::default(), output: out.clone(), crop_x: 0.5 });
            let state = loop {
                let s = job.state();
                if !matches!(s, ExportState::Running { .. }) {
                    break s;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            };
            assert!(matches!(state, ExportState::Done { .. }), "{:?}: {:?}", preset, state);
            let d = crate::clip_index::probe_duration(&out);
            assert!(d > 5.0 && d < 9.5, "{:?} duration {}", preset, d);
            if preset == ExportPreset::Discord {
                assert!(std::fs::metadata(&out).unwrap().len() <= DISCORD_LIMIT_BYTES);
            }
            eprintln!("{:?}: {:.2}s {:?}", preset, d, state);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
