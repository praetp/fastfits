mod bayer;
pub mod headers;
mod histogram;
mod stretch;

pub use histogram::{HistogramData, compute_histogram};

use anyhow::{bail, Context, Result};
use fitsio::hdu::HduInfo;
#[allow(unused_imports)]
use fitsio::images::ReadImage;
use fitsio::FitsFile;
use fitsio_sys;
use std::path::Path;
use std::sync::Arc;

use bayer::{debayer_u16, detect_bayer_pattern};
use headers::read_headers;
use stretch::{data_min_max, to_rgba_gray, to_rgba_rgb};

/// Which channel to display.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ChannelView {
    /// Composite RGB (only meaningful when channels == 3)
    Rgb,
    /// Single channel index (0 = R or the only channel, 1 = G, 2 = B)
    Single(usize),
}

/// Stretch algorithm applied before display.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Stretch {
    Linear,
    AutoStretch,
}

/// Demosaic algorithm used when debayering a Bayer-pattern image.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DemosaicMode {
    Cubic,
    Bilinear,
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
    pub data: Arc<Vec<f32>>,
    /// FITS header key/value pairs from the image HDU.
    pub headers: Vec<(String, String)>,
    /// Full-scale maximum for the image's bit depth (e.g. 65535 for 16-bit).
    /// 0.0 means unknown / float data: autostretch falls back to data range.
    pub bitdepth_max: f32,
    /// True when the image was loaded via Bayer debayering.
    pub is_bayer: bool,
    /// Original single-channel sensor data before debayering (Bayer images only).
    pub raw_bayer: Option<Vec<f32>>,
}

impl FitsImage {
    /// Load the first image HDU that contains data from `path`.
    pub fn load(path: &Path, demosaic: DemosaicMode) -> Result<Self> {
        // CFITSIO calls C `fopen()` with the path as a byte string.  On Windows,
        // `fopen()` interprets bytes using the ANSI codepage (typically Windows-1252),
        // but Rust passes UTF-8 — so any non-ASCII character in the filename (e.g. `°`)
        // gets garbled and CFITSIO returns status 104 "could not open the named file".
        //
        // Workaround: if the direct open fails and the path has non-ASCII characters,
        // create a temp symlink with an ASCII-safe name and open that instead.
        // Symlinks are O(1) vs copying potentially large FITS files.  On Windows,
        // symlinks require admin or Developer Mode — fall back to copying if that fails.
        let temp_path: Option<std::path::PathBuf>;
        let mut fits = match FitsFile::open(path) {
            Ok(f) => { temp_path = None; f }
            Err(_) if !path.to_string_lossy().is_ascii() => {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("fits");
                let tmp = std::env::temp_dir().join(format!("fastfits_open.{ext}"));
                let _ = std::fs::remove_file(&tmp); // clean up any stale temp entry
                // Canonicalize to absolute path so that symlinks resolve correctly
                // regardless of which directory CFITSIO is invoked from.
                let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
                // Try symlink first (fast), fall back to copy (always works).
                let linked = {
                    #[cfg(unix)]
                    { std::os::unix::fs::symlink(&abs, &tmp).is_ok() }
                    #[cfg(windows)]
                    { std::os::windows::fs::symlink_file(&abs, &tmp).is_ok() }
                };
                if !linked {
                    std::fs::copy(path, &tmp)
                        .with_context(|| "copying to temp path for non-ASCII filename workaround")?;
                }
                let f = FitsFile::open(&tmp)
                    .with_context(|| format!("opening temp link/copy of {}", path.display()))?;
                temp_path = Some(tmp);
                f
            }
            Err(e) => return Err(e).with_context(|| format!("opening {}", path.display())),
        };

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

        // fitsio reverses shape to C order: [NAXIS3, NAXIS2, NAXIS1] for 3-D,
        // [NAXIS2, NAXIS1] for 2-D.
        let (width, height, naxis3, image_type) = match &hdu.info {
            HduInfo::ImageInfo { shape, image_type, .. } => match shape.len() {
                2 => (shape[1], shape[0], 1usize, *image_type),
                3 => (shape[2], shape[1], shape[0], *image_type),
                n => bail!("unsupported FITS image NAXIS={n}"),
            },
            _ => bail!("HDU {idx} is not an image"),
        };

        let file_headers = read_headers(path, idx)?;
        // Only attempt Bayer debayering for integer data.
        // Float/Double data cannot be meaningfully read as u16 and is never raw sensor data.
        // Note: for tile-compressed images, the FITS BITPIX header reflects the storage
        // format (binary table, BITPIX=8), not the image data type — so we use the
        // image_type reported by cfitsio rather than parsing the BITPIX keyword.
        use fitsio::images::ImageType;
        let is_integer_image = !matches!(image_type, ImageType::Float | ImageType::Double);
        let bayer_cfa = if naxis3 == 1 && is_integer_image {
            detect_bayer_pattern(&file_headers)
        } else {
            None
        };
        let is_bayer = bayer_cfa.is_some();

        let (channels, data, bitdepth_max, raw_bayer) = if let Some(cfa) = bayer_cfa {
            let hdu = fits.hdu(idx)?;
            let raw_u16: Vec<u16> = hdu.read_image(&mut fits)?;
            let raw_f32: Vec<f32> = raw_u16.iter().map(|&v| v as f32).collect();
            let debayered = debayer_u16(&raw_u16, width, height, cfa, demosaic)?;
            (3usize, debayered, 65535.0f32, Some(raw_f32))
        } else {
            let hdu = fits.hdu(idx)?;
            let raw: Vec<f32> = hdu.read_image(&mut fits)?;
            // Use image_type from cfitsio rather than the raw BITPIX header keyword.
            // For tile-compressed images the FITS BITPIX reflects the binary table
            // storage (always 8), not the actual image data type; cfitsio corrects this.
            let bd_max = match image_type {
                ImageType::UnsignedByte                              => 255.0f32,
                ImageType::Short | ImageType::UnsignedShort         => 65535.0f32,
                ImageType::Long  | ImageType::UnsignedLong
                    | ImageType::LongLong                           => 65535.0f32,
                ImageType::Float | ImageType::Double | ImageType::Byte => 0.0f32,
            };
            (naxis3, raw, bd_max, None)
        };

        // Clean up temp symlink/copy (if any) now that all data has been read.
        if let Some(tmp) = temp_path {
            let _ = std::fs::remove_file(&tmp);
        }

        Ok(FitsImage { width, height, channels, data: Arc::new(data), headers: file_headers, bitdepth_max, is_bayer, raw_bayer })
    }

