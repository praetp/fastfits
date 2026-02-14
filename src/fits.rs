use anyhow::{bail, Context, Result};
use fitsio::hdu::HduInfo;
#[allow(unused_imports)]
use fitsio::images::ReadImage; // trait needed for hdu.read_image()
use fitsio::FitsFile;
use std::io::Cursor;
use std::path::Path;

/// Which channel to display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChannelView {
    /// Composite RGB (only meaningful when channels == 3)
    Rgb,
    /// Single channel index (0 = R or the only channel, 1 = G, 2 = B)
    Single(usize),
}

/// Stretch algorithm applied before display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stretch {
    Linear,
    AutoStretch,
}

/// Raw float pixel data loaded from one FITS image HDU.
///
/// Data layout: planar, `channels` planes each of `width * height` f32 values.
/// Index: `data[channel * width * height + row * width + col]`
pub struct FitsImage {
    pub width: usize,
    pub height: usize,
    /// 1 = grayscale, 3 = RGB (either debayered or pre-separated)
    pub channels: usize,
    /// Raw float pixels in planar order.
    pub data: Vec<f32>,
    /// FITS header key/value pairs from the image HDU.
    pub headers: Vec<(String, String)>,
    /// Full-scale maximum for the image's bit depth (e.g. 65535 for 16-bit, 255 for 8-bit).
    /// Used to anchor statistics in autostretch so sky normalisation matches Siril.
    /// 0.0 means unknown / float data: autostretch falls back to data range.
    pub bitdepth_max: f32,
}

impl FitsImage {
    /// Load the first image HDU that contains data from `path`.
    pub fn load(path: &Path) -> Result<Self> {
        let mut fits =
            FitsFile::open(path).with_context(|| format!("opening {}", path.display()))?;

        // Find first HDU with non-empty image data
        let hdu_count = fits.iter().count();
        let mut image_hdu_idx = None;
        for i in 0..hdu_count {
            let hdu = fits.hdu(i)?;
            if let HduInfo::ImageInfo { ref shape, .. } = hdu.info {
                if !shape.is_empty() && shape.iter().product::<usize>() > 0 {
                    image_hdu_idx = Some(i);
                    break;
                }
            }
        }
        let idx = image_hdu_idx.ok_or_else(|| anyhow::anyhow!("no image HDU found in file"))?;
        let hdu = fits.hdu(idx)?;

        // cfitsio reports shape in FITS axis order: [NAXIS1, NAXIS2, NAXIS3, ...]
        // NAXIS1 = fastest-varying (columns = width)
        // NAXIS2 = rows = height
        // NAXIS3 = planes / channels (if present)
        let (width, height, naxis3) = match &hdu.info {
            HduInfo::ImageInfo { shape, .. } => match shape.len() {
                2 => (shape[0], shape[1], 1usize),
                3 => (shape[0], shape[1], shape[2]),
                n => bail!("unsupported FITS image NAXIS={n}"),
            },
            _ => bail!("HDU {idx} is not an image"),
        };

        // Collect headers first (needed for Bayer detection)
        let headers = read_headers(path, idx)?;

        // Detect Bayer pattern for single-plane images
        let bayer_cfa = if naxis3 == 1 {
            detect_bayer_pattern(&headers)
        } else {
            None
        };

        let (channels, data, bitdepth_max) = if let Some(cfa) = bayer_cfa {
            // Debayer path: read as u16, run cubic demosaic, store as 3-channel f32.
            // u16 data is always [0, 65535].
            let hdu = fits.hdu(idx)?;
            let raw_u16: Vec<u16> = hdu.read_image(&mut fits)?;
            let debayered = debayer_u16(&raw_u16, width, height, cfa)?;
            (3usize, debayered, 65535.0f32)
        } else {
            // Standard path: read as f32 directly (cfitsio applies BSCALE/BZERO).
            let hdu = fits.hdu(idx)?;
            let raw: Vec<f32> = hdu.read_image(&mut fits)?;
            // Derive the bitdepth ceiling from the BITPIX header keyword.
            let bd_max = headers
                .iter()
                .find(|(k, _)| k == "BITPIX")
                .and_then(|(_, v)| v.trim().parse::<i32>().ok())
                .map(|bitpix| match bitpix {
                    8 => 255.0f32,
                    16 => 65535.0f32,
                    32 => 65535.0f32, // 32-bit int FITS are usually 16-bit data in disguise
                    _ => 0.0f32,      // float (BITPIX=-32/-64): 0 → fall back to data range
                })
                .unwrap_or(0.0f32);
            (naxis3, raw, bd_max)
        };

        Ok(FitsImage {
            width,
            height,
            channels,
            data,
            headers,
            bitdepth_max,
        })
    }

