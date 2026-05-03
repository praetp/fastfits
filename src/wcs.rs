use std::f64::consts::PI;

pub struct WcsTransform {
    crpix1: f64,
    crpix2: f64,
    pub crval1: f64, // RA degrees
    pub crval2: f64, // Dec degrees
    cd: [[f64; 2]; 2],
    cd_inv: [[f64; 2]; 2],
    /// Pixel scale in degrees (sqrt(|det(CD)|)), available for external use
    pub pixel_scale_deg: f64,
}

pub struct GridLine {
    pub segments: Vec<Vec<(f64, f64)>>,
    pub label: String,
    pub label_pos: Option<(f64, f64)>,
}

impl WcsTransform {
    pub fn from_headers(headers: &[(String, String)]) -> Option<Self> {
        let get = |key: &str| -> Option<f64> {
            headers.iter()
                .find(|(k, _)| k.trim().eq_ignore_ascii_case(key))
                .and_then(|(_, v)| v.trim().parse::<f64>().ok())
        };
        let get_str = |key: &str| -> Option<String> {
            headers.iter()
                .find(|(k, _)| k.trim().eq_ignore_ascii_case(key))
                .map(|(_, v)| v.trim().to_ascii_uppercase())
        };

        // Require RA and Dec CTYPEs.
        let ctype1 = get_str("CTYPE1").unwrap_or_default();
        let ctype2 = get_str("CTYPE2").unwrap_or_default();
        if !ctype1.contains("RA") || !ctype2.contains("DEC") {
            return None;
        }

        let crpix1 = get("CRPIX1")?;
        let crpix2 = get("CRPIX2")?;
        let crval1 = get("CRVAL1")?;
        let crval2 = get("CRVAL2")?;

        // Build CD matrix — three equivalent representations accepted.
        let cd = if let (Some(cd11), Some(cd12), Some(cd21), Some(cd22)) =
            (get("CD1_1"), get("CD1_2"), get("CD2_1"), get("CD2_2"))
        {
            // CD matrix formalism: CDi_j in deg/pixel
            [[cd11, cd12], [cd21, cd22]]
        } else if let (Some(pc11), Some(pc12), Some(pc21), Some(pc22)) =
            (get("PC1_1"), get("PC1_2"), get("PC2_1"), get("PC2_2"))
        {
            // PC matrix formalism: CDi_j = CDELTi * PCi_j
            let cdelt1 = get("CDELT1")?;
            let cdelt2 = get("CDELT2")?;
            [
                [cdelt1 * pc11, cdelt1 * pc12],
                [cdelt2 * pc21, cdelt2 * pc22],
            ]
        } else {
            // Legacy CROTA2 formalism
            let cdelt1 = get("CDELT1")?;
            let cdelt2 = get("CDELT2")?;
            let theta = get("CROTA2").unwrap_or(0.0).to_radians();
            let cos_t = theta.cos();
            let sin_t = theta.sin();
            [
                [cdelt1 * cos_t, -cdelt2 * sin_t],
                [cdelt1 * sin_t,  cdelt2 * cos_t],
            ]
        };

        // Invert CD matrix.
        let det = cd[0][0] * cd[1][1] - cd[0][1] * cd[1][0];
        if det.abs() < 1e-30 {
            return None;
        }
        let cd_inv = [
            [ cd[1][1] / det, -cd[0][1] / det],
            [-cd[1][0] / det,  cd[0][0] / det],
        ];

        let pixel_scale_deg = det.abs().sqrt();

        Some(Self { crpix1, crpix2, crval1, crval2, cd, cd_inv, pixel_scale_deg })
    }

    /// Convert 0-indexed pixel (col, row) to (RA, Dec) in degrees.
    pub fn pixel_to_sky(&self, col: f64, row: f64) -> Option<(f64, f64)> {
        let di = col - (self.crpix1 - 1.0);
        let dj = row - (self.crpix2 - 1.0);
        let xi  = self.cd[0][0] * di + self.cd[0][1] * dj;
        let eta = self.cd[1][0] * di + self.cd[1][1] * dj;

        let phi   = xi.atan2(-eta);
        let r_deg = (xi * xi + eta * eta).sqrt();
        // theta = atan(180 / (PI * R_deg))
        let theta = (180.0 / (PI * r_deg)).atan();

        let delta0 = self.crval2.to_radians();
        let alpha0 = self.crval1.to_radians();

        let sin_d = theta.sin() * delta0.sin()
            - theta.cos() * delta0.cos() * phi.cos();
        let dec_r = sin_d.clamp(-1.0, 1.0).asin();

        let ra_r = alpha0 + (theta.cos() * phi.sin()).atan2(
            theta.sin() * delta0.cos() + theta.cos() * delta0.sin() * phi.cos()
        );

        let ra  = ra_r.to_degrees().rem_euclid(360.0);
        let dec = dec_r.to_degrees();
        Some((ra, dec))
    }

