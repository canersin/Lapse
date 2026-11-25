//! Joins several clips into one video (a quick montage).
//!
//! Clips recorded with the same settings are joined losslessly and instantly with the
//! concat demuxer. Mixed resolutions/codecs are re-encoded to the first clip's size.

use std::path::{Path, PathBuf};

use crate::export::{h264_encoder, quality_to_bitrate_kbps, run_job, video_encode_args_for, ExportJob, H264Encoder};

/// `Merged_2026-11-15_23-10-04.mp4` in the folder of the first clip.
pub fn default_output(first: &Path) -> PathBuf {
    let time = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    first.parent().unwrap_or_else(|| Path::new(".")).join(format!("Merged_{}.mp4", time))
}

/// Everything that has to match for a lossless join.
#[derive(Debug, Clone, PartialEq, Default)]
struct StreamInfo {
    video: String,
    width: u32,
    height: u32,
    fps: String,
    audio: Vec<String>,
}

fn probe(path: &Path) -> StreamInfo {
    let out = crate::utils::ffprobe_cmd()
        .args(["-v", "error", "-show_entries", "stream=codec_type,codec_name,width,height,r_frame_rate", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let mut info = StreamInfo::default();
    for line in out.lines() {
        // Field order follows the stream: codec_name,codec_type,width,height,r_frame_rate
        let f: Vec<&str> = line.split(',').collect();
        match f.get(1).copied() {
            Some("video") if info.video.is_empty() => {
                info.video = f[0].to_string();
                info.width = f.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
                info.height = f.get(3).and_then(|v| v.parse().ok()).unwrap_or(0);
                info.fps = f.get(4).unwrap_or(&"").to_string();
            }
            Some("audio") => info.audio.push(f[0].to_string()),
            _ => {}
        }
    }
    info
}

fn fps_value(rate: &str) -> f64 {
    match rate.split_once('/') {
        Some((n, d)) => n.parse::<f64>().unwrap_or(60.0) / d.parse::<f64>().unwrap_or(1.0).max(1.0),
        None => rate.parse().unwrap_or(60.0),
    }
}

const PROGRESS: [&str; 8] = ["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-progress", "pipe:1", "-nostats"];

/// Lossless join; needs a list file for ffmpeg's concat demuxer.
fn copy_args(inputs: &[PathBuf], output: &Path) -> Vec<String> {
    let list = std::env::temp_dir().join(format!("lapse-merge-{}.txt", std::process::id()));
    let body: String = inputs
        .iter()
        .map(|p| format!("file '{}'\n", p.to_string_lossy().replace('\'', "'\\''")))
        .collect();
    let _ = std::fs::write(&list, body);
    let mut args: Vec<String> = PROGRESS.map(String::from).to_vec();
    args.extend(["-f", "concat", "-safe", "0", "-i"].map(String::from));
    args.push(list.to_string_lossy().to_string());
    args.extend(["-map", "0", "-c", "copy", "-movflags", "+faststart"].map(String::from));
    args.push(output.to_string_lossy().to_string());
    args
}

/// Re-encoding join: every clip is scaled/padded to the first one's size and frame rate,
/// and its audio tracks are mixed to one stereo track (silence for clips without audio).
fn reencode_args(inputs: &[(PathBuf, f64)], infos: &[StreamInfo], output: &Path) -> Vec<String> {
    reencode_args_for(h264_encoder(), inputs, infos, output)
}

fn reencode_args_for(encoder: H264Encoder, inputs: &[(PathBuf, f64)], infos: &[StreamInfo], output: &Path) -> Vec<String> {
    let (w, h) = (infos[0].width.max(2) / 2 * 2, infos[0].height.max(2) / 2 * 2);
    let fps = fps_value(&infos[0].fps).round().clamp(1.0, 240.0);
    let mut args: Vec<String> = PROGRESS.map(String::from).to_vec();
    let vaapi = !cfg!(windows) && encoder == H264Encoder::Vaapi && std::path::Path::new("/dev/dri/renderD128").exists();
    if vaapi {
        args.extend(["-vaapi_device".into(), "/dev/dri/renderD128".into()]);
    }
    for (path, _) in inputs {
        args.push("-i".into());
        args.push(path.to_string_lossy().to_string());
    }

    let mut graph = Vec::new();
    let mut pairs = String::new();
    for (i, ((_, secs), info)) in inputs.iter().zip(infos).enumerate() {
        graph.push(format!(
            "[{i}:v:0]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps}[v{i}]"
        ));
        let audio = match info.audio.len() {
            0 => format!("anullsrc=r=48000:cl=stereo,atrim=0:{:.3}", secs.max(0.1)),
            1 => format!("[{i}:a:0]anull"),
            n => format!("{}amix=inputs={n}:normalize=0", (0..n).map(|t| format!("[{i}:a:{t}]")).collect::<String>()),
        };
        graph.push(format!("{audio},aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}]"));
        pairs.push_str(&format!("[v{i}][a{i}]"));
    }
    let mut filters = Vec::new();
    let mut encode = Vec::new();
    let bitrate = if encoder == H264Encoder::MediaFoundation {
        Some(quality_to_bitrate_kbps(20, w, h, fps))
    } else {
        None
    };
    video_encode_args_for(encoder, &mut encode, &mut filters, Some(20), bitrate);
    let tail = if filters.is_empty() { "null".to_string() } else { filters.join(",") };
    graph.push(format!("{pairs}concat=n={}:v=1:a=1[vcat][aout]", inputs.len()));
    graph.push(format!("[vcat]{tail}[vout]"));

    args.extend(["-filter_complex".into(), graph.join(";"), "-map".into(), "[vout]".into(), "-map".into(), "[aout]".into()]);
    args.extend(encode);
    args.extend(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"].map(String::from));
    args.push(output.to_string_lossy().to_string());
    args
}

/// Joins `inputs` (path, duration in seconds) in order into `output`.
pub fn start(inputs: Vec<(PathBuf, f64)>, output: PathBuf) -> ExportJob {
    let total: f64 = inputs.iter().map(|(_, d)| d.max(0.0)).sum();
    run_job(total.max(0.1), output, move |tmp| {
        let infos: Vec<StreamInfo> = inputs.iter().map(|(p, _)| probe(p)).collect();
        let paths: Vec<PathBuf> = inputs.iter().map(|(p, _)| p.clone()).collect();
        if infos.windows(2).all(|w| w[0] == w[1]) && !infos[0].video.is_empty() {
            // If the copy join still fails, the second attempt re-encodes.
            vec![copy_args(&paths, tmp), reencode_args(&inputs, &infos, tmp)]
        } else {
            vec![reencode_args(&inputs, &infos, tmp)]
        }
    }, |_| true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::video_encode_args;

    #[test]
    fn fps_parsing() {
        assert_eq!(fps_value("60/1"), 60.0);
        assert!((fps_value("30000/1001") - 29.97).abs() < 0.01);
    }

    #[test]
    fn merge_reencode_args_media_foundation() {
        let inputs = vec![(PathBuf::from("/v/1.mp4"), 5.0), (PathBuf::from("/v/2.mp4"), 5.0)];
        let infos = vec![
            StreamInfo { video: "h264".into(), width: 1920, height: 1080, fps: "60/1".into(), audio: vec!["aac".into()] },
            StreamInfo { video: "h264".into(), width: 1280, height: 720, fps: "30/1".into(), audio: vec!["aac".into()] },
        ];
        let args = reencode_args_for(H264Encoder::MediaFoundation, &inputs, &infos, Path::new("/v/out.mp4"));
        let joined = args.join(" ");
        assert!(joined.contains("h264_mf"), "{}", joined);
        assert!(joined.contains("nv12"), "{}", joined);
        assert!(joined.contains("-b:v "), "{}", joined);
        assert!(!joined.contains("libx264"), "{}", joined);
        assert!(!joined.contains("crf"), "{}", joined);
    }

    /// Joins real clips (same settings, then mixed sizes); `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn merges_real_clips() {
        let dir = std::env::temp_dir().join(format!("lapse-merge-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let make = |name: &str, size: &str, tracks: usize| {
            let path = dir.join(name);
            let mut cmd = crate::utils::ffmpeg_cmd();
            cmd.args(["-v", "error", "-y", "-f", "lavfi", "-i", &format!("testsrc2=size={}:rate=30:duration=3", size)]);
            for t in 0..tracks {
                cmd.args(["-f", "lavfi", "-i", &format!("sine=f={}:d=3", 440 + t * 220)]);
            }
            cmd.args(["-map", "0"]);
            for t in 0..tracks {
                cmd.args(["-map", &(t + 1).to_string()]);
            }
            let mut enc_args = Vec::new();
            let mut enc_filters = Vec::new();
            video_encode_args(&mut enc_args, &mut enc_filters, Some(23), None);
            if !enc_filters.is_empty() {
                cmd.args(["-vf", &enc_filters.join(",")]);
            }
            cmd.args(&enc_args);
            cmd.args(["-c:a", "aac"]).arg(&path);
            assert!(cmd.status().unwrap().success());
            (path, 3.0)
        };
        let a = make("a.mp4", "640x360", 2);
        let b = make("b.mp4", "640x360", 2);
        let c = make("c.mp4", "1280x720", 0);
        for (name, inputs) in [("same.mp4", vec![a.clone(), b.clone()]), ("mixed.mp4", vec![a, b, c])] {
            let out = dir.join(name);
            let job = start(inputs.clone(), out.clone());
            let state = loop {
                let s = job.state();
                if !matches!(s, crate::export::ExportState::Running { .. }) {
                    break s;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            };
            assert!(matches!(state, crate::export::ExportState::Done { .. }), "{}: {:?}", name, state);
            let d = crate::clip_index::probe_duration(&out);
            assert!((d - 3.0 * inputs.len() as f64).abs() < 0.6, "{} duration {}", name, d);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
