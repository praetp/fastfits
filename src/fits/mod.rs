mod bayer;
mod headers;
mod histogram;
mod stretch;

pub use histogram::{HistogramData, compute_histogram};

use anyhow::{bail, Context, Result};
use fitsio::hdu::HduInfo;
#[allow(unused_imports)]
use fitsio::images::ReadImage;
use fitsio::FitsFile;
use std::path::Path;

use bayer::{debayer_u16, detect_bayer_pattern};
use headers::read_headers;
use stretch::{to_rgba_gray, to_rgba_rgb};

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
    pub data: Vec<f32>,
    /// FITS header key/value pairs from the image HDU.
    pub headers: Vec<(String, String)>,
    /// Full-scale maximum for the image's bit depth (e.g. 65535 for 16-bit).
    /// 0.0 means unknown / float data: autostretch falls back to data range.
    pub bitdepth_max: f32,
    /// True when the image was loaded via Bayer debayering.
    pub is_bayer: bool,
}

impl FitsImage {
    /// Load the first image HDU that contains data from `path`.
    pub fn load(path: &Path, demosaic: DemosaicMode) -> Result<Self> {
        let mut fits =
            FitsFile::open(path).with_context(|| format!("opening {}", path.display()))?;

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
        let (width, height, naxis3) = match &hdu.info {
            HduInfo::ImageInfo { shape, .. } => match shape.len() {
                2 => (shape[0], shape[1], 1usize),
                3 => (shape[0], shape[1], shape[2]),
                n => bail!("unsupported FITS image NAXIS={n}"),
            },
            _ => bail!("HDU {idx} is not an image"),
        };

        let file_headers = read_headers(path, idx)?;
        let bayer_cfa = if naxis3 == 1 { detect_bayer_pattern(&file_headers) } else { None };
        let is_bayer = bayer_cfa.is_some();

        let (channels, data, bitdepth_max) = if let Some(cfa) = bayer_cfa {
            let hdu = fits.hdu(idx)?;
            let raw_u16: Vec<u16> = hdu.read_image(&mut fits)?;
            let debayered = debayer_u16(&raw_u16, width, height, cfa, demosaic)?;
            (3usize, debayered, 65535.0f32)
        } else {
            let hdu = fits.hdu(idx)?;
            let raw: Vec<f32> = hdu.read_image(&mut fits)?;
            let bd_max = file_headers
                .iter()
                .find(|(k, _)| k == "BITPIX")
                .and_then(|(_, v)| v.trim().parse::<i32>().ok())
                .map(|bitpix| match bitpix {
                    8  => 255.0f32,
                    16 => 65535.0f32,
                    32 => 65535.0f32,
                    _  => 0.0f32,
                })
                .unwrap_or(0.0f32);
            (naxis3, raw, bd_max)
        };

        Ok(FitsImage { width, height, channels, data, headers: file_headers, bitdepth_max, is_bayer })
    }

    /// Build an RGBA byte buffer for display, applying `stretch` and showing `view`.
    /// Returns `width * height * 4` bytes in RGBA order (top-left origin).
    pub fn to_rgba(&self, stretch: Stretch, view: ChannelView) -> Vec<u8> {
        let npix = self.width * self.height;
        let bd = self.bitdepth_max;
        match (self.channels, view) {
            (1, _) => to_rgba_gray(&self.data[..npix], stretch, bd),
            (_, ChannelView::Single(c)) => {
                let c = c.min(self.channels - 1);
                let offset = c * npix;
                to_rgba_gray(&self.data[offset..offset + npix], stretch, bd)
            }
            (3, ChannelView::Rgb) => {
                let r = &self.data[0..npix];
                let g = &self.data[npix..2 * npix];
                let b = &self.data[2 * npix..3 * npix];
                to_rgba_rgb(r, g, b, stretch, bd)
            }
            _ => to_rgba_gray(&self.data[..npix.min(self.data.len())], stretch, bd),
        }
    }
}