    /// Convert (RA, Dec) in degrees to 0-indexed pixel (col, row).
    pub fn sky_to_pixel(&self, ra_deg: f64, dec_deg: f64) -> Option<(f64, f64)> {
        let alpha0 = self.crval1.to_radians();
        let delta0 = self.crval2.to_radians();
        let ra_r  = ra_deg.to_radians();
        let dec_r = dec_deg.to_radians();

        let dphi = ra_r - alpha0;

        // Inverse TAN (Calabretta & Greisen 2002) with phi_p = PI (the TAN convention).
        // sin(theta) = sin(dec)*sin(delta0) + cos(dec)*cos(delta0)*cos(dphi)
        let sin_theta = dec_r.sin() * delta0.sin()
            + dec_r.cos() * delta0.cos() * dphi.cos();
        let theta = sin_theta.clamp(-1.0, 1.0).asin();

        // phi = phi_p + atan2(-cos(dec)*sin(dphi),
        //                      sin(dec)*cos(delta0) - cos(dec)*sin(delta0)*cos(dphi))
        let phi = PI + (-dec_r.cos() * dphi.sin()).atan2(
            dec_r.sin() * delta0.cos() - dec_r.cos() * delta0.sin() * dphi.cos()
        );

        if theta <= 0.0 { return None; }

        let r_deg = (180.0 / PI) / theta.tan();
        let xi  =  r_deg * phi.sin();
        let eta = -r_deg * phi.cos();

        let di = self.cd_inv[0][0] * xi + self.cd_inv[0][1] * eta;
        let dj = self.cd_inv[1][0] * xi + self.cd_inv[1][1] * eta;
        let col = di + (self.crpix1 - 1.0);
        let row = dj + (self.crpix2 - 1.0);

        Some((col, row))
    }

    /// Nudge CRVAL1/CRVAL2 so that all sky annotations shift by (dcol, drow) pixels
    /// on screen. The CD matrix encodes the image orientation, so the shift is
    /// rotation-aware: pressing "right" always moves annotations right regardless of
    /// CROTA2. Returns the updated (crval1, crval2) in degrees.
    pub fn apply_pixel_nudge(&mut self, dcol: f64, drow: f64) -> (f64, f64) {
        let dec_rad = self.crval2.to_radians();
        // To shift annotation at fixed sky (ra, dec) by (dcol, drow) pixels:
        // Δ(ra·cos(dec), dec) = -CD · (dcol, drow)
        let dra_cosdec = -(self.cd[0][0] * dcol + self.cd[0][1] * drow);
        let ddec       = -(self.cd[1][0] * dcol + self.cd[1][1] * drow);
        self.crval1 = (self.crval1 + dra_cosdec / dec_rad.cos()).rem_euclid(360.0);
        self.crval2 = (self.crval2 + ddec).clamp(-90.0, 90.0);
        (self.crval1, self.crval2)
    }

    /// Rotation angle (radians, CCW in screen-Y-down coords) to put North up.
    pub fn north_up_angle(&self) -> f64 {
        let nx = self.cd_inv[0][1];
        let ny = self.cd_inv[1][1];
        (-nx).atan2(-ny)
    }

    /// True if, after north_up_angle rotation, East would be to the right
    /// (requiring a horizontal mirror for East-left convention).
    pub fn east_needs_flip(&self) -> bool {
        let angle = self.north_up_angle() as f32;
        let ex = self.cd_inv[0][0] as f32;
        let ey = self.cd_inv[1][0] as f32;
        ex * angle.cos() - ey * angle.sin() > 0.0
    }

