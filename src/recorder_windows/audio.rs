#[cfg(windows)]
use anyhow::{Context, Result};

#[cfg_attr(not(windows), allow(unused_imports))]
use std::collections::VecDeque;
#[cfg(windows)]
use std::io::Write;
#[cfg(windows)]
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use std::sync::{Arc, Mutex};
#[cfg(windows)]
use std::thread::{self, JoinHandle};
#[cfg(windows)]
use std::time::{Duration, Instant};
#[cfg(windows)]
use wasapi::*;
#[cfg(windows)]
use windows::Win32::Media::MediaFoundation::{MFShutdown, MFStartup, MFSTARTUP_NOSOCKET, MF_VERSION};

#[cfg(windows)]
use crate::recorder_windows::aac_track::AacTrack;

/// Standard downmix attenuation coefficient (-3 dB = 1 / sqrt(2)).
pub const DOWNMIX_COEFF: f32 = 0.70710678;

/// Converts a 32-bit float audio sample in [-1.0, 1.0] to a 16-bit signed integer.
pub fn float_to_i16(sample: f32) -> i16 {
    if sample.is_nan() {
        return 0;
    }
    let scaled = sample * 32768.0;
    if scaled >= 32767.0 {
        32767
    } else if scaled <= -32768.0 {
        -32768
    } else {
        scaled.round() as i16
    }
}

/// Downmixes multi-channel audio frames to stereo (left, right).
pub fn downmix_frame_to_stereo(frame: &[f32]) -> (f32, f32) {
    match frame.len() {
        0 => (0.0, 0.0),
        1 => (frame[0], frame[0]),
        2 => (frame[0], frame[1]),
        4 => (
            frame[0] + DOWNMIX_COEFF * frame[2],
            frame[1] + DOWNMIX_COEFF * frame[3],
        ),
        6 => (
            frame[0] + DOWNMIX_COEFF * (frame[2] + frame[3] + frame[4]),
            frame[1] + DOWNMIX_COEFF * (frame[2] + frame[3] + frame[5]),
        ),
        8 => (
            frame[0] + DOWNMIX_COEFF * (frame[2] + frame[3] + frame[4] + frame[6]),
            frame[1] + DOWNMIX_COEFF * (frame[2] + frame[3] + frame[5] + frame[7]),
        ),
        _ => (frame[0], frame[1]),
    }
}

/// Resamples stereo audio to 48 kHz using linear interpolation while preserving phase across buffers.
pub struct LinearResampler {
    in_rate: u32,
    out_rate: u32,
    step: f64,
    phase: f64,
    curr_frame: [f32; 2],
    next_frame: [f32; 2],
    pending_first: Option<[f32; 2]>,
    initialized: bool,
}

impl LinearResampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        Self {
            in_rate,
            out_rate,
            step: in_rate as f64 / out_rate.max(1) as f64,
            phase: 0.0,
            curr_frame: [0.0, 0.0],
            next_frame: [0.0, 0.0],
            pending_first: None,
            initialized: false,
        }
    }

    pub fn process(&mut self, input: &[[f32; 2]]) -> Vec<[f32; 2]> {
        if self.in_rate == self.out_rate {
            return input.to_vec();
        }
        if input.is_empty() {
            return Vec::new();
        }

        let mut in_idx = 0;

        if !self.initialized {
            if let Some(first) = self.pending_first.take() {
                self.curr_frame = first;
                self.next_frame = input[in_idx];
                in_idx += 1;
                self.phase = 0.0;
                self.initialized = true;
            } else if input.len() == 1 {
                self.pending_first = Some(input[0]);
                return Vec::new();
            } else {
                self.curr_frame = input[0];
                self.next_frame = input[1];
                in_idx = 2;
                self.phase = 0.0;
                self.initialized = true;
            }
        }

        let mut output = Vec::with_capacity((input.len() as f64 / self.step).ceil() as usize + 8);

        while self.phase >= 1.0 && in_idx < input.len() {
            self.curr_frame = self.next_frame;
            self.next_frame = input[in_idx];
            in_idx += 1;
            self.phase -= 1.0;
        }

        while self.phase < 1.0 {
            let alpha = self.phase as f32;
            let inv_alpha = 1.0 - alpha;
            let l = self.curr_frame[0] * inv_alpha + self.next_frame[0] * alpha;
            let r = self.curr_frame[1] * inv_alpha + self.next_frame[1] * alpha;
            output.push([l, r]);

            self.phase += self.step;
            while self.phase >= 1.0 {
                if in_idx < input.len() {
                    self.curr_frame = self.next_frame;
                    self.next_frame = input[in_idx];
                    in_idx += 1;
                    self.phase -= 1.0;
                } else {
                    break;
                }
            }
        }

        output
    }
}

/// Converts raw device packets (PCM/float, arbitrary channels, rate) into 48 kHz stereo frames.
pub struct AudioDeviceProcessor {
    pub channels: usize,
    pub bits_per_sample: u16,
    pub is_float: bool,
    pub resampler: LinearResampler,
}

impl AudioDeviceProcessor {
    pub fn new(channels: usize, sample_rate: u32, bits_per_sample: u16, is_float: bool) -> Self {
        Self {
            channels,
            bits_per_sample,
            is_float,
            resampler: LinearResampler::new(sample_rate, 48000),
        }
    }

