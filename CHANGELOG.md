# Changelog

All notable changes to this project will be documented here.

## [Unreleased]

## [0.5.0] – 2026-03-01

### Added
- **DSO catalogue overlay** — press `D` or click **DSO** in the menu bar to overlay labelled circles for ~14 000 Deep Sky Objects (Messier + NGC/IC) from the [OpenNGC](https://github.com/mattiaverga/OpenNGC) catalogue (CC-BY-SA); objects are colour-coded by type: orange = galaxies, cyan = open clusters, yellow = globular clusters, violet = emission/reflection/HII nebulae, teal = planetary nebulae; circles scale with zoom level; Messier objects are labelled `M1`…`M110`, others `NGC 224` / `IC 1`; the overlay is silently absent for files without valid WCS headers; the toggle is persisted across sessions

## [0.4.2] – 2026-03-01

### Added
- **PNG / JPEG export** — save the current stretched view via **Export JPG** / **Export PNG** buttons in the menu bar or `Ctrl+E`; JPEG is saved at quality 90; PNG is lossless; the filename defaults to `<source>_export.jpg/png`; the full image at the current stretch and channel view is saved (not the zoomed/cropped viewport)
- **Keyboard pan nudge** — when zoomed in, arrow keys pan the image by 50 screen pixels per press; at autofit they continue to navigate files as before
- **FITS header search** — live filter box at the top of the header panel; type to filter key/value rows instantly; ✕ button clears the filter; single-key shortcuts are suppressed while the search box has focus

### Fixed
- **WCS grid labels** — arcminute (`′`) and arcsecond (`″`) Unicode primes are replaced with plain ASCII `'` / `"` so they render correctly in egui's default font instead of appearing as □
- **Persistent settings** — stretch mode, demosaic algorithm, histogram visibility, and WCS grid toggle are now saved on exit and restored on next launch; stored in the OS-standard app data directory (`~/.local/share/fastfits/app.json` on Linux, `%APPDATA%\fastfits\app.json` on Windows, `~/Library/Application Support/fastfits/app.json` on macOS); corrupted or missing files fall back to defaults silently
- **Bottom bar layout** — < Prev, Next >, and Delete buttons are permanently centred; pixel/sky info is reserved on the left and delete-error messages on the right; neither the image nor the buttons shift when the cursor moves over the image
- **WCS coordinate grid** — press `G` or click the **Grid** button in the menu bar to overlay RA/Dec grid lines on the image; spacing is chosen automatically (~5 lines across the shorter axis); lines are labelled in `HHhMMm` / `±DD°MM′` notation; requires `CTYPE1`/`CTYPE2` containing `RA`/`DEC` and standard WCS keywords (`CRPIX`, `CRVAL`, plus either a `CD` matrix or `CDELT`/`CROTA2`); files without valid WCS headers silently show no grid
- **RA/Dec on hover** — when WCS headers are present the bottom status bar appends the celestial coordinates (`RA HHhMMm Dec ±DD°MM′`) alongside the pixel position and ADU value(s)
- **Zoom-to-cursor** — mouse wheel zooms into (or out of) the point under the cursor rather than the image center; drag to pan when zoomed in; `F` resets to fit and re-centres
- **Pixel value on hover** — while the cursor is over the image the status bar shows the pixel coordinates and raw ADU value(s): `(x, y)  R=… G=… B=…` for RGB images or `(x, y)  val=…` for mono / single-channel views
- **Crosshair overlay** — a semi-transparent white crosshair follows the cursor across the image; disappears when the cursor leaves the image
- **Release CI** — GitHub Actions workflow builds static binaries for Linux x86-64, Linux arm64, Windows x86-64, and macOS arm64 on every version tag (`v*`); assets are attached to the GitHub Release automatically
- **File open dialog** — **Open…** button in the menu bar (also `Ctrl+O`) opens a native file picker filtered to `.fits`/`.fit`/`.fz`; selecting a file switches the browser to that file's directory automatically

## [0.3.0] – 2026-02-15

### Added
- **Histogram panel** — per-channel image histogram displayed above the file list in the right panel; shows overlapping semi-transparent R/G/B bars for colour images (or a single gray bar for mono); when AutoStretch is active, vertical marker lines indicate the black point, midtone, and white point for each channel; toggle with `H` or the **Hist** button in the menu bar
- **About dialog** — shows version, author, license, repository link, build date, and Rust compiler version; open with `A` or the **About** button in the menu bar

### Changed
- **AutoStretch algorithm rewritten** — now follows the PixInsight STF (Screen Transfer Function) approach: sky background estimated via histogram median, noise σ derived from the one-sided spread (median − p16), black point at median − 2.8 σ, white point at the 99.98th percentile, and scale anchored to the actual data range `[c0, white]` rather than the sensor ceiling; sky median is placed at 25% display brightness (TARGET_BG = 0.25); produces significantly more natural results across a wide range of targets (bright nebulae, faint galaxies, globulars) without the near-step-function behaviour of the previous algorithm

## [0.2.2] – 2026-02-14

### Changed
- **Parallel image loading** — all CPU-bound steps (histogram, min/max, RGBA pixel loop, debayer conversion) now use rayon; ~2–4× faster on multi-core systems, especially noticeable on full-frame images

## [0.2.1] – 2026-02-14

### Changed
- **AutoStretch tuning** — midtone K reduced from 3.0 to 1.5 (mode + 1.5 σ); better balance between faint-signal visibility and noise suppression

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
