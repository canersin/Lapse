//! Finds the loudest moments of a clip (shots, explosions, shouting) as suggested
//! markers for the trimmer.

use std::io::Read;
use std::path::Path;
use std::process::Stdio;

/// Loudness is measured over windows of this length.
const WINDOW_SECS: f64 = 0.25;
/// Two highlights are at least this far apart.
const MIN_GAP_SECS: f64 = 8.0;
const MAX_HIGHLIGHTS: usize = 8;

/// Loudness (dB) of the first audio track, one value per window.
fn loudness(path: &Path) -> anyhow::Result<Vec<f32>> {
    const RATE: usize = 8000;
    let mut child = crate::utils::ffmpeg_cmd()
        .args(["-loglevel", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-ac", "1", "-ar", "8000", "-f", "s16le", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut raw = Vec::new();
    child.stdout.take().map(|mut s| s.read_to_end(&mut raw));
    child.wait()?;
    let win = (RATE as f64 * WINDOW_SECS) as usize;
    Ok(raw
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
        .collect::<Vec<f32>>()
        .chunks(win)
        .map(|c| 10.0 * (c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32 + 1e-10).log10())
        .collect())
}

/// Times (s) of the loudest moments that stand out from the rest, in time order.
pub fn peaks(db: &[f32], window_secs: f64) -> Vec<f64> {
    if db.len() < 8 {
        return Vec::new();
    }
    let mut sorted = db.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    // Compared with the typical level, so a loud game doesn't count as one long highlight.
    let median = sorted[sorted.len() / 2];
    let threshold = median + 10.0;
    let mut order: Vec<usize> = (0..db.len()).filter(|&i| db[i] >= threshold).collect();
    order.sort_by(|&a, &b| db[b].total_cmp(&db[a]));
    let gap = (MIN_GAP_SECS / window_secs) as usize;
    let mut picked: Vec<usize> = Vec::new();
    for i in order {
        if picked.iter().all(|&p| p.abs_diff(i) >= gap) {
            picked.push(i);
            if picked.len() == MAX_HIGHLIGHTS {
                break;
            }
        }
    }
    picked.sort_unstable();
    picked.into_iter().map(|i| i as f64 * window_secs).collect()
}

pub fn find(path: &Path) -> anyhow::Result<Vec<f64>> {
    Ok(peaks(&loudness(path)?, WINDOW_SECS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_spaced_loud_moments() {
        let mut db = vec![-40.0f32; 400]; // 100 s of quiet game
        for (i, v) in [(40, -10.0), (41, -8.0), (200, -12.0), (300, -35.0)] {
            db[i] = v;
        }
        // 41 and 40 are one moment; 300 is not loud enough.
        assert_eq!(peaks(&db, 0.25), vec![10.25, 50.0]);
    }
}
