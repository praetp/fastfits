use std::collections::HashMap;

#[derive(Clone)]
pub struct SnEntry {
    pub name: String,
    pub ra: f64,  // degrees
    pub dec: f64, // degrees
}

// Cache key: (ra*10 as i32, dec*10 as i32, year*100+month)
type CacheKey = (i32, i32, u32);

#[derive(Default)]
pub struct SnCache {
    data: HashMap<CacheKey, Vec<SnEntry>>,
}

impl SnCache {
    fn key(ra: f64, dec: f64, date_obs: &str) -> CacheKey {
        let (year, month) = parse_year_month(date_obs);
        ((ra * 10.0).round() as i32, (dec * 10.0).round() as i32, year * 100 + month)
    }

    pub fn get(&self, ra: f64, dec: f64, date_obs: &str) -> Option<&Vec<SnEntry>> {
        self.data.get(&Self::key(ra, dec, date_obs))
    }

    pub fn insert(&mut self, ra: f64, dec: f64, date_obs: &str, entries: Vec<SnEntry>) {
        self.data.insert(Self::key(ra, dec, date_obs), entries);
    }
}

fn parse_year_month(date_obs: &str) -> (u32, u32) {
    let mut parts = date_obs.splitn(3, '-');
    let year: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let month: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (year, month)
}

/// Extract the discovery year from an IAU-style transient name.
/// "SN 2023ixf" → 2023, "PTF 10hv" → 2010, "ZTF18abcd" → 2018.
fn sn_year_from_name(name: &str) -> Option<i32> {
    if let Some(rest) = name.strip_prefix("SN ") {
        if rest.len() >= 4 {
            if let Ok(y) = rest[..4].parse::<i32>() { return Some(y); }
        }
    }
    if let Some(rest) = name.strip_prefix("ZTF") {
        if rest.len() >= 2 {
            if let Ok(yy) = rest[..2].parse::<i32>() { return Some(2000 + yy); }
        }
    }
    let ptf = name.strip_prefix("PTF ").or_else(|| name.strip_prefix("PTF"));
    if let Some(rest) = ptf {
        if rest.len() >= 2 {
            if let Ok(yy) = rest[..2].parse::<i32>() { return Some(2000 + yy); }
        }
    }
    None
}

fn parse_ra_hms(s: &str) -> Option<f64> {
    let mut p = s.trim().splitn(3, ':');
    let h: f64 = p.next()?.parse().ok()?;
    let m: f64 = p.next()?.parse().ok()?;
    let sec: f64 = p.next()?.parse().ok()?;
    Some((h + m / 60.0 + sec / 3600.0) * 15.0)
}

fn parse_dec_dms(s: &str) -> Option<f64> {
    let s = s.trim();
    let sign = if s.starts_with('-') { -1.0_f64 } else { 1.0 };
    let mut p = s.trim_start_matches(['+', '-']).splitn(3, ':');
    let d: f64 = p.next()?.parse().ok()?;
    let m: f64 = p.next()?.parse().ok()?;
    let sec: f64 = p.next()?.parse().ok()?;
    Some(sign * (d + m / 60.0 + sec / 3600.0))
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut in_quotes = false;
    let mut current = String::new();
    for ch in line.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => { fields.push(current.clone()); current.clear(); }
            _ => current.push(ch),
        }
    }
    fields.push(current);
    fields
}

/// Fetch classified supernovae from the Transient Name Server public search.
/// No API key required. Results are filtered to within 1 year before `obs_date`.
pub fn fetch_supernovae(
    ra: f64,
    dec: f64,
    radius_deg: f64,
    obs_date: &str,
) -> anyhow::Result<Vec<SnEntry>> {
    let radius_arcsec = (radius_deg * 3600.0).round() as u32;
    let url = format!(
        "https://www.wis-tns.org/search?format=csv\
         &ra={ra:.5}&decl={dec:.5}&radius={radius}&coords_unit=arcsec&classified_sne=1",
        ra = ra, dec = dec, radius = radius_arcsec,
    );

    let body = ureq::get(&url)
        .set("User-Agent", "Mozilla/5.0 (compatible; fastfits)")
        .timeout(std::time::Duration::from_secs(15))
        .call()?
        .into_string()?;

    let (obs_year, obs_month) = parse_year_month(obs_date);
    let obs_months = obs_year as i32 * 12 + obs_month as i32;

    let mut entries = Vec::new();
    for line in body.lines().skip(1) {
        if line.trim().is_empty() { continue; }
        let cols = parse_csv_line(line);
        if cols.len() < 4 { continue; }
        let name = cols[1].trim().to_string();
        if name.is_empty() { continue; }
        let Some(ra_val)  = parse_ra_hms(&cols[2])  else { continue };
        let Some(dec_val) = parse_dec_dms(&cols[3]) else { continue };

        if let Some(sn_year) = sn_year_from_name(&name) {
            let sn_months_latest = sn_year * 12 + 12;
            if obs_months - sn_months_latest > 12 { continue; }
        }

        entries.push(SnEntry { name, ra: ra_val, dec: dec_val });
    }

    Ok(entries)
}
