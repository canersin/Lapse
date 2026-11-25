//! In-process microphone noise suppression using RNNoise.
//!
//! Creates a virtual microphone source via PulseAudio/PipeWire `module-pipe-source`,
//! running audio through `nnnoiseless` at 48kHz mono.

pub const FRAME_SIZE: usize = 480;

/// Pure DSP soft limiter: prevents clipping by saturating smoothly above 0.75 towards ceiling 0.95.
pub fn soft_limit(x: f32) -> f32 {
    const THRESHOLD: f32 = 0.75;
    const CEILING: f32 = 0.95;
    let abs_x = x.abs();
    if abs_x <= THRESHOLD {
        x
    } else {
        let diff = abs_x - THRESHOLD;
        let margin = CEILING - THRESHOLD;
        let y = (THRESHOLD + margin * (diff / margin).tanh()).min(CEILING);
        x.signum() * y
    }
}

/// Pure DSP helper: denoises one 480-sample frame of float32 mono audio at 48kHz.
pub fn process_frame_dsp(state: &mut nnnoiseless::DenoiseState, input: &[f32], output: &mut [f32]) {
    assert_eq!(input.len(), FRAME_SIZE);
    assert_eq!(output.len(), FRAME_SIZE);
    let mut in_scaled = [0.0f32; FRAME_SIZE];
    for (dst, &src) in in_scaled.iter_mut().zip(input.iter()) {
        *dst = src * 32768.0;
    }
    let mut out_scaled = [0.0f32; FRAME_SIZE];
    state.process_frame(&mut out_scaled, &in_scaled);
    for (dst, &src) in output.iter_mut().zip(out_scaled.iter()) {
        *dst = src / 32768.0;
    }
}

/// Resolves the device name to pass to `parec -d <dev>`.
///
/// When the configured source is "default_input" (or empty) and the system default
/// is "lapse_denoise", returns the remembered previous default to prevent a feedback loop.
#[allow(dead_code)]
pub fn resolve_parec_source<'a>(
    source: &'a str,
    current_default: Option<&'a str>,
    previous_default: Option<&'a str>,
) -> Option<&'a str> {
    let is_default = source.is_empty() || source == "default_input";
    if is_default {
        if current_default == Some("lapse_denoise") {
            previous_default.filter(|p| !p.is_empty() && *p != "lapse_denoise")
        } else {
            None
        }
    } else if source == "lapse_denoise" {
        previous_default.filter(|p| !p.is_empty() && *p != "lapse_denoise")
    } else {
        Some(source)
    }
}

#[cfg(not(target_os = "linux"))]
pub struct Denoiser;

#[cfg(not(target_os = "linux"))]
impl Denoiser {
    pub fn start(_source: &str, _noise_as_default: bool, _denoise_enabled: bool, _mic_gain: f32) -> anyhow::Result<Self> {
        anyhow::bail!("Noise suppression is only supported on Linux")
    }

    pub fn stop(self) {}

    pub fn is_alive(&mut self) -> bool {
        false
    }

    pub fn set_gain(&self, _mic_gain: f32) {}
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;

    /// Path to the FIFO used by the virtual microphone pipe source.
    pub(crate) fn fifo_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !dir.is_empty() && std::path::Path::new(&dir).is_dir() {
            return PathBuf::from(dir).join("lapse-denoise.fifo");
        }
    }
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/lapse-denoise-{}.fifo", uid))
}

/// Unloads any leftover modules created by previous runs.
fn unload_leftover_modules() {
    let output = match Command::new("pactl")
        .args(["list", "short", "modules"])
        .output()
    {
        Ok(out) if out.status.success() => out,
        _ => return,
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut to_unload = Vec::new();
    for line in stdout.lines() {
        if line.contains("lapse_denoise") {
            if let Some(id_str) = line.split_whitespace().next() {
                if let Ok(id) = id_str.parse::<u32>() {
                    if line.contains("module-remap-source") {
                        to_unload.insert(0, id);
                    } else {
                        to_unload.push(id);
                    }
                }
            }
        }
    }
    for id in to_unload {
        let _ = Command::new("pactl")
            .args(["unload-module", &id.to_string()])
            .output();
    }
}

/// Loads a PulseAudio module via `pactl load-module` and returns the module ID.
fn load_module(args: &[&str]) -> anyhow::Result<u32> {
    let output = Command::new("pactl")
        .arg("load-module")
        .args(args)
        .output()?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("pactl load-module {:?} failed: {}", args, err.trim());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let id_str = stdout.trim();
    let id = id_str
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("pactl load-module returned invalid id: '{}'", id_str))?;
    Ok(id)
}

