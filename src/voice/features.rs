//! Audio feature extraction: pre-emphasis, Hamming window, radix-2 FFT, mel filterbank, DCT MFCCs, and CMN.

use std::f32::consts::PI;

pub const SAMPLE_RATE: u32 = 16_000;
pub const FRAME_LEN: usize = 400; // 25 ms at 16 kHz
pub const HOP_LEN: usize = 160;   // 10 ms at 16 kHz
pub const FFT_LEN: usize = 512;
pub const NUM_MEL_FILTERS: usize = 26;
pub const NUM_MFCC_COEFFS: usize = 12; // 13 MFCCs minus c0

/// Complex number for radix-2 FFT.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Complex {
    pub re: f32,
    pub im: f32,
}

impl Complex {
    pub fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }

    pub fn zero() -> Self {
        Self { re: 0.0, im: 0.0 }
    }

    pub fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }

    pub fn sub(self, other: Self) -> Self {
        Self {
            re: self.re - other.re,
            im: self.im - other.im,
        }
    }

    pub fn mul(self, other: Self) -> Self {
        Self {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }

    pub fn norm_sq(self) -> f32 {
        self.re * self.re + self.im * self.im
    }
}

/// In-place radix-2 Cooley-Tukey FFT. Length of `buf` must be a power of 2.
pub fn fft_radix2(buf: &mut [Complex]) {
    let n = buf.len();
    assert!(n.is_power_of_two(), "FFT size must be a power of 2");

    let mut j = 0;
    for i in 0..n {
        if i < j {
            buf.swap(i, j);
        }
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
    }

    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let angle = -2.0 * PI / (len as f32);
        let w_step = Complex::new(angle.cos(), angle.sin());
        let mut i = 0;
        while i < n {
            let mut w = Complex::new(1.0, 0.0);
            for k in 0..half {
                let u = buf[i + k];
                let v = buf[i + k + half].mul(w);
                buf[i + k] = u.add(v);
                buf[i + k + half] = u.sub(v);
                w = w.mul(w_step);
            }
            i += len;
        }
        len <<= 1;
    }
}

/// Naive DFT for verification and tests.
#[cfg(test)]
pub fn dft_naive(input: &[Complex]) -> Vec<Complex> {
    let n = input.len();
    let mut out = vec![Complex::zero(); n];
    for k in 0..n {
        let mut sum = Complex::zero();
        for (t, x) in input.iter().enumerate() {
            let angle = -2.0 * PI * (k * t) as f32 / n as f32;
            let w = Complex::new(angle.cos(), angle.sin());
            sum = sum.add(x.mul(w));
        }
        out[k] = sum;
    }
    out
}

/// Linear resampler converting mono f32 from `src_rate` to 16 kHz.
pub fn resample_to_16k(input: &[f32], src_rate: u32) -> Vec<f32> {
    if src_rate == 0 || src_rate == SAMPLE_RATE || input.is_empty() {
        return input.to_vec();
    }
    let ratio = src_rate as f64 / SAMPLE_RATE as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        let s0 = input[idx];
        let s1 = if idx + 1 < input.len() { input[idx + 1] } else { s0 };
        out.push(s0 * (1.0 - frac) + s1 * frac);
    }
    out
}

/// Pre-emphasis filter: y[n] = x[n] - 0.97 * x[n-1].
pub fn pre_emphasis(signal: &[f32], alpha: f32) -> Vec<f32> {
    if signal.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(signal.len());
    out.push(signal[0]);
    for i in 1..signal.len() {
        out.push(signal[i] - alpha * signal[i - 1]);
    }
    out
}

/// Precomputed triangular Mel filterbank.
pub struct MelFilterbank {
    filters: Vec<Vec<(usize, f32)>>,
}

impl MelFilterbank {
    /// Builds 26 triangular mel filters covering 0 Hz to 8000 Hz for 512-point FFT.
    pub fn new() -> Self {
        let hz_to_mel = |hz: f32| 1127.01048 * (1.0 + hz / 700.0).ln();
        let mel_to_hz = |mel: f32| 700.0 * ((mel / 1127.01048).exp() - 1.0);

        let mel_min = hz_to_mel(0.0);
        let mel_max = hz_to_mel(SAMPLE_RATE as f32 / 2.0);
        let num_points = NUM_MEL_FILTERS + 2;
        let mel_step = (mel_max - mel_min) / ((NUM_MEL_FILTERS + 1) as f32);

        let mut bin_indices = Vec::with_capacity(num_points);
        for i in 0..num_points {
            let mel = mel_min + (i as f32) * mel_step;
            let hz = mel_to_hz(mel);
            let bin = ((FFT_LEN as f32) * hz / (SAMPLE_RATE as f32)).round() as usize;
            bin_indices.push(bin.min(FFT_LEN / 2));
        }

        let mut filters = Vec::with_capacity(NUM_MEL_FILTERS);
        for m in 0..NUM_MEL_FILTERS {
            let left = bin_indices[m];
            let center = bin_indices[m + 1];
            let right = bin_indices[m + 2];
            let mut filter = Vec::new();

            for k in left..=right {
                if k < center && center > left {
                    let weight = (k - left) as f32 / (center - left) as f32;
                    filter.push((k, weight));
                } else if k >= center && right > center {
                    let weight = (right - k) as f32 / (right - center) as f32;
                    filter.push((k, weight));
                }
            }
            filters.push(filter);
        }

        Self { filters }
    }