    pub fn process_packet(&mut self, data: &[u8], is_silent: bool) -> Vec<[f32; 2]> {
        let bytes_per_sample = (self.bits_per_sample as usize / 8).max(1);
        let frame_bytes = self.channels * bytes_per_sample;
        if frame_bytes == 0 || data.len() < frame_bytes {
            return Vec::new();
        }
        let num_frames = data.len() / frame_bytes;

        if is_silent {
            let silence = vec![[0.0f32, 0.0f32]; num_frames];
            return self.resampler.process(&silence);
        }

        let mut stereo_frames = Vec::with_capacity(num_frames);
        let mut channel_buf = vec![0.0f32; self.channels];

        for i in 0..num_frames {
            let frame_offset = i * frame_bytes;
            for c in 0..self.channels {
                let sample_offset = frame_offset + c * bytes_per_sample;
                if sample_offset + bytes_per_sample > data.len() {
                    channel_buf[c] = 0.0;
                    continue;
                }
                let sample_bytes = &data[sample_offset..sample_offset + bytes_per_sample];
                channel_buf[c] = if self.is_float {
                    if sample_bytes.len() >= 4 {
                        f32::from_le_bytes([
                            sample_bytes[0],
                            sample_bytes[1],
                            sample_bytes[2],
                            sample_bytes[3],
                        ])
                    } else {
                        0.0
                    }
                } else {
                    match self.bits_per_sample {
                        16 => {
                            if sample_bytes.len() >= 2 {
                                let v = i16::from_le_bytes([sample_bytes[0], sample_bytes[1]]);
                                v as f32 / 32768.0
                            } else {
                                0.0
                            }
                        }
                        24 => {
                            if sample_bytes.len() >= 3 {
                                let v = ((sample_bytes[2] as i8 as i32) << 16)
                                    | ((sample_bytes[1] as i32) << 8)
                                    | (sample_bytes[0] as i32);
                                v as f32 / 8388608.0
                            } else {
                                0.0
                            }
                        }
                        32 => {
                            if sample_bytes.len() >= 4 {
                                let v = i32::from_le_bytes([
                                    sample_bytes[0],
                                    sample_bytes[1],
                                    sample_bytes[2],
                                    sample_bytes[3],
                                ]);
                                v as f32 / 2147483648.0
                            } else {
                                0.0
                            }
                        }
                        _ => 0.0,
                    }
                };
            }
            let (l, r) = downmix_frame_to_stereo(&channel_buf);
            stereo_frames.push([l, r]);
        }

        self.resampler.process(&stereo_frames)
    }
}

#[cfg(windows)]
#[derive(Clone)]
struct AudioTarget {
    path: PathBuf,
    start_qpc: i64,
    is_ram: bool,
}

#[cfg(windows)]
struct AudioRotateRequest {
    target: Option<AudioTarget>,
    done_tx: Option<std::sync::mpsc::SyncSender<()>>,
}

/// A cut waiting until the audio up to it is encoded.
#[cfg(windows)]
struct PendingCut {
    target: AudioTarget,
    sample: u64,
    done_tx: Option<std::sync::mpsc::SyncSender<()>>,
    deadline: Instant,
}

#[cfg(windows)]
impl PendingCut {
    fn run(self, game: &mut Option<AacTrack>, apps: &mut [Option<AacTrack>], mic: &mut Option<AacTrack>) {
        let path = &self.target.path;
        let cut = |track: &mut Option<AacTrack>, path: &Path| {
            if let Some(Err(e)) = track.as_mut().map(|t| t.cut(self.sample, path, self.target.is_ram)) {
                eprintln!("Audio segment error: {e}");
                let _ = std::io::stderr().flush();
            }
        };
        cut(game, path);
        for (i, app) in apps.iter_mut().enumerate() {
            cut(app, &app_path(path, i));
        }
        cut(mic, &path.with_extension("mic.m4a"));
        if let Some(tx) = self.done_tx {
            let _ = tx.send(());
        }
    }
}

#[cfg(windows)]
fn app_path(segment: &Path, idx: usize) -> PathBuf {
    segment.with_extension(format!("app-{idx}.m4a"))
}

/// Stores the track names next to the segment: its audio is seg_N.m4a, the joiner reads
/// them from seg_N.tracks.json.
#[cfg(windows)]
fn save_track_names(target: &AudioTarget, names: &[String]) {
    if names.len() < 2 {
        return;
    }
    let sidecar = target.path.with_extension("tracks.json");
    if let Ok(json) = serde_json::to_vec(names) {
        if target.is_ram {
            crate::recorder_windows::mem_store::put(sidecar, json);
        } else {
            let _ = std::fs::write(sidecar, json);
        }
    }
}

#[cfg(windows)]
struct ActiveAudioStream {
    client: AudioClient,
    capture: AudioCaptureClient,
    processor: AudioDeviceProcessor,
    raw_buffer: Vec<u8>,
    pending: PendingAudio,
    gain: f32,
}

/// Captured 48 kHz stereo frames of one stream not yet mixed into a track.
#[cfg_attr(not(windows), allow(dead_code))]
struct PendingAudio {
    queue: VecDeque<[f32; 2]>,
    /// Sent nothing for a while: it doesn't hold its track back, and its next audio is
    /// placed at the clock.
    idle: bool,
}

/// How far a stream may trail the clock before its missing audio counts as silence. Must stay
/// above normal delivery jitter, or a live stream gets cut into pieces.
#[cfg_attr(not(windows), allow(dead_code))]
const MIX_SLACK_FRAMES: usize = 48000 * 60 / 1000;

