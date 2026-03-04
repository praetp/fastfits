use rayon::prelude::*;

use super::Stretch;

pub(super) const LUT_SIZE: usize = 4096;

pub(super) fn to_rgba_gray(
    plane: &[f32],
    stretch: Stretch,
    bitdepth_max: f32,
    clip_thresh: Option<f32>,
) -> Vec<u8> {
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
            if clip_thresh.map_or(false, |t| v >= t) {
                chunk[0] = 255; chunk[1] = 0; chunk[2] = 0;
                return;
            }
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
    clip_thresh: Option<f32>,
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
            if clip_thresh.map_or(false, |t| rv >= t || gv >= t || bv >= t) {
                chunk[0] = 255; chunk[1] = 0; chunk[2] = 0;
                return;
            }
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

/// Autostretch LUT following the PixInsight STF (Screen Transfer Function) approach.
///
/// Algorithm:
///   1. Build a parallel 4096-bin histogram.
///   2. Derive sky background via the histogram median (50th percentile).
///   3. Estimate noise σ from the one-sided spread (median − p16), which is
///      robust against stars and galaxy signal in the upper tail.
///   4. Black point c0 = median − 2.8 σ  (clips noise floor to near-black).
///   5. White point = 99.98th percentile  (clips hot pixels / saturation).
///   6. MTF midtone: place the sky median at TARGET_BG = 0.25 display brightness.
///   7. Scale is relative to [c0, white] — not the sensor ceiling — so the MTF
///      curve is optimised for the actual data range rather than an empty tail.
pub(super) fn autostretch_lut(
    data: &[f32],
    data_min: f32,
    data_max: f32,
    _bitdepth_max: f32,   // kept for API compatibility; no longer used
) -> Vec<u8> {
    const TARGET_BG: f32 = 0.25;    // sky background maps to this display level
    const HIGH_PCTILE: f64 = 0.9998; // white-point clip percentile
    const CLIP_SIGMA: f32  = 2.8;    // black point placed this many σ below sky
    const BINS: usize = 4096;

    let range = data_max - data_min;
    if range == 0.0 { return vec![128u8; LUT_SIZE]; }

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

    // Walk the histogram once to a target cumulative fraction → returns the
    // data value at that percentile.
    let pctile = |frac: f64| -> f32 {
        let target = ((count as f64 * frac).ceil() as u64).min(count);
        let mut cum = 0u64;
        for (i, &h) in hist.iter().enumerate() {
            cum += h;
            if cum >= target {
                return data_min + (i as f32 / (BINS - 1) as f32) * range;
            }
        }
        data_max
    };

    let median    = pctile(0.50);
    // One-sided sigma: uses only the lower half of the sky distribution,
    // which is uncontaminated by stars/galaxy signal in the upper tail.
    let sigma     = (median - pctile(0.16)).max(bin_width);
    let c0        = (median - CLIP_SIGMA * sigma).max(data_min);
    let white     = pctile(HIGH_PCTILE);
    // Scale spans [c0, white] — the actual usable signal range.
    let scale     = (white - c0).max(1.0);
    // Midtone anchor: place sky median at TARGET_BG.
    let x_mid     = ((median - c0) / scale).clamp(1e-9, 1.0 - 1e-9);
    let t         = TARGET_BG;
    let denom     = 2.0 * x_mid * t - t - x_mid;
    let m = if denom.abs() > 1e-9 {
        (x_mid * (t - 1.0) / denom).clamp(1e-9, 1.0 - 1e-9)
    } else { t };

    (0..LUT_SIZE)
        .map(|i| {
            let v = data_min + (i as f32 / (LUT_SIZE - 1) as f32) * range;
            if v <= c0    { return 0u8; }
            if v >= white { return 255u8; }
            let x = ((v - c0) / scale).clamp(0.0, 1.0);
            (mtf(x, m) * 255.0).round().clamp(0.0, 255.0) as u8
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
