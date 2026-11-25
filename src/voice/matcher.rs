//! DTW matching, enrollment, and template persistence for voice trigger.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Stored voice trigger templates, baseline threshold, durations, and syllable counts.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct VoiceTemplates {
    pub threshold: f32,
    pub templates: Vec<Vec<Vec<f32>>>,
    #[serde(default)]
    pub durations_ms: Vec<usize>,
    #[serde(default)]
    pub syllable_counts: Vec<usize>,
}

impl VoiceTemplates {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let t: Self = serde_json::from_str(&content)?;
        Ok(t)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Computes the median duration of enrolled templates in milliseconds.
    pub fn median_duration_ms(&self) -> usize {
        let mut durs: Vec<usize> = if !self.durations_ms.is_empty() {
            self.durations_ms.clone()
        } else {
            self.templates.iter().map(|t| t.len() * 10).collect()
        };
        if durs.is_empty() {
            return 0;
        }
        durs.sort_unstable();
        durs[durs.len() / 2]
    }

    /// Computes the median syllable count of enrolled templates.
    pub fn median_syllables(&self) -> usize {
        let mut syls: Vec<usize> = if !self.syllable_counts.is_empty() {
            self.syllable_counts.clone()
        } else {
            self.templates.iter().map(|t| count_syllables(t)).collect()
        };
        if syls.is_empty() {
            return 1;
        }
        syls.sort_unstable();
        syls[syls.len() / 2]
    }
}

/// Euclidean distance between two MFCC feature vectors.
#[inline]
pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
    let mut sum = 0.0f32;
    for (&x, &y) in a.iter().zip(b.iter()) {
        let diff = x - y;
        sum += diff * diff;
    }
    sum.sqrt()
}

/// Dynamic Time Warping with Sakoe-Chiba band relative to the longer sequence,
/// fixed endpoints over the whole template, and normalisation by (template length + utterance length).
pub fn dtw_distance(seq_a: &[Vec<f32>], seq_b: &[Vec<f32>]) -> f32 {
    let n = seq_a.len();
    let m = seq_b.len();
    if n == 0 && m == 0 {
        return 0.0;
    }
    if n == 0 || m == 0 {
        return f32::INFINITY;
    }

    // Sakoe-Chiba band of 20% relative to the longer sequence.
    // The band must also cover the length difference, or the end cell is unreachable
    // and every take a bit longer or shorter than another scores infinity.
    let band = n.abs_diff(m) + ((0.20 * (n.max(m) as f32)).ceil() as usize).max(1);

    let mut dp = vec![vec![f32::INFINITY; m]; n];
    dp[0][0] = euclidean_distance(&seq_a[0], &seq_b[0]);

    for i in 0..n {
        for j in 0..m {
            if i == 0 && j == 0 {
                continue;
            }
            if i.abs_diff(j) > band {
                continue;
            }

            let cost = euclidean_distance(&seq_a[i], &seq_b[j]);
            let mut best_prev = f32::INFINITY;

            // Diagonal predecessor
            if i > 0 && j > 0 && dp[i - 1][j - 1] < best_prev {
                best_prev = dp[i - 1][j - 1];
            }

            // Vertical predecessor
            if i > 0 && dp[i - 1][j] < best_prev {
                best_prev = dp[i - 1][j];
            }

            // Horizontal predecessor
            if j > 0 {
                let h_cost = dp[i][j - 1];
                if h_cost < best_prev {
                    best_prev = h_cost;
                } else if (h_cost - best_prev).abs() < 1e-7 && i > j {
                    // Tie-breaker: transpose-symmetric choice
                    best_prev = h_cost;
                }
            }

            if best_prev.is_finite() {
                dp[i][j] = best_prev + cost;
            }
        }
    }

    let total_cost = dp[n - 1][m - 1];
    if total_cost.is_finite() {
        total_cost / ((n + m) as f32)
    } else {
        f32::INFINITY
    }
}