/// Reads the current system default source name via `pactl get-default-source`.
fn get_default_source() -> Option<String> {
    let output = Command::new("pactl")
        .arg("get-default-source")
        .output()
        .ok()?;
    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// The configured microphone if it is a concrete device, else the first real input source.
fn first_real_source(configured: &str) -> Option<String> {
    if !configured.trim().is_empty() && configured != "default_input" && configured != "lapse_denoise" {
        return Some(configured.to_string());
    }
    let output = Command::new("pactl").args(["list", "short", "sources"]).output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|l| l.split('\t').nth(1))
        .find(|n| *n != "lapse_denoise" && !n.ends_with(".monitor"))
        .map(str::to_string)
}

/// Sets the system default source via `pactl set-default-source`.
fn set_default_source(source: &str) {
    let _ = Command::new("pactl")
        .args(["set-default-source", source])
        .output();
}

/// Live noise suppression pipeline.
pub struct Denoiser {
    source_module_id: Option<u32>,
    parec_child: Option<Child>,
    worker_thread: Option<JoinHandle<()>>,
    fifo_path: PathBuf,
    previous_default: Option<String>,
    /// Microphone gain as `f32` bits, read by the worker for every frame.
    gain: Arc<AtomicU32>,
}

impl Denoiser {
    /// Starts the noise suppression and/or microphone gain pipeline for `source`.
    pub fn start(
        source: &str,
        noise_as_default: bool,
        denoise_enabled: bool,
        mic_gain: f32,
    ) -> anyhow::Result<Self> {
        for bin in ["pactl", "parec"] {
            if crate::utils::find_in_path(bin).is_none() {
                anyhow::bail!("PulseAudio tool '{}' not found in PATH", bin);
            }
        }

        unload_leftover_modules();

        // A leftover default of our own virtual mic would make us restore it; use a real mic instead.
        let previous_default = get_default_source()
            .filter(|p| p != "lapse_denoise")
            .or_else(|| first_real_source(source));

        let fifo = fifo_path();
        let _ = std::fs::remove_file(&fifo);

        let file_arg = format!("file={}", fifo.display());
        let source_id = match load_module(&[
            "module-pipe-source",
            "source_name=lapse_denoise",
            &file_arg,
            "format=float32le",
            "rate=48000",
            "channels=1",
            "source_properties=device.description=Virtual-Microphone",
        ]) {
            Ok(id) => id,
            Err(e) => {
                let _ = std::fs::remove_file(&fifo);
                return Err(e);
            }
        };

        if noise_as_default {
            set_default_source("lapse_denoise");
        } else if let Some(ref prev) = previous_default {
            if !prev.is_empty() && prev != "lapse_denoise" {
                set_default_source(prev);
                let prev_clone = prev.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    set_default_source(&prev_clone);
                });
            }
        }

        let current_default = get_default_source();
        let resolved_source = resolve_parec_source(
            source,
            current_default.as_deref(),
            previous_default.as_deref(),
        );

        let mut parec_cmd = Command::new("parec");
        parec_cmd.args([
            "--raw",
            "--format=float32le",
            "--rate=48000",
            "--channels=1",
            "--latency-msec=10",
        ]);
        if let Some(src) = resolved_source {
            parec_cmd.arg("-d").arg(src);
        }
        parec_cmd.stdout(Stdio::piped());
        parec_cmd.stderr(Stdio::null());

        let mut parec_child = match parec_cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                if get_default_source().as_deref() == Some("lapse_denoise") {
                    if let Some(ref prev) = previous_default {
                        if !prev.is_empty() && prev != "lapse_denoise" {
                            set_default_source(prev);
                        }
                    }
                }
                let _ = Command::new("pactl").args(["unload-module", &source_id.to_string()]).output();
                let _ = std::fs::remove_file(&fifo);
                return Err(e.into());
            }
        };

        let mut reader = match parec_child.stdout.take() {
            Some(r) => r,
            None => {
                let _ = parec_child.kill();
                let _ = parec_child.wait();
                if get_default_source().as_deref() == Some("lapse_denoise") {
                    if let Some(ref prev) = previous_default {
                        if !prev.is_empty() && prev != "lapse_denoise" {
                            set_default_source(prev);
                        }
                    }
                }
                let _ = Command::new("pactl").args(["unload-module", &source_id.to_string()]).output();
                let _ = std::fs::remove_file(&fifo);
                anyhow::bail!("Failed to capture parec stdout");
            }
        };

        let fifo_clone = fifo.clone();
        let gain = Arc::new(AtomicU32::new(mic_gain.to_bits()));
        let worker_gain = Arc::clone(&gain);
        let worker_thread = std::thread::spawn(move || {
            use std::fs::OpenOptions;
            use std::io::{Read, Write};

            let mut writer = match OpenOptions::new().write(true).open(&fifo_clone) {
                Ok(w) => w,
                Err(_) => return,
            };

            let mut denoise_state = nnnoiseless::DenoiseState::new();
            let mut byte_buf = [0u8; FRAME_SIZE * 4];
            let mut in_frame = [0.0f32; FRAME_SIZE];
            let mut out_frame = [0.0f32; FRAME_SIZE];
            let mut out_bytes = [0u8; FRAME_SIZE * 4];
            let mut first_frame = true;

            loop {
                if reader.read_exact(&mut byte_buf).is_err() {
                    break;
                }
                for (i, chunk) in byte_buf.chunks_exact(4).enumerate() {
                    in_frame[i] = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                }

                if denoise_enabled {
                    process_frame_dsp(&mut denoise_state, &in_frame, &mut out_frame);
                } else {
                    out_frame.copy_from_slice(&in_frame);
                }

                let mic_gain = f32::from_bits(worker_gain.load(Ordering::Relaxed));
                if (mic_gain - 1.0).abs() >= 0.01 {
                    for s in out_frame.iter_mut() {
                        *s = soft_limit(*s * mic_gain);
                    }
                }

                if first_frame {
                    first_frame = false;
                    continue;
                }

                for (i, &sample) in out_frame.iter().enumerate() {
                    let bytes = sample.to_le_bytes();
                    out_bytes[i * 4..i * 4 + 4].copy_from_slice(&bytes);
                }

                if writer.write_all(&out_bytes).is_err() {
                    break;
                }
                let _ = writer.flush();
            }
        });

        Ok(Self {
            source_module_id: Some(source_id),
            parec_child: Some(parec_child),
            worker_thread: Some(worker_thread),
            fifo_path: fifo,
            previous_default,
            gain,
        })
    }

    /// Changes the microphone gain without restarting the pipeline (and the recorder with it).
    pub fn set_gain(&self, mic_gain: f32) {
        self.gain.store(mic_gain.to_bits(), Ordering::Relaxed);
    }

    /// Stops noise suppression, terminating worker processes and unloading modules.
    /// False once parec has exited, e.g. after the audio server restarted.
    pub fn is_alive(&mut self) -> bool {
        self.parec_child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    pub fn stop(mut self) {
        self.cleanup();
    }

    fn cleanup(&mut self) {
        if let Some(mut child) = self.parec_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if get_default_source().as_deref() == Some("lapse_denoise") {
            if let Some(ref prev) = self.previous_default {
                if !prev.is_empty() && prev != "lapse_denoise" {
                    set_default_source(prev);
                }
            }
        }
        if let Some(id) = self.source_module_id.take() {
            let _ = Command::new("pactl")
                .args(["unload-module", &id.to_string()])
                .output();
        }
        if let Some(handle) = self.worker_thread.take() {
            let _ = handle.join();
        }
        let _ = std::fs::remove_file(&self.fifo_path);
    }
}

