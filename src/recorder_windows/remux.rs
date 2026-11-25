use anyhow::{bail, Context, Result};
use mp4::*;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use crate::recorder_windows::segment::{
    audio_offset_from_video_duration, select_cut_point, RemuxTimestampTracker,
};

/// Remuxes multiple MP4 video segments and corresponding M4A audio segments into a single
/// continuous MP4 file with two tracks (video + audio) without re-encoding.
/// When `requested_seconds` is provided, skips leading samples so the output starts at the
/// first video keyframe at or after (total - requested), and drops audio samples before that timestamp.
pub fn join_segments(
    segment_paths: &[PathBuf],
    output_path: &Path,
    requested_seconds: Option<f64>,
) -> Result<()> {
    let part_path = output_path.with_extension("part.mp4");
    let mut needed = Vec::new();
    for p in segment_paths {
        if !p.is_file() && crate::recorder_windows::mem_store::exists(p) {
            needed.push(p.clone());
        }
        let sides = ["m4a", "chat.m4a", "mic.m4a", "tracks.json"].map(|ext| p.with_extension(ext));
        for side in sides.into_iter().chain([crate::tracks::path_for_clip(p)]) {
            if !side.is_file() && crate::recorder_windows::mem_store::exists(&side) {
                needed.push(side);
            }
        }
        for i in 0..16 {
            let side = p.with_extension(format!("app-{}.m4a", i));
            if !side.is_file() && crate::recorder_windows::mem_store::exists(&side) {
                needed.push(side);
            }
        }
    }
    let local_materialized = crate::recorder_windows::mem_store::materialize(&needed).unwrap_or_default();

    let res = (|| -> Result<()> {
        if segment_paths.is_empty() {
            bail!("No segments provided to join");
        }

    let valid_video_paths: Vec<&PathBuf> = segment_paths
        .iter()
        .filter(|p| {
            p.is_file()
                && File::open(p)
                    .and_then(|f| f.metadata())
                    .map(|m| m.len() > 0)
                    .unwrap_or(false)
        })
        .collect();

    if valid_video_paths.is_empty() {
        bail!("No valid segment files to join");
    }

    let first_file = File::open(valid_video_paths[0])
        .with_context(|| format!("Failed to open first segment: {}", valid_video_paths[0].display()))?;
    let first_size = first_file.metadata()?.len();
    let first_video_reader = Mp4Reader::read_header(BufReader::new(first_file), first_size)
        .with_context(|| format!("Failed to read header of {}", valid_video_paths[0].display()))?;

    let config = Mp4Config {
        major_brand: *first_video_reader.major_brand(),
        minor_version: first_video_reader.minor_version(),
        compatible_brands: first_video_reader.compatible_brands().to_vec(),
        timescale: first_video_reader.timescale(),
    };

    let mut video_track_info: Option<(u32, u32, TrackConfig, Option<FourCC>, Option<Vec<u8>>)> = None;
    for (&orig_track_id, track) in first_video_reader.tracks() {
        if track.track_type().ok() == Some(TrackType::Video) {
            let box_type = track.box_type().ok();
            let is_avc1 = box_type == Some(FourCC::from(BoxType::Avc1Box));
            let (media_conf, raw_stsd) = if is_avc1 {
                let conf = MediaConfig::AvcConfig(AvcConfig {
                    width: track.width(),
                    height: track.height(),
                    seq_param_set: track.sequence_parameter_set().unwrap_or_default().to_vec(),
                    pic_param_set: track.picture_parameter_set().unwrap_or_default().to_vec(),
                });
                (conf, None)
            } else {
                let conf = MediaConfig::HevcConfig(HevcConfig {
                    width: track.width(),
                    height: track.height(),
                });
                let stsd = crate::recorder_windows::mp4_box::video_stsd(&std::fs::read(valid_video_paths[0])?)
                    .ok_or_else(|| anyhow::anyhow!("Failed to find video stsd in first segment {}", valid_video_paths[0].display()))?;
                (conf, Some(stsd))
            };
            let conf = TrackConfig {
                track_type: TrackType::Video,
                timescale: track.timescale(),
                language: track.language().to_string(),
                media_conf,
            };
            video_track_info = Some((orig_track_id, track.timescale(), conf, box_type, raw_stsd));
            break;
        }
    }

    let (video_orig_id, video_timescale, video_track_conf, first_video_box_type, raw_stsd) = match video_track_info {
        Some(v) => v,
        None => bail!("No video track found in segments"),
    };

    struct AudioTrackSpec {
        name: String,
        ext: String,
        orig_id: u32,
        timescale: u32,
        #[allow(dead_code)]
        conf: TrackConfig,
        out_id: u32,
        tracker_offset: u64,
        /// The last packet, kept back so its duration can still cover a gap.
        held: Option<Mp4Sample>,
    }

    fn inspect_audio_track(paths: &[&PathBuf], ext: &str) -> Option<(u32, u32, TrackConfig)> {
        for &v_path in paths {
            let a_path = v_path.with_extension(ext);
            let a_file = match File::open(&a_path).or_else(|_| if ext == "m4a" { File::open(v_path) } else { Err(std::io::Error::from(std::io::ErrorKind::NotFound)) }) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let a_size = match a_file.metadata() {
                Ok(m) if m.len() > 0 => m.len(),
                _ => continue,
            };
            let a_reader = match Mp4Reader::read_header(BufReader::new(a_file), a_size) {
                Ok(r) => r,
                Err(_) => continue,
            };
            for (&orig_id, track) in a_reader.tracks() {
                if track.track_type().ok() == Some(TrackType::Audio)
                    && track.box_type().ok() == Some(FourCC::from(BoxType::Mp4aBox))
                {
                    let freq_index = match track.sample_freq_index() {
                        Ok(f) => f,
                        Err(_) => continue,
                    };
                    let chan_conf = match track.channel_config() {
                        Ok(c) => c,
                        Err(_) => continue,
                    };
                    let conf = TrackConfig {
                        track_type: TrackType::Audio,
                        timescale: track.timescale(),
                        language: track.language().to_string(),
                        media_conf: MediaConfig::AacConfig(AacConfig {
                            bitrate: track.bitrate(),
                            profile: track
                                .audio_profile()
                                .unwrap_or(AudioObjectType::AacLowComplexity),
                            freq_index,
                            chan_conf,
                        }),
                    };
                    return Some((orig_id, track.timescale(), conf));
                }
            }
        }
        None
    }

    let mut candidates: Vec<(String, String)> = Vec::new();
    let saved_tracks = crate::tracks::load_for(valid_video_paths[0], 0);
    if !saved_tracks.is_empty() {
        let mut app_idx = 0;
        for tr in &saved_tracks {
            match tr.as_str() {
                "game" => {
                    candidates.push(("game".to_string(), "m4a".to_string()));
                }
                "chat" => {
                    candidates.push(("chat".to_string(), "chat.m4a".to_string()));
                }
                "mic" => {
                    candidates.push(("mic".to_string(), "mic.m4a".to_string()));
                }
                name if name.starts_with("app:") => {
                    candidates.push((name.to_string(), format!("app-{}.m4a", app_idx)));
                    app_idx += 1;
                }
                other => {
                    candidates.push((other.to_string(), format!("app-{}.m4a", app_idx)));
                    app_idx += 1;
                }
            }
        }
    } else {
        candidates.push(("game".to_string(), "m4a".to_string()));
        candidates.push(("chat".to_string(), "chat.m4a".to_string()));
        for idx in 0..16 {
            let ext = format!("app-{}.m4a", idx);
            if valid_video_paths.iter().any(|p| p.with_extension(&ext).exists()) {
                candidates.push((format!("app:{}", idx), ext));
            }
        }
        candidates.push(("mic".to_string(), "mic.m4a".to_string()));
    }

    let detected_audio: Vec<(String, String, u32, u32, TrackConfig)> = candidates
        .into_iter()
        .filter_map(|(name, ext)| {
            inspect_audio_track(&valid_video_paths, &ext)
                .map(|(orig_id, ts, conf)| (name, ext, orig_id, ts, conf))
        })
        .collect();

    // Inspect segments to determine keyframe timestamps and total video duration
    let mut all_keyframes = Vec::new();
    let mut seg_durations = Vec::with_capacity(valid_video_paths.len());
    let mut total_video_duration = 0u64;

    for &seg_v_path in &valid_video_paths {
        let seg_file = match File::open(seg_v_path) {
            Ok(f) => f,
            Err(_) => {
                seg_durations.push(0);
                continue;
            }
        };
        let seg_size = match seg_file.metadata() {
            Ok(m) if m.len() > 0 => m.len(),
            _ => {
                seg_durations.push(0);
                continue;
            }
        };
        let mut seg_reader = match Mp4Reader::read_header(BufReader::new(seg_file), seg_size) {
            Ok(r) => r,
            Err(_) => {
                seg_durations.push(0);
                continue;
            }
        };

        let (v_id, v_track) = if seg_reader.tracks().contains_key(&video_orig_id) {
            (video_orig_id, seg_reader.tracks().get(&video_orig_id))
        } else {
            seg_reader
                .tracks()
                .iter()
                .find(|(_, t)| t.track_type().ok() == Some(TrackType::Video))
                .map(|(&id, t)| (id, Some(t)))
                .unwrap_or((0, None))
        };

        if v_id == 0 || v_track.is_none() {
            seg_durations.push(0);
            continue;
        }
        let v_track = v_track.unwrap();
        if v_track.box_type().ok() != first_video_box_type {
            eprintln!(
                "Warning: skipping segment {} with different video codec",
                seg_v_path.display()
            );
            seg_durations.push(0);
            continue;
        }

        let v_sample_count = seg_reader.sample_count(v_id).unwrap_or(0);
        let mut seg_dur = 0u64;
        for s_id in 1..=v_sample_count {
            if let Ok(Some(sample)) = seg_reader.read_sample(v_id, s_id) {
                if sample.is_sync {
                    all_keyframes.push(total_video_duration + sample.start_time);
                }
                seg_dur += sample.duration as u64;
            }
        }
        seg_durations.push(seg_dur);
        total_video_duration += seg_dur;
    }
    all_keyframes.dedup();

    let cut_point_video = match requested_seconds {
        Some(req_secs) if req_secs > 0.0 => {
            let req_units = (req_secs * video_timescale as f64).round() as u64;
            select_cut_point(&all_keyframes, total_video_duration, req_units)
        }
        _ => 0,
    };

    if let Some(parent) = part_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let out_file = File::create(&part_path)
        .with_context(|| format!("Failed to create output file: {}", part_path.display()))?;
    let mut writer = Mp4Writer::write_start(BufWriter::new(out_file), &config)?;

    writer.add_track(&video_track_conf)?;
    let out_video_id = 1u32;

    let mut audio_specs: Vec<AudioTrackSpec> = Vec::new();
    let mut next_track_id = 2u32;
    for (name, ext, orig_id, timescale, conf) in detected_audio {
        writer.add_track(&conf)?;
        let out_id = next_track_id;
        next_track_id += 1;
        audio_specs.push(AudioTrackSpec {
            name,
            ext,
            orig_id,
            timescale,
            conf,
            out_id,
            tracker_offset: 0,
            held: None,
        });
    }

    let mut tracker = RemuxTimestampTracker::new();
    let mut cumulative_video_ts = 0u64;

    for (seg_idx, &seg_v_path) in valid_video_paths.iter().enumerate() {
        let seg_track_dur = seg_durations.get(seg_idx).copied().unwrap_or(0);
        if seg_track_dur == 0 {
            continue;
        }

        if cumulative_video_ts + seg_track_dur <= cut_point_video {
            cumulative_video_ts += seg_track_dur;
            continue;
        }

        let seg_v_file = match File::open(seg_v_path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Warning: failed to open video segment {}: {e}", seg_v_path.display());
                cumulative_video_ts += seg_track_dur;
                continue;
            }
        };
        let seg_v_size = match seg_v_file.metadata() {
            Ok(m) if m.len() > 0 => m.len(),
            _ => {
                cumulative_video_ts += seg_track_dur;
                continue;
            }
        };
        let mut seg_v_reader = match Mp4Reader::read_header(BufReader::new(seg_v_file), seg_v_size) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Warning: failed to parse video segment header {}: {e}", seg_v_path.display());
                cumulative_video_ts += seg_track_dur;
                continue;
            }
        };

        let (v_id, v_track) = if seg_v_reader.tracks().contains_key(&video_orig_id) {
            (video_orig_id, seg_v_reader.tracks().get(&video_orig_id))
        } else {
            seg_v_reader
                .tracks()
                .iter()
                .find(|(_, t)| t.track_type().ok() == Some(TrackType::Video))
                .map(|(&id, t)| (id, Some(t)))
                .unwrap_or((0, None))
        };

        if v_id == 0 || v_track.is_none() {
            cumulative_video_ts += seg_track_dur;
            continue;
        }
        let v_track = v_track.unwrap();
        if v_track.box_type().ok() != first_video_box_type {
            eprintln!(
                "Warning: skipping segment {} with different video codec",
                seg_v_path.display()
            );
            cumulative_video_ts += seg_track_dur;
            continue;
        }

        let cut_in_segment = cut_point_video.saturating_sub(cumulative_video_ts);
        let seg_video_duration = seg_track_dur - cut_in_segment;
        let seg_video_start = tracker.video_offset;

        // The audio packets of all segments form one unbroken stream, so they are kept whole
        // and the segment's last picture is stretched to end with them (a few ms).
        let mut audio: Vec<Vec<Mp4Sample>> = Vec::with_capacity(audio_specs.len());
        for spec in &audio_specs {
            let to_audio = |v: u64| audio_offset_from_video_duration(v, video_timescale, spec.timescale);
            audio.push(read_segment_audio(
                seg_v_path,
                &spec.ext,
                spec.orig_id,
                to_audio(cut_in_segment),
                to_audio(seg_video_duration) + spec.timescale as u64 / 4,
            ));
        }
        let audio_end = audio.first().zip(audio_specs.first()).filter(|(samples, _)| !samples.is_empty()).map(|(samples, spec)| {
            let total: u64 = samples.iter().map(|s| s.duration as u64).sum();
            let start = audio_offset_from_video_duration(seg_video_start, video_timescale, spec.timescale);
            let offset = if start > spec.tracker_offset + spec.timescale as u64 / 4 { start } else { spec.tracker_offset };
            audio_offset_from_video_duration(offset + total, spec.timescale, video_timescale)
        });
        let stretch = audio_end.map_or(0, |end| end as i64 - (seg_video_start + seg_video_duration) as i64);
        let stretch = if stretch.unsigned_abs() <= video_timescale as u64 / 4 { stretch } else { 0 };

        let v_sample_count = seg_v_reader.sample_count(v_id).unwrap_or(0);
        for s_id in 1..=v_sample_count {
            if let Ok(Some(mut sample)) = seg_v_reader.read_sample(v_id, s_id) {
                let sample_global_ts = cumulative_video_ts + sample.start_time;
                if sample_global_ts < cut_point_video {
                    continue;
                }
                if s_id == v_sample_count {
                    sample.duration = (sample.duration as i64 + stretch).max(1) as u32;
                }
                sample.start_time = tracker.video_offset;
                tracker.advance_video(sample.duration as u64);
                writer
                    .write_sample(out_video_id, &sample)
                    .context("Failed to write video sample")?;
            }
        }

        for (spec, samples) in audio_specs.iter_mut().zip(audio) {
            // The writer only adds up durations: audio missing before this segment has to be
            // made up by the packet before it.
            let start = audio_offset_from_video_duration(seg_video_start, video_timescale, spec.timescale);
            if start > spec.tracker_offset + spec.timescale as u64 / 4 && !samples.is_empty() {
                if let Some(held) = &mut spec.held {
                    held.duration += (start - spec.tracker_offset) as u32;
                    spec.tracker_offset = start;
                }
            }
            for mut sample in samples {
                sample.start_time = spec.tracker_offset;
                spec.tracker_offset += sample.duration as u64;
                if let Some(held) = spec.held.replace(sample) {
                    writer.write_sample(spec.out_id, &held).context("Failed to write audio sample")?;
                }
            }
        }

        cumulative_video_ts += seg_track_dur;
    }

    for spec in &mut audio_specs {
        if let Some(held) = spec.held.take() {
            writer.write_sample(spec.out_id, &held).context("Failed to write audio sample")?;
        }
    }

    writer.write_end()?;
    drop(writer);

    if let Some(ref stsd_bytes) = raw_stsd {
        use std::io::{Read, Seek, SeekFrom, Write};
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&part_path)
            .with_context(|| format!("Failed to open {} for patching stsd", part_path.display()))?;

        let file_len = file.metadata()?.len();
        let mut offset: u64 = 0;
        let mut moov_offset: Option<u64> = None;
        let mut moov_size: Option<u64> = None;

        while offset < file_len {
            file.seek(SeekFrom::Start(offset))?;
            let mut header = [0u8; 8];
            file.read_exact(&mut header)
                .with_context(|| format!("Failed to read box header at offset {}", offset))?;
            let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
            let fourcc = [header[4], header[5], header[6], header[7]];

            let box_total_size = if size32 == 1 {
                let mut large_buf = [0u8; 8];
                file.read_exact(&mut large_buf)?;
                u64::from_be_bytes(large_buf)
            } else if size32 == 0 {
                file_len - offset
            } else if size32 >= 8 {
                size32
            } else {
                bail!("Invalid box size {} at offset {}", size32, offset);
            };

            if fourcc == *b"moov" {
                moov_offset = Some(offset);
                moov_size = Some(box_total_size);
                offset += box_total_size;
                if offset != file_len {
                    bail!("moov box is not the last top-level box in {}", part_path.display());
                }
                break;
            } else {
                offset += box_total_size;
            }
        }

        let moov_off = moov_offset
            .ok_or_else(|| anyhow::anyhow!("moov box not found in {}", part_path.display()))?;
        let moov_sz = moov_size.unwrap() as usize;

        file.seek(SeekFrom::Start(moov_off))?;
        let mut moov_bytes = vec![0u8; moov_sz];
        file.read_exact(&mut moov_bytes)?;

        let new_moov = crate::recorder_windows::mp4_box::replace_video_stsd(&moov_bytes, stsd_bytes)
            .ok_or_else(|| anyhow::anyhow!("Failed to replace video stsd in moov box"))?;

        file.set_len(moov_off)?;
        file.seek(SeekFrom::Start(moov_off))?;
        file.write_all(&new_moov)?;
        file.flush()?;
    }

    std::fs::rename(&part_path, output_path).with_context(|| {
        format!(
            "Failed to rename {} to {}",
            part_path.display(),
            output_path.display()
        )
    })?;

    let track_names: Vec<String> = audio_specs.iter().map(|s| s.name.to_string()).collect();
    if !track_names.is_empty() {
        crate::tracks::save(output_path.parent().unwrap_or(output_path), output_path, &track_names);
    }
    Ok(())
    })();

    for p in &local_materialized {
        let _ = std::fs::remove_file(p);
    }
    if res.is_err() {
        let _ = std::fs::remove_file(&part_path);
    }
    res
}

