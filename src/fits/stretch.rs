use rayon::prelude::*;

use super::Stretch;

pub(super) const LUT_SIZE: usize = 4096;

pub(super) fn to_rgba_gray(plane: &[f32], stretch: Stretch, bitdepth_max: f32) -> Vec<u8> {
    let (min, max) = data_min_max(plane);
    let lut = match stretch {
        Stretch::Linear => linear_lut(min, max),
        Stretch::AutoStretch => autostretch_lut(plane, min, max, bitdepth_max),
    };
    let scale = if max == min { 0.0 } else { (LUT_SIZE - 1) as f32 / (max - min) };
    let mut out = vec![255u8; plane.len() * 4];
    out.par_chunks_mut(4)
        .zip(plane.par_iter())
        .for_each(|(chunk, &v)| {
            let idx = (((v - min) * scale + 0.5) as usize).min(LUT_SIZE - 1);
            let px = lut[idx];
            chunk[0] = px;
            chunk[1] = px;
            chunk[2] = px;
            // chunk[3] = 255 already
        });
    out
}

pub(super) fn to_rgba_rgb(
    r: &[f32],
    g: &[f32],
    b: &[f32],
    stretch: Stretch,
    bitdepth_max: f32,
) -> Vec<u8> {
    let (rmin, rmax) = data_min_max(r);
    let (gmin, gmax) = data_min_max(g);
    let (bmin, bmax) = data_min_max(b);

    let (r_lut, g_lut, b_lut) = match stretch {
        Stretch::Linear => (
            linear_lut(rmin, rmax),
            linear_lut(gmin, gmax),
            linear_lut(bmin, bmax),
        ),
        Stretch::AutoStretch => {
            // Each channel's autostretch is independent: run R, G, B in parallel.
            std::thread::scope(|s| {
                let rh = s.spawn(|| autostretch_lut(r, rmin, rmax, bitdepth_max));
                let gh = s.spawn(|| autostretch_lut(g, gmin, gmax, bitdepth_max));
                let bh = s.spawn(|| autostretch_lut(b, bmin, bmax, bitdepth_max));
                (rh.join().unwrap(), gh.join().unwrap(), bh.join().unwrap())
            })
        }
    };

    let rscale = if rmax == rmin { 0.0 } else { (LUT_SIZE - 1) as f32 / (rmax - rmin) };
    let gscale = if gmax == gmin { 0.0 } else { (LUT_SIZE - 1) as f32 / (gmax - gmin) };
    let bscale = if bmax == bmin { 0.0 } else { (LUT_SIZE - 1) as f32 / (bmax - bmin) };

    let npix = r.len();
    let mut out = vec![255u8; npix * 4];
    out.par_chunks_mut(4)
        .zip(r.par_iter().zip(g.par_iter()).zip(b.par_iter()))
        .for_each(|(chunk, ((&rv, &gv), &bv))| {
            let ri = (((rv - rmin) * rscale + 0.5) as usize).min(LUT_SIZE - 1);
            let gi = (((gv - gmin) * gscale + 0.5) as usize).min(LUT_SIZE - 1);
            let bi = (((bv - bmin) * bscale + 0.5) as usize).min(LUT_SIZE - 1);
            chunk[0] = r_lut[ri];
            chunk[1] = g_lut[gi];
            chunk[2] = b_lut[bi];
            // chunk[3] = 255 already
        });
    out
}

pub(super) fn linear_lut(_min: f32, _max: f32) -> Vec<u8> {
    (0..LUT_SIZE)
        .map(|i| ((i as f32 / (LUT_SIZE - 1) as f32) * 255.0).round() as u8)
        .collect()
}

