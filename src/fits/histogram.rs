use rayon::prelude::*;

use super::FitsImage;
use super::stretch::data_min_max;

pub const HIST_BINS: usize = 256;

/// Per-channel histogram data with optional stretch marker positions.
#[derive(Clone)]
pub struct ChannelHistData {
    /// 256 bins covering the full [data_min, data_max] range of this channel.
    pub bins: Vec<u64>,
    /// Black point as fraction [0,1] of data range (None for Linear stretch).
    pub black_frac: Option<f32>,
    /// Midtone point as fraction [0,1] (None for Linear stretch).
    pub mid_frac: Option<f32>,
    /// White point as fraction [0,1] (None for Linear stretch).
    pub white_frac: Option<f32>,
}

/// Histogram for a loaded image — one entry per channel.
#[derive(Clone)]
pub struct HistogramData {
    /// 1 element for grayscale, 3 for RGB (R=0, G=1, B=2).
    pub channels: Vec<ChannelHistData>,
}

/// Build a 256-bin histogram for one channel plane.
/// If `with_markers` is true, derives black/mid/white fractions using the
/// same algorithm as `autostretch_lut`.
fn compute_channel_histogram(data: &[f32], with_markers: bool) -> ChannelHistData {
    let (data_min, data_max) = data_min_max(data);
    let range = data_max - data_min;

    let bins: Vec<u64> = if range == 0.0 {
        let mut b = vec![0u64; HIST_BINS];
        b[0] = data.len() as u64;
        b
    } else {
        data.par_iter()
            .filter(|v| v.is_finite())
            .fold(
                || vec![0u64; HIST_BINS],
                |mut h, &v| {
                    let bin = (((v - data_min) / range).clamp(0.0, 1.0)
                        * (HIST_BINS - 1) as f32) as usize;
                    h[bin.min(HIST_BINS - 1)] += 1;
                    h
                },
            )
            .reduce(
                || vec![0u64; HIST_BINS],
                |mut ha, hb| {
                    ha.iter_mut().zip(hb.iter()).for_each(|(a, b)| *a += b);
                    ha
                },
            )
    };

    if !with_markers || range == 0.0 {
        return ChannelHistData { bins, black_frac: None, mid_frac: None, white_frac: None };
    }

    const HIGH_PCTILE: f64 = 0.9998;
    const K_MIDTONE: f32 = 1.5;

    let count: u64 = bins.iter().sum();
    if count == 0 {
        return ChannelHistData { bins, black_frac: None, mid_frac: None, white_frac: None };
    }

    let bin_width = range / (HIST_BINS - 1) as f32;
    let search_end = HIST_BINS / 3;
    let mode_bin = bins[..search_end]
        .iter()
        .enumerate()
        .max_by_key(|&(_, &c)| c)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let c0_abs = data_min + (mode_bin as f32 / (HIST_BINS - 1) as f32) * range;

    let sigma = {
        let half_count = bins[mode_bin] / 2;
        let left_half_bin = (0..mode_bin).rev().find(|&i| bins[i] <= half_count).unwrap_or(0);
        let sigma_bins = (mode_bin - left_half_bin) as f32 / 0.8326_f32;
        (sigma_bins * bin_width).max(bin_width)
    };
    let mid_abs = (c0_abs + K_MIDTONE * sigma).min(data_max);

    let white_abs = {
        let target = ((count as f64 * HIGH_PCTILE).ceil() as u64).min(count);
        let mut cumsum = 0u64;
        let mut frac_val = 1.0f32;
        for (i, &h) in bins.iter().enumerate() {
            cumsum += h;
            if cumsum >= target {
                frac_val = i as f32 / (HIST_BINS - 1) as f32;
                break;
            }
        }
        data_min + frac_val * range
    };

    let to_frac = |v: f32| ((v - data_min) / range).clamp(0.0, 1.0);

    ChannelHistData {
        bins,
        black_frac: Some(to_frac(c0_abs)),
        mid_frac:   Some(to_frac(mid_abs)),
        white_frac: Some(to_frac(white_abs)),
    }
}

/// Compute a full histogram (1 or 3 channels) for a `FitsImage`.
/// `with_markers` should be true when the current stretch is `AutoStretch`.
pub fn compute_histogram(img: &FitsImage, with_markers: bool) -> HistogramData {
    let npix = img.width * img.height;

    let channels = if img.channels == 3 {
        let r = &img.data[0..npix];
        let g = &img.data[npix..2 * npix];
        let b = &img.data[2 * npix..3 * npix];
        std::thread::scope(|s| {
            let rh = s.spawn(|| compute_channel_histogram(r, with_markers));
            let gh = s.spawn(|| compute_channel_histogram(g, with_markers));
            let bh = s.spawn(|| compute_channel_histogram(b, with_markers));
            vec![rh.join().unwrap(), gh.join().unwrap(), bh.join().unwrap()]
        })
    } else {
        let plane = &img.data[..npix.min(img.data.len())];
        vec![compute_channel_histogram(plane, with_markers)]
    };

    HistogramData { channels }
}
