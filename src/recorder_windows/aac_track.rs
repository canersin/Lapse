//! One audio track's AAC encoder, which keeps running while the recording is cut into
//! segments: the finished packets are handed to the segment they belong to, so joining the
//! segments later gives back the unbroken stream.

use std::collections::VecDeque;

/// Samples in one AAC packet.
pub const PACKET_SAMPLES: u64 = 1024;
/// Packets kept back (about 4 s), as a cut is announced only after the next video segment
/// has started.
const HOLD: usize = 190;

/// The first packet belonging after a cut at `sample` (packet n decodes to the samples from
/// n * PACKET_SAMPLES on).
pub fn packet_boundary(sample: u64) -> u64 {
    (sample + PACKET_SAMPLES / 2) / PACKET_SAMPLES
}

/// Encoded packets not yet given to a segment.
#[derive(Default)]
pub struct PacketQueue {
    /// Number of the packet at the front.
    first: u64,
    packets: VecDeque<Vec<u8>>,
}

impl PacketQueue {
    pub fn push(&mut self, packet: Vec<u8>) {
        self.packets.push_back(packet);
    }

    /// Number the next packet will get.
    pub fn end(&self) -> u64 {
        self.first + self.packets.len() as u64
    }

    /// Takes the packets before number `boundary`.
    pub fn take_before(&mut self, boundary: u64) -> Vec<Vec<u8>> {
        let n = (boundary.saturating_sub(self.first) as usize).min(self.packets.len());
        self.first += n as u64;
        self.packets.drain(..n).collect()
    }

    /// Takes what is too old to still end up after a cut.
    pub fn take_settled(&mut self) -> Vec<Vec<u8>> {
        self.take_before(self.end().saturating_sub(HOLD as u64))
    }
}

#[cfg(windows)]
pub use encoder::AacTrack;