    /// Build an RGBA byte buffer for display, applying `stretch` and showing `view`.
    /// Returns `width * height * 4` bytes in RGBA order (top-left origin).
    pub fn to_rgba(&self, stretch: Stretch, view: ChannelView) -> Vec<u8> {
        let npix = self.width * self.height;
        let bd = self.bitdepth_max;

        let result = match (self.channels, view) {
            (1, _) => {
                let plane = &self.data[..npix];
                to_rgba_gray(plane, stretch, bd)
            }
            (_, ChannelView::Single(c)) => {
                let c = c.min(self.channels - 1);
                let offset = c * npix;
                let plane = &self.data[offset..offset + npix];
                to_rgba_gray(plane, stretch, bd)
            }
            (3, ChannelView::Rgb) => {
                let r = &self.data[0..npix];
                let g = &self.data[npix..2 * npix];
                let b = &self.data[2 * npix..3 * npix];
                to_rgba_rgb(r, g, b, stretch, bd)
            }
            _ => {
                // Fallback: show first plane as grayscale
                let plane = &self.data[..npix.min(self.data.len())];
                to_rgba_gray(plane, stretch, bd)
            }
        };
        result
    }
}

// ---------------------------------------------------------------------------
// Bayer / debayering
// ---------------------------------------------------------------------------

/// Detect the Bayer CFA pattern from FITS headers.
/// Returns None if no Bayer pattern is detected (grayscale image).
fn detect_bayer_pattern(headers: &[(String, String)]) -> Option<bayer::CFA> {
    // Check explicit BAYERPAT keyword first
    let pat = headers
        .iter()
        .find(|(k, _)| k == "BAYERPAT")
        .map(|(_, v)| v.trim().to_uppercase());

    match pat.as_deref() {
        Some("RGGB") => return Some(bayer::CFA::RGGB),
        Some("BGGR") => return Some(bayer::CFA::BGGR),
        Some("GRBG") => return Some(bayer::CFA::GRBG),
        Some("GBRG") => return Some(bayer::CFA::GBRG),
        _ => {}
    }

    // Check COLORTYP (used by some cameras)
    let colortyp = headers
        .iter()
        .find(|(k, _)| k == "COLORTYP")
        .map(|(_, v)| v.trim().to_uppercase());

    match colortyp.as_deref() {
        Some("RGGB") => return Some(bayer::CFA::RGGB),
        Some("BGGR") => return Some(bayer::CFA::BGGR),
        Some("GRBG") => return Some(bayer::CFA::GRBG),
        Some("GBRG") => return Some(bayer::CFA::GBRG),
        _ => {}
    }

    // Check INSTRUME for known colour cameras and assume RGGB as most common
    let instrume = headers
        .iter()
        .find(|(k, _)| k == "INSTRUME")
        .map(|(_, v)| v.trim().to_uppercase());

    // Only auto-assume Bayer for known colour sensors; do not guess for unknown instruments
    // to avoid accidentally debayering monochrome images.
    match instrume.as_deref() {
        Some(s) if s.contains("COLOR") || s.contains("COLOUR") || s.contains("OSC") => {
            Some(bayer::CFA::RGGB)
        }
        _ => None,
    }
}