    /// Build an RGBA byte buffer from the raw single-channel Bayer data (no debayering).
    /// Falls back to the first channel of `data` if `raw_bayer` is absent.
    pub fn to_rgba_raw(&self, stretch: Stretch, show_clipping: bool) -> Vec<u8> {
        let npix = self.width * self.height;
        let plane = self.raw_bayer.as_deref()
            .unwrap_or(&self.data[..npix.min(self.data.len())]);
        // Some cameras saturate a few ADU below the theoretical maximum; use 98 % of
        // bitdepth_max so those pixels are still highlighted as clipped.
        let clip_thresh = show_clipping.then_some(self.bitdepth_max * 0.98).filter(|&v| v > 0.0);
        to_rgba_gray(plane, stretch, self.bitdepth_max, clip_thresh)
    }

    /// Build an RGBA byte buffer for display, applying `stretch` and showing `view`.
    /// Returns `width * height * 4` bytes in RGBA order (top-left origin).
    /// When `show_clipping` is true, pixels at the sensor ceiling are rendered red.
    pub fn to_rgba(&self, stretch: Stretch, view: ChannelView, show_clipping: bool) -> Vec<u8> {
        let npix = self.width * self.height;
        let bd = self.bitdepth_max;
        // Clip threshold: 98 % of sensor ceiling for integer data (cameras often saturate
        // a few ADU below the theoretical maximum); data max for float data.
        let clip_thresh = if show_clipping {
            if self.bitdepth_max > 0.0 {
                Some(self.bitdepth_max * 0.98)
            } else {
                // Float data: use the actual data maximum as the ceiling.
                let plane = match view {
                    ChannelView::Rgb => &self.data[..npix.min(self.data.len())],
                    ChannelView::Single(c) => {
                        let c = c.min(self.channels.saturating_sub(1));
                        let offset = c * npix;
                        &self.data[offset..(offset + npix).min(self.data.len())]
                    }
                };
                let (_, max) = data_min_max(plane);
                Some(max)
            }
        } else {
            None
        };
        match (self.channels, view) {
            (1, _) => to_rgba_gray(&self.data[..npix], stretch, bd, clip_thresh),
            (_, ChannelView::Single(c)) => {
                let c = c.min(self.channels - 1);
                let offset = c * npix;
                to_rgba_gray(&self.data[offset..offset + npix], stretch, bd, clip_thresh)
            }
            (3, ChannelView::Rgb) => {
                let r = &self.data[0..npix];
                let g = &self.data[npix..2 * npix];
                let b = &self.data[2 * npix..3 * npix];
                to_rgba_rgb(r, g, b, stretch, bd, clip_thresh)
            }
            _ => to_rgba_gray(&self.data[..npix.min(self.data.len())], stretch, bd, clip_thresh),
        }
    }
}

