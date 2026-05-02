# Roadmap

Potential improvements, grouped by theme. No particular order or priority.

## Image rendering

- Additional stretch modes: `sqrt`, `log`, `asinh` (common in astronomy viewers like DS9)
- Per-channel brightness/contrast sliders
- Color maps / LUTs (grayscale, inferno, viridis, …) for mono images

## Navigation & inspection

- **Supernova overlay: very recent events** — SIMBAD typically ingests new TNS objects within days to weeks; for same-night coverage of brand-new transients, add a secondary query against the Transient Name Server (TNS) API (requires a free API key from tns.astronomers.org)
- **Multi-HDU browser** — navigate between HDUs in a single file (some FITS files contain multiple image extensions)
- **Sky quality / Bortle estimate** — estimate sky brightness (mag/arcsec²) and Bortle class from image background; requires photometric calibration against catalogue stars in the field to derive the instrumental zero point without assuming e⁻/ADU or QE
- **Polar alignment error estimate** — measure declination drift across consecutive images in a directory to estimate polar alignment error in arcminutes; group files by target (matching `CRVAL`), extract `DATE-OBS` and field center Dec from WCS, fit a linear regression through Dec vs. time; the systematic drift trend survives random dithering noise given enough frames (~10–15); combine drift rate with hour angle to separate azimuth and altitude error components; most useful when Dec guiding can't fully correct due to backlash

## Zoom & pan

## File handling

- Recent files list (last N opened files, persisted across sessions)
- **FWHM in file browser** — show the measured FWHM value next to each filename in the right panel (computed in background as files are browsed); allow sorting the file list by FWHM (ascending/descending) to quickly identify the sharpest or worst frames, similar to DeepSkyStacker's frame quality scoring

## Export

## UX polish

- Cache stretched RGBA texture alongside raw image data for truly instant switching (currently stretch is recomputed on each view)
- Resizable panels remember their size between sessions (persist to a config file)
- Thumbnail strip in the file browser