/// Mixes the captured audio of `streams` into the next block of one track, whose writer is
/// at `written` with the clock at `target` (frames). Live streams set the pace; a quiet one
/// (loopback sends nothing then) is silence, so nothing piles up behind it.
#[cfg_attr(not(windows), allow(dead_code))]
fn mix_streams(streams: &mut [&mut PendingAudio], written: usize, target: usize) -> Vec<[f32; 2]> {
    let due = target.saturating_sub(written + MIX_SLACK_FRAMES);
    for s in streams.iter_mut() {
        if s.idle && !s.queue.is_empty() {
            for _ in 0..target.saturating_sub(written + s.queue.len()) {
                s.queue.push_front([0.0, 0.0]);
            }
            s.idle = false;
        } else if s.queue.len() < due {
            s.idle = true;
        }
    }
    let count = streams.iter().filter(|s| !s.idle).map(|s| s.queue.len()).min().unwrap_or(due);
    let mut out = vec![[0.0f32, 0.0f32]; count];
    for s in streams.iter_mut() {
        let take = count.min(s.queue.len());
        for (o, f) in out.iter_mut().zip(s.queue.drain(..take)) {
            o[0] += f[0];
            o[1] += f[1];
        }
    }
    for o in &mut out {
        *o = [o[0].clamp(-1.0, 1.0), o[1].clamp(-1.0, 1.0)];
    }
    out
}

/// Writes the next mixed block of one track; without a writer the audio is dropped.
#[cfg(windows)]
fn advance_track(writer: Option<&mut AacTrack>, streams: Vec<&mut ActiveAudioStream>, target: usize) {
    let mut pending: Vec<&mut PendingAudio> = streams.into_iter().map(|s| &mut s.pending).collect();
    match writer {
        Some(w) => {
            let frames = mix_streams(&mut pending, w.written(), target);
            let _ = w.write(&frames);
        }
        None => pending.iter_mut().for_each(|s| s.queue.clear()),
    }
}

#[cfg(windows)]
impl ActiveAudioStream {
    /// Starts capturing on an initialized-to-be client, converted by Windows to 48 kHz stereo.
    fn start(mut client: AudioClient) -> Result<Self> {
        let format = WaveFormat::new(32, 32, SampleType::Float, 48000, 2, None);
        // 200 ms: the mixer thread also enumerates processes and opens streams, and a buffer
        // of one device period overflows (drops audio) on any such stall.
        let mode = StreamMode::PollingShared { autoconvert: true, buffer_duration_hns: 2_000_000 };
        client.initialize_client(&format, Direction::Capture, mode, None).context("Failed to initialize audio client")?;
        let capture = client.get_audiocaptureclient().context("Failed to get AudioCaptureClient")?;
        client.start_stream().context("Failed to start audio stream")?;
        Ok(Self {
            client,
            capture,
            processor: AudioDeviceProcessor::new(2, 48000, 32, true),
            raw_buffer: Vec::new(),
            pending: PendingAudio { queue: VecDeque::new(), idle: true },
            gain: 1.0,
        })
    }

    /// Moves newly captured audio into `queue`, keeping at most one second.
    fn read(&mut self) {
        let mut temp = VecDeque::new();
        self.read_available_packets(&mut temp);
        if (self.gain - 1.0).abs() >= 0.01 {
            for f in &mut temp {
                *f = [crate::denoise::soft_limit(f[0] * self.gain), crate::denoise::soft_limit(f[1] * self.gain)];
            }
        }
        let queue = &mut self.pending.queue;
        queue.extend(temp);
        if queue.len() > 48000 {
            let excess = queue.len() - 48000;
            queue.drain(..excess);
        }
    }

    fn read_available_packets(&mut self, queue: &mut VecDeque<[f32; 2]>) -> usize {
        let bytes_per_sample = (self.processor.bits_per_sample as usize / 8).max(1);
        let bytes_per_frame = self.processor.channels * bytes_per_sample;
        if bytes_per_frame == 0 {
            return 0;
        }

        let mut total_produced = 0;
        while let Ok(Some(packet_size)) = self.capture.get_next_packet_size() {
            if packet_size == 0 {
                break;
            }
            let bytes_needed = packet_size as usize * bytes_per_frame;
            if self.raw_buffer.len() < bytes_needed {
                self.raw_buffer.resize(bytes_needed, 0);
            }
            match self.capture.read_from_device(&mut self.raw_buffer[..bytes_needed]) {
                Ok((frames_read, info)) => {
                    if frames_read > 0 {
                        let actual_bytes = frames_read as usize * bytes_per_frame;
                        let resampled = self.processor.process_packet(
                            &self.raw_buffer[..actual_bytes],
                            info.flags.silent,
                        );
                        total_produced += resampled.len();
                        queue.extend(resampled);
                    }
                }
                Err(e) => {
                    eprintln!("Audio read error: {e}");
                    let _ = std::io::stderr().flush();
                    break;
                }
            }
        }
        total_produced
    }
}

/// Captures desktop loopback and microphone via WASAPI and mixes them into a single stereo 48 kHz stream,
/// writing AAC audio segments via Media Foundation.
#[cfg(windows)]
pub struct AudioMixer {
    running: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
    target: Arc<Mutex<Option<AudioRotateRequest>>>,
    target_changed: Arc<AtomicBool>,
}