/// Write corrected CRVAL1/CRVAL2 back to the FITS file's image HDU in-place.
/// Uses cfitsio's `fits_update_key_dbl` (ffukyd) so existing keywords are
/// overwritten rather than duplicated. Also writes OCRVAL1/OCRVAL2 (original
/// plate-solve values) on the first call — skipped if those keywords already exist.
pub fn write_crval(
    path: &Path,
    orig_crval1: f64, orig_crval2: f64,
    new_crval1: f64,  new_crval2: f64,
) -> Result<()> {
    let mut fits = FitsFile::edit(path)
        .with_context(|| format!("opening {} for WCS update", path.display()))?;
    let hdu_count = fits.iter().count();
    for i in 0..hdu_count {
        let hdu = fits.hdu(i)?;
        if let HduInfo::ImageInfo { ref shape, .. } = hdu.info {
            if !shape.is_empty() && shape.iter().product::<usize>() > 0 {
                let fptr = unsafe { fits.as_raw() };
                // Preserve original plate-solve values on first correction only.
                if hdu.read_key::<f64>(&mut fits, "OCRVAL1").is_err() {
                    update_key_f64(fptr, "OCRVAL1", orig_crval1)?;
                    update_key_f64(fptr, "OCRVAL2", orig_crval2)?;
                }
                update_key_f64(fptr, "CRVAL1", new_crval1)?;
                update_key_f64(fptr, "CRVAL2", new_crval2)?;
                return Ok(());
            }
        }
    }
    anyhow::bail!("no image HDU found in {} to write WCS", path.display())
}

/// Restore CRVAL1/CRVAL2 to the given values (used to revert to OCRVAL values).
/// Does not touch OCRVAL1/OCRVAL2.
pub fn restore_crval(path: &Path, crval1: f64, crval2: f64) -> Result<()> {
    let mut fits = FitsFile::edit(path)
        .with_context(|| format!("opening {} for WCS restore", path.display()))?;
    let hdu_count = fits.iter().count();
    for i in 0..hdu_count {
        let hdu = fits.hdu(i)?;
        if let HduInfo::ImageInfo { ref shape, .. } = hdu.info {
            if !shape.is_empty() && shape.iter().product::<usize>() > 0 {
                let fptr = unsafe { fits.as_raw() };
                update_key_f64(fptr, "CRVAL1", crval1)?;
                update_key_f64(fptr, "CRVAL2", crval2)?;
                return Ok(());
            }
        }
    }
    anyhow::bail!("no image HDU found in {} to restore WCS", path.display())
}

/// Call cfitsio `fits_update_key_dbl` (ffukyd): updates the keyword in-place if it
/// already exists, or appends it if not — never creates a duplicate.
fn update_key_f64(fptr: *mut fitsio_sys::fitsfile, key: &str, value: f64) -> Result<()> {
    use std::ffi::CString;
    let c_key = CString::new(key)?;
    let mut status = 0i32;
    unsafe {
        fitsio_sys::ffukyd(fptr, c_key.as_ptr(), value, 15, std::ptr::null(), &mut status);
    }
    anyhow::ensure!(status == 0, "cfitsio ffukyd({key}) failed with status {status}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_non_ascii_filename() {
        let path = Path::new("testdata/sven/Boogeyman Nb-400mm-f5.0- 04-41-47 Ha -EAF 580- G100_OS50_ - Temp -5.00\u{00b0}C - exp300.00sec -0001-HG off-UM off.fits");
        if !path.exists() {
            // Skip in environments without testdata (e.g. CI without LFS).
            return;
        }
        let img = FitsImage::load(path, DemosaicMode::Bilinear)
            .expect("should load FITS file with non-ASCII characters in path");
        assert!(img.width > 0);
        assert!(img.height > 0);
    }
}

