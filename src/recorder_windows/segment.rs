#![cfg_attr(not(windows), allow(dead_code))]

use std::path::PathBuf;

/// A recorded buffer segment file with its duration in seconds and its start QPC in 100ns units.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentInfo {
    pub path: PathBuf,
    pub duration_secs: f64,
    pub start_qpc: i64,
}

/// Selects the newest segments from `segments` that together cover at least
/// `requested_secs`. The input `segments` are assumed to be in chronological order.
/// Returns the selected segments in chronological order.
pub fn select_segments_for_duration(
    segments: &[SegmentInfo],
    requested_secs: f64,
) -> Vec<SegmentInfo> {
    if segments.is_empty() || requested_secs <= 0.0 {
        return Vec::new();
    }

    let mut accumulated = 0.0;
    let mut selected = Vec::new();

    for seg in segments.iter().rev() {
        selected.push(seg.clone());
        accumulated += seg.duration_secs;
        if accumulated >= requested_secs {
            break;
        }
    }

    selected.reverse();
    selected
}

/// Calculates the maximum number of segments to keep in the buffer directory:
/// ceil(replay_seconds / 10) + 3.
pub fn max_buffer_segments(replay_seconds: u32) -> usize {
    let base = (replay_seconds as f64 / 10.0).ceil() as usize;
    base + 3
}

/// Deletes older segment files from memory and disk and removes them from the list,
/// keeping only the newest `max_segments`.
pub fn prune_old_segments(segments: &mut Vec<SegmentInfo>, max_segments: usize) {
    if segments.len() > max_segments {
        let to_remove = segments.len() - max_segments;
        for old in segments.drain(..to_remove) {
            let _ = crate::recorder_windows::mem_store::remove(&old.path);
            let _ = std::fs::remove_file(&old.path);
            let _ = crate::recorder_windows::mem_store::remove(old.path.with_extension("m4a"));
            let _ = std::fs::remove_file(old.path.with_extension("m4a"));
            let _ = crate::recorder_windows::mem_store::remove(old.path.with_extension("chat.m4a"));
            let _ = std::fs::remove_file(old.path.with_extension("chat.m4a"));
            let _ = crate::recorder_windows::mem_store::remove(old.path.with_extension("mic.m4a"));
            let _ = std::fs::remove_file(old.path.with_extension("mic.m4a"));
            let tracks_clip = crate::tracks::path_for_clip(&old.path);
            let _ = crate::recorder_windows::mem_store::remove(&tracks_clip);
            let _ = std::fs::remove_file(tracks_clip);
            let tracks_json = old.path.with_extension("tracks.json");
            let _ = crate::recorder_windows::mem_store::remove(&tracks_json);
            let _ = std::fs::remove_file(tracks_json);
            for i in 0..16 {
                let p = old.path.with_extension(format!("app-{}.m4a", i));
                let mem_removed = crate::recorder_windows::mem_store::remove(&p).is_some();
                let disk_exists = p.exists();
                if disk_exists {
                    let _ = std::fs::remove_file(&p);
                }
                if !disk_exists && !mem_removed {
                    break;
                }
            }
        }
    }
}

/// Decides whether the next replay buffer segment should be stored in RAM (`true`) or on disk (`false`).
///
/// Hysteresis:
/// - When currently using RAM, free memory must stay >= 15 % of total to remain in RAM.
/// - Once fallen back to disk, free memory must reach >= 30 % of total to switch back to RAM.
/// - If `low_memory_guard` is false, always uses RAM when `replay_storage == "ram"`.
/// - If `replay_storage != "ram"`, always uses disk.
pub fn decide_replay_storage(
    replay_storage: &str,
    low_memory_guard: bool,
    current_is_ram: bool,
    available_mb: f64,
    total_mb: f64,
) -> bool {
    if replay_storage != "ram" {
        return false;
    }
    if !low_memory_guard {
        return true;
    }
    if total_mb <= 0.0 {
        return current_is_ram;
    }
    let ratio = available_mb / total_mb;
    if current_is_ram {
        ratio >= 0.15
    } else {
        ratio >= 0.30
    }
}