#[cfg(windows)]
impl AudioMixer {
    pub fn new(config: &crate::config::Config) -> Result<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let target: Arc<Mutex<Option<AudioRotateRequest>>> = Arc::new(Mutex::new(None));
        let target_clone = target.clone();
        let target_changed = Arc::new(AtomicBool::new(false));
        let target_changed_clone = target_changed.clone();

        let separate_apps = if config.separate_app_audio { config.separate_apps.clone() } else { Vec::new() };
        let excluded_apps = config.excluded_apps.clone();
        let input_choice = config.audio_input.clone();
        let output_choice = config.audio_output.clone();
        let mic_gain = config.mic_gain;
        let separate_audio_tracks = config.separate_audio_tracks;

        let thread = thread::Builder::new()
            .name("lapse-wasapi-audio".into())
            .spawn(move || {
                let _ = initialize_mta();
                unsafe {
                    let _ = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET);
                }

                // With apps split off or excluded, the main track is built per process instead
                // of from the whole device, so those apps stay out of it.
                let mut process_loopback_active = false;
                if !separate_apps.is_empty() || !excluded_apps.is_empty() {
                    match init_application_loopback(std::process::id(), true) {
                        Ok(test_stream) => {
                            let _ = test_stream.client.stop_stream();
                            process_loopback_active = true;
                        }
                        Err(e) => {
                            eprintln!("WASAPI process loopback unavailable; recording the whole device, app separation and exclusions are off: {e}");
                            let _ = std::io::stderr().flush();
                        }
                    }
                }

                let mut desktop_stream = if process_loopback_active {
                    None
                } else {
                    match init_desktop_loopback(&output_choice) {
                        Ok(stream) => Some(stream),
                        Err(e) => {
                            eprintln!("WASAPI desktop loopback init error: {e}");
                            let _ = std::io::stderr().flush();
                            None
                        }
                    }
                };

                let mut app_streams: Vec<(u32, usize, ActiveAudioStream)> = Vec::new();
                let mut other_streams: Vec<(u32, ActiveAudioStream)> = Vec::new();

                let mut mic_stream = if !input_choice.is_empty()
                    && !input_choice.eq_ignore_ascii_case("none")
                {
                    match init_mic_capture(&input_choice) {
                        Ok(mut stream) => {
                            stream.gain = mic_gain;
                            Some(stream)
                        }
                        Err(e) => {
                            eprintln!("WASAPI microphone capture init error: {e}");
                            let _ = std::io::stderr().flush();
                            None
                        }
                    }
                } else {
                    None
                };


                let mut game_writer: Option<AacTrack> = None;
                let mut app_writers: Vec<Option<AacTrack>> = Vec::new();
                let mut mic_writer: Option<AacTrack> = None;
                // When the first segment started; every track counts its samples from there.
                let mut start_qpc: Option<i64> = None;
                let mut pending_cut: Option<PendingCut> = None;
                let mut track_names: Vec<String> = Vec::new();
                let mut last_proc_refresh = Instant::now() - Duration::from_secs(10);

                while running_clone.load(Ordering::Relaxed) {
                    let tick_start = std::time::Instant::now();

                    if target_changed_clone.swap(false, Ordering::SeqCst) {
                        let req = target_clone.lock().ok().and_then(|mut t| t.take());
                        if let Some(cut) = pending_cut.take() {
                            cut.run(&mut game_writer, &mut app_writers, &mut mic_writer);
                        }
                        match req {
                            Some(AudioRotateRequest { target: Some(target), done_tx }) if start_qpc.is_some() => {
                                let sample = (target.start_qpc - start_qpc.unwrap_or(0)).max(0) as u64 * 48000 / 10_000_000;
                                pending_cut = Some(PendingCut { target, sample, done_tx, deadline: Instant::now() + Duration::from_millis(300) });
                            }
                            Some(AudioRotateRequest { target: Some(target), done_tx }) => {
                                start_qpc = Some(target.start_qpc);
                                let open = |path: &Path| match AacTrack::new(path, target.is_ram) {
                                    Ok(track) => Some(track),
                                    Err(e) => {
                                        eprintln!("AAC encoder error: {e}");
                                        let _ = std::io::stderr().flush();
                                        None
                                    }
                                };
                                game_writer = open(&target.path);
                                if process_loopback_active {
                                    app_writers = (0..separate_apps.len()).map(|i| open(&app_path(&target.path, i))).collect();
                                }
                                if mic_stream.is_some() && (process_loopback_active || separate_audio_tracks) {
                                    mic_writer = open(&target.path.with_extension("mic.m4a"));
                                }
                                let mut names = vec!["game".to_string()];
                                names.extend(
                                    separate_apps.iter().zip(&app_writers).filter(|(_, w)| w.is_some()).map(|(app, _)| format!("app:{app}")),
                                );
                                names.extend(mic_writer.is_some().then(|| "mic".to_string()));
                                track_names = names;
                                save_track_names(&target, &track_names);
                                if let Some(tx) = done_tx {
                                    let _ = tx.send(());
                                }
                            }
                            Some(AudioRotateRequest { target: None, done_tx }) => {
                                for w in game_writer.take().into_iter().chain(app_writers.drain(..).flatten()).chain(mic_writer.take()) {
                                    let _ = w.finish();
                                }
                                start_qpc = None;
                                if let Some(tx) = done_tx {
                                    let _ = tx.send(());
                                }
                            }
                            None => {}
                        }
                        if let Some(cut) = &pending_cut {
                            save_track_names(&cut.target, &track_names);
                        }
                    }

                    let cut_ready = pending_cut.as_ref().is_some_and(|cut| {
                        Instant::now() >= cut.deadline
                            || game_writer.iter().chain(app_writers.iter().flatten()).chain(&mic_writer).all(|w| w.has_until(cut.sample))
                    });
                    if cut_ready {
                        if let Some(cut) = pending_cut.take() {
                            cut.run(&mut game_writer, &mut app_writers, &mut mic_writer);
                        }
                    }

                    if process_loopback_active && last_proc_refresh.elapsed() > Duration::from_secs(2) {
                        last_proc_refresh = Instant::now();
                        let procs = crate::hardware::game_detector_windows::list_processes();
                        let view: Vec<Proc> = procs.iter().map(|p| Proc { pid: p.pid, parent: p.parent_pid, exe: &p.exe_name }).collect();
                        let plan = plan_capture(&view, &session_pids(false), &separate_apps, &excluded_apps);

                        app_streams.retain(|(pid, idx, stream)| {
                            let keep = plan.apps.contains(&(*pid, *idx));
                            if !keep {
                                let _ = stream.client.stop_stream();
                            }
                            keep
                        });
                        for (pid, idx) in plan.apps {
                            if !app_streams.iter().any(|(p, i, _)| *p == pid && *i == idx) {
                                if let Ok(stream) = init_application_loopback(pid, true) {
                                    app_streams.push((pid, idx, stream));
                                }
                            }
                        }

                        other_streams.retain(|(pid, stream)| {
                            let keep = plan.others.contains(pid);
                            if !keep {
                                let _ = stream.client.stop_stream();
                            }
                            keep
                        });
                        for pid in plan.others {
                            if !other_streams.iter().any(|(p, _)| *p == pid) {
                                if let Ok(stream) = init_application_loopback(pid, true) {
                                    other_streams.push((pid, stream));
                                }
                            }
                        }
                    }

                    for s in desktop_stream.iter_mut().chain(mic_stream.iter_mut()) {
                        s.read();
                    }
                    for (_, s) in &mut other_streams {
                        s.read();
                    }
                    for (_, _, s) in &mut app_streams {
                        s.read();
                    }

                    let now_qpc = crate::recorder_windows::capture::current_qpc_100ns();
                    let elapsed_100ns = (now_qpc - start_qpc.unwrap_or(now_qpc)).max(0);
                    let target_frames = ((elapsed_100ns * 48000) / 10_000_000) as usize;

                    // The microphone joins the main track unless it has a track of its own.
                    let mic_alone = mic_writer.is_some();
                    let mut main: Vec<&mut ActiveAudioStream> = desktop_stream.iter_mut().chain(other_streams.iter_mut().map(|(_, s)| s)).collect();
                    if !mic_alone {
                        main.extend(mic_stream.iter_mut());
                    }
                    advance_track(game_writer.as_mut(), main, target_frames);
                    for (idx, writer) in app_writers.iter_mut().enumerate() {
                        let streams = app_streams.iter_mut().filter(|(_, i, _)| *i == idx).map(|(_, _, s)| s).collect();
                        advance_track(writer.as_mut(), streams, target_frames);
                    }
                    if mic_alone {
                        advance_track(mic_writer.as_mut(), mic_stream.iter_mut().collect(), target_frames);
                    }

                    let elapsed = tick_start.elapsed();
                    if elapsed < Duration::from_millis(5) {
                        thread::sleep(Duration::from_millis(5) - elapsed);
                    }
                }

                for w in game_writer.take().into_iter().chain(app_writers.drain(..).flatten()).chain(mic_writer.take()) {
                    let _ = w.finish();
                }

                unsafe {
                    let _ = MFShutdown();
                }

                if let Some(stream) = desktop_stream {
                    let _ = stream.client.stop_stream();
                }
                for (_, _, stream) in app_streams {
                    let _ = stream.client.stop_stream();
                }
                for (_, stream) in other_streams {
                    let _ = stream.client.stop_stream();
                }
                if let Some(stream) = mic_stream {
                    let _ = stream.client.stop_stream();
                }
            })
            .context("Failed to spawn audio mixer thread")?;

        Ok(Self {
            running,
            thread: Mutex::new(Some(thread)),
            target,
            target_changed,
        })
    }

    /// Continues the audio in a new segment file (`None`: ends it). The receiver hears back
    /// once the finished segment's file is complete.
    pub fn rotate_segment(&self, path: Option<PathBuf>, start_qpc: i64, is_ram: bool) -> std::sync::mpsc::Receiver<()> {
        let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
        if let Ok(mut lock) = self.target.lock() {
            *lock = Some(AudioRotateRequest {
                target: path.map(|p| AudioTarget { path: p, start_qpc, is_ram }),
                done_tx: Some(done_tx),
            });
            self.target_changed.store(true, Ordering::SeqCst);
        }
        done_rx
    }

    pub fn stop(&self) {
        self.rotate_segment(None, 0, false);
        self.running.store(false, Ordering::Relaxed);
        if let Ok(mut lock) = self.thread.lock() {
            if let Some(t) = lock.take() {
                let _ = t.join();
            }
        }
    }
}