#[cfg(windows)]
mod encoder {
    use std::fs::File;
    use std::io::{BufWriter, Cursor, Seek, SeekFrom, Write};
    use std::mem::ManuallyDrop;
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};
    use mp4::{
        AacConfig, AudioObjectType, ChannelConfig, MediaConfig, Mp4Config, Mp4Sample, Mp4Writer, SampleFreqIndex,
        TrackConfig, TrackType,
    };
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

    use super::{packet_boundary, PacketQueue, PACKET_SAMPLES};

    const BYTES_PER_SECOND: u32 = 24000;

    /// Media Foundation's AAC encoder: 48 kHz stereo s16 in, raw AAC packets out.
    struct Encoder {
        mft: IMFTransform,
        output_size: u32,
        fed: u64,
    }

    impl Encoder {
        fn new() -> Result<Self> {
            unsafe {
                let mft: IMFTransform =
                    CoCreateInstance(&AACMFTEncoder, None, CLSCTX_INPROC_SERVER).context("AAC encoder unavailable")?;
                let format = |subtype: &windows::core::GUID| -> Result<IMFMediaType> {
                    let t = MFCreateMediaType()?;
                    t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
                    t.SetGUID(&MF_MT_SUBTYPE, subtype)?;
                    t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                    t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48000)?;
                    t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
                    Ok(t)
                };
                let input = format(&MFAudioFormat_PCM)?;
                input.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4)?;
                input.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 48000 * 4)?;
                let output = format(&MFAudioFormat_AAC)?;
                output.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, BYTES_PER_SECOND)?;
                mft.SetInputType(0, &input, 0)?;
                mft.SetOutputType(0, &output, 0)?;
                let output_size = mft.GetOutputStreamInfo(0)?.cbSize.max(8192);
                mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
                mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
                Ok(Self { mft, output_size, fed: 0 })
            }
        }

        fn encode(&mut self, frames: &[[f32; 2]], out: &mut PacketQueue) -> Result<()> {
            for block in frames.chunks(PACKET_SAMPLES as usize) {
                let pcm: Vec<u8> = block
                    .iter()
                    .flatten()
                    .flat_map(|&s| crate::recorder_windows::audio::float_to_i16(s).to_le_bytes())
                    .collect();
                unsafe {
                    let sample = sample_of(pcm.len() as u32)?;
                    let buffer = sample.GetBufferByIndex(0)?;
                    let mut ptr = std::ptr::null_mut();
                    buffer.Lock(&mut ptr, None, None)?;
                    std::ptr::copy_nonoverlapping(pcm.as_ptr(), ptr, pcm.len());
                    buffer.Unlock()?;
                    buffer.SetCurrentLength(pcm.len() as u32)?;
                    sample.SetSampleTime((self.fed as i64 * 10_000_000) / 48000)?;
                    sample.SetSampleDuration((block.len() as i64 * 10_000_000) / 48000)?;
                    self.mft.ProcessInput(0, &sample, 0)?;
                }
                self.fed += block.len() as u64;
                self.collect(out)?;
            }
            Ok(())
        }

        /// Fetches every packet the encoder has finished.
        fn collect(&mut self, out: &mut PacketQueue) -> Result<()> {
            loop {
                unsafe {
                    let mut data = [MFT_OUTPUT_DATA_BUFFER {
                        pSample: ManuallyDrop::new(Some(sample_of(self.output_size)?)),
                        ..Default::default()
                    }];
                    let mut status = 0;
                    let res = self.mft.ProcessOutput(0, &mut data, &mut status);
                    let [data] = data;
                    let sample = ManuallyDrop::into_inner(data.pSample);
                    drop(ManuallyDrop::into_inner(data.pEvents));
                    match res {
                        Ok(()) => {
                            let Some(sample) = sample else { return Ok(()) };
                            let buffer = sample.ConvertToContiguousBuffer()?;
                            let (mut ptr, mut len) = (std::ptr::null_mut(), 0);
                            buffer.Lock(&mut ptr, None, Some(&mut len))?;
                            let packet = std::slice::from_raw_parts(ptr, len as usize).to_vec();
                            buffer.Unlock()?;
                            if !packet.is_empty() {
                                out.push(packet);
                            }
                        }
                        Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                        Err(e) => return Err(e.into()),
                    }
                }
            }
        }

        fn drain(&mut self, out: &mut PacketQueue) -> Result<()> {
            unsafe { self.mft.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)? };
            self.collect(out)
        }
    }

    fn sample_of(size: u32) -> Result<IMFSample> {
        unsafe {
            let sample = MFCreateSample()?;
            sample.AddBuffer(&MFCreateMemoryBuffer(size)?)?;
            Ok(sample)
        }
    }

    enum Out {
        File(BufWriter<File>),
        Ram(Cursor<Vec<u8>>),
    }

    impl Write for Out {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            match self {
                Out::File(f) => f.write(buf),
                Out::Ram(c) => c.write(buf),
            }
        }

        fn flush(&mut self) -> std::io::Result<()> {
            match self {
                Out::File(f) => f.flush(),
                Out::Ram(c) => c.flush(),
            }
        }
    }

    impl Seek for Out {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            match self {
                Out::File(f) => f.seek(pos),
                Out::Ram(c) => c.seek(pos),
            }
        }
    }

    /// The .m4a file (or RAM entry) of one segment.
    struct SegmentFile {
        path: PathBuf,
        writer: Mp4Writer<Out>,
        samples: u64,
    }

    impl SegmentFile {
        fn create(path: &Path, is_ram: bool) -> Result<Self> {
            let out = if is_ram { Out::Ram(Cursor::new(Vec::new())) } else { Out::File(BufWriter::new(File::create(path)?)) };
            let config = Mp4Config {
                major_brand: "mp42".parse()?,
                minor_version: 0,
                compatible_brands: vec!["mp42".parse()?, "isom".parse()?],
                timescale: 48000,
            };
            let mut writer = Mp4Writer::write_start(out, &config)?;
            writer.add_track(&TrackConfig {
                track_type: TrackType::Audio,
                timescale: 48000,
                language: "und".into(),
                media_conf: MediaConfig::AacConfig(AacConfig {
                    bitrate: BYTES_PER_SECOND * 8,
                    profile: AudioObjectType::AacLowComplexity,
                    freq_index: SampleFreqIndex::Freq48000,
                    chan_conf: ChannelConfig::Stereo,
                }),
            })?;
            Ok(Self { path: path.to_path_buf(), writer, samples: 0 })
        }

        fn write(&mut self, packets: Vec<Vec<u8>>) -> Result<()> {
            for packet in packets {
                self.writer.write_sample(
                    1,
                    &Mp4Sample {
                        start_time: self.samples,
                        duration: PACKET_SAMPLES as u32,
                        rendering_offset: 0,
                        is_sync: true,
                        bytes: packet.into(),
                    },
                )?;
                self.samples += PACKET_SAMPLES;
            }
            Ok(())
        }

        fn finish(mut self) -> Result<()> {
            self.writer.write_end()?;
            match self.writer.into_writer() {
                Out::File(mut f) => f.flush()?,
                Out::Ram(c) => crate::recorder_windows::mem_store::put(&self.path, c.into_inner()),
            }
            Ok(())
        }
    }

    pub struct AacTrack {
        encoder: Encoder,
        queue: PacketQueue,
        file: Option<SegmentFile>,
    }

    impl AacTrack {
        pub fn new(path: &Path, is_ram: bool) -> Result<Self> {
            Ok(Self { encoder: Encoder::new()?, queue: PacketQueue::default(), file: Some(SegmentFile::create(path, is_ram)?) })
        }

        /// Samples written since the recording started.
        pub fn written(&self) -> usize {
            self.encoder.fed as usize
        }

        pub fn write(&mut self, frames: &[[f32; 2]]) -> Result<()> {
            self.encoder.encode(frames, &mut self.queue)?;
            let settled = self.queue.take_settled();
            match &mut self.file {
                Some(file) => file.write(settled),
                None => Ok(()),
            }
        }

        /// Whether everything before the cut at `sample` is encoded.
        pub fn has_until(&self, sample: u64) -> bool {
            self.queue.end() >= packet_boundary(sample)
        }

        /// Ends the current segment at `sample` and continues in a new file.
        pub fn cut(&mut self, sample: u64, path: &Path, is_ram: bool) -> Result<()> {
            let packets = self.queue.take_before(packet_boundary(sample));
            let next = SegmentFile::create(path, is_ram);
            if let Some(mut file) = self.file.take() {
                file.write(packets)?;
                file.finish()?;
            }
            self.file = Some(next?);
            Ok(())
        }

        pub fn finish(mut self) -> Result<()> {
            let _ = self.encoder.drain(&mut self.queue);
            let packets = self.queue.take_before(u64::MAX);
            if let Some(mut file) = self.file.take() {
                file.write(packets)?;
                file.finish()?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packets_go_to_their_segment_without_loss() {
        let mut q = PacketQueue::default();
        let mut segments = Vec::new();
        let mut got = Vec::new();
        for n in 0..2000u32 {
            q.push(n.to_le_bytes().to_vec());
            got.extend(q.take_settled());
            // Each cut is announced 8 packets (170 ms) late.
            if n % 469 == 468 {
                let boundary = q.end() - 8;
                got.extend(q.take_before(boundary));
                segments.push(std::mem::take(&mut got));
                assert_eq!(q.end(), n as u64 + 1);
            }
        }
        got.extend(q.take_before(u64::MAX));
        segments.push(got);
        assert_eq!(segments.len(), 5);
        let all: Vec<u32> = segments.concat().iter().map(|p| u32::from_le_bytes(p[..].try_into().unwrap())).collect();
        assert_eq!(all, (0..2000).collect::<Vec<u32>>());
    }

    #[test]
    fn test_boundaries_follow_the_clock() {
        // Cuts every 10.05 s: the rounding of each cut doesn't add up.
        for i in 0..100u64 {
            let sample = i * 482_400;
            let start = packet_boundary(sample) * PACKET_SAMPLES;
            assert!(start.abs_diff(sample) <= PACKET_SAMPLES / 2);
        }
    }
}