    /// Generate RA and Dec grid lines in pixel coordinates.
    pub fn grid_lines(
        &self,
        img_width: usize,
        img_height: usize,
        n_samples: usize,
    ) -> (Vec<GridLine>, Vec<GridLine>) {
        let w = img_width as f64;
        let h = img_height as f64;

        // Convert corners to sky to estimate RA/Dec range.
        let corners = [
            (0.0, 0.0), (w - 1.0, 0.0), (0.0, h - 1.0), (w - 1.0, h - 1.0),
            (w * 0.5, 0.0), (w * 0.5, h - 1.0), (0.0, h * 0.5), (w - 1.0, h * 0.5),
        ];
        let sky_pts: Vec<(f64, f64)> = corners.iter()
            .filter_map(|&(c, r)| self.pixel_to_sky(c, r))
            .collect();
        if sky_pts.is_empty() {
            return (vec![], vec![]);
        }

        let dec_min = sky_pts.iter().map(|p| p.1).fold(f64::INFINITY,    f64::min);
        let dec_max = sky_pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);

        // RA range — handle 0/360 wrap by using the center pixel as reference.
        let (ra_center, _) = self.pixel_to_sky(w * 0.5, h * 0.5).unwrap_or((180.0, 0.0));
        let ra_pts: Vec<f64> = sky_pts.iter().map(|p| {
            let mut d = p.0 - ra_center;
            if d > 180.0  { d -= 360.0; }
            if d < -180.0 { d += 360.0; }
            d
        }).collect();
        let ra_span_half = ra_pts.iter().map(|d| d.abs()).fold(0.0_f64, f64::max);
        let ra_span = (ra_span_half * 2.0).min(360.0);
        let dec_span = (dec_max - dec_min).abs();

        // Choose grid spacing: target ~5 lines across the shorter axis.
        let target_n: f64 = 5.0;
        let candidate_spacings = [
            1.0 / 3600.0, 2.0 / 3600.0, 5.0 / 3600.0,
            10.0 / 3600.0, 15.0 / 3600.0, 30.0 / 3600.0,
            1.0 / 60.0, 2.0 / 60.0, 5.0 / 60.0, 10.0 / 60.0, 15.0 / 60.0, 30.0 / 60.0,
            1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 45.0, 90.0,
        ];
        let span = ra_span.min(dec_span);
        let spacing = candidate_spacings.iter().copied()
            .find(|&s| span / s <= target_n * 2.0 && span / s >= 1.0)
            .unwrap_or(1.0);

        let margin = spacing * 0.5;

        // RA grid lines: lines of constant RA, varying Dec.
        let ra_start = ((ra_center - ra_span * 0.5 - margin) / spacing).floor() * spacing;
        let ra_end   = ra_center + ra_span * 0.5 + margin;
        let mut ra_lines: Vec<GridLine> = Vec::new();
        let mut ra_val = ra_start;
        while ra_val <= ra_end {
            let ra_norm = ra_val.rem_euclid(360.0);
            let pts = sample_line(n_samples, dec_min - margin, dec_max + margin, |dec| {
                self.sky_to_pixel(ra_norm, dec)
            });
            let segs = split_segments(pts, w * 0.05);
            let label = format_ra(ra_norm);
            let label_pos = first_near_edge(&segs, w, h);
            if !segs.is_empty() {
                ra_lines.push(GridLine { segments: segs, label, label_pos });
            }
            ra_val += spacing;
        }

        // Dec grid lines: lines of constant Dec, varying RA.
        let dec_start = ((dec_min - margin) / spacing).floor() * spacing;
        let dec_end   = dec_max + margin;
        let mut dec_lines: Vec<GridLine> = Vec::new();
        let mut dec_val = dec_start;
        while dec_val <= dec_end {
            let ra_lo = ra_center - ra_span * 0.5 - margin;
            let ra_hi = ra_center + ra_span * 0.5 + margin;
            let dec_fixed = dec_val;
            let pts = sample_line(n_samples, ra_lo, ra_hi, |ra| {
                self.sky_to_pixel(ra.rem_euclid(360.0), dec_fixed)
            });
            let segs = split_segments(pts, w * 0.05);
            let label = format_dec(dec_val);
            let label_pos = first_near_edge(&segs, w, h);
            if !segs.is_empty() {
                dec_lines.push(GridLine { segments: segs, label, label_pos });
            }
            dec_val += spacing;
        }