/// Buffer directory in %LOCALAPPDATA%\lapse\buffer
pub fn get_buffer_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local_app_data).join("lapse").join("buffer");
        }
    }
    directories::ProjectDirs::from("com", "lapse", "lapse")
        .map(|d| d.data_local_dir().join("buffer"))
        .unwrap_or_else(|| std::env::temp_dir().join("lapse").join("buffer"))
}

/// Calculates the audio timestamp offset advance for a skipped segment based on video duration.
pub fn audio_offset_from_video_duration(
    video_duration: u64,
    video_timescale: u32,
    audio_timescale: u32,
) -> u64 {
    if video_timescale == 0 {
        return 0;
    }
    ((video_duration as u128 * audio_timescale as u128 + (video_timescale as u128 / 2))
        / video_timescale as u128) as u64
}

/// Tracks continuous timestamps across segments for video and audio tracks.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RemuxTimestampTracker {
    pub video_offset: u64,
    pub audio_offset: u64,
}

impl RemuxTimestampTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance video offset by the given sample duration (in video timescale units).
    pub fn advance_video(&mut self, duration: u64) {
        self.video_offset += duration;
    }

    /// Advance audio offset by the given sample duration (in audio timescale units).
    #[allow(dead_code)]
    pub fn advance_audio(&mut self, duration: u64) {
        self.audio_offset += duration;
    }

    /// When a segment's audio is missing or empty, advance the audio offset by the
    /// video duration of the skipped segment to keep timing continuous.
    #[allow(dead_code)]
    pub fn skip_audio_segment(
        &mut self,
        video_duration: u64,
        video_timescale: u32,
        audio_timescale: u32,
    ) {
        let offset =
            audio_offset_from_video_duration(video_duration, video_timescale, audio_timescale);
        self.audio_offset += offset;
    }
}

/// Calculates how many audio samples to keep so the total audio duration
/// matches `max_audio_duration` as closely as possible without trailing excess.
#[allow(dead_code)]
pub fn trim_audio_sample_count(sample_durations: &[u32], max_audio_duration: u64) -> usize {
    let mut accumulated = 0u64;
    let mut count = 0;
    for &dur in sample_durations {
        if accumulated + dur as u64 > max_audio_duration {
            if accumulated + (dur as u64 / 2) <= max_audio_duration {
                count += 1;
            }
            break;
        }
        accumulated += dur as u64;
        count += 1;
    }
    count
}

/// Selects the cut-point timestamp (in timescale units) from a slice of keyframe timestamps.
/// The output starts at the first video keyframe at or after (total - requested).
/// Returns 0 if total <= requested or if keyframes is empty.
pub fn select_cut_point(
    keyframes: &[u64],
    total_duration: u64,
    requested_duration: u64,
) -> u64 {
    if total_duration <= requested_duration || keyframes.is_empty() {
        return 0;
    }
    let target = total_duration - requested_duration;
    for &kf in keyframes {
        if kf >= target {
            return kf;
        }
    }
    *keyframes.last().unwrap_or(&0)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EncoderCacheEntry {
    pub target_width: u32,
    pub target_height: u32,
}

pub fn encoder_cache_key(monitor_w: u32, monitor_h: u32, encoder_name: &str) -> String {
    format!("{}x{}:{}", monitor_w, monitor_h, encoder_name)
}

/// Cache path in %LOCALAPPDATA%\lapse\encoder.json
pub fn get_encoder_cache_path() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local_app_data).join("lapse").join("encoder.json");
        }
    }
    directories::ProjectDirs::from("com", "lapse", "lapse")
        .map(|d| d.data_local_dir().join("encoder.json"))
        .unwrap_or_else(|| std::env::temp_dir().join("lapse").join("encoder.json"))
}

static SESSION_ENCODER_CACHE: std::sync::Mutex<Option<std::collections::HashMap<String, EncoderCacheEntry>>> =
    std::sync::Mutex::new(None);

pub fn get_cached_downscale(monitor_w: u32, monitor_h: u32, encoder_name: &str) -> Option<(u32, u32)> {
    let mut guard = SESSION_ENCODER_CACHE.lock().ok()?;
    if guard.is_none() {
        let path = get_encoder_cache_path();
        let map = if path.is_file() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            std::collections::HashMap::new()
        };
        *guard = Some(map);
    }

    let key = encoder_cache_key(monitor_w, monitor_h, encoder_name);
    guard
        .as_ref()
        .and_then(|map| map.get(&key))
        .map(|e| (e.target_width, e.target_height))
}