#[cfg(windows)]
impl Drop for AudioMixer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(windows)]
fn init_desktop_loopback(output_name: &str) -> Result<ActiveAudioStream> {
    let enumerator = DeviceEnumerator::new().context("Failed to create DeviceEnumerator")?;
    let chosen = if is_default_device(output_name) {
        None
    } else {
        enumerator
            .get_device_collection(Direction::Render)
            .ok()
            .and_then(|coll| coll.get_device_with_name(output_name).ok())
    };
    let device = match chosen {
        Some(d) => d,
        None => enumerator
            .get_default_device(Direction::Render)
            .context("Failed to get default render device")?,
    };
    let name = device
        .get_friendlyname()
        .unwrap_or_else(|_| "Default Render Device".into());

    let audio_client = device
        .get_iaudioclient()
        .context("Failed to get IAudioClient for render device")?;
    if let Ok(f) = audio_client.get_mixformat() {
        eprintln!("Audio device opened: loopback ({}) format: {} Hz, {} channels", name, f.get_samplespersec(), f.get_nchannels());
        let _ = std::io::stderr().flush();
    }
    ActiveAudioStream::start(audio_client).context("Failed to start desktop loopback")
}

/// Processes with an audio session on any output device (only ones playing right now when
/// `active_only`), without Lapse itself and Windows' system sounds.
#[cfg(windows)]
fn session_pids(active_only: bool) -> Vec<u32> {
    let _ = initialize_mta();
    let Ok(enumerator) = DeviceEnumerator::new() else { return Vec::new() };
    let Ok(devices) = enumerator.get_device_collection(Direction::Render) else { return Vec::new() };
    let me = std::process::id();
    let mut pids = Vec::new();
    for device in &devices {
        let Ok(device) = device else { continue };
        let Some(sessions) = device.get_iaudiosessionmanager().ok().and_then(|m| m.get_audiosessionenumerator().ok()) else {
            continue;
        };
        for i in 0..sessions.get_count().unwrap_or(0) {
            let Ok(session) = sessions.get_session(i) else { continue };
            let wanted = match session.get_state() {
                Ok(SessionState::Active) => true,
                Ok(SessionState::Inactive) => !active_only,
                _ => false,
            };
            let pid = session.get_process_id().unwrap_or(0);
            if wanted && pid != 0 && pid != me && !pids.contains(&pid) {
                pids.push(pid);
            }
        }
    }
    pids
}