        (ra_lines, dec_lines)
    }
}

/// Sample a parametric line by varying `param` from `lo` to `hi` in `n` steps.
fn sample_line<F>(n: usize, lo: f64, hi: f64, f: F) -> Vec<Option<(f64, f64)>>
where F: Fn(f64) -> Option<(f64, f64)>
{
    (0..n).map(|i| {
        let t = lo + (hi - lo) * (i as f64) / ((n - 1) as f64);
        f(t)
    }).collect()
}

/// Split a series of optional pixel points into contiguous segments,
/// breaking at None values or when the step exceeds `max_gap`.
fn split_segments(pts: Vec<Option<(f64, f64)>>, max_gap: f64) -> Vec<Vec<(f64, f64)>> {
    let mut segments: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut current: Vec<(f64, f64)> = Vec::new();
    for opt in pts {
        if let Some(p) = opt {
            if let Some(&last) = current.last() {
                let dx = p.0 - last.0;
                let dy = p.1 - last.1;
                if (dx * dx + dy * dy).sqrt() > max_gap {
                    if current.len() >= 2 { segments.push(current.clone()); }
                    current.clear();
                }
            }
            current.push(p);
        } else {
            if current.len() >= 2 { segments.push(current.clone()); }
            current.clear();
        }
    }
    if current.len() >= 2 { segments.push(current); }
    segments
}

/// Pick a label position: first point of the first segment that lies inside
/// (or near) the image, within a 20px border region.
fn first_near_edge(segs: &[Vec<(f64, f64)>], w: f64, h: f64) -> Option<(f64, f64)> {
    let border = 20.0;
    for seg in segs {
        for &(col, row) in seg {
            if col >= -border && col <= w + border && row >= -border && row <= h + border {
                return Some((col, row));
            }
        }
    }
    None
}

/// Liang-Barsky clip of segment (p0→p1) to the axis-aligned box [0, w] × [0, h].
/// Returns the clipped endpoints, or None if the segment is entirely outside.
pub fn clip_segment_to_image(
    p0: (f64, f64), p1: (f64, f64),
    w: f64, h: f64,
) -> Option<((f64, f64), (f64, f64))> {
    let dx = p1.0 - p0.0;
    let dy = p1.1 - p0.1;
    let mut t0 = 0.0f64;
    let mut t1 = 1.0f64;

    // Each edge: p*t <= q, where p < 0 → entering, p > 0 → leaving.
    for (p, q) in [(-dx, p0.0), (dx, w - p0.0), (-dy, p0.1), (dy, h - p0.1)] {
        if p == 0.0 {
            if q < 0.0 { return None; } // parallel and outside
        } else {
            let t = q / p;
            if p < 0.0 { t0 = t0.max(t); } else { t1 = t1.min(t); }
            if t0 > t1 { return None; }
        }
    }

    Some((
        (p0.0 + t0 * dx, p0.1 + t0 * dy),
        (p0.0 + t1 * dx, p0.1 + t1 * dy),
    ))
}

/// Format RA degrees as HH:MM or HH:MM:SS depending on precision.
pub fn format_ra(deg: f64) -> String {
    let h_total = deg / 15.0;
    let h = h_total.floor() as u32 % 24;
    let m_total = (h_total - h_total.floor()) * 60.0;
    let m = m_total.floor() as u32;
    let s = (m_total - m_total.floor()) * 60.0;
    if s.round() == 0.0 {
        format!("{h:02}h{m:02}m")
    } else {
        format!("{h:02}h{m:02}m{:.0}s", s)
    }
}

/// Format Dec degrees as ±DD°MM' or ±DD°MM'SS".
pub fn format_dec(deg: f64) -> String {
    let sign = if deg < 0.0 { '-' } else { '+' };
    let d = deg.abs();
    let dd = d.floor() as u32;
    let m_total = (d - d.floor()) * 60.0;
    let mm = m_total.floor() as u32;
    let ss = (m_total - m_total.floor()) * 60.0;
    if ss.round() == 0.0 {
        format!("{sign}{dd:02}°{mm:02}'")
    } else {
        format!("{sign}{dd:02}°{mm:02}'{:.0}\"", ss)
    }
}