/// Computes the energy envelope from MFCC frames and counts energy peaks (syllable nuclei).
pub fn count_syllables(frames: &[Vec<f32>]) -> usize {
    if frames.len() < 3 {
        return frames.len().min(1);
    }

    // Use c0 (coefficient 11) which contains the log frame energy.
    let energy: Vec<f32> = frames.iter().map(|f| f[11]).collect();
    let min_e = energy.iter().cloned().fold(f32::INFINITY, f32::min);
    let max_e = energy.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let dynamic_range = max_e - min_e;

    if dynamic_range < 2.0 {
        return 1;
    }

    // Normalized envelope from 0.0 to 1.0
    let env: Vec<f32> = energy.iter().map(|&v| (v - min_e) / dynamic_range).collect();

    // Smooth envelope with 3-frame moving average (30 ms)
    let n = env.len();
    let mut smooth = vec![0.0f32; n];
    for i in 0..n {
        let start = i.saturating_sub(1);
        let end = (i + 2).min(n);
        let sum: f32 = env[start..end].iter().sum();
        smooth[i] = sum / ((end - start) as f32);
    }

    let peak_thresh = 0.55f32;
    let valley_thresh = 0.35f32;

    let mut peaks = 0usize;
    let mut in_peak = false;

    for &val in &smooth {
        if !in_peak {
            if val >= peak_thresh {
                peaks += 1;
                in_peak = true;
            }
        } else if val < valley_thresh {
            in_peak = false;
        }
    }

    peaks.max(1)
}

/// Error returned when enrollment validation fails.
#[derive(Debug, PartialEq, Clone)]
pub enum EnrollmentError {
    TooFewTakes,
    Outlier {
        take_index: usize,
        distance: f32,
        mean: f32,
    },
}

impl std::fmt::Display for EnrollmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooFewTakes => write!(f, "At least 3 recordings are required"),
            Self::Outlier { take_index, .. } => {
                write!(f, "Take {} is inconsistent with other takes", take_index + 1)
            }
        }
    }
}

impl std::error::Error for EnrollmentError {}

/// Enrolls 5-6 takes into templates and computes the baseline threshold.
pub fn enroll(templates: &[Vec<Vec<f32>>]) -> Result<VoiceTemplates, EnrollmentError> {
    let k = templates.len();
    if k < 3 {
        return Err(EnrollmentError::TooFewTakes);
    }

    let mut pairwise_dists = Vec::new();
    let mut dist_matrix = vec![vec![0.0f32; k]; k];
    let mut max_pairwise = 0.0f32;

    for i in 0..k {
        for j in (i + 1)..k {
            let d = dtw_distance(&templates[i], &templates[j]);
            dist_matrix[i][j] = d;
            dist_matrix[j][i] = d;
            pairwise_dists.push(d);
            if d > max_pairwise {
                max_pairwise = d;
            }
        }
    }

    let mean_dist = pairwise_dists.iter().sum::<f32>() / (pairwise_dists.len() as f32);

    // Reject enrollment if any take deviates significantly from others.
    for i in 0..k {
        let avg_dist = dist_matrix[i].iter().sum::<f32>() / ((k - 1) as f32);
        if avg_dist > mean_dist * 1.45 && avg_dist > 0.15 {
            return Err(EnrollmentError::Outlier {
                take_index: i,
                distance: avg_dist,
                mean: mean_dist,
            });
        }
    }

    // Leave-one-out distances: for each take i, compute distance against the remaining k-1 templates
    let mut loo_dists = Vec::with_capacity(k);
    for i in 0..k {
        let mut d_from_i: Vec<f32> = (0..k)
            .filter(|&j| j != i)
            .map(|j| dist_matrix[i][j])
            .collect();
        d_from_i.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let loo_dist = if d_from_i.len() >= 3 {
            d_from_i[1]
        } else {
            d_from_i[0]
        };
        loo_dists.push(loo_dist);
    }

    let loo_mean = loo_dists.iter().sum::<f32>() / (k as f32);
    let loo_var = loo_dists
        .iter()
        .map(|&d| {
            let diff = d - loo_mean;
            diff * diff
        })
        .sum::<f32>()
        / (k as f32);
    let loo_std = loo_var.sqrt();

    // Threshold: mean + 2*std of leave-one-out distances, capped at 1.05 * max pairwise distance
    // so one sloppy take cannot loosen it enough to accept ordinary speech.
    let threshold = (loo_mean + 2.0 * loo_std).min(1.05 * max_pairwise);
    if !threshold.is_finite() {
        return Err(EnrollmentError::TooFewTakes);
    }

    let durations_ms: Vec<usize> = templates.iter().map(|t| t.len() * 10).collect();
    let syllable_counts: Vec<usize> = templates.iter().map(|t| count_syllables(t)).collect();

    Ok(VoiceTemplates {
        threshold,
        templates: templates.to_vec(),
        durations_ms,
        syllable_counts,
    })
}