/// Programs playing sound right now on any output device, as (display name, process name)
/// for the "Separate app audio" picker.
#[cfg(windows)]
pub fn playing_apps() -> Vec<(String, String)> {
    let mut apps: Vec<(String, String)> = Vec::new();
    for pid in session_pids(true) {
        let Some(stem) = crate::hardware::game_detector_windows::process_exe_path(pid)
            .and_then(|p| p.file_stem().map(|f| f.to_string_lossy().to_string()))
        else {
            continue;
        };
        if stem.eq_ignore_ascii_case("lapse") || apps.iter().any(|(_, m)| m.eq_ignore_ascii_case(&stem)) {
            continue;
        }
        let mut c = stem.chars();
        let name = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
        apps.push((name, stem));
    }
    apps
}

/// A running process as the capture plan sees it.
struct Proc<'a> {
    pid: u32,
    parent: u32,
    exe: &'a str,
}

/// Which processes to record when some apps get their own track or are never recorded.
/// Windows can only capture a process together with its whole child tree, so every capture
/// is placed at a tree that holds no app it must not contain.
#[derive(Debug, Default, PartialEq)]
struct CapturePlan {
    /// (pid, index into the separated apps): the app's own track.
    apps: Vec<(u32, usize)>,
    /// Everything else that has an audio session: mixed into the main track.
    others: Vec<u32>,
}

fn plan_capture(procs: &[Proc], sessions: &[u32], separate: &[String], excluded: &[String]) -> CapturePlan {
    let stem = |exe: &str| exe.strip_suffix(".exe").or_else(|| exe.strip_suffix(".EXE")).unwrap_or(exe).to_ascii_lowercase();
    let find = |list: &[String], exe: &str| {
        let s = stem(exe);
        list.iter().position(|a| stem(a) == s)
    };
    let by_pid: std::collections::HashMap<u32, &Proc> = procs.iter().map(|p| (p.pid, p)).collect();
    let ancestors = |pid: u32| {
        let mut out = Vec::new();
        let mut cur = by_pid.get(&pid).map(|p| p.parent);
        while let Some(p) = cur {
            if p == 0 || p == pid || out.contains(&p) || out.len() > 64 {
                break;
            }
            out.push(p);
            cur = by_pid.get(&p).map(|x| x.parent);
        }
        out
    };
    let app_idx = |pid: u32| by_pid.get(&pid).and_then(|p| find(separate, p.exe));
    let removed = |pid: u32| {
        by_pid.get(&pid).is_some_and(|p| find(excluded, p.exe).is_some() || find(separate, p.exe).is_some())
    };

    let mut plan = CapturePlan::default();
    for p in procs {
        if find(excluded, p.exe).is_some() {
            continue;
        }
        if let Some(idx) = find(separate, p.exe) {
            // Child processes of the same app are already in their parent's tree.
            if !ancestors(p.pid).into_iter().any(|a| app_idx(a).is_some()) {
                plan.apps.push((p.pid, idx));
            }
        }
    }
    for &s in sessions {
        if removed(s) || ancestors(s).into_iter().any(removed) {
            continue;
        }
        // Its tree would carry a separated or excluded app along.
        if procs.iter().any(|p| removed(p.pid) && ancestors(p.pid).contains(&s)) {
            continue;
        }
        plan.others.push(s);
    }
    let chosen = plan.others.clone();
    plan.others.retain(|&s| !ancestors(s).iter().any(|a| chosen.contains(a)));
    plan
}

