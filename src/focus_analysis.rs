use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use rayon::prelude::*;

use crate::fits::headers::read_headers;
use crate::fits::{FitsImage, DemosaicMode};
use crate::seeing::estimate_seeing;

const MIN_ROUNDNESS: f32 = 0.9;

pub struct FocusPoint {
    pub temp: f64,
    pub focuspos: f64,
    pub fwhm_px: f32,
    pub filename: String,
}

pub struct FocusAnalysisResult {
    pub points: Vec<FocusPoint>,
    pub n_scanned: usize,
    pub n_skipped_headers: usize,
    pub n_skipped_roundness: usize,
    pub n_skipped_no_stars: usize,
}

enum FileOutcome {
    Point(FocusPoint),
    SkippedHeaders,
    SkippedNoStars,
    SkippedRoundness,
}

pub fn run_focus_analysis(
    files: Vec<PathBuf>,
    demosaic: DemosaicMode,
    progress: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
    ctx: egui::Context,
) -> FocusAnalysisResult {
    let n_scanned = files.len();

    let outcomes: Vec<FileOutcome> = files.par_iter()
        .map(|path| {
            if cancel.load(Ordering::Relaxed) {
                return FileOutcome::SkippedHeaders;
            }
            let outcome = process_file(path, demosaic);
            progress.fetch_add(1, Ordering::Relaxed);
            ctx.request_repaint();
            outcome
        })
        .collect();

    let mut points = Vec::new();
    let mut n_skipped_headers = 0usize;
    let mut n_skipped_roundness = 0usize;
    let mut n_skipped_no_stars = 0usize;

    for outcome in outcomes {
        match outcome {
            FileOutcome::Point(p)      => points.push(p),
            FileOutcome::SkippedHeaders  => n_skipped_headers += 1,
            FileOutcome::SkippedNoStars  => n_skipped_no_stars += 1,
            FileOutcome::SkippedRoundness => n_skipped_roundness += 1,
        }
    }

    // Sort by temperature so the plot renders left-to-right.
    points.sort_by(|a, b| a.temp.total_cmp(&b.temp));

    FocusAnalysisResult {
        points,
        n_scanned,
        n_skipped_headers,
        n_skipped_roundness,
        n_skipped_no_stars,
    }
}

fn process_file(path: &PathBuf, demosaic: DemosaicMode) -> FileOutcome {
    // Fast header-only read first to avoid loading pixel data unnecessarily.
    let headers = match read_headers(path, 0) {
        Ok(h) => h,
        Err(_) => return FileOutcome::SkippedHeaders,
    };

    let find = |key: &str| -> Option<f64> {
        headers.iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.trim().parse::<f64>().ok())
    };

    let (Some(temp), Some(focuspos)) = (find("FOCUSTEM"), find("FOCUSPOS")) else {
        return FileOutcome::SkippedHeaders;
    };

    let img = match FitsImage::load(path, demosaic) {
        Ok(img) => img,
        Err(_) => return FileOutcome::SkippedNoStars,
    };

    let seeing = estimate_seeing(
        &img.data,
        img.width,
        img.height,
        img.channels,
        img.bitdepth_max,
        None,
    );

    let Some(result) = seeing else {
        return FileOutcome::SkippedNoStars;
    };

    if result.roundness < MIN_ROUNDNESS {
        return FileOutcome::SkippedRoundness;
    }

    let filename = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    FileOutcome::Point(FocusPoint { temp, focuspos, fwhm_px: result.fwhm_px, filename })
}

/// Weighted linear regression on (x, y, fwhm_px) tuples. Weight = 1/fwhm_px².
/// Returns (slope, intercept, r_squared).
pub fn weighted_regression(pts: &[(f64, f64, f32)]) -> (f64, f64, f64) {
    if pts.len() < 2 { return (0.0, 0.0, 0.0); }

    let mut s_w   = 0.0f64;
    let mut s_wx  = 0.0f64;
    let mut s_wy  = 0.0f64;
    let mut s_wxx = 0.0f64;
    let mut s_wxy = 0.0f64;

    for &(x, y, fwhm) in pts {
        let w = 1.0 / (fwhm as f64 * fwhm as f64);
        s_w   += w;
        s_wx  += w * x;
        s_wy  += w * y;
        s_wxx += w * x * x;
        s_wxy += w * x * y;
    }

    let denom = s_w * s_wxx - s_wx * s_wx;
    if denom.abs() < f64::EPSILON {
        return (0.0, 0.0, 0.0);
    }

    let slope     = (s_w * s_wxy - s_wx * s_wy) / denom;
    let intercept = (s_wy - slope * s_wx) / s_w;

    let mean_y = s_wy / s_w;
    let mut ss_res = 0.0f64;
    let mut ss_tot = 0.0f64;
    for &(x, y, fwhm) in pts {
        let w = 1.0 / (fwhm as f64 * fwhm as f64);
        let predicted = slope * x + intercept;
        ss_res += w * (y - predicted).powi(2);
        ss_tot += w * (y - mean_y).powi(2);
    }

    let r_squared = if ss_tot.abs() < f64::EPSILON { 0.0 } else { 1.0 - ss_res / ss_tot };

    (slope, intercept, r_squared)
}
