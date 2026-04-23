//! Atmospheric seeing estimation via stellar FWHM measurement.
//!
//! Detects point sources (stars), measures their PSF width via a half-maximum
//! profile walk, applies outlier rejection, and returns the median FWHM with a
//! MAD-based uncertainty — all without matrix algebra or fitting libraries.

#[derive(Clone)]
pub struct SeeingResult {
    pub fwhm_px: f32,
    pub error_px: f32,              // MAD × 1.4826
    pub fwhm_arcsec: Option<f32>,   // None if no WCS pixel scale available
    pub error_arcsec: Option<f32>,
    pub star_count: usize,
    /// Median axis ratio min(h,v)/max(h,v) across accepted stars; 1.0 = perfect circle.
    pub roundness: f32,
}

const MARGIN: usize = 21;           // max(10 border, 20 profile_half + 1)
const MAX_STARS: usize = 100;
const SAT_FRAC: f32 = 0.90;  // reject stars within 10 % of sensor ceiling
const PROFILE_HALF: usize = 20;     // half-width of the extracted profile
const MIN_FWHM: f32 = 1.5;
const MAX_FWHM: f32 = 15.0;  // compact nebula knots / blended sources (15 px ≈ 46″ at 3″/px)
const MIN_STARS: usize = 3;

/// Estimate atmospheric seeing for an image.
///
/// * `data`               — planar float pixels, all channels packed: \[ch0\]\[ch1\]…
/// * `width` / `height`   — image dimensions in pixels
/// * `channels`           — number of colour planes
/// * `bitdepth_max`       — maximum representable value (e.g. 65535.0 for 16-bit)
/// * `pixel_scale_arcsec` — arcsec/pixel from WCS, or `None`
///
/// Returns `None` if fewer than `MIN_STARS` usable stars were found.
pub fn estimate_seeing(
    data: &[f32],
    width: usize,
    height: usize,
    channels: usize,
    bitdepth_max: f32,
    pixel_scale_arcsec: Option<f64>,
) -> Option<SeeingResult> {
    let npix = width * height;
    if npix == 0 || width < 2 * MARGIN + 1 || height < 2 * MARGIN + 1 {
        return None;
    }

    // Choose the luminance plane: green channel for RGB, otherwise channel 0.
    let plane: &[f32] = if channels >= 3 {
        &data[npix..2 * npix]   // green
    } else {
        &data[..npix]
    };

    let (background, noise) = background_noise(plane);
    let threshold = background + 8.0 * noise;
    if noise <= 0.0 { return None; }

    // Saturation limit — guard against float data where bitdepth_max is 1.0.
    let data_max = plane.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let sat_limit = (bitdepth_max * SAT_FRAC).max(data_max * SAT_FRAC);

    let stars = find_local_maxima(plane, width, height, threshold, sat_limit);
    if stars.is_empty() { return None; }

    let measurements: Vec<(f32, f32)> = stars.iter()
        .filter_map(|&(cx, cy)| measure_fwhm(plane, width, height, cx, cy))
        .filter(|&(f, _)| (MIN_FWHM..=MAX_FWHM).contains(&f))
        .collect();

    if measurements.len() < MIN_STARS { return None; }

    let fwhms: Vec<f32> = measurements.iter().map(|&(f, _)| f).collect();
    let roundnesses: Vec<f32> = measurements.iter().map(|&(_, r)| r).collect();

    let fwhm_px = median(&fwhms);
    let error_px = mad(&fwhms, fwhm_px) * 1.4826;
    let roundness = median(&roundnesses);

    let (fwhm_arcsec, error_arcsec) = if let Some(scale) = pixel_scale_arcsec {
        let s = scale as f32;
        (Some(fwhm_px * s), Some(error_px * s))
    } else {
        (None, None)
    };

    Some(SeeingResult {
        fwhm_px,
        error_px,
        fwhm_arcsec,
        error_arcsec,
        star_count: fwhms.len(),
        roundness,
    })
}

// ── Background / noise estimation ────────────────────────────────────────────

/// Return (background, noise) from a 256-bin histogram of the pixel plane.
///
/// * background = 50th-percentile (median)
/// * noise      = (84th − 16th percentile) / 2  (robust 1-sigma)
fn background_noise(plane: &[f32]) -> (f32, f32) {
    let n = plane.len();
    if n == 0 { return (0.0, 0.0); }

    let min = plane.iter().copied().fold(f32::INFINITY,     f32::min);
    let max = plane.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if (max - min).abs() < f32::EPSILON { return (min, 0.0); }

    const BINS: usize = 4096;
    let mut hist = [0u64; BINS];
    let scale = (BINS - 1) as f32 / (max - min);
    for &v in plane {
        let b = ((v - min) * scale) as usize;
        hist[b.min(BINS - 1)] += 1;
    }

    let total = n as f64;
    let p16 = percentile_from_hist(&hist, min, max, total, 0.16);
    let p50 = percentile_from_hist(&hist, min, max, total, 0.50);
    let p84 = percentile_from_hist(&hist, min, max, total, 0.84);

    (p50, (p84 - p16) / 2.0)
}

fn percentile_from_hist(hist: &[u64], min: f32, max: f32, total: f64, frac: f64) -> f32 {
    let target = (frac * total) as u64;
    let mut cum = 0u64;
    let bin_width = (max - min) / hist.len() as f32;
    for (i, &count) in hist.iter().enumerate() {
        cum += count;
        if cum >= target {
            return min + (i as f32 + 0.5) * bin_width;
        }
    }
    max
}

