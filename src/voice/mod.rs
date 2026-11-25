//! Voice-triggered replay recording module: feature extraction, DTW keyword matching,
//! and microphone listener.

pub mod features;
pub mod matcher;
pub mod listener;
pub mod wizard;

pub use features::extract_mfcc;
pub use matcher::{match_utterance, VoiceTemplates};
pub use listener::{VoiceListener, WizardCapture};
pub use wizard::{VoiceWizardController, WizardCmd, WizardEvent};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::matcher::{dtw_distance, enroll, EnrollmentError};
    use crate::voice::features::{
        dbfs_to_linear, dft_naive, fft_radix2, Complex, FRAME_LEN, HOP_LEN,
    };
    use crate::voice::listener::EnergyVad;
    use std::f32::consts::PI;

    #[test]
    fn test_fft_vs_naive_dft() {
        let n = 64;
        let mut fft_input = Vec::with_capacity(n);
        let mut dft_input = Vec::with_capacity(n);

        for i in 0..n {
            let val = (2.0 * PI * 3.0 * (i as f32) / (n as f32)).sin()
                + 0.5 * (2.0 * PI * 7.0 * (i as f32) / (n as f32)).cos();
            fft_input.push(Complex::new(val, 0.0));
            dft_input.push(Complex::new(val, 0.0));
        }

        fft_radix2(&mut fft_input);
        let dft_output = dft_naive(&dft_input);

        for k in 0..n {
            let diff_re = (fft_input[k].re - dft_output[k].re).abs();
            let diff_im = (fft_input[k].im - dft_output[k].im).abs();
            assert!(
                diff_re < 1e-3 && diff_im < 1e-3,
                "FFT mismatch at bin {}: fft=({},{}), dft=({},{})",
                k,
                fft_input[k].re,
                fft_input[k].im,
                dft_output[k].re,
                dft_output[k].im
            );
        }
    }

    #[test]
    fn test_mel_mfcc_shape() {
        let sample_rate = 16000;
        let duration_secs = 1.0;
        let num_samples = (sample_rate as f32 * duration_secs) as usize;

        // Generate synthetic audio signal
        let mut audio = Vec::with_capacity(num_samples);
        for i in 0..num_samples {
            let t = i as f32 / sample_rate as f32;
            audio.push((2.0 * PI * 440.0 * t).sin());
        }

        let mfcc_frames = extract_mfcc(&audio);
        let expected_frames = (num_samples - FRAME_LEN) / HOP_LEN + 1;
        assert_eq!(mfcc_frames.len(), expected_frames);

        for frame in &mfcc_frames {
            assert_eq!(frame.len(), 12);
        }

        // Verify Cepstral Mean Normalisation (mean of each coeff is ~0)
        let num_frames = mfcc_frames.len() as f32;
        for c in 0..12 {
            let sum: f32 = mfcc_frames.iter().map(|f| f[c]).sum();
            assert!((sum / num_frames).abs() < 1e-4, "CMN mean not zero for coeff {}", c);
        }
    }

    #[test]
    fn test_dtw_identity_and_symmetry() {
        let seq_a: Vec<Vec<f32>> = (0..30)
            .map(|i| (0..12).map(|k| ((i * 12 + k) as f32 * 0.1).sin()).collect())
            .collect();
        let seq_b: Vec<Vec<f32>> = (0..25)
            .map(|i| (0..12).map(|k| ((i * 12 + k) as f32 * 0.12).cos()).collect())
            .collect();

        // Identity = 0
        assert_eq!(dtw_distance(&seq_a, &seq_a), 0.0);
        assert_eq!(dtw_distance(&seq_b, &seq_b), 0.0);

        // Symmetry
        let d_ab = dtw_distance(&seq_a, &seq_b);
        let d_ba = dtw_distance(&seq_b, &seq_a);
        assert!(
            (d_ab - d_ba).abs() < 1e-5,
            "DTW not symmetric: d_ab={}, d_ba={}",
            d_ab,
            d_ba
        );
    }

    #[test]
    fn test_enrollment_threshold() {
        let base: Vec<Vec<f32>> = (0..20)
            .map(|i| (0..12).map(|k| ((i + k) as f32 * 0.2).sin()).collect())
            .collect();

        let mut takes = Vec::new();
        for t in 0..5 {
            let take: Vec<Vec<f32>> = base
                .iter()
                .map(|frame| {
                    frame
                        .iter()
                        .map(|&v| v + (t as f32 * 0.01))
                        .collect()
                })
                .collect();
            takes.push(take);
        }

        let templates = enroll(&takes).expect("Enrollment failed");
        assert!(templates.threshold > 0.0);

        // Outlier rejection test
        let mut bad_takes = takes.clone();
        bad_takes[4] = (0..20)
            .map(|i| (0..12).map(|k| i as f32 * 5.0 + k as f32).collect())
            .collect();

        let res = enroll(&bad_takes);
        assert!(
            matches!(res, Err(EnrollmentError::Outlier { take_index: 4, .. })),
            "Expected take 4 to be rejected as outlier"
        );
    }

    fn generate_chirp(f0: f32, f1: f32, duration: f32, noise_amp: f32, seed: u32) -> Vec<f32> {
        let sample_rate = 16000.0;
        let num_samples = (sample_rate * duration) as usize;
        let mut audio = Vec::with_capacity(num_samples);
        let mut rng_state = seed;

        for i in 0..num_samples {
            let t = i as f32 / sample_rate;
            let f = f0 + (f1 - f0) * t / (2.0 * duration);
            let phase = 2.0 * PI * f * t;
            let chirp = phase.sin();

            // Simple LCG pseudo-random noise
            rng_state = rng_state.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = ((rng_state as f32 / u32::MAX as f32) - 0.5) * 2.0 * noise_amp;

            audio.push(chirp + noise);
        }
        audio
    }

    #[test]
    fn test_detection_synthetic_signals() {
        // Enrolled word: chirp from 400 Hz to 1200 Hz
        let mut takes = Vec::new();
        let variations = [0.48, 0.49, 0.50, 0.51, 0.52];
        for (idx, &dur) in variations.iter().enumerate() {
            let signal = generate_chirp(400.0, 1200.0, dur, 0.05, 1000 + idx as u32);
            takes.push(extract_mfcc(&signal));
        }

        let templates = enroll(&takes).expect("Chirp enrollment failed");

        // Stretched variant of the same word (dur = 0.505s) must match
        let test_match = generate_chirp(400.0, 1200.0, 0.505, 0.05, 9999);
        let test_match_mfcc = extract_mfcc(&test_match);
        let res_match = match_utterance(&templates, &test_match_mfcc, 1.0);
        assert!(
            res_match.matched,
            "Target chirp variant should match (dist={:.3}, thresh={:.3})",
            res_match.distance,
            res_match.threshold
        );

        // White noise must NOT match
        let mut noise = Vec::with_capacity(8000);
        let mut rng = 54321u32;
        for _ in 0..8000 {
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            noise.push(((rng as f32 / u32::MAX as f32) - 0.5) * 0.8);
        }
        let noise_mfcc = extract_mfcc(&noise);
        let res_noise = match_utterance(&templates, &noise_mfcc, 1.0);
        assert!(
            !res_noise.matched,
            "White noise should not match (dist={:.3}, thresh={:.3})",
            res_noise.distance,
            res_noise.threshold
        );

        // A different chirp (downwards 2500 Hz -> 500 Hz) must NOT match
        let diff_chirp = generate_chirp(2500.0, 500.0, 0.50, 0.05, 8888);
        let diff_mfcc = extract_mfcc(&diff_chirp);
        let res_diff = match_utterance(&templates, &diff_mfcc, 1.0);
        assert!(
            !res_diff.matched,
            "Different chirp should not match (dist={:.3}, thresh={:.3})",
            res_diff.distance,
            res_diff.threshold
        );
    }

    fn generate_3_syllable_phrase(dur_scale: f32, jitter: f32, seed: u32) -> Vec<f32> {
        let bursts = [
            (350.0 + jitter * 10.0, 700.0 + jitter * 15.0, 0.12 * dur_scale),
            (700.0 + jitter * 15.0, 1100.0 + jitter * 20.0, 0.12 * dur_scale),
            (1100.0 + jitter * 20.0, 1600.0 + jitter * 25.0, 0.12 * dur_scale),
        ];
        let gap_dur = 0.05 * dur_scale;
        let mut audio = Vec::new();
        let mut rng = seed;

        for (i, &(f0, f1, dur)) in bursts.iter().enumerate() {
            if i > 0 {
                let gap_samples = (16000.0 * gap_dur) as usize;
                for _ in 0..gap_samples {
                    rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                    let noise = ((rng as f32 / u32::MAX as f32) - 0.5) * 0.01;
                    audio.push(noise);
                }
            }
            let num_samples = (16000.0 * dur) as usize;
            let fade_len = 160usize;
            for s in 0..num_samples {
                let t = s as f32 / 16000.0;
                let f = f0 + (f1 - f0) * t / (2.0 * dur);
                let mut val = (2.0 * PI * f * t).sin();
                if s < fade_len {
                    val *= s as f32 / fade_len as f32;
                } else if s + fade_len > num_samples {
                    val *= (num_samples - s) as f32 / fade_len as f32;
                }
                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = ((rng as f32 / u32::MAX as f32) - 0.5) * 0.02;
                audio.push(val + noise);
            }
        }
        audio
    }

    #[test]
    fn test_detection_synthetic_3_syllable_phrase() {
        // Enrolled 6x with jitter
        let mut takes = Vec::new();
        let jitters = [-0.02, -0.01, 0.0, 0.01, 0.02, 0.015];
        for (idx, &j) in jitters.iter().enumerate() {
            let dur_scale = 1.0 + j;
            let signal = generate_3_syllable_phrase(dur_scale, j, 2000 + idx as u32);
            takes.push(extract_mfcc(&signal));
        }

        let templates = enroll(&takes).expect("Enrollment of 3-syllable phrase failed");
        assert!(templates.threshold > 0.0);
        assert_eq!(templates.median_syllables(), 3);

        // Single burst (one word) must NOT match
        let single_burst = generate_chirp(350.0, 700.0, 0.12, 0.02, 3001);
        let single_mfcc = extract_mfcc(&single_burst);
        let res_single = match_utterance(&templates, &single_mfcc, 1.0);
        assert!(
            !res_single.matched,
            "Single burst (one word) must NOT match (dist={:.3}, thresh={:.3}, reason={:?})",
            res_single.distance, res_single.threshold, res_single.rejection_reason
        );

        // Two bursts must NOT match
        let mut two_bursts = generate_chirp(350.0, 700.0, 0.12, 0.02, 3002);
        two_bursts.resize(two_bursts.len() + (16000.0 * 0.05) as usize, 0.0);
        two_bursts.extend(generate_chirp(700.0, 1100.0, 0.12, 0.02, 3003));
        let two_mfcc = extract_mfcc(&two_bursts);
        let res_two = match_utterance(&templates, &two_mfcc, 1.0);
        assert!(
            !res_two.matched,
            "Two bursts must NOT match (dist={:.3}, thresh={:.3}, reason={:?})",
            res_two.distance, res_two.threshold, res_two.rejection_reason
        );

        // Noise must NOT match
        let mut noise = Vec::with_capacity(8000);
        let mut rng = 44444u32;
        for _ in 0..8000 {
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            noise.push(((rng as f32 / u32::MAX as f32) - 0.5) * 0.8);
        }
        let noise_mfcc = extract_mfcc(&noise);
        let res_noise = match_utterance(&templates, &noise_mfcc, 1.0);
        assert!(
            !res_noise.matched,
            "Noise must NOT match (dist={:.3}, thresh={:.3}, reason={:?})",
            res_noise.distance, res_noise.threshold, res_noise.rejection_reason
        );

        // Time-stretched (+15%) full phrase MUST match
        let stretched_plus = generate_3_syllable_phrase(1.15, 0.0, 5001);
        let stretch_plus_mfcc = extract_mfcc(&stretched_plus);
        let res_plus = match_utterance(&templates, &stretch_plus_mfcc, 1.0);
        assert!(
            res_plus.matched,
            "Time-stretched (+15%) full phrase MUST match (dist={:.3}, thresh={:.3}, reason={:?})",
            res_plus.distance, res_plus.threshold, res_plus.rejection_reason
        );

        // Time-stretched (-15%) full phrase MUST match
        let stretched_minus = generate_3_syllable_phrase(0.85, 0.0, 5002);
        let stretch_minus_mfcc = extract_mfcc(&stretched_minus);
        let res_minus = match_utterance(&templates, &stretch_minus_mfcc, 1.0);
        assert!(
            res_minus.matched,
            "Time-stretched (-15%) full phrase MUST match (dist={:.3}, thresh={:.3}, reason={:?})",
            res_minus.distance, res_minus.threshold, res_minus.rejection_reason
        );
    }

    #[test]
    fn test_quiet_mic_vad_and_gain_normalisation() {
        let sample_rate = 16000;
        let block_size = 160;

        // 1. Synthetic -80 dBFS noise floor (linear amplitude ~ 0.0001)
        let floor_amp = dbfs_to_linear(-80.0);
        let mut rng_state = 123456789u32;
        let mut gen_noise = |n: usize| -> Vec<f32> {
            let mut buf = Vec::with_capacity(n);
            for _ in 0..n {
                rng_state = rng_state.wrapping_mul(1664525).wrapping_add(1013904223);
                let r = ((rng_state as f32 / u32::MAX as f32) - 0.5) * 2.0;
                buf.push(r * floor_amp);
            }
            buf
        };

        // 2. Synthetic -45 dBFS speech signal (chirp 300 to 900 Hz, 400 ms)
        let speech_dur_secs = 0.40;
        let speech_samples = (speech_dur_secs * sample_rate as f32) as usize;
        let speech_target_rms = dbfs_to_linear(-45.0); // ~0.00562
        let mut speech = Vec::with_capacity(speech_samples);
        for i in 0..speech_samples {
            let t = i as f32 / sample_rate as f32;
            let freq = 300.0 + 600.0 * (t / speech_dur_secs);
            let s = (2.0 * PI * freq * t).sin();
            speech.push(s);
        }
        let cur_rms = (speech.iter().map(|&s| s * s).sum::<f32>() / speech.len() as f32).sqrt();
        let noise_sample = gen_noise(speech_samples);
        for (i, s) in speech.iter_mut().enumerate() {
            *s = (*s * (speech_target_rms / cur_rms)) + noise_sample[i];
        }

        // 3. Assemble: 0.5s silence (-80 dBFS) + 0.4s speech (-45 dBFS) + 0.5s silence (-80 dBFS)
        let pre_silence = gen_noise((0.5 * sample_rate as f32) as usize);
        let post_silence = gen_noise((0.5 * sample_rate as f32) as usize);

        let mut full_audio = pre_silence;
        full_audio.extend_from_slice(&speech);
        full_audio.extend_from_slice(&post_silence);

        // 4. Feed into EnergyVad
        let mut vad = EnergyVad::new();
        let mut detected_utterances = Vec::new();

        for chunk in full_audio.chunks(block_size) {
            if chunk.len() == block_size {
                let (utt, _rms) = vad.process_block(chunk);
                if let Some(u) = utt {
                    detected_utterances.push(u);
                }
            }
        }

        assert_eq!(
            detected_utterances.len(),
            1,
            "VAD should detect exactly 1 utterance for -45 dBFS speech over -80 dBFS floor"
        );

        let utt_quiet = &detected_utterances[0];
        assert!(
            utt_quiet.len() >= 4000 && utt_quiet.len() <= 20000,
            "Utterance length {} out of expected range",
            utt_quiet.len()
        );

        // 5. Test automatic gain normalisation:
        // Speech at -20 dBFS vs speech at -45 dBFS produce near-identical MFCCs
        let loud_scale = dbfs_to_linear(-20.0 - (-45.0)); // +25 dB louder
        let utt_loud: Vec<f32> = utt_quiet.iter().map(|&s| s * loud_scale).collect();

        let mfcc_quiet = extract_mfcc(utt_quiet);
        let mfcc_loud = extract_mfcc(&utt_loud);

        assert!(!mfcc_quiet.is_empty());
        assert_eq!(mfcc_quiet.len(), mfcc_loud.len());

        let dist = dtw_distance(&mfcc_quiet, &mfcc_loud);
        assert!(
            dist < 0.15,
            "DTW distance between -45 dBFS and -20 dBFS utterances should be small due to normalisation, got {}",
            dist
        );
    }
}
