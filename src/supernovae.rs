use std::collections::HashMap;

#[derive(Clone)]
pub struct SnEntry {
    pub name: String, // SIMBAD main identifier (e.g. "SN 2023ixf")
    pub ra: f64,      // degrees
    pub dec: f64,     // degrees
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
/// Returns None for names that don't follow a known pattern.
fn sn_year_from_name(name: &str) -> Option<i32> {
    // Standard IAU: "SN YYYY..."
    if let Some(rest) = name.strip_prefix("SN ") {
        if rest.len() >= 4 {
            if let Ok(y) = rest[..4].parse::<i32>() {
                return Some(y);
            }
        }
    }
    // ZTF: "ZTFYYxxx" → 20YY
    if let Some(rest) = name.strip_prefix("ZTF") {
        if rest.len() >= 2 {
            if let Ok(yy) = rest[..2].parse::<i32>() {
                return Some(2000 + yy);
            }
        }
    }
    // PTF: "PTF YYxxx" or "PTF YYxxx" → 20YY
    let ptf_rest = name.strip_prefix("PTF ").or_else(|| name.strip_prefix("PTF"));
    if let Some(rest) = ptf_rest {
        if rest.len() >= 2 {
            if let Ok(yy) = rest[..2].parse::<i32>() {
                return Some(2000 + yy);
            }
        }
    }
    None
}

/// Fetch supernovae from the SIMBAD TAP service (CDS, Strasbourg).
/// Queries otype='SN*' objects within a cone of `radius_deg` degrees,
/// then retains only those discovered within 1 year before `obs_date`.
/// No API key required; SIMBAD ingests new TNS objects within days of announcement.
pub fn fetch_supernovae(
    ra: f64,
    dec: f64,
    radius_deg: f64,
    obs_date: &str,
) -> anyhow::Result<Vec<SnEntry>> {
    let adql = format!(
        "SELECT main_id, ra, dec FROM basic \
         WHERE CONTAINS(POINT('ICRS',ra,dec),CIRCLE('ICRS',{ra:.5},{dec:.5},{radius:.5}))=1 \
         AND otype='SN*'",
        ra = ra,
        dec = dec,
        radius = radius_deg,
    );

    let url = format!(
        "https://simbad.u-strasbg.fr/simbad/sim-tap/sync\
         ?REQUEST=doQuery&LANG=ADQL&FORMAT=json&QUERY={}",
        urlencoding::encode(&adql),
    );

    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(15))
        .call()?;

    let json: serde_json::Value = response.into_json()?;

    let (obs_year, obs_month) = parse_year_month(obs_date);
    // obs as months since year 0, for easy subtraction
    let obs_months = obs_year as i32 * 12 + obs_month as i32;

    let mut entries = Vec::new();
    if let Some(rows) = json["data"].as_array() {
        for row in rows {
            let cols = match row.as_array() {
                Some(c) if c.len() >= 3 => c,
                _ => continue,
            };
            let name = match cols[0].as_str() {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => continue,
            };
            let ra_val = cols[1].as_f64().unwrap_or(0.0);
            let dec_val = cols[2].as_f64().unwrap_or(0.0);

            // Skip SNe older than ~1 year before the observation.
            // If the year can't be parsed, keep the entry (don't discard unknown names).
            if let Some(sn_year) = sn_year_from_name(&name) {
                // Compare sn_year (December, conservative latest estimate) to obs
                let sn_months_latest = sn_year * 12 + 12;
                if obs_months - sn_months_latest > 12 {
                    continue;
                }
            }

            entries.push(SnEntry { name, ra: ra_val, dec: dec_val });
        }
    }

    Ok(entries)
}
