use anyhow::Result;
use rayon::prelude::*;
use std::io::Cursor;

use super::DemosaicMode;

/// Detect the Bayer CFA pattern from FITS headers.
/// Returns None if no Bayer pattern is detected (grayscale image).
pub(super) fn detect_bayer_pattern(headers: &[(String, String)]) -> Option<bayer::CFA> {
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
pub(super) fn debayer_u16(
    raw: &[u16],
    width: usize,
    height: usize,
    cfa: bayer::CFA,
    demosaic: DemosaicMode,
) -> Result<Vec<f32>> {
    // Convert u16 slice to little-endian bytes for the bayer crate.
    let mut bytes = vec![0u8; raw.len() * 2];
    bytes
        .par_chunks_mut(2)
        .zip(raw.par_iter())
        .for_each(|(chunk, &v)| {
            let le = v.to_le_bytes();
            chunk[0] = le[0];
            chunk[1] = le[1];
        });

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
        let algo = match demosaic {
            DemosaicMode::Cubic    => bayer::Demosaic::Cubic,
            DemosaicMode::Bilinear => bayer::Demosaic::Linear,
        };
        bayer::run_demosaic(
            &mut Cursor::new(&bytes),
            bayer::BayerDepth::Depth16LE,
            cfa,
            algo,
            &mut dst,
        )
        .map_err(|e| anyhow::anyhow!("debayer error: {e:?}"))?;
    }

    // Convert interleaved RGB u16 → planar f32 in parallel.
    // rgb_buf layout: [R0_lo, R0_hi, G0_lo, G0_hi, B0_lo, B0_hi, R1_lo, ...]
    let mut data = vec![0f32; npix * 3];
    {
        let (r_out, rest) = data.split_at_mut(npix);
        let (g_out, b_out) = rest.split_at_mut(npix);
        r_out
            .par_iter_mut()
            .zip(g_out.par_iter_mut())
            .zip(b_out.par_iter_mut())
            .zip(rgb_buf.par_chunks(6))
            .for_each(|(((r, g), b), chunk)| {
                *r = u16::from_le_bytes([chunk[0], chunk[1]]) as f32;
                *g = u16::from_le_bytes([chunk[2], chunk[3]]) as f32;
                *b = u16::from_le_bytes([chunk[4], chunk[5]]) as f32;
            });
    }

    Ok(data)
}
