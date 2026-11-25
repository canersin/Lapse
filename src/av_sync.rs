//! Measures how far a clip's audio runs ahead of (or behind) its picture.
//!
//! Loud sounds in games (shots, explosions, hits) usually come with a sudden change on
//! screen. We compare when the sound gets louder with when the picture changes and look
//! for the shift that lines the two up best.

use std::io::Read;
use std::path::Path;
use std::process::Stdio;

/// Analysis grid: one value every 5 ms.
const STEP_MS: i32 = 5;
const MAX_LAG_MS: i32 = 300;
const ANALYZE_SECS: u32 = 90;

pub struct Measurement {
    /// How much later the audio should play, in ms (positive: audio was early).
    pub offset_ms: i32,
    /// Peak strength over the background; below ~2 the result is unreliable.
    pub confidence: f32,
}

fn ffmpeg_raw(args: &[&str], path: &Path) -> anyhow::Result<Vec<u8>> {
    let mut child = crate::utils::ffmpeg_cmd()
        .args(["-loglevel", "error", "-t", &ANALYZE_SECS.to_string(), "-i"])
        .arg(path)
        .args(args)
        .arg("-")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut out = Vec::new();
    child.stdout.take().map(|mut s| s.read_to_end(&mut out));
    child.wait()?;
    Ok(out)
}

/// Rises in loudness per 5 ms step.
fn audio_onsets(path: &Path) -> anyhow::Result<Vec<f32>> {
    const RATE: usize = 8000;
    let raw = ffmpeg_raw(&["-map", "0:a:0", "-ac", "1", "-ar", "8000", "-f", "s16le"], path)?;
    let samples: Vec<f32> = raw.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    let win = RATE * STEP_MS as usize / 1000;
    let energy: Vec<f32> = samples
        .chunks(win)
        .map(|c| (c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32 + 1e-9).ln())
        .collect();
    Ok(rises(&energy))
}

/// Picture change per 5 ms step (frame differences held over each frame's duration).
fn video_onsets(path: &Path, fps: f64, steps: usize) -> anyhow::Result<Vec<f32>> {
    const W: usize = 64;
    const H: usize = 36;
    let raw = ffmpeg_raw(&["-map", "0:v:0", "-vf", "scale=64:36,format=gray", "-f", "rawvideo"], path)?;
    let frames: Vec<&[u8]> = raw.chunks_exact(W * H).collect();
    let mut diff = vec![0.0f32; frames.len()];
    for i in 1..frames.len() {
        let d: u32 = frames[i].iter().zip(frames[i - 1]).map(|(a, b)| a.abs_diff(*b) as u32).sum();
        diff[i] = d as f32 / (W * H) as f32;
    }
    let per_step: Vec<f32> = (0..steps)
        .map(|s| {
            let t = s as f64 * STEP_MS as f64 / 1000.0;
            diff.get((t * fps) as usize).copied().unwrap_or(0.0)
        })
        .collect();
    // Only the first step of each frame counts as a change.
    let mut out = vec![0.0f32; steps];
    let mut last_frame = usize::MAX;
    for (s, v) in per_step.iter().enumerate() {
        let f = (s as f64 * STEP_MS as f64 / 1000.0 * fps) as usize;
        if f != last_frame {
            out[s] = *v;
            last_frame = f;
        }
    }
    Ok(rises(&smooth(&out, 3)))
}

fn smooth(v: &[f32], radius: usize) -> Vec<f32> {
    (0..v.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(radius), (i + radius + 1).min(v.len()));
            v[a..b].iter().sum::<f32>() / (b - a) as f32
        })
        .collect()
}

/// Positive changes only, normalized to zero mean / unit variance.
fn rises(v: &[f32]) -> Vec<f32> {
    let mut d: Vec<f32> = (0..v.len()).map(|i| if i == 0 { 0.0 } else { (v[i] - v[i - 1]).max(0.0) }).collect();
    let n = d.len().max(1) as f32;
    let mean = d.iter().sum::<f32>() / n;
    let sd = (d.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n).sqrt().max(1e-6);
    d.iter_mut().for_each(|x| *x = (*x - mean) / sd);
    d
}

/// Best shift of `audio` against `video`, in steps (positive: audio comes early).
pub fn best_lag(audio: &[f32], video: &[f32], max_lag: i32) -> (i32, f32) {
    let n = audio.len().min(video.len()) as i32;
    let score = |lag: i32| -> f32 {
        let mut sum = 0.0;
        for i in 0..n {
            let j = i - lag;
            if j >= 0 && j < n {
                sum += video[i as usize] * audio[j as usize];
            }
        }
        sum
    };
    let scores: Vec<(i32, f32)> = (-max_lag..=max_lag).map(|l| (l, score(l))).collect();
    let (lag, peak) = scores.iter().copied().fold((0, f32::MIN), |b, s| if s.1 > b.1 { s } else { b });
    let others: Vec<f32> = scores.iter().filter(|(l, _)| (l - lag).abs() > 8).map(|(_, s)| *s).collect();
    let mean = others.iter().sum::<f32>() / others.len().max(1) as f32;
    let sd = (others.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / others.len().max(1) as f32).sqrt().max(1e-6);
    (lag, (peak - mean) / sd)
}

pub fn measure(path: &Path) -> anyhow::Result<Measurement> {
    let fps = probe_fps(path).unwrap_or(60.0);
    let audio = audio_onsets(path)?;
    if audio.len() < 400 {
        anyhow::bail!("The clip has no usable sound");
    }
    let video = video_onsets(path, fps, audio.len())?;
    let (lag, confidence) = best_lag(&audio, &video, MAX_LAG_MS / STEP_MS);
    Ok(Measurement { offset_ms: lag * STEP_MS, confidence })
}

fn probe_fps(path: &Path) -> Option<f64> {
    let out = crate::utils::ffprobe_cmd()
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=avg_frame_rate", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (a, b) = text.trim().split_once('/')?;
    let (a, b): (f64, f64) = (a.parse().ok()?, b.parse().ok()?);
    (b > 0.0 && a > 0.0).then(|| a / b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_known_shift() {
        // Picture events every so often; the same events in the audio 12 steps earlier.
        let mut video = vec![0.0f32; 2000];
        for (k, i) in [100usize, 260, 470, 800, 1150, 1500, 1800].iter().enumerate() {
            video[*i] = 1.0 + k as f32 * 0.1;
        }
        let audio: Vec<f32> = (0..2000).map(|i| video.get(i + 12).copied().unwrap_or(0.0)).collect();
        let (lag, conf) = best_lag(&audio, &video, 60);
        assert_eq!(lag, 12);
        assert!(conf > 3.0);
    }
}

#[cfg(test)]
mod real_clip {
    /// LAPSE_SYNC_CLIP=/path/clip.mp4 cargo test --release real_clip -- --ignored --nocapture
    #[test]
    #[ignore]
    fn measure_real_clip() {
        let path = std::env::var("LAPSE_SYNC_CLIP").expect("set LAPSE_SYNC_CLIP");
        let m = super::measure(std::path::Path::new(&path)).unwrap();
        println!("offset {} ms, confidence {:.1}", m.offset_ms, m.confidence);
    }
}

