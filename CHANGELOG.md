# Changelog

All notable changes to this project will be documented here.

## [0.2.0] – 2026-02-14

### Added
- **Preferences dialog** — press `,` or click the **Prefs** button in the menu bar to open
- **Demosaic algorithm selector** — choose between **Bilinear** (default, faster) and **Cubic** (higher quality) debayering for Bayer-pattern images; option only shown when a Bayer image is loaded; changing the mode reloads the image automatically
- `,` keyboard shortcut to toggle the Preferences dialog; `Escape` closes it

### Changed
- **AutoStretch completely rewritten** — clips sky background to black (histogram mode as black point), then applies an MTF that places the midtone at mode + 3 σ above sky; produces results visually comparable to ASIFitsView/Siril for both bright targets (M31) and faint ones (M33) with significantly reduced noise amplification

## [0.1.0] – 2026-02-13

Initial release.

### Added
- FITS file viewer using egui/eframe
- File browser panel (right) listing `.fits` / `.fit` / `.fz` files in the current directory; keyboard navigation with arrow keys
- FITS header inspector panel (left) showing all key/value pairs alphabetically, parsed from raw 80-byte FITS records
- Image rendering with two stretch modes:
  - **AutoStretch** — histogram-based MTF equalisation (Siril/KStars approach) with per-channel colour balance
  - **Linear** — raw min/max normalisation
- Bayer RGGB debayering for single-plane colour FITS files
- Multi-channel support: composite RGB view and individual R/G/B channel views
- Zoom: fit-to-window (default), zoom in/out (`+` / `-`), 1:1 (`0`), fit (`F`)
- `Delete` key moves the current file to the system trash (falls back to permanent delete); auto-advances to the next file
- `?` key opens a keyboard-shortcuts help popup; `Escape` closes it
- "Loading…" message in the image viewport while a file is being read
- Bottom toolbar with **< Prev**, **Next >**, and **Delete** buttons
- Hover tooltips on all interactive widgets
- CLI argument: accepts a file path, a directory, or defaults to the current directory
