//! Microphone capture via CPAL, energy-based VAD, and background trigger listener.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::voice::features::{
    dbfs_to_linear, extract_mfcc, linear_to_dbfs, resample_to_16k, HOP_LEN,
};
use crate::voice::matcher::{match_utterance, VoiceTemplates};

const BLOCK_SAMPLES: usize = HOP_LEN; // 160 samples = 10 ms at 16 kHz
const PADDING_SAMPLES: usize = 2400;  // 150 ms at 16 kHz
const MIN_UTTERANCE_SAMPLES: usize = 3200;  // 0.20 s at 16 kHz
const MAX_UTTERANCE_SAMPLES: usize = 32000; // 2.0 s at 16 kHz
const SILENCE_BLOCKS_LIMIT: usize = 25;     // 250 ms silence (25 * 10 ms)
const ONSET_BLOCKS_LIMIT: usize = 8;        // 80 ms speech onset (8 * 10 ms)
const ABSOLUTE_MIN_THRESHOLD_DB: f32 = -70.0;
const MIN_NOISE_FLOOR_DB: f32 = -85.0;
const MAX_NOISE_FLOOR_DB: f32 = -30.0;

/// Energy-based Voice Activity Detector with adaptive noise floor.
pub struct EnergyVad {
    noise_floor: f32,
    ring_buf: Vec<f32>,
    ring_pos: usize,
    in_utterance: bool,
    utterance_buf: Vec<f32>,
    consecutive_voiced: usize,
    silence_blocks: usize,
}

impl EnergyVad {
    pub fn new() -> Self {
        Self {
            noise_floor: dbfs_to_linear(-75.0),
            ring_buf: vec![0.0; PADDING_SAMPLES],
            ring_pos: 0,
            in_utterance: false,
            utterance_buf: Vec::new(),
            consecutive_voiced: 0,
            silence_blocks: 0,
        }
    }

    /// Current tracked noise floor (linear RMS amplitude).
    pub fn noise_floor(&self) -> f32 {
        self.noise_floor
    }

    /// Whether the detector is currently inside an active utterance.
    pub fn in_utterance(&self) -> bool {
        self.in_utterance
    }

    /// Appends audio samples into the pre-padding ring buffer.
    fn push_to_ring(&mut self, block: &[f32]) {
        for &s in block {
            self.ring_buf[self.ring_pos] = s;
            self.ring_pos = (self.ring_pos + 1) % PADDING_SAMPLES;
        }
    }

    /// Extracts the last 150 ms (ordered) from the ring buffer.
    fn get_pre_padding(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(PADDING_SAMPLES);
        for i in 0..PADDING_SAMPLES {
            let idx = (self.ring_pos + i) % PADDING_SAMPLES;
            out.push(self.ring_buf[idx]);
        }
        out
    }

    /// Processes a 10 ms block (160 samples at 16 kHz). Returns an utterance if completed.
    pub fn process_block(&mut self, block: &[f32]) -> (Option<Vec<f32>>, f32) {
        if block.is_empty() {
            return (None, 0.0);
        }

        let mut sum_sq = 0.0f32;
        for &s in block {
            sum_sq += s * s;
        }
        let rms = (sum_sq / (block.len() as f32)).sqrt();

        // Thresholds relative to tracked noise floor
        let start_threshold =
            (self.noise_floor * dbfs_to_linear(12.0)).max(dbfs_to_linear(ABSOLUTE_MIN_THRESHOLD_DB));
        let end_threshold = self.noise_floor * dbfs_to_linear(6.0);

        if !self.in_utterance {
            self.push_to_ring(block);
            if rms >= start_threshold {
                self.consecutive_voiced += 1;
                if self.consecutive_voiced >= ONSET_BLOCKS_LIMIT {
                    self.in_utterance = true;
                    self.consecutive_voiced = 0;
                    self.silence_blocks = 0;
                    self.utterance_buf = self.get_pre_padding();
                }
            } else {
                self.consecutive_voiced = 0;
                // Fast fall when rms is lower than floor, slow rise during ambient silence, never chase speech
                if rms < self.noise_floor {
                    self.noise_floor =
                        (self.noise_floor * 0.85 + rms * 0.15).max(dbfs_to_linear(MIN_NOISE_FLOOR_DB));
                } else {
                    self.noise_floor = (self.noise_floor * 0.999 + rms * 0.001)
                        .clamp(dbfs_to_linear(MIN_NOISE_FLOOR_DB), dbfs_to_linear(MAX_NOISE_FLOOR_DB));
                }
            }
            (None, rms)
        } else {
            self.utterance_buf.extend_from_slice(block);
            if rms < end_threshold {
                self.silence_blocks += 1;
            } else {
                self.silence_blocks = 0;
            }

            let reached_silence = self.silence_blocks >= SILENCE_BLOCKS_LIMIT;
            let reached_max = self.utterance_buf.len() >= MAX_UTTERANCE_SAMPLES;

            if reached_silence || reached_max {
                self.in_utterance = false;
                self.silence_blocks = 0;
                self.consecutive_voiced = 0;

                let total_len = self.utterance_buf.len();
                if total_len >= MIN_UTTERANCE_SAMPLES
                    && total_len <= MAX_UTTERANCE_SAMPLES + PADDING_SAMPLES
                {
                    let utterance = std::mem::take(&mut self.utterance_buf);
                    (Some(utterance), rms)
                } else {
                    self.utterance_buf.clear();
                    (None, rms)
                }
            } else {
                (None, rms)
            }
        }
    }
}