pub fn set_cached_downscale(
    monitor_w: u32,
    monitor_h: u32,
    encoder_name: &str,
    target_w: u32,
    target_h: u32,
) {
    let mut guard = match SESSION_ENCODER_CACHE.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if guard.is_none() {
        let path = get_encoder_cache_path();
        let map = if path.is_file() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            std::collections::HashMap::new()
        };
        *guard = Some(map);
    }

    let key = encoder_cache_key(monitor_w, monitor_h, encoder_name);
    if let Some(ref mut map) = *guard {
        map.insert(
            key,
            EncoderCacheEntry {
                target_width: target_w,
                target_height: target_h,
            },
        );
        let path = get_encoder_cache_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(map) {
            let _ = std::fs::write(&path, json);
        }
    }
}

/// Scaler choice for video encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalerChoice {
    None,
    D3D11,
    Cpu,
}

/// Decides which scaler to use given capture dimensions and encoder dimensions.
/// Same size uses no scaling; otherwise prefers D3D11 if available, falling back to CPU.
pub fn decide_scaler(
    frame_w: u32,
    frame_h: u32,
    encoder_w: u32,
    encoder_h: u32,
    d3d11_available: bool,
) -> ScalerChoice {
    if frame_w == encoder_w && frame_h == encoder_h {
        ScalerChoice::None
    } else if d3d11_available {
        ScalerChoice::D3D11
    } else {
        ScalerChoice::Cpu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_segments_empty() {
        let selected = select_segments_for_duration(&[], 30.0);
        assert!(selected.is_empty());
    }

    #[test]
    fn test_select_segments_zero_duration() {
        let segs = vec![SegmentInfo {
            path: PathBuf::from("seg_1.mp4"),
            duration_secs: 10.0,
            start_qpc: 0,
        }];
        let selected = select_segments_for_duration(&segs, 0.0);
        assert!(selected.is_empty());
    }

    #[test]
    fn test_select_segments_covers_requested_duration() {
        let segs = vec![
            SegmentInfo {
                path: PathBuf::from("seg_1.mp4"),
                duration_secs: 10.0,
                start_qpc: 0,
            },
            SegmentInfo {
                path: PathBuf::from("seg_2.mp4"),
                duration_secs: 10.0,
                start_qpc: 0,
            },
            SegmentInfo {
                path: PathBuf::from("seg_3.mp4"),
                duration_secs: 10.0,
                start_qpc: 0,
            },
            SegmentInfo {
                path: PathBuf::from("seg_4.mp4"),
                duration_secs: 4.5,
                start_qpc: 0,
            },
        ];

        // Request 4s -> only newest segment needed (4.5s >= 4.0s)
        let sel = select_segments_for_duration(&segs, 4.0);
        assert_eq!(sel.len(), 1);
        assert_eq!(sel[0].path, PathBuf::from("seg_4.mp4"));

        // Request 12s -> needs seg_4 (4.5s) + seg_3 (10.0s) = 14.5s >= 12.0s
        let sel = select_segments_for_duration(&segs, 12.0);
        assert_eq!(sel.len(), 2);
        assert_eq!(sel[0].path, PathBuf::from("seg_3.mp4"));
        assert_eq!(sel[1].path, PathBuf::from("seg_4.mp4"));

        // Request 25s -> needs seg_4 + seg_3 + seg_2 = 24.5s < 25s -> needs seg_1 as well (34.5s)
        let sel = select_segments_for_duration(&segs, 25.0);
        assert_eq!(sel.len(), 4);
        assert_eq!(sel[0].path, PathBuf::from("seg_1.mp4"));
        assert_eq!(sel[3].path, PathBuf::from("seg_4.mp4"));

        // Exact match (10.0 + 4.5 = 14.5)
        let sel = select_segments_for_duration(&segs, 14.5);
        assert_eq!(sel.len(), 2);
        assert_eq!(sel[0].path, PathBuf::from("seg_3.mp4"));
        assert_eq!(sel[1].path, PathBuf::from("seg_4.mp4"));

        // Request exceeding total -> returns all available
        let sel = select_segments_for_duration(&segs, 100.0);
        assert_eq!(sel.len(), 4);
    }

    #[test]
    fn test_max_buffer_segments_calculation() {
        assert_eq!(max_buffer_segments(30), 6);
        assert_eq!(max_buffer_segments(10), 4);
        assert_eq!(max_buffer_segments(15), 5);
        assert_eq!(max_buffer_segments(60), 9);
        assert_eq!(max_buffer_segments(0), 3);
    }

    #[test]
    fn test_prune_old_segments() {
        let mut segs = vec![
            SegmentInfo {
                path: PathBuf::from("seg_1.mp4"),
                duration_secs: 10.0,
                start_qpc: 0,
            },
            SegmentInfo {
                path: PathBuf::from("seg_2.mp4"),
                duration_secs: 10.0,
                start_qpc: 0,
            },
            SegmentInfo {
                path: PathBuf::from("seg_3.mp4"),
                duration_secs: 10.0,
                start_qpc: 0,
            },
        ];
        prune_old_segments(&mut segs, 2);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].path, PathBuf::from("seg_2.mp4"));
        assert_eq!(segs[1].path, PathBuf::from("seg_3.mp4"));
    }

    #[test]
    fn test_audio_offset_from_video_duration() {
        let audio_dur = audio_offset_from_video_duration(10_000, 1000, 48000);
        assert_eq!(audio_dur, 480_000);

        let audio_dur = audio_offset_from_video_duration(900_000, 90000, 48000);
        assert_eq!(audio_dur, 480_000);

        assert_eq!(audio_offset_from_video_duration(0, 1000, 48000), 0);
        assert_eq!(audio_offset_from_video_duration(1000, 0, 48000), 0);
    }

    #[test]
    fn test_remux_timestamp_tracker_continuous_audio_and_video() {
        let mut tracker = RemuxTimestampTracker::new();
        let video_timescale = 1000;
        let audio_timescale = 48000;

        // Segment 0: 10s video, 10s audio
        tracker.advance_video(10_000);
        tracker.advance_audio(480_000);
        assert_eq!(tracker.video_offset, 10_000);
        assert_eq!(tracker.audio_offset, 480_000);

        // Segment 1: 5s video, missing audio
        let seg1_video_dur = 5_000;
        tracker.advance_video(seg1_video_dur);
        tracker.skip_audio_segment(seg1_video_dur, video_timescale, audio_timescale);
        assert_eq!(tracker.video_offset, 15_000);
        assert_eq!(tracker.audio_offset, 720_000);

        // Segment 2: 10s video, 10s audio
        tracker.advance_video(10_000);
        tracker.advance_audio(480_000);
        assert_eq!(tracker.video_offset, 25_000);
        assert_eq!(tracker.audio_offset, 1_200_000);

        assert_eq!(tracker.video_offset as f64 / video_timescale as f64, 25.0);
        assert_eq!(tracker.audio_offset as f64 / audio_timescale as f64, 25.0);
    }

    #[test]
    fn test_remux_timestamp_tracker_first_segment_audio_missing() {
        let mut tracker = RemuxTimestampTracker::new();
        let video_timescale = 1000;
        let audio_timescale = 48000;

        // Segment 0: 10s video, missing audio
        let seg0_video_dur = 10_000;
        tracker.advance_video(seg0_video_dur);
        tracker.skip_audio_segment(seg0_video_dur, video_timescale, audio_timescale);
        assert_eq!(tracker.video_offset, 10_000);
        assert_eq!(tracker.audio_offset, 480_000);

        // Segment 1: 5s video, 5s audio
        tracker.advance_video(5_000);
        tracker.advance_audio(240_000);
        assert_eq!(tracker.video_offset, 15_000);
        assert_eq!(tracker.audio_offset, 720_000);

        assert_eq!(tracker.video_offset as f64 / video_timescale as f64, 15.0);
        assert_eq!(tracker.audio_offset as f64 / audio_timescale as f64, 15.0);
    }

    #[test]
    fn test_trim_audio_sample_count_longer_audio() {
        let max_audio = 480_000u64; // 10.0s at 48kHz
        let samples = vec![1024u32; 550]; // 550 * 1024 = 563,200 units (~11.73s)
        let kept = trim_audio_sample_count(&samples, max_audio);
        // 468 * 1024 = 479,232 (diff 768)
        // 469 * 1024 = 480,256 (diff 256) -> closest
        assert_eq!(kept, 469);
        let kept_units = (kept as u64) * 1024;
        let diff_secs = ((kept_units as f64 - max_audio as f64) / 48000.0).abs();
        assert!(diff_secs < 0.01);
    }

    #[test]
    fn test_trim_audio_sample_count_shorter_audio() {
        let max_audio = 480_000u64;
        let samples = vec![1024u32; 200]; // 200 * 1024 = 204,800 units
        let kept = trim_audio_sample_count(&samples, max_audio);
        assert_eq!(kept, 200);
    }

    #[test]
    fn test_trim_audio_sample_count_zero_video() {
        let max_audio = 0u64;
        let samples = vec![1024u32; 100];
        let kept = trim_audio_sample_count(&samples, max_audio);
        assert_eq!(kept, 0);
    }

    #[test]
    fn test_remux_audio_video_duration_alignment() {
        let mut tracker = RemuxTimestampTracker::new();
        let video_timescale = 1000u32;
        let audio_timescale = 48000u32;

        let segments = [(10_000u64, 520usize), (8_500u64, 420usize), (4_200u64, 230usize)];
        for (v_dur, a_count) in segments {
            tracker.advance_video(v_dur);
            let max_a = audio_offset_from_video_duration(v_dur, video_timescale, audio_timescale);
            let a_samples = vec![1024u32; a_count];
            let kept = trim_audio_sample_count(&a_samples, max_a);
            let seg_a_dur = (kept as u64) * 1024;
            tracker.advance_audio(seg_a_dur);

            // Re-align audio offset to segment video boundary to prevent cumulative drift
            tracker.audio_offset = audio_offset_from_video_duration(tracker.video_offset, video_timescale, audio_timescale);
        }

        let total_v_sec = tracker.video_offset as f64 / video_timescale as f64;
        let total_a_sec = tracker.audio_offset as f64 / audio_timescale as f64;
        assert!((total_v_sec - total_a_sec).abs() < 0.01);
    }

    #[test]
    fn test_downscale_cpu_bottom_up_box_filter_2_1() {
        // 4x4 BGRA image -> 2x2 bottom-up
        let mut src = vec![0u8; 4 * 4 * 4];
        // Fill row 0 with (10, 20, 30, 255)
        for x in 0..4 {
            let idx = x * 4;
            src[idx..idx + 4].copy_from_slice(&[10, 20, 30, 255]);
        }
        // Fill row 1 with (30, 40, 50, 255)
        for x in 0..4 {
            let idx = 16 + x * 4;
            src[idx..idx + 4].copy_from_slice(&[30, 40, 50, 255]);
        }
        // Top 2 rows averaged: (20, 30, 40, 255)

        // Fill row 2 with (70, 80, 90, 255)
        for x in 0..4 {
            let idx = 32 + x * 4;
            src[idx..idx + 4].copy_from_slice(&[70, 80, 90, 255]);
        }
        // Fill row 3 with (90, 100, 110, 255)
        for x in 0..4 {
            let idx = 48 + x * 4;
            src[idx..idx + 4].copy_from_slice(&[90, 100, 110, 255]);
        }
        // Bottom 2 rows averaged: (80, 90, 100, 255)

        let mut dst = vec![0u8; 2 * 2 * 4];
        downscale_cpu_bottom_up(&src, 4, 4, 16, &mut dst, 2, 2);

        // Row 0 of dst (bottom-up) must be bottom of image: (80, 90, 100, 255)
        assert_eq!(&dst[0..4], &[80, 90, 100, 255]);
        assert_eq!(&dst[4..8], &[80, 90, 100, 255]);

        // Row 1 of dst (bottom-up) must be top of image: (20, 30, 40, 255)
        assert_eq!(&dst[8..12], &[20, 30, 40, 255]);
        assert_eq!(&dst[12..16], &[20, 30, 40, 255]);
    }

    #[test]
    fn test_select_cut_point_exact_match() {
        let keyframes = vec![0, 2000, 4000, 6000, 8000, 10000];
        let total = 10000;
        let requested = 6000; // total - requested = 4000
        assert_eq!(select_cut_point(&keyframes, total, requested), 4000);
    }

    #[test]
    fn test_select_cut_point_between_keyframes() {
        let keyframes = vec![0, 2000, 4000, 6000, 8000, 10000];
        let total = 10000;
        let requested = 7000; // total - requested = 3000 -> next kf is 4000
        assert_eq!(select_cut_point(&keyframes, total, requested), 4000);

        let requested = 5500; // total - requested = 4500 -> next kf is 6000
        assert_eq!(select_cut_point(&keyframes, total, requested), 6000);
    }

    #[test]
    fn test_select_cut_point_60s_from_69_4s() {
        let mut keyframes = Vec::new();
        for sec in (0..=68).step_by(2) {
            keyframes.push(sec * 1000);
        }
        let total = 69400; // 69.4s at 1000 timescale
        let requested = 60000; // 60.0s
        // total - requested = 9400 -> first kf >= 9400 is 10000 (10.0s)
        let cut = select_cut_point(&keyframes, total, requested);
        assert_eq!(cut, 10000);
        let remaining = total - cut;
        assert_eq!(remaining, 59400); // 59.4s, within one GOP (2000ms) of 60s
    }

    #[test]
    fn test_select_cut_point_total_less_than_or_equal_requested() {
        let keyframes = vec![0, 2000, 4000];
        assert_eq!(select_cut_point(&keyframes, 4000, 5000), 0);
        assert_eq!(select_cut_point(&keyframes, 4000, 4000), 0);
    }

    #[test]
    fn test_select_cut_point_empty_or_zero() {
        assert_eq!(select_cut_point(&[], 1000, 500), 0);
        let keyframes = vec![0, 2000];
        assert_eq!(select_cut_point(&keyframes, 0, 0), 0);
    }

    #[test]
    fn test_select_cut_point_past_all_keyframes_falls_back() {
        let keyframes = vec![0, 2000, 4000];
        // total 5000, requested 500 -> target 4500 -> past all keyframes, returns 4000
        assert_eq!(select_cut_point(&keyframes, 5000, 500), 4000);
    }

    #[test]
    fn test_encoder_cache_roundtrip() {
        let mut cache = std::collections::HashMap::new();
        cache.insert(
            encoder_cache_key(2560, 1440, "Media Foundation"),
            EncoderCacheEntry {
                target_width: 1920,
                target_height: 1080,
            },
        );
        let json = serde_json::to_string(&cache).unwrap();
        let deserialized: std::collections::HashMap<String, EncoderCacheEntry> =
            serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized
                .get(&encoder_cache_key(2560, 1440, "Media Foundation"))
                .unwrap()
                .target_height,
            1080
        );
    }

    #[test]
    fn test_decide_scaler() {
        assert_eq!(decide_scaler(1920, 1080, 1920, 1080, true), ScalerChoice::None);
        assert_eq!(decide_scaler(1920, 1080, 1920, 1080, false), ScalerChoice::None);
        assert_eq!(decide_scaler(3840, 2160, 3840, 2160, true), ScalerChoice::None);
        assert_eq!(decide_scaler(3840, 2160, 3840, 2160, false), ScalerChoice::None);

        assert_eq!(decide_scaler(3840, 2160, 1920, 1080, true), ScalerChoice::D3D11);
        assert_eq!(decide_scaler(2560, 1440, 1920, 1080, true), ScalerChoice::D3D11);
        assert_eq!(decide_scaler(1920, 1080, 1280, 720, true), ScalerChoice::D3D11);

        assert_eq!(decide_scaler(3840, 2160, 1920, 1080, false), ScalerChoice::Cpu);
        assert_eq!(decide_scaler(2560, 1440, 1920, 1080, false), ScalerChoice::Cpu);
        assert_eq!(decide_scaler(1920, 1080, 1280, 720, false), ScalerChoice::Cpu);
    }

    #[test]
    fn test_decide_replay_storage_hysteresis() {
        // Disk storage config always selects disk
        assert!(!decide_replay_storage("disk", true, true, 8000.0, 16000.0));
        assert!(!decide_replay_storage("disk", false, false, 8000.0, 16000.0));

        // Low memory guard off always stays in RAM if storage is "ram"
        assert!(decide_replay_storage("ram", false, true, 100.0, 16000.0));
        assert!(decide_replay_storage("ram", false, false, 100.0, 16000.0));

        // When total memory is zero or invalid, preserve current state
        assert!(decide_replay_storage("ram", true, true, 0.0, 0.0));
        assert!(!decide_replay_storage("ram", true, false, 0.0, 0.0));

        // When currently in RAM, stays in RAM if >= 15% free
        // Total: 10000 MB. 15% = 1500 MB.
        assert!(decide_replay_storage("ram", true, true, 2000.0, 10000.0)); // 20% >= 15% -> RAM
        assert!(decide_replay_storage("ram", true, true, 1500.0, 10000.0)); // 15% >= 15% -> RAM
        assert!(!decide_replay_storage("ram", true, true, 1490.0, 10000.0)); // 14.9% < 15% -> Disk

        // Once fallen back to Disk, needs >= 30% free to return to RAM
        // 30% of 10000 MB = 3000 MB
        assert!(!decide_replay_storage("ram", true, false, 1500.0, 10000.0)); // 15% < 30% -> stays Disk
        assert!(!decide_replay_storage("ram", true, false, 2000.0, 10000.0)); // 20% < 30% -> stays Disk
        assert!(!decide_replay_storage("ram", true, false, 2990.0, 10000.0)); // 29.9% < 30% -> stays Disk
        assert!(decide_replay_storage("ram", true, false, 3000.0, 10000.0)); // 30% >= 30% -> returns to RAM
        assert!(decide_replay_storage("ram", true, false, 3500.0, 10000.0)); // 35% >= 30% -> returns to RAM
    }
}