/// Empty or one of the "default" names the settings have used for "follow Windows' default device".
fn is_default_device(name: &str) -> bool {
    ["", "default", "default_output", "default_input"].iter().any(|d| name.eq_ignore_ascii_case(d))
}

#[cfg(windows)]
fn init_mic_capture(input_name: &str) -> Result<ActiveAudioStream> {
    let enumerator = DeviceEnumerator::new().context("Failed to create DeviceEnumerator")?;
    let device = if !is_default_device(input_name) {
        if let Ok(coll) = enumerator.get_device_collection(Direction::Capture) {
            coll.get_device_with_name(input_name).ok()
        } else {
            None
        }
    } else {
        None
    };

    let device = match device {
        Some(d) => d,
        None => enumerator
            .get_default_device(Direction::Capture)
            .context("Failed to get default capture device")?,
    };

    let name = device
        .get_friendlyname()
        .unwrap_or_else(|_| "Default Capture Device".into());

    let audio_client = device
        .get_iaudioclient()
        .context("Failed to get IAudioClient for capture device")?;
    if let Ok(f) = audio_client.get_mixformat() {
        eprintln!("Audio device opened: microphone ({}) format: {} Hz, {} channels", name, f.get_samplespersec(), f.get_nchannels());
        let _ = std::io::stderr().flush();
    }
    ActiveAudioStream::start(audio_client).context("Failed to start microphone capture")
}