/// Finds the target CPAL input device matching `name_hint` or falls back to default.
#[allow(deprecated)]
pub fn select_input_device(host: &cpal::Host, name_hint: &str) -> Option<cpal::Device> {
    // On Linux the sound server owns the hardware, so opening a `hw:` device fails
    // (EBUSY); go through its ALSA plugin, which records the default source.
    #[cfg(target_os = "linux")]
    {
        let _ = name_hint;
        let devices: Vec<cpal::Device> = host.input_devices().map(|d| d.collect()).unwrap_or_default();
        for want in ["pipewire", "pulse", "default"] {
            if let Some(d) = devices.iter().find(|d| d.name().map(|n| n == want).unwrap_or(false)) {
                return Some(d.clone());
            }
        }
        return host.default_input_device();
    }
    #[allow(unreachable_code)]
    let hint = name_hint.trim();
    if !hint.is_empty() && hint != "default_input" && hint != "None" {
        if let Ok(devices) = host.input_devices() {
            let list: Vec<cpal::Device> = devices.collect();
            // 1. Direct match or substring
            for d in &list {
                if let Ok(name) = d.name() {
                    if name.eq_ignore_ascii_case(hint) || name.contains(hint) || hint.contains(&name) {
                        return Some(d.clone());
                    }
                }
            }
            // 2. Token match for PipeWire/Pulse device names (e.g. alsa_input.usb-Razer_...)
            let tokens: Vec<&str> = hint
                .split(|c: char| !c.is_alphanumeric())
                .filter(|s| {
                    s.len() >= 3
                        && !s.eq_ignore_ascii_case("alsa")
                        && !s.eq_ignore_ascii_case("input")
                        && !s.eq_ignore_ascii_case("mono")
                        && !s.eq_ignore_ascii_case("stereo")
                })
                .collect();
            if !tokens.is_empty() {
                for d in &list {
                    if let Ok(name) = d.name() {
                        let lower = name.to_lowercase();
                        if tokens.iter().any(|tok| lower.contains(&tok.to_lowercase())) {
                            return Some(d.clone());
                        }
                    }
                }
            }
            eprintln!(
                "Voice input device '{}' not found in audio devices, falling back to default input",
                hint
            );
        }
    }
    host.default_input_device()
}

