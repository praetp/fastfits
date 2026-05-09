const SYNODIC_PERIOD: f64 = 29.530_588_70;
// Known new moon: 2000-01-06 18:14 UTC → JD 2451549.2597
const KNOWN_NEW_MOON_JD: f64 = 2_451_549.259_7;

pub struct LunarPhase {
    pub illumination: f64, // 0.0–1.0
    pub age_days: f64,     // days since new moon
    pub name: &'static str,
    pub emoji: &'static str,
}

/// Parse a FITS DATE-OBS string ("2025-05-06T02:14:07" or "2025-05-06") into
/// a Julian Day Number.
pub fn date_obs_to_jd(date_obs: &str) -> Option<f64> {
    let s = date_obs.trim().trim_matches('\'');
    if s.len() < 10 { return None; }
    let mut parts = s[..10].split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;

    let h: f64 = if s.len() >= 13 {
        let tp = &s[11..];
        let mut it = tp.splitn(3, ':');
        let hh: f64 = it.next()?.parse().ok()?;
        let mm: f64 = it.next().and_then(|p| p.parse().ok()).unwrap_or(0.0);
        let ss: f64 = it.next().and_then(|p| p.parse().ok()).unwrap_or(0.0);
        hh + mm / 60.0 + ss / 3600.0
    } else {
        12.0
    };

    let a = (14 - m) / 12;
    let y2 = y + 4800 - a;
    let m2 = m + 12 * a - 3;
    let jdn: i64 = d + (153 * m2 + 2) / 5 + 365 * y2 + y2 / 4 - y2 / 100 + y2 / 400 - 32045;
    Some(jdn as f64 + (h - 12.0) / 24.0)
}

pub fn compute_phase(jd: f64) -> LunarPhase {
    let age = ((jd - KNOWN_NEW_MOON_JD) % SYNODIC_PERIOD + SYNODIC_PERIOD) % SYNODIC_PERIOD;
    let illumination = (1.0 - (2.0 * std::f64::consts::PI * age / SYNODIC_PERIOD).cos()) / 2.0;

    let (name, emoji) = if age < 1.845 {
        ("New Moon", "🌑")
    } else if age < 7.383 {
        ("Waxing Crescent", "🌒")
    } else if age < 9.228 {
        ("First Quarter", "🌓")
    } else if age < 14.765 {
        ("Waxing Gibbous", "🌔")
    } else if age < 16.611 {
        ("Full Moon", "🌕")
    } else if age < 22.148 {
        ("Waning Gibbous", "🌖")
    } else if age < 23.993 {
        ("Last Quarter", "🌗")
    } else {
        ("Waning Crescent", "🌘")
    };

    LunarPhase { illumination, age_days: age, name, emoji }
}