/// Result of matching an utterance against enrolled templates.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub matched: bool,
    pub distance: f32,
    pub threshold: f32,
    pub is_near_miss: bool,
    pub duration_ms: usize,
    pub median_duration_ms: usize,
    pub syllables: usize,
    pub median_syllables: usize,
    pub rejection_reason: Option<String>,
}

/// Matches an utterance against enrolled templates using duration gate, syllable count check, and 3-best median rule.
pub fn match_utterance(
    templates: &VoiceTemplates,
    utterance_mfcc: &[Vec<f32>],
    sensitivity: f32,
) -> MatchResult {
    let utt_duration_ms = utterance_mfcc.len() * 10;
    let med_duration_ms = templates.median_duration_ms();
    let utt_syllables = count_syllables(utterance_mfcc);
    let med_syllables = templates.median_syllables();
    let effective_threshold = templates.threshold * sensitivity;

    if templates.templates.is_empty() || utterance_mfcc.is_empty() {
        let reason = "empty templates or utterance".to_string();
        println!(
            "Voice: duration {} ms vs template {} ms | syllables {} vs template {} | DTW inf vs threshold {:.3} | Rejected: {}",
            utt_duration_ms, med_duration_ms, utt_syllables, med_syllables, effective_threshold, reason
        );
        return MatchResult {
            matched: false,
            distance: f32::INFINITY,
            threshold: effective_threshold,
            is_near_miss: false,
            duration_ms: utt_duration_ms,
            median_duration_ms: med_duration_ms,
            syllables: utt_syllables,
            median_syllables: med_syllables,
            rejection_reason: Some(reason),
        };
    }

    // 1. Duration gate: reject if voiced length is outside [0.7, 1.4] x median template length
    let min_dur = (med_duration_ms as f32 * 0.70).round() as usize;
    let max_dur = (med_duration_ms as f32 * 1.40).round() as usize;
    let duration_ok = utt_duration_ms >= min_dur && utt_duration_ms <= max_dur;

    // 2. Syllable/energy-envelope check: reject if syllable count differs by more than 1
    let syllable_diff = (utt_syllables as isize - med_syllables as isize).abs();
    let syllables_ok = syllable_diff <= 1;

    // 3. DTW distance with 3-best median rule
    let mut dists: Vec<f32> = templates
        .templates
        .iter()
        .map(|t| dtw_distance(t, utterance_mfcc))
        .collect();

    dists.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median_dist = if dists.len() >= 3 {
        dists[1] // Median of the 3 best distances
    } else {
        dists[0]
    };

    let distance_ok = median_dist < effective_threshold;

    let rejection_reason = if !duration_ok {
        Some(format!(
            "duration {} ms outside [{}-{}] ms ([0.7, 1.4]x template median {} ms)",
            utt_duration_ms, min_dur, max_dur, med_duration_ms
        ))
    } else if !syllables_ok {
        Some(format!(
            "syllable count {} differs from template median {} by > 1",
            utt_syllables, med_syllables
        ))
    } else if !distance_ok {
        Some(format!(
            "DTW distance {:.3} exceeds threshold {:.3}",
            median_dist, effective_threshold
        ))
    } else {
        None
    };

    let matched = duration_ok && syllables_ok && distance_ok;
    let is_near_miss = !matched && duration_ok && syllables_ok && median_dist < effective_threshold * 1.30;

    let reason_str = match &rejection_reason {
        Some(r) => format!(" | Rejected: {}", r),
        None => String::new(),
    };
    println!(
        "Voice: duration {} ms vs template {} ms | syllables {} vs template {} | DTW {:.3} vs threshold {:.3}{}",
        utt_duration_ms, med_duration_ms, utt_syllables, med_syllables, median_dist, effective_threshold, reason_str
    );

    MatchResult {
        matched,
        distance: median_dist,
        threshold: effective_threshold,
        is_near_miss,
        duration_ms: utt_duration_ms,
        median_duration_ms: med_duration_ms,
        syllables: utt_syllables,
        median_syllables: med_syllables,
        rejection_reason,
    }
}