/// Downscales BGRA pixels from `src_bytes` (top-down) to `dst_buf` (bottom-up).
/// If `src_width == dst_width * 2` and `src_height == dst_height * 2`, performs a 2:1 box filter.
/// Otherwise, falls back to nearest-neighbor sampling.
pub fn downscale_cpu_bottom_up(
    src_bytes: &[u8],
    src_width: u32,
    src_height: u32,
    row_pitch: usize,
    dst_buf: &mut [u8],
    dst_width: u32,
    dst_height: u32,
) {
    let out_w = dst_width as usize;
    let out_h = dst_height as usize;
    let in_w = src_width as usize;
    let in_h = src_height as usize;

    if in_w == out_w * 2 && in_h == out_h * 2 {
        for out_y in 0..out_h {
            let dst_y = out_h - 1 - out_y;
            let dst_row_start = dst_y * out_w * 4;
            let src_y0 = out_y * 2;
            let src_y1 = src_y0 + 1;
            let row0 = &src_bytes[src_y0 * row_pitch..];
            let row1 = &src_bytes[src_y1 * row_pitch..];

            for out_x in 0..out_w {
                let src_x0 = out_x * 8;
                let src_x1 = src_x0 + 4;
                let dst_idx = dst_row_start + out_x * 4;

                let b = (row0[src_x0] as u16 + row0[src_x1] as u16 + row1[src_x0] as u16 + row1[src_x1] as u16 + 2) >> 2;
                let g = (row0[src_x0 + 1] as u16 + row0[src_x1 + 1] as u16 + row1[src_x0 + 1] as u16 + row1[src_x1 + 1] as u16 + 2) >> 2;
                let r = (row0[src_x0 + 2] as u16 + row0[src_x1 + 2] as u16 + row1[src_x0 + 2] as u16 + row1[src_x1 + 2] as u16 + 2) >> 2;
                let a = (row0[src_x0 + 3] as u16 + row0[src_x1 + 3] as u16 + row1[src_x0 + 3] as u16 + row1[src_x1 + 3] as u16 + 2) >> 2;

                dst_buf[dst_idx] = b as u8;
                dst_buf[dst_idx + 1] = g as u8;
                dst_buf[dst_idx + 2] = r as u8;
                dst_buf[dst_idx + 3] = a as u8;
            }
        }
    } else {
        for out_y in 0..out_h {
            let dst_y = out_h - 1 - out_y;
            let dst_row_start = dst_y * out_w * 4;
            let src_y = (out_y * in_h) / out_h;
            let row = &src_bytes[src_y * row_pitch..];

            for out_x in 0..out_w {
                let src_x = ((out_x * in_w) / out_w) * 4;
                let dst_idx = dst_row_start + out_x * 4;
                dst_buf[dst_idx..dst_idx + 4].copy_from_slice(&row[src_x..src_x + 4]);
            }
        }
    }
}