// ── Star detection ────────────────────────────────────────────────────────────

/// Return up to MAX_STARS candidate star centres (col, row), sorted brightest first.
fn find_local_maxima(
    plane: &[f32],
    width: usize,
    height: usize,
    threshold: f32,
    sat_limit: f32,
) -> Vec<(usize, usize)> {
    let mut candidates: Vec<(f32, usize, usize)> = Vec::new();

    for row in MARGIN..height.saturating_sub(MARGIN) {
        for col in MARGIN..width.saturating_sub(MARGIN) {
            let v = plane[row * width + col];
            if v < threshold || v >= sat_limit { continue; }

            // Strict local maximum in 5×5 neighbourhood.
            let mut is_max = true;
            'outer: for dr in -2i32..=2 {
                for dc in -2i32..=2 {
                    if dr == 0 && dc == 0 { continue; }
                    let r2 = (row as i32 + dr) as usize;
                    let c2 = (col as i32 + dc) as usize;
                    if plane[r2 * width + c2] >= v { is_max = false; break 'outer; }
                }
            }
            if is_max {
                candidates.push((v, col, row));
            }
        }
    }

    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    candidates.truncate(MAX_STARS);
    candidates.into_iter().map(|(_, col, row)| (col, row)).collect()
}

// ── FWHM measurement ──────────────────────────────────────────────────────────

/// Measure the mean FWHM and axis ratio of the star at (cx, cy).
///
/// Returns `None` if the half-maximum crossing cannot be found, the star is
/// saturated / out-of-range, or the source is too elongated.
/// Returns `Some((mean_fwhm, roundness))` where roundness = min/max axis ratio.
fn measure_fwhm(
    plane: &[f32],
    width: usize,
    height: usize,
    cx: usize,
    cy: usize,
) -> Option<(f32, f32)> {
    let (fwhm_h, fwhm_v) = (
        profile_fwhm(plane, width, height, cx, cy, true)?,
        profile_fwhm(plane, width, height, cx, cy, false)?,
    );

    // Reject elongated sources (not a point-spread function).
    let mean = (fwhm_h + fwhm_v) * 0.5;
    if (fwhm_h - fwhm_v).abs() > 0.5 * mean { return None; }

    // Also reject if either axis alone is out of range.
    if !(MIN_FWHM..=MAX_FWHM).contains(&fwhm_h) { return None; }
    if !(MIN_FWHM..=MAX_FWHM).contains(&fwhm_v) { return None; }

    let roundness = fwhm_h.min(fwhm_v) / fwhm_h.max(fwhm_v);
    Some((mean, roundness))
}

/// Extract a 1-D profile of length `2 * PROFILE_HALF + 1`, background-subtract,
/// normalise to peak = 1.0, then walk outward from centre to find the FWHM.
fn profile_fwhm(
    plane: &[f32],
    width: usize,
    _height: usize,
    cx: usize,
    cy: usize,
    horizontal: bool,
) -> Option<f32> {
    let len = 2 * PROFILE_HALF + 1;
    let mut prof = [0.0f32; 2 * PROFILE_HALF + 1];

    for (i, slot) in prof.iter_mut().enumerate() {
        let offset = i as i32 - PROFILE_HALF as i32;
        let (col, row) = if horizontal {
            ((cx as i32 + offset) as usize, cy)
        } else {
            (cx, (cy as i32 + offset) as usize)
        };
        *slot = plane[row * width + col];
    }

    // Background: mean of the 3 outermost values at each end.
    let bg = (prof[0] + prof[1] + prof[2] + prof[len - 3] + prof[len - 2] + prof[len - 1]) / 6.0;
    for v in prof.iter_mut() { *v = (*v - bg).max(0.0); }

    let peak = prof[PROFILE_HALF];
    if peak <= 0.0 { return None; }
    let half = peak * 0.5;

    // Walk left from centre.
    let left_half = {
        let mut half_pos: Option<f32> = None;
        for i in (0..PROFILE_HALF).rev() {
            if prof[i] <= half {
                // Linear interpolation between i and i+1.
                let d = prof[i + 1] - prof[i];
                let frac = if d.abs() > 1e-6 { (half - prof[i]) / d } else { 0.5 };
                half_pos = Some(PROFILE_HALF as f32 - (i as f32 + frac));
                break;
            }
        }
        half_pos?
    };

    // Walk right from centre.
    let right_half = {
        let mut half_pos: Option<f32> = None;
        for i in (PROFILE_HALF + 1)..len {
            if prof[i] <= half {
                let d = prof[i] - prof[i - 1];
                let frac = if d.abs() > 1e-6 { (half - prof[i - 1]) / d } else { 0.5 };
                half_pos = Some((i as f32 - 1.0 + frac) - PROFILE_HALF as f32);
                break;
            }
        }
        half_pos?
    };

    Some(left_half + right_half)
}

// ── Statistics helpers ────────────────────────────────────────────────────────

fn median(values: &[f32]) -> f32 {
    let mut v = values.to_vec();
    v.sort_by(f32::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) * 0.5 }
}

fn mad(values: &[f32], med: f32) -> f32 {
    let devs: Vec<f32> = values.iter().map(|&x| (x - med).abs()).collect();
    median(&devs)
}