/// Debayer a u16 single-plane image into three f32 planes (R, G, B).
/// Output is stored as planar f32: [R plane, G plane, B plane], values in [0, 65535].
fn debayer_u16(
    raw: &[u16],
    width: usize,
    height: usize,
    cfa: bayer::CFA,
) -> Result<Vec<f32>> {
    // Convert u16 slice to little-endian bytes for the bayer crate
    let mut bytes = Vec::with_capacity(raw.len() * 2);
    for &v in raw {
        bytes.extend_from_slice(&v.to_le_bytes());
    }

    // Output buffer: 3 bytes per pixel at 16-bit = 6 bytes/pixel
    let npix = width * height;
    let mut rgb_buf = vec![0u8; npix * 6];

    {
        let mut dst = bayer::RasterMut::new(
            width,
            height,
            bayer::RasterDepth::Depth16,
            &mut rgb_buf,
        );
        bayer::run_demosaic(
            &mut Cursor::new(&bytes),
            bayer::BayerDepth::Depth16LE,
            cfa,
            bayer::Demosaic::Cubic,
            &mut dst,
        )
        .map_err(|e| anyhow::anyhow!("debayer error: {e:?}"))?;
    }

    // Convert interleaved RGB u16 → planar f32
    // rgb_buf layout: [R0_lo, R0_hi, G0_lo, G0_hi, B0_lo, B0_hi, R1_lo, ...]
    let mut data = vec![0f32; npix * 3];
    for i in 0..npix {
        let base = i * 6;
        data[i]          = u16::from_le_bytes([rgb_buf[base],     rgb_buf[base + 1]]) as f32;
        data[npix + i]   = u16::from_le_bytes([rgb_buf[base + 2], rgb_buf[base + 3]]) as f32;
        data[2 * npix + i] = u16::from_le_bytes([rgb_buf[base + 4], rgb_buf[base + 5]]) as f32;
    }

    Ok(data)
}

// ---------------------------------------------------------------------------
// Stretch helpers
// ---------------------------------------------------------------------------

fn to_rgba_gray(plane: &[f32], stretch: Stretch, bitdepth_max: f32) -> Vec<u8> {
    let (min, max) = data_min_max(plane);
    let lut = match stretch {
        Stretch::Linear => linear_lut(min, max),
        Stretch::AutoStretch => autostretch_lut(plane, min, max, bitdepth_max),
    };
    // Pre-compute scale once: avoids a division per pixel inside the loop.
    let scale = if max == min { 0.0 } else { (LUT_SIZE - 1) as f32 / (max - min) };
    let mut out = vec![255u8; plane.len() * 4];
    for (i, &v) in plane.iter().enumerate() {
        let idx = (((v - min) * scale + 0.5) as usize).min(LUT_SIZE - 1);
        let g = lut[idx];
        out[i * 4]     = g;
        out[i * 4 + 1] = g;
        out[i * 4 + 2] = g;
        // [i*4+3] = 255 already
    }
    out
}

fn to_rgba_rgb(r: &[f32], g: &[f32], b: &[f32], stretch: Stretch, bitdepth_max: f32) -> Vec<u8> {
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
            // std::thread::scope keeps it dependency-free; each thread owns its
            // histogram allocation so there is no cache contention.
            std::thread::scope(|s| {
                let rh = s.spawn(|| autostretch_lut(r, rmin, rmax, bitdepth_max));
                let gh = s.spawn(|| autostretch_lut(g, gmin, gmax, bitdepth_max));
                let bh = s.spawn(|| autostretch_lut(b, bmin, bmax, bitdepth_max));
                (rh.join().unwrap(), gh.join().unwrap(), bh.join().unwrap())
            })
        }
    };

    // Pre-compute per-channel scale: avoids a division per pixel inside the loop.
    let rscale = if rmax == rmin { 0.0 } else { (LUT_SIZE - 1) as f32 / (rmax - rmin) };
    let gscale = if gmax == gmin { 0.0 } else { (LUT_SIZE - 1) as f32 / (gmax - gmin) };
    let bscale = if bmax == bmin { 0.0 } else { (LUT_SIZE - 1) as f32 / (bmax - bmin) };

    let npix = r.len();
    let mut out = vec![255u8; npix * 4];
    for i in 0..npix {
        let ri = (((r[i] - rmin) * rscale + 0.5) as usize).min(LUT_SIZE - 1);
        let gi = (((g[i] - gmin) * gscale + 0.5) as usize).min(LUT_SIZE - 1);
        let bi = (((b[i] - bmin) * bscale + 0.5) as usize).min(LUT_SIZE - 1);
        out[i * 4]     = r_lut[ri];
        out[i * 4 + 1] = g_lut[gi];
        out[i * 4 + 2] = b_lut[bi];
        // [i*4+3] = 255 already
    }
    out
}

// ---------------------------------------------------------------------------
// Stretch implementation
// ---------------------------------------------------------------------------

const LUT_SIZE: usize = 4096;