/// Builds an input audio capture stream converting to 16 kHz mono f32 samples.
#[allow(deprecated)]
pub fn start_audio_stream(
    device: &cpal::Device,
    tx_samples: Sender<Vec<f32>>,
) -> anyhow::Result<(cpal::Stream, String, u32, usize)> {
    let supported = device.default_input_config()?;
    let sample_rate = supported.sample_rate();
    let channels = supported.channels() as usize;
    let dev_name = device.name().unwrap_or_else(|_| "default".to_string());
    eprintln!(
        "Voice capture: {} @ {} Hz, {} ch",
        dev_name, sample_rate, channels
    );

    let err_fn = |err| eprintln!("Lapse voice capture error: {}", err);

    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &supported.into(),
            move |data: &[f32], _| {
                let mono: Vec<f32> = if channels == 1 {
                    data.to_vec()
                } else {
                    data.chunks(channels)
                        .map(|chunk| chunk.iter().sum::<f32>() / (chunk.len() as f32))
                        .collect()
                };
                let resampled = resample_to_16k(&mono, sample_rate);
                let _ = tx_samples.send(resampled);
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &supported.into(),
            move |data: &[i16], _| {
                let mono: Vec<f32> = if channels == 1 {
                    data.iter().map(|&s| s as f32 / 32768.0).collect()
                } else {
                    data.chunks(channels)
                        .map(|chunk| {
                            chunk.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                                / (chunk.len() as f32)
                        })
                        .collect()
                };
                let resampled = resample_to_16k(&mono, sample_rate);
                let _ = tx_samples.send(resampled);
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            &supported.into(),
            move |data: &[u16], _| {
                let mono: Vec<f32> = if channels == 1 {
                    data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).collect()
                } else {
                    data.chunks(channels)
                        .map(|chunk| {
                            chunk
                                .iter()
                                .map(|&s| (s as f32 - 32768.0) / 32768.0)
                                .sum::<f32>()
                                / (chunk.len() as f32)
                        })
                        .collect()
                };
                let resampled = resample_to_16k(&mono, sample_rate);
                let _ = tx_samples.send(resampled);
            },
            err_fn,
            None,
        )?,
        _ => anyhow::bail!("Unsupported audio sample format"),
    };

    stream.play()?;
    Ok((stream, dev_name, sample_rate, channels))
}

/// Active daemon voice listener running in a background thread.
pub struct VoiceListener {
    running: Arc<AtomicBool>,
    _stream: Option<cpal::Stream>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

const IMPOSTOR_WINDOW: usize = 30;
const IMPOSTOR_MIN: usize = 8;
const IMPOSTOR_RATIO: f32 = 0.8;

/// Once enough ordinary speech has been heard, a trigger must be at most
/// `IMPOSTOR_RATIO` of that speech's median distance.
fn impostor_limit(impostors: &std::collections::VecDeque<f32>) -> Option<f32> {
    if impostors.len() < IMPOSTOR_MIN {
        return None;
    }
    let mut v: Vec<f32> = impostors.iter().copied().collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(v[v.len() / 2] * IMPOSTOR_RATIO)
}

impl VoiceListener {
    /// Starts the background listener if enabled and templates exist.
    pub fn start<F>(
        templates: VoiceTemplates,
        device_hint: String,
        sensitivity: f32,
        on_trigger: F,
    ) -> anyhow::Result<Self>
    where
        F: Fn() + Send + 'static,
    {
        let host = cpal::default_host();
        let device = select_input_device(&host, &device_hint)
            .ok_or_else(|| anyhow::anyhow!("No input audio device available"))?;

        let (tx, rx) = channel();
        let (stream, _dev_name, _sample_rate, _channels) = start_audio_stream(&device, tx)?;

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);