impl Drop for Denoiser {
    fn drop(&mut self) {
        self.cleanup();
    }
}
}
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_frame_dsp_silence() {
        let mut state = nnnoiseless::DenoiseState::new();
        let input = [0.0f32; FRAME_SIZE];
        let mut output = [1.0f32; FRAME_SIZE];

        process_frame_dsp(&mut state, &input, &mut output);

        assert_eq!(output.len(), input.len());
        for &sample in &output {
            assert!(sample.abs() < 1e-3, "Expected near-silence, got {}", sample);
        }
    }

    #[test]
    fn test_process_frame_dsp_multiple_frames() {
        let mut state = nnnoiseless::DenoiseState::new();
        let input = [0.0f32; FRAME_SIZE];
        let mut output = [0.0f32; FRAME_SIZE];

        for _ in 0..5 {
            process_frame_dsp(&mut state, &input, &mut output);
            assert_eq!(output.len(), input.len());
            for &sample in &output {
                assert!(sample.abs() < 1e-3, "Expected near-silence, got {}", sample);
            }
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_fifo_path() {
        let path = fifo_path();
        assert!(path.to_string_lossy().contains("lapse-denoise"));
        assert!(path.to_string_lossy().ends_with(".fifo"));
    }

    #[test]
    fn test_resolve_parec_source_default_input_when_default_is_denoise() {
        let res = resolve_parec_source(
            "default_input",
            Some("lapse_denoise"),
            Some("alsa_input.pci-mic"),
        );
        assert_eq!(res, Some("alsa_input.pci-mic"));

        let res_empty = resolve_parec_source(
            "",
            Some("lapse_denoise"),
            Some("alsa_input.pci-mic"),
        );
        assert_eq!(res_empty, Some("alsa_input.pci-mic"));
    }

    #[test]
    fn test_resolve_parec_source_default_input_when_default_is_not_denoise() {
        let res = resolve_parec_source(
            "default_input",
            Some("alsa_input.pci-mic"),
            Some("alsa_input.pci-mic"),
        );
        assert_eq!(res, None);
    }

    #[test]
    fn test_resolve_parec_source_explicit_source() {
        let res = resolve_parec_source(
            "alsa_input.usb-mic",
            Some("lapse_denoise"),
            Some("alsa_input.pci-mic"),
        );
        assert_eq!(res, Some("alsa_input.usb-mic"));
    }

    #[test]
    fn test_resolve_parec_source_feedback_prevention() {
        let res = resolve_parec_source(
            "lapse_denoise",
            Some("lapse_denoise"),
            Some("alsa_input.pci-mic"),
        );
        assert_eq!(res, Some("alsa_input.pci-mic"));

        let res_loop = resolve_parec_source(
            "lapse_denoise",
            Some("lapse_denoise"),
            Some("lapse_denoise"),
        );
        assert_eq!(res_loop, None);
    }

    #[test]
    fn test_soft_limit_behavior() {
        // Below threshold, linear response
        assert_eq!(soft_limit(0.0), 0.0);
        assert_eq!(soft_limit(0.5), 0.5);
        assert_eq!(soft_limit(-0.5), -0.5);
        assert_eq!(soft_limit(0.75), 0.75);

        // Above threshold, smooth compression towards 0.95
        let sat_1 = soft_limit(1.0);
        assert!(sat_1 > 0.75 && sat_1 < 0.95);

        let sat_3 = soft_limit(3.0);
        assert!(sat_3 > sat_1 && sat_3 <= 0.95);

        let sat_10 = soft_limit(10.0);
        assert!(sat_10 >= sat_3 && sat_10 <= 0.95);

        // Symmetry
        assert_eq!(soft_limit(-1.0), -sat_1);
        assert_eq!(soft_limit(-3.0), -sat_3);
    }
}