/// Autostretch LUT modelled after ASIFitsView / PixInsight STF behaviour.
///
/// Builds a single parallel histogram over the data, then derives:
/// - sky background (histogram mode in lower third → black point c0)
/// - midtone at c0 + 1.5 σ_noise (σ from left-side half-max of mode peak)
/// - white point at 99.98th percentile
/// Then computes the MTF parameter m and fills the LUT.
pub(super) fn autostretch_lut(
    data: &[f32],
    data_min: f32,
    data_max: f32,
    bitdepth_max: f32,
) -> Vec<u8> {
    const TARGET_BG: f32 = 0.20;
    const HIGH_PCTILE: f64 = 0.9998;
    const K_MIDTONE: f32 = 1.5;
    const BINS: usize = 4096;

    let range = data_max - data_min;
    if range == 0.0 { return vec![128u8; LUT_SIZE]; }
    let bd = if bitdepth_max > 0.0 { bitdepth_max } else { data_max };
    if bd == 0.0 { return vec![128u8; LUT_SIZE]; }

    let (hist, count) = data
        .par_iter()
        .filter(|v| v.is_finite())
        .fold(
            || (vec![0u64; BINS], 0u64),
            |(mut h, cnt), &v| {
                let bin = (((v - data_min) / range).clamp(0.0, 1.0) * (BINS - 1) as f32) as usize;
                h[bin.min(BINS - 1)] += 1;
                (h, cnt + 1)
            },
        )
        .reduce(
            || (vec![0u64; BINS], 0u64),
            |(mut ha, ca), (hb, cb)| {
                ha.iter_mut().zip(hb.iter()).for_each(|(a, b)| *a += b);
                (ha, ca + cb)
            },
        );

    if count == 0 { return vec![128u8; LUT_SIZE]; }

    let bin_width = range / (BINS - 1) as f32;
    let search_end = BINS / 3;
    let mode_bin = hist[..search_end]
        .iter()
        .enumerate()
        .max_by_key(|&(_, &c)| c)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let c0_abs = data_min + (mode_bin as f32 / (BINS - 1) as f32) * range;

    let sigma = {
        let half_count = hist[mode_bin] / 2;
        let left_half_bin = (0..mode_bin).rev().find(|&i| hist[i] <= half_count).unwrap_or(0);
        let sigma_bins = (mode_bin - left_half_bin) as f32 / 0.8326_f32;
        (sigma_bins * bin_width).max(bin_width)
    };
    let mid_abs = (c0_abs + K_MIDTONE * sigma).min(data_max);

    let white_abs = {
        let target = ((count as f64 * HIGH_PCTILE).ceil() as u64).min(count);
        let mut cumsum = 0u64;
        let mut frac = 1.0f32;
        for (i, &h) in hist.iter().enumerate() {
            cumsum += h;
            if cumsum >= target {
                frac = i as f32 / (BINS - 1) as f32;
                break;
            }
        }
        data_min + frac * range
    };

    let scale = (bd - c0_abs).max(1.0);
    let x_mid = ((mid_abs - c0_abs) / scale).clamp(1e-9, 1.0 - 1e-9);
    let t = TARGET_BG;
    let denom = 2.0 * x_mid * t - t - x_mid;
    let m = if denom.abs() > 1e-9 {
        (x_mid * (t - 1.0) / denom).clamp(1e-9, 1.0 - 1e-9)
    } else {
        t
    };

    (0..LUT_SIZE)
        .map(|i| {
            let v = data_min + (i as f32 / (LUT_SIZE - 1) as f32) * range;
            if v <= c0_abs    { return 0u8; }
            if v >= white_abs { return 255u8; }
            let x = ((v - c0_abs) / scale).clamp(0.0, 1.0);
            let y = mtf(x, m);
            (y * 255.0).round().clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// Midtone Transfer Function used by Siril/PixInsight.
/// Maps 0→0, m→0.5, 1→1 with a smooth S-ish curve.
fn mtf(x: f32, m: f32) -> f32 {
    if x <= 0.0 { return 0.0; }
    if x >= 1.0 { return 1.0; }
    if m <= 0.0 { return 0.0; }
    if m >= 1.0 { return 1.0; }
    let num = (m - 1.0) * x;
    let den = (2.0 * m - 1.0) * x - m;
    if den.abs() < 1e-9 { return 0.5; }
    (num / den).clamp(0.0, 1.0)
}

pub(super) fn data_min_max(data: &[f32]) -> (f32, f32) {
    let (min, max) = data
        .par_iter()
        .filter(|v| v.is_finite())
        .fold(
            || (f32::MAX, f32::MIN),
            |(mn, mx), &v| (mn.min(v), mx.max(v)),
        )
        .reduce(
            || (f32::MAX, f32::MIN),
            |(mn1, mx1), (mn2, mx2)| (mn1.min(mn2), mx1.max(mx2)),
        );
    if min > max { (0.0, 1.0) } else { (min, max) }
}
