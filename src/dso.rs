use std::sync::OnceLock;
use crate::wcs::WcsTransform;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DsoType {
    Galaxy,
    OpenCluster,
    GlobularCluster,
    Nebula,
    PlanetaryNebula,
    Other,
}

impl DsoType {
    fn from_str(s: &str) -> Self {
        match s {
            "G" | "GGroup" | "GPair" | "GTrpl" => Self::Galaxy,
            "OCl" | "*Ass"                       => Self::OpenCluster,
            "GCl"                                => Self::GlobularCluster,
            "HII" | "EmN" | "RfN" | "SNR"
            | "Neb" | "Cl+N"                     => Self::Nebula,
            "PN"                                 => Self::PlanetaryNebula,
            _                                    => Self::Other,
        }
    }

    pub fn color(self) -> egui::Color32 {
        match self {
            Self::Galaxy          => egui::Color32::from_rgba_unmultiplied(255, 165,   0, 200),
            Self::OpenCluster     => egui::Color32::from_rgba_unmultiplied( 80, 200, 255, 200),
            Self::GlobularCluster => egui::Color32::from_rgba_unmultiplied(255, 255,  80, 200),
            Self::Nebula          => egui::Color32::from_rgba_unmultiplied(180,  80, 255, 200),
            Self::PlanetaryNebula => egui::Color32::from_rgba_unmultiplied( 80, 255, 160, 200),
            Self::Other           => egui::Color32::from_rgba_unmultiplied(180, 180, 180, 160),
        }
    }
}

pub struct DsoEntry {
    pub name:          String,
    pub ra:            f64,   // degrees [0, 360)
    pub dec:           f64,   // degrees [-90, +90]
    pub maj_ax_arcmin: f32,   // 0.0 if absent
    pub dso_type:      DsoType,
}

static CATALOGUE: OnceLock<Vec<DsoEntry>> = OnceLock::new();

pub fn catalogue() -> &'static [DsoEntry] {
    CATALOGUE.get_or_init(|| parse_catalogue(include_str!("../data/ngc.csv")))
}

fn parse_ra(s: &str) -> Option<f64> {
    // HH:MM:SS.ss
    let mut parts = s.trim().splitn(3, ':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let sec: f64 = parts.next()?.parse().ok()?;
    Some((h * 3600.0 + m * 60.0 + sec) / 3600.0 * 15.0)
}

fn parse_dec(s: &str) -> Option<f64> {
    // ±DD:MM:SS.s
    let s = s.trim();
    let (sign, rest) = if let Some(r) = s.strip_prefix('-') {
        (-1.0_f64, r)
    } else {
        let r = s.strip_prefix('+').unwrap_or(s);
        (1.0_f64, r)
    };
    let mut parts = rest.splitn(3, ':');
    let d: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let sec: f64 = parts.next()?.parse().ok()?;
    Some(sign * (d + m / 60.0 + sec / 3600.0))
}

fn parse_catalogue(csv: &str) -> Vec<DsoEntry> {
    let mut entries = Vec::with_capacity(14_000);
    let mut lines = csv.lines();
    lines.next(); // skip header

    for line in lines {
        let cols: Vec<&str> = line.splitn(32, ';').collect();
        if cols.len() < 6 { continue; }

        let type_str = cols[1].trim();
        if type_str == "NonEx" || type_str == "Dup" { continue; }

        let ra  = match parse_ra(cols[2])  { Some(v) => v, None => continue };
        let dec = match parse_dec(cols[3]) { Some(v) => v, None => continue };

        let maj_ax_arcmin: f32 = cols[5].trim().parse().unwrap_or(0.0);
        let dso_type = DsoType::from_str(type_str);

        // Prefer Messier label if available.
        let messier = cols.get(23).map(|s| s.trim()).unwrap_or("").to_string();
        let name = if !messier.is_empty() {
            let n: u32 = messier.trim_start_matches('0').parse().unwrap_or(0);
            format!("M {n}")
        } else {
            // Reformat "NGC0224" → "NGC 224", "IC0001" → "IC 1",
            // "NGC0061A" → "NGC 61A", "IC0080 NED01" → "IC 80 NED01".
            let raw = cols[0].trim();
            let reformat = |prefix: &str, rest: &str| {
                let digit_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
                let (digits, tail) = rest.split_at(digit_end);
                let n: u32 = digits.parse().unwrap_or(0);
                format!("{prefix} {n}{tail}")
            };
            if let Some(rest) = raw.strip_prefix("NGC") {
                reformat("NGC", rest)
            } else if let Some(rest) = raw.strip_prefix("IC") {
                reformat("IC", rest)
            } else {
                raw.to_string()
            }
        };

        entries.push(DsoEntry { name, ra, dec, maj_ax_arcmin, dso_type });
    }
    entries
}

/// Project every catalogue entry through the WCS; return those inside the image
/// (plus a `margin_px` border).  Returns `(entry, col_px, row_px)`.
pub fn visible_objects<'a>(
    entries: &'a [DsoEntry],
    wcs: &WcsTransform,
    img_w: usize,
    img_h: usize,
    margin_px: f64,
) -> Vec<(&'a DsoEntry, f64, f64)> {
    let w = img_w as f64;
    let h = img_h as f64;
    entries.iter().filter_map(|entry| {
        let (col, row) = wcs.sky_to_pixel(entry.ra, entry.dec)?;
        if col >= -margin_px && col <= w + margin_px
            && row >= -margin_px && row <= h + margin_px
        {
            Some((entry, col, row))
        } else {
            None
        }
    }).collect()
}