        let thread_handle = std::thread::spawn(move || {
            let mut vad = EnergyVad::new();
            let mut block_acc = Vec::with_capacity(BLOCK_SAMPLES);
            let mut last_trigger = Instant::now() - Duration::from_secs(10);
            // Distances of recent non-trigger speech; a trigger must be clearly closer than these.
            let mut impostors: std::collections::VecDeque<f32> = std::collections::VecDeque::new();

            while running_clone.load(Ordering::Relaxed) {
                let Ok(samples) = rx.recv_timeout(Duration::from_millis(100)) else {
                    continue;
                };

                for s in samples {
                    block_acc.push(s);
                    if block_acc.len() >= BLOCK_SAMPLES {
                        let (utterance, _rms) = vad.process_block(&block_acc);
                        block_acc.clear();

                        if let Some(utt) = utterance {
                            if last_trigger.elapsed() < Duration::from_secs(5) {
                                continue;
                            }

                            let mfcc = extract_mfcc(&utt);
                            let mut res = match_utterance(&templates, &mfcc, sensitivity);
                            if res.matched {
                                if let Some(limit) = impostor_limit(&impostors) {
                                    if res.distance >= limit {
                                        res.matched = false;
                                        res.is_near_miss = true;
                                    }
                                }
                            }
                            if !res.matched && res.distance.is_finite() {
                                impostors.push_back(res.distance);
                                if impostors.len() > IMPOSTOR_WINDOW {
                                    impostors.pop_front();
                                }
                            }
                            if res.matched {
                                last_trigger = Instant::now();
                                eprintln!(
                                    "Voice trigger matched (distance {:.3} / threshold {:.3})",
                                    res.distance, res.threshold
                                );
                                on_trigger();
                            } else if res.is_near_miss {
                                eprintln!(
                                    "Voice near miss (distance {:.3} / threshold {:.3})",
                                    res.distance, res.threshold
                                );
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            running,
            _stream: Some(stream),
            thread_handle: Some(thread_handle),
        })
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(h) = self.thread_handle.take() {
            let _ = h.join();
        }
        self._stream = None;
    }
}

impl Drop for VoiceListener {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Helper for GUI setup wizard and CLI test: captures mic level and individual takes.
#[derive(Clone)]
pub struct WizardCapture {
    running: Arc<AtomicBool>,
    level: Arc<AtomicU32>,
    noise_floor: Arc<AtomicU32>,
    vad_state: Arc<AtomicBool>,
    rx_utterances: Arc<std::sync::Mutex<Receiver<Vec<f32>>>>,
    _stream: Arc<std::sync::Mutex<Option<cpal::Stream>>>,
    device_name: String,
    sample_rate: u32,
    channels: usize,
}

impl WizardCapture {
    pub fn start(device_hint: &str) -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = select_input_device(&host, device_hint)
            .ok_or_else(|| anyhow::anyhow!("No input audio device available"))?;

        let (tx_samples, rx_samples) = channel();
        let (tx_utt, rx_utt) = channel();
        let (stream, device_name, sample_rate, channels) =
            start_audio_stream(&device, tx_samples)?;

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);
        let level = Arc::new(AtomicU32::new(0));
        let level_clone = Arc::clone(&level);
        let noise_floor = Arc::new(AtomicU32::new(dbfs_to_linear(-75.0).to_bits()));
        let noise_floor_clone = Arc::clone(&noise_floor);
        let vad_state = Arc::new(AtomicBool::new(false));
        let vad_state_clone = Arc::clone(&vad_state);

        std::thread::spawn(move || {
            let mut vad = EnergyVad::new();
            let mut block_acc = Vec::with_capacity(BLOCK_SAMPLES);

            while running_clone.load(Ordering::Relaxed) {
                let Ok(samples) = rx_samples.recv_timeout(Duration::from_millis(100)) else {
                    continue;
                };

                for s in samples {
                    block_acc.push(s);
                    if block_acc.len() >= BLOCK_SAMPLES {
                        let (utterance, rms) = vad.process_block(&block_acc);
                        block_acc.clear();

                        level_clone.store(rms.to_bits(), Ordering::Relaxed);
                        noise_floor_clone.store(vad.noise_floor().to_bits(), Ordering::Relaxed);
                        vad_state_clone.store(vad.in_utterance(), Ordering::Relaxed);

                        if let Some(utt) = utterance {
                            let _ = tx_utt.send(utt);
                        }
                    }
                }
            }
        });

        Ok(Self {
            running,
            level,
            noise_floor,
            vad_state,
            rx_utterances: Arc::new(std::sync::Mutex::new(rx_utt)),
            _stream: Arc::new(std::sync::Mutex::new(Some(stream))),
            device_name,
            sample_rate,
            channels,
        })
    }

    /// Current live mic RMS level (0.0 to 1.0).
    pub fn current_level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Current live mic RMS level in dBFS.
    pub fn current_level_dbfs(&self) -> f32 {
        linear_to_dbfs(self.current_level())
    }

    /// Current tracked noise floor (linear RMS amplitude).
    pub fn noise_floor_linear(&self) -> f32 {
        f32::from_bits(self.noise_floor.load(Ordering::Relaxed))
    }

    /// Tracked noise floor in dBFS.
    pub fn noise_floor_dbfs(&self) -> f32 {
        linear_to_dbfs(self.noise_floor_linear())
    }

    /// Current VAD state (true when speech utterance is detected).
    pub fn in_utterance(&self) -> bool {
        self.vad_state.load(Ordering::Relaxed)
    }

    /// Name of active input device.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Sample rate of active input stream.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Channel count of active input stream.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Try to receive a completed utterance from the VAD.
    pub fn try_recv_utterance(&self) -> Option<Vec<f32>> {
        self.rx_utterances.lock().ok()?.try_recv().ok()
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Ok(mut s) = self._stream.lock() {
            *s = None;
        }
    }
}

impl Drop for WizardCapture {
    fn drop(&mut self) {
        self.stop();
    }
}