/// The audio packets of one segment from `skip` on (in audio time), at most `limit` long.
fn read_segment_audio(video: &Path, ext: &str, track_id: u32, skip: u64, limit: u64) -> Vec<Mp4Sample> {
    let file = File::open(video.with_extension(ext)).or_else(|e| if ext == "m4a" { File::open(video) } else { Err(e) });
    let Some((file, size)) = file.ok().and_then(|f| f.metadata().ok().map(|m| (f, m.len()))).filter(|(_, size)| *size > 0) else {
        return Vec::new();
    };
    let Ok(mut reader) = Mp4Reader::read_header(BufReader::new(file), size) else { return Vec::new() };
    let id = if reader.tracks().contains_key(&track_id) {
        track_id
    } else {
        let first = reader.tracks().iter().find(|(_, t)| t.track_type().ok() == Some(TrackType::Audio));
        first.map(|(&id, _)| id).unwrap_or(0)
    };
    let mut samples = Vec::new();
    let mut total = 0u64;
    for s_id in 1..=reader.sample_count(id).unwrap_or(0) {
        let Ok(Some(sample)) = reader.read_sample(id, s_id) else { continue };
        if sample.start_time + (sample.duration as u64 / 2) < skip {
            continue;
        }
        total += sample.duration as u64;
        if total > limit {
            break;
        }
        samples.push(sample);
    }
    samples
}