    /// Multiplies the power spectrum by the mel filters and returns 26 log energies.
    pub fn filter_energies(&self, power_spec: &[f32]) -> Vec<f32> {
        self.filters
            .iter()
            .map(|filter| {
                let mut sum = 0.0f32;
                for &(bin, weight) in filter {
                    if bin < power_spec.len() {
                        sum += power_spec[bin] * weight;
                    }
                }
                sum.max(1e-10).ln()
            })
            .collect()
    }
}

/// DCT-II transform converting 26 log mel energies into 11 MFCCs plus c0 (log energy), total 12.
pub fn dct_mfcc(log_energies: &[f32]) -> Vec<f32> {
    let m = log_energies.len();
    let mut mfcc = Vec::with_capacity(NUM_MFCC_COEFFS);
    for i in 1..NUM_MFCC_COEFFS {
        let mut sum = 0.0f32;
        for (j, &val) in log_energies.iter().enumerate() {
            let angle = PI * (i as f32) * (j as f32 + 0.5) / (m as f32);
            sum += val * angle.cos();
        }
        mfcc.push(sum);
    }
    // c0: average log mel energy (total frame energy)
    let c0 = log_energies.iter().sum::<f32>() / (m as f32);
    mfcc.push(c0);
    mfcc
}

/// Applies per-utterance Cepstral Mean Normalisation (CMN).
pub fn apply_cmn(frames: &mut [Vec<f32>]) {
    if frames.is_empty() {
        return;
    }
    let num_coeffs = frames[0].len();
    let num_frames = frames.len() as f32;

    let mut means = vec![0.0f32; num_coeffs];
    for frame in frames.iter() {
        for (k, &v) in frame.iter().enumerate() {
            means[k] += v;
        }
    }
    for m in &mut means {
        *m /= num_frames;
    }

    for frame in frames.iter_mut() {
        for (k, v) in frame.iter_mut().enumerate() {
            *v -= means[k];
        }
    }
}

/// Converts linear RMS/amplitude to dBFS (clamped to at least -140 dBFS).
#[inline]
pub fn linear_to_dbfs(linear: f32) -> f32 {
    20.0 * linear.max(1e-7).log10()
}

/// Converts dBFS to linear amplitude ratio.
#[inline]
pub fn dbfs_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// Applies automatic gain normalisation (RMS normalisation with peak ceiling)
/// so quiet and loud utterances produce consistent feature representations.
pub fn normalize_utterance(audio: &[f32]) -> Vec<f32> {
    if audio.is_empty() {
        return Vec::new();
    }
    let mut max_abs = 0.0f32;
    let mut sum_sq = 0.0f32;
    for &s in audio {
        let a = s.abs();
        if a > max_abs {
            max_abs = a;
        }
        sum_sq += s * s;
    }
    if max_abs < 1e-6 {
        return audio.to_vec();
    }
    let rms = (sum_sq / (audio.len() as f32)).sqrt();
    let target_rms = 0.1f32;
    let gain = (target_rms / rms.max(1e-6)).min(0.90 / max_abs);
    audio.iter().map(|&s| s * gain).collect()
}

/// Extracts normalised MFCC frames (each length 12) from a 16 kHz audio utterance.
pub fn extract_mfcc(audio: &[f32]) -> Vec<Vec<f32>> {
    if audio.len() < FRAME_LEN {
        return Vec::new();
    }

    let norm = normalize_utterance(audio);
    let preemph = pre_emphasis(&norm, 0.97);
    let mel_fb = MelFilterbank::new();

    let mut hamming = [0.0f32; FRAME_LEN];
    for n in 0..FRAME_LEN {
        hamming[n] = 0.54 - 0.46 * (2.0 * PI * (n as f32) / ((FRAME_LEN - 1) as f32)).cos();
    }

    let mut fft_buf = [Complex::zero(); FFT_LEN];
    let mut power_spec = [0.0f32; FFT_LEN / 2 + 1];
    let num_frames = (preemph.len() - FRAME_LEN) / HOP_LEN + 1;
    let mut mfcc_frames = Vec::with_capacity(num_frames);

    for frame_idx in 0..num_frames {
        let start = frame_idx * HOP_LEN;
        for n in 0..FRAME_LEN {
            fft_buf[n] = Complex::new(preemph[start + n] * hamming[n], 0.0);
        }
        for n in FRAME_LEN..FFT_LEN {
            fft_buf[n] = Complex::zero();
        }

        fft_radix2(&mut fft_buf);

        for k in 0..=FFT_LEN / 2 {
            power_spec[k] = fft_buf[k].norm_sq() / (FFT_LEN as f32);
        }

        let log_energies = mel_fb.filter_energies(&power_spec);
        let mfccs = dct_mfcc(&log_energies);
        mfcc_frames.push(mfccs);
    }

    apply_cmn(&mut mfcc_frames);
    mfcc_frames
}