fn linear_lut(_min: f32, _max: f32) -> Vec<u8> {
    (0..LUT_SIZE)
        .map(|i| ((i as f32 / (LUT_SIZE - 1) as f32) * 255.0).round() as u8)
        .collect()
}

/// Siril-style MTF autostretch LUT.
///
/// `data_min` / `data_max` define the LUT's input range (actual pixel values).
/// `bitdepth_max` is the full-scale ceiling for the bit depth (e.g. 65535 for 16-bit).
/// Setting `bitdepth_max = 0.0` means "unknown / float data": fall back to data range.
///
/// Algorithm:
/// 1. Normalise pixel values to [0, 1] using the bitdepth ceiling.
/// 2. Clip outlier percentiles (p0.02 % low / p99.98 % high) to remove dead/hot pixels.
/// 3. Find the background median in bitdepth-normalised space.
/// 4. Compute MTF midpoint m so that MTF(median, m) = TARGET_BG exactly.
///    Formula: m = median*(T−1) / (2*median*T − T − median)
///    This guarantees a neutral sky for per-channel stretch: every channel's background
///    maps to the same output fraction regardless of raw ADU level.
fn autostretch_lut(data: &[f32], data_min: f32, data_max: f32, bitdepth_max: f32) -> Vec<u8> {
    const TARGET_BG: f32 = 0.10;
    const LOW_PCTILE: f64 = 0.0002;
    const HIGH_PCTILE: f64 = 0.9998;

    let range = data_max - data_min;
    if range == 0.0 {
        return vec![128u8; LUT_SIZE];
    }

    // Use bitdepth ceiling as the normalization reference.
    // For float FITS (bitdepth_max == 0), fall back to data range.
    let bd = if bitdepth_max > 0.0 { bitdepth_max } else { data_max };
    if bd == 0.0 {
        return vec![128u8; LUT_SIZE];
    }

    // Percentile clips in bitdepth-normalised [0, bd] space.
    let lo_bd = percentile_norm(data, 0.0, bd, LOW_PCTILE);
    let hi_bd = percentile_norm(data, 0.0, bd, HIGH_PCTILE);
    if hi_bd <= lo_bd {
        return vec![128u8; LUT_SIZE];
    }

    // Background median in bitdepth-normalised space.
    let eff_min = lo_bd * bd;
    let eff_max = hi_bd * bd;
    let (median_frac, _mad_frac) = median_mad_hist(data, eff_min, eff_max);
    let eff_span = (eff_max - eff_min) / bd;
    let x_bg = (lo_bd + median_frac * eff_span).clamp(1e-6, 1.0 - 1e-6);

    // MTF midpoint m such that MTF(x_bg, m) = TARGET_BG.
    // Derived by inverting MTF(x, m) = y for m:
    //   m = x*(y-1) / (2*x*y - y - x)
    let t = TARGET_BG;
    let denom = 2.0 * x_bg * t - t - x_bg;
    let m = if denom.abs() > 1e-9 {
        (x_bg * (t - 1.0) / denom).clamp(0.0, 1.0)
    } else {
        t
    };

    // Build the LUT.  Entry i corresponds to pixel value:
    //   v = data_min + (i / (LUT_SIZE-1)) * range
    (0..LUT_SIZE)
        .map(|i| {
            let v = data_min + (i as f32 / (LUT_SIZE - 1) as f32) * range;
            // Normalise v to bitdepth space [0, 1]
            let x = (v / bd).clamp(0.0, 1.0);

            // Clip black point (dead pixels / debayer border) and white point (hot pixels)
            if x <= lo_bd {
                return 0u8;
            }
            if x >= hi_bd {
                return 255u8;
            }

            // Apply MTF directly — no shadow-clip rescaling of the input
            let y = mtf(x, m);
            (y * 255.0).round().clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// Find the value at `pctile` (e.g. 0.9999) of `data`, returned as a fraction
/// of the [min, max] range (so 0.0 = min, 1.0 = max).
fn percentile_norm(data: &[f32], min: f32, max: f32, pctile: f64) -> f32 {
    const BINS: usize = 4096;
    let range = max - min;
    if range == 0.0 {
        return 1.0;
    }
    let mut hist = vec![0u64; BINS];
    let mut count = 0u64;
    for &v in data {
        if v.is_finite() {
            let bin = (((v - min) / range).clamp(0.0, 1.0) * (BINS - 1) as f32) as usize;
            hist[bin.min(BINS - 1)] += 1;
            count += 1;
        }
    }
    if count == 0 {
        return 1.0;
    }
    let target = ((count as f64 * pctile).ceil() as u64).min(count);
    let mut cumsum = 0u64;
    for (i, &h) in hist.iter().enumerate() {
        cumsum += h;
        if cumsum >= target {
            return i as f32 / (BINS - 1) as f32;
        }
    }
    1.0
}

/// Midtone Transfer Function used by Siril/PixInsight.
/// Maps 0→0, m→0.5, 1→1 with a smooth S-ish curve.
fn mtf(x: f32, m: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    if m <= 0.0 {
        return 0.0;
    }
    if m >= 1.0 {
        return 1.0;
    }
    // MTF(x, m) = (m − 1)·x / ((2m − 1)·x − m)
    let num = (m - 1.0) * x;
    let den = (2.0 * m - 1.0) * x - m;
    if den.abs() < 1e-9 {
        return 0.5;
    }
    (num / den).clamp(0.0, 1.0)
}

/// Estimate median and MAD of `data` normalised to [0,1] using a histogram.
/// Avoids sorting the full pixel array (important for 9 MP images).
fn median_mad_hist(data: &[f32], min: f32, max: f32) -> (f32, f32) {
    const BINS: usize = 4096;
    let range = max - min;
    if range == 0.0 {
        return (0.5, 0.0);
    }

    // Build histogram of normalised values
    let mut hist = vec![0u64; BINS];
    let mut count = 0u64;
    for &v in data {
        if v.is_finite() {
            let bin = (((v - min) / range).clamp(0.0, 1.0) * (BINS - 1) as f32) as usize;
            hist[bin.min(BINS - 1)] += 1;
            count += 1;
        }
    }
    if count == 0 {
        return (0.5, 0.0);
    }

    // Median: bin where cumulative count crosses count/2
    let half = (count + 1) / 2;
    let mut cumsum = 0u64;
    let mut median_bin = 0usize;
    for (i, &h) in hist.iter().enumerate() {
        cumsum += h;
        if cumsum >= half {
            median_bin = i;
            break;
        }
    }
    let median = median_bin as f32 / (BINS - 1) as f32;

    // MAD: histogram of |x_norm − median|, range [0, max_dev]
    let max_dev = median.max(1.0 - median).max(1e-9);
    let mut mad_hist = vec![0u64; BINS];
    for &v in data {
        if v.is_finite() {
            let norm = ((v - min) / range).clamp(0.0, 1.0);
            let dev = (norm - median).abs();
            let bin = ((dev / max_dev) * (BINS - 1) as f32) as usize;
            mad_hist[bin.min(BINS - 1)] += 1;
        }
    }

    cumsum = 0;
    let mut mad_bin = 0usize;
    for (i, &h) in mad_hist.iter().enumerate() {
        cumsum += h;
        if cumsum >= half {
            mad_bin = i;
            break;
        }
    }
    let mad = mad_bin as f32 / (BINS - 1) as f32 * max_dev;

    (median, mad)
}


fn data_min_max(data: &[f32]) -> (f32, f32) {
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    for &v in data {
        if v.is_finite() {
            if v < min { min = v; }
            if v > max { max = v; }
        }
    }
    if min > max { (0.0, 1.0) } else { (min, max) }
}

// ---------------------------------------------------------------------------
// Header reading
// ---------------------------------------------------------------------------

/// Read all header records from `hdu_idx` by parsing the raw FITS file.
///
/// FITS headers consist of 80-byte ASCII records packed into 2880-byte blocks.
/// Each record is `KEY     = value / comment` or a commentary card (COMMENT,
/// HISTORY, blank).  We skip structural/commentary cards and return the rest
/// sorted alphabetically by key name.
fn read_headers(fits_path: &Path, hdu_idx: usize) -> Result<Vec<(String, String)>> {
    use std::io::{BufReader, Read, Seek, SeekFrom};

    let file = std::fs::File::open(fits_path)
        .with_context(|| format!("opening {} for header read", fits_path.display()))?;
    let mut reader = BufReader::new(file);

    // Skip over preceding HDUs (each HDU = header blocks + data blocks).
    // For HDU 0 we start at byte 0.
    let mut block = [0u8; 2880];
    let mut hdus_seen = 0usize;

    loop {
        // --- Read header blocks for the current HDU ---
        let mut header_bytes: Vec<u8> = Vec::new();
        let mut found_end = false;
        while !found_end {
            reader.read_exact(&mut block)
                .context("reading FITS header block")?;
            header_bytes.extend_from_slice(&block);
            // Scan this block for an END record
            for rec in block.chunks_exact(80) {
                if rec.starts_with(b"END     ") || rec.starts_with(b"END\x20\x20") || rec == b"END                                                                             " {
                    found_end = true;
                    break;
                }
            }
        }

        if hdus_seen == hdu_idx {
            // Parse and return headers for this HDU
            let mut headers: Vec<(String, String)> = Vec::new();
            for rec in header_bytes.chunks_exact(80) {
                let card = std::str::from_utf8(rec).unwrap_or("").trim_end();
                if card.len() < 8 {
                    continue;
                }
                let key = card[..8].trim().to_string();
                // Skip structural/commentary records
                if key.is_empty()
                    || key == "COMMENT"
                    || key == "HISTORY"
                    || key == "END"
                    || key == "CONTINUE"
                {
                    continue;
                }
                // Value is after "= " at position 8–9 (if present)
                let value = if card.len() > 10 && &card[8..10] == "= " {
                    let val_str = strip_fits_comment(card[10..].trim()).trim();
                    // Strip surrounding FITS string quotes and inner trailing spaces
                    if val_str.starts_with('\'') && val_str.ends_with('\'') && val_str.len() >= 2 {
                        val_str[1..val_str.len() - 1]
                            .replace("''", "'")
                            .trim()
                            .to_string()
                    } else {
                        val_str.to_string()
                    }
                } else if card.len() > 8 {
                    card[8..].trim().to_string()
                } else {
                    String::new()
                };
                headers.push((key, value));
            }
            headers.sort_by(|a, b| a.0.cmp(&b.0));
            return Ok(headers);
        }

        hdus_seen += 1;

        // Skip the data blocks for this HDU.
        // Data size comes from NAXIS + NAXISn + BITPIX keywords.
        let bitpix = find_header_int(&header_bytes, "BITPIX").unwrap_or(8);
        let naxis = find_header_int(&header_bytes, "NAXIS").unwrap_or(0);
        let mut data_size: u64 = if naxis == 0 {
            0
        } else {
            let bits_per_element = bitpix.unsigned_abs() as u64;
            let mut npix: u64 = 1;
            for i in 1..=naxis {
                let key = format!("NAXIS{i}");
                npix *= find_header_int(&header_bytes, &key).unwrap_or(0).max(0) as u64;
            }
            (npix * bits_per_element + 7) / 8
        };
        // Round up to next 2880-byte boundary
        if data_size % 2880 != 0 {
            data_size += 2880 - data_size % 2880;
        }
        if data_size > 0 {
            reader.seek(SeekFrom::Current(data_size as i64))
                .context("seeking past FITS data block")?;
        }
    }
}

/// Remove the ` / comment` part from a FITS value field, respecting quoted strings.
fn strip_fits_comment(s: &str) -> &str {
    let s = s.trim();
    if s.starts_with('\'') {
        // Quoted string — find closing quote (doubled quotes are escaped)
        let mut i = 1;
        let bytes = s.as_bytes();
        while i < bytes.len() {
            if bytes[i] == b'\'' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    i += 2; // escaped quote
                } else {
                    // closing quote found — everything up to and including it is the value
                    return &s[..=i];
                }
            } else {
                i += 1;
            }
        }
        s // no closing quote found, return as-is
    } else {
        // Unquoted — split at first ' / '
        if let Some(pos) = s.find(" / ") {
            s[..pos].trim_end()
        } else {
            s
        }
    }
}

/// Extract an integer value from raw 80-byte FITS header records by keyword name.
fn find_header_int(header_bytes: &[u8], key: &str) -> Option<i64> {
    let key_padded = format!("{key:<8}");
    for rec in header_bytes.chunks_exact(80) {
        if rec.starts_with(key_padded.as_bytes()) {
            let card = std::str::from_utf8(rec).ok()?;
            if card.len() > 10 && &card[8..10] == "= " {
                let val = strip_fits_comment(card[10..].trim());
                return val.trim().parse::<i64>().ok();
            }
        }
    }
    None
}