#[cfg(windows)]
fn init_application_loopback(process_id: u32, include_tree: bool) -> Result<ActiveAudioStream> {
    let audio_client = AudioClient::new_application_loopback_client(process_id, include_tree)
        .context("Failed to create application loopback client")?;
    ActiveAudioStream::start(audio_client)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn procs() -> Vec<Proc<'static>> {
        vec![
            Proc { pid: 1, parent: 0, exe: "explorer.exe" },
            Proc { pid: 10, parent: 1, exe: "firefox.exe" },
            Proc { pid: 11, parent: 10, exe: "firefox.exe" },
            Proc { pid: 20, parent: 1, exe: "steam.exe" },
            Proc { pid: 21, parent: 20, exe: "RDR2.exe" },
            Proc { pid: 30, parent: 1, exe: "Discord.exe" },
            Proc { pid: 31, parent: 30, exe: "Discord.exe" },
            Proc { pid: 40, parent: 1, exe: "Spotify.exe" },
        ]
    }

    #[test]
    fn test_plan_capture_keeps_split_apps_out_of_main_track() {
        let sessions = [11, 20, 21, 31, 40];
        let plan = plan_capture(&procs(), &sessions, &["firefox".to_string()], &["spotify".to_string()]);
        // Firefox once, at the top of its tree; the child's session isn't captured twice.
        assert_eq!(plan.apps, vec![(10, 0)]);
        // Steam's tree holds RDR2 (not removed), so Steam stays and covers RDR2; Spotify is excluded.
        assert_eq!(plan.others, vec![20, 31]);
    }

    #[test]
    fn test_plan_capture_skips_trees_holding_a_split_app() {
        let sessions = [20, 21, 30, 31];
        let plan = plan_capture(&procs(), &sessions, &["rdr2".to_string()], &["discord.exe".to_string()]);
        assert_eq!(plan.apps, vec![(21, 0)]);
        // Steam's tree would carry RDR2 along, Discord is excluded: nothing else is captured.
        assert!(plan.others.is_empty());
    }

    /// Runs 10 s segments in 5 ms ticks with a live stream and one that is quiet except in
    /// `active` (ticks); returns the live stream's backlog after each segment.
    fn run_segments(segments: usize, active: std::ops::Range<usize>) -> Vec<usize> {
        let mut live = PendingAudio { queue: VecDeque::new(), idle: true };
        let mut other = PendingAudio { queue: VecDeque::new(), idle: true };
        let mut backlog = Vec::new();
        let mut tick = 0;
        for _ in 0..segments {
            let mut written = 0;
            for t in 1..=2000 {
                live.queue.extend([[0.5, 0.5]; 240]);
                if active.contains(&tick) {
                    other.queue.extend([[0.25, 0.25]; 240]);
                }
                tick += 1;
                let out = mix_streams(&mut [&mut live, &mut other], written, t * 240);
                // The live stream is never interrupted.
                assert!(out.iter().all(|f| f[0] >= 0.5));
                written += out.len();
            }
            backlog.push(live.queue.len());
        }
        backlog
    }

    #[test]
    fn test_mix_streams_does_not_drift_behind_a_quiet_stream() {
        assert_eq!(run_segments(8, 0..0), vec![0; 8]);
        // The other stream plays for a while and stops: no lasting backlog either.
        assert_eq!(run_segments(8, 500..3000), vec![0; 8]);
    }

    #[test]
    fn test_mix_streams_fills_silence_when_all_are_quiet() {
        let mut s = PendingAudio { queue: VecDeque::new(), idle: true };
        assert!(mix_streams(&mut [&mut s], 0, MIX_SLACK_FRAMES).is_empty());
        assert_eq!(mix_streams(&mut [&mut s], 0, 48000).len(), 48000 - MIX_SLACK_FRAMES);
    }

    #[test]
    fn test_float_to_s16_conversion() {
        assert_eq!(float_to_i16(0.0), 0);
        assert_eq!(float_to_i16(1.0), 32767);
        assert_eq!(float_to_i16(-1.0), -32768);
        assert_eq!(float_to_i16(0.5), 16384);
        assert_eq!(float_to_i16(-0.5), -16384);

        // Clamping out-of-range values
        assert_eq!(float_to_i16(1.5), 32767);
        assert_eq!(float_to_i16(-2.0), -32768);
        assert_eq!(float_to_i16(100.0), 32767);
        assert_eq!(float_to_i16(-100.0), -32768);

        // Special floating point values
        assert_eq!(float_to_i16(f32::NAN), 0);
        assert_eq!(float_to_i16(f32::INFINITY), 32767);
        assert_eq!(float_to_i16(f32::NEG_INFINITY), -32768);
    }

    #[test]
    fn test_6ch_to_stereo_downmix() {
        // 5.1 layout: [FL, FR, FC, LFE, BL, BR]
        // Left only
        let (l, r) = downmix_frame_to_stereo(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(l, 1.0);
        assert_eq!(r, 0.0);

        // Right only
        let (l, r) = downmix_frame_to_stereo(&[0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(l, 0.0);
        assert_eq!(r, 1.0);

        // Center only folds to both Left and Right with DOWNMIX_COEFF
        let (l, r) = downmix_frame_to_stereo(&[0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        assert!((l - DOWNMIX_COEFF).abs() < 1e-6);
        assert!((r - DOWNMIX_COEFF).abs() < 1e-6);

        // LFE only folds to both Left and Right with DOWNMIX_COEFF
        let (l, r) = downmix_frame_to_stereo(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        assert!((l - DOWNMIX_COEFF).abs() < 1e-6);
        assert!((r - DOWNMIX_COEFF).abs() < 1e-6);

        // Back Left only folds to Left
        let (l, r) = downmix_frame_to_stereo(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        assert!((l - DOWNMIX_COEFF).abs() < 1e-6);
        assert_eq!(r, 0.0);

        // Back Right only folds to Right
        let (l, r) = downmix_frame_to_stereo(&[0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        assert_eq!(l, 0.0);
        assert!((r - DOWNMIX_COEFF).abs() < 1e-6);

        // All channels active
        let (l, r) = downmix_frame_to_stereo(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        let expected = 1.0 + 3.0 * DOWNMIX_COEFF;
        assert!((l - expected).abs() < 1e-5);
        assert!((r - expected).abs() < 1e-5);
    }

    #[test]
    fn test_resampler_44_1k_to_48k_length_continuity() {
        let in_rate = 44100;
        let out_rate = 48000;
        let num_in_samples = 44101;
        let freq = 440.0f32;

        let in_samples: Vec<[f32; 2]> = (0..num_in_samples)
            .map(|i| {
                let s = ((i as f32 * freq * 2.0 * std::f32::consts::PI) / in_rate as f32).sin();
                [s, s]
            })
            .collect();

        // 1. Single buffer processing
        let mut resampler_single = LinearResampler::new(in_rate, out_rate);
        let out_single = resampler_single.process(&in_samples);

        // At 48000 Hz, 44101 samples at 44.1 kHz corresponds to 48001 frames
        assert_eq!(out_single.len(), 48001);

        // 2. Chunked processing with variable packet sizes across buffers
        let mut resampler_chunked = LinearResampler::new(in_rate, out_rate);
        let mut out_chunked = Vec::new();
        let chunk_sizes = [441, 100, 256, 512, 128, 441, 1000, 300];
        let mut pos = 0;
        let mut chunk_idx = 0;

        while pos < num_in_samples {
            let sz = chunk_sizes[chunk_idx % chunk_sizes.len()].min(num_in_samples - pos);
            let chunk_out = resampler_chunked.process(&in_samples[pos..pos + sz]);
            out_chunked.extend(chunk_out);
            pos += sz;
            chunk_idx += 1;
        }

        // Length must match
        assert_eq!(out_chunked.len(), out_single.len());

        // Continuity & sample-for-sample equivalence across buffer boundaries
        for i in 0..out_single.len() {
            assert!(
                (out_single[i][0] - out_chunked[i][0]).abs() < 1e-5,
                "Sample mismatch at index {i}: single={}, chunked={}",
                out_single[i][0],
                out_chunked[i][0]
            );
            assert!(
                (out_single[i][1] - out_chunked[i][1]).abs() < 1e-5,
                "Sample mismatch at index {i}: single={}, chunked={}",
                out_single[i][1],
                out_chunked[i][1]
            );

            // Continuity check: derivative of 440 Hz sine at 48 kHz cannot jump abruptly
            if i > 0 {
                let diff_l = (out_chunked[i][0] - out_chunked[i - 1][0]).abs();
                let diff_r = (out_chunked[i][1] - out_chunked[i - 1][1]).abs();
                assert!(
                    diff_l < 0.06,
                    "Discontinuity detected in left channel at index {i}: jump={diff_l}"
                );
                assert!(
                    diff_r < 0.06,
                    "Discontinuity detected in right channel at index {i}: jump={diff_r}"
                );
            }
        }
    }
}
