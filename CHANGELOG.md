# Changelog

All notable changes to this project will be documented here.

## [1.5.3] – 2026-04-24

### Added
- **Session detection** — imaging sessions are detected automatically from `DATE-OBS` headers (gap ≥ 6 hours = new session); computed in a background thread using rayon; sessions are separated by horizontal rulers with "Session N" labels in the file list; session count shown in the title bar as `Session [X/N]` and in the file list heading
- **Session navigation** — `[` / `]` jump to the first file of the previous / next session; `Ctrl+Home` / `Ctrl+End` jump to the first / last file of the current session (press again to move to the adjacent session)

## [1.5.2] – 2026-04-24

### Added
- **Focus scatter plot: right-click to exclude** — right-clicking a data point in the focus temperature scatter plot removes it from the regression; the slope, R², and N update immediately; excluded points are shown as grey circles with an ✕ mark; right-click again to restore; excluded points are reset when a new scan is started
- **Focus scan cancellation** — closing the Focus T°C window while a scan is in progress now cancels the background scan immediately (each rayon worker checks a shared cancel flag); previously the scan continued silently in the background

### Fixed
- **Right-click in scatter plot no longer places sky markers** — secondary-click events consumed by the focus scatter plot are no longer forwarded to the image panel

## [1.5.1] – 2026-04-23

### Fixed
- **Non-ASCII filename workaround on Windows** — the symlink target is now canonicalized to an absolute path before creation; a relative-path symlink resolved incorrectly when CFITSIO accessed it from the temp directory (regression when the Windows runner has Developer Mode / symlink privileges enabled)

## [1.5.0] – 2026-04-23

### Added
- **Focus temperature compensation** — new **Focus T°C** button (or `K`) scans all FITS files in the current directory in parallel, reads `FOCUSTEM` and `FOCUSPOS` headers, measures FWHM and roundness for each file, and fits a weighted linear regression (weight = 1/FWHM²); result window shows slope in steps/°C, R² (colour-coded), a scatter plot, and a skip summary; only images with roundness ≥ 0.9 are used; progress bar and file counter update live during the scan
- **Star roundness indicator** — the right panel now shows a median axis ratio (b/a) for the detected stars below the FWHM row; colour-coded green (≥ 0.90, round) / yellow (≥ 0.75) / red (elongated); hover for tooltip
- **Header decimal notation** — numeric FITS header values (e.g. `1.23E+02`) are reformatted to plain decimal (`123`); toggled with the **1.23 / 1E0** button in the header panel title bar; persisted across sessions; raw value is always used for clipboard copy

### Changed
- **File list auto-scrolls to selected file** — switching files via keyboard or button now scrolls the right-panel file list to keep the current entry visible
- **Focus scatter plot: FWHM colour coding** — data points are coloured green (sharp) → yellow → red (soft) matching the regression weight; a colour legend is shown in the plot corner; cursor changes to a hand on hover
- **Focus scatter plot: click to open** — clicking a data point in the scatter plot navigates the viewer to that file

## [1.4.4] – 2026-04-19

### Changed
- **Non-ASCII filename workaround uses symlinks** — the CFITSIO non-ASCII filename fallback now tries creating a symlink (instant, O(1)) before falling back to copying the file; on Windows, symlinks require admin or Developer Mode — copy is used automatically when symlinks are unavailable

## [1.4.3] – 2026-04-19

### Added
- **WASD viewport panning** — when zoomed in, `W`/`A`/`S`/`D` pan the viewport; arrow keys are now exclusively for file navigation regardless of zoom level
- **Collapsible headers panel** — press `L` or click **Hdr** in the menu bar to show/hide the FITS headers panel; an **X** close button is also available in the panel header

### Changed
- **Keyboard shortcut reassignments** — Stretch toggle moved from `S` to `T`; About moved from `A` to `I`; DSO overlay moved from `D` to `B`; avoids conflicts with WASD panning
- **Zoom and pan preserved across files** — switching files no longer resets the zoom level or viewport position
- **Right panel resizable** — the file browser panel can now be shrunk below the longest filename (minimum 100 px)
- **FWHM quality indicator** — uses a coloured circle instead of coloured text for better readability

### Fixed
- **Hover box clipping when zoomed** — the pixel-info tooltip now stays within the visible panel area instead of following the (off-screen) image boundary
- **Non-ASCII filenames on Windows** — CFITSIO cannot open paths containing non-ASCII characters (e.g. `°`) via its C `fopen()` on Windows; fastfits now copies to a temp file with an ASCII-safe name as a workaround

## [1.4.2] – 2026-04-16

### Fixed
- **macOS "damaged" dialog on first launch** — the `.app` is now ad-hoc code-signed in CI; modern macOS rejects fully-unsigned binaries outright, but ad-hoc signing downgrades that to the normal Gatekeeper warning which users can bypass. README documents the `xattr -cr` fallback as the reliable workaround.

## [1.4.1] – 2026-04-16

### Changed
- **macOS release artifact is now a `.dmg`** — the release job wraps the binary in an `fastfits.app` bundle and packages it as a disk image with an `/Applications` symlink for drag-to-install. Still unsigned / un-notarised, so a one-time right-click → **Open** is needed on first launch.

## [1.4.0] – 2026-04-15

### Added
- **UI scale preference** — a 0.5× – 2.0× slider in Preferences (`,`) lets the user shrink or enlarge the UI on top of the OS display scale; useful on Windows machines with high default scaling. Applied immediately and persisted across launches.

## [1.3.0] – 2026-04-15

### Added
- **Remember last-used directory** — launching `fastfits` without a path argument now restores the last browsed directory from prefs (falling back to the current working directory if the stored path no longer exists); a CLI path argument still takes precedence

## [1.2.0] – 2026-04-15

### Added
- **First-run welcome popup** — shows a short overview and the full keyboard shortcut list on first launch; a "Don't show this again" checkbox persists the choice in prefs
- **Home / End** — jump to the first / last file in the current directory
- **PageUp / PageDown** — skip 10 files back / forward (conventional list-scroll direction, clamped to bounds)

### Changed
- **Removed egui 0.32 deprecation warnings** — migrated to `MenuBar::new().ui()`, `Button::selectable()`, `Context::copy_text()`, and `Ui::close()`; build is now deprecation-free

## [1.1.0] – 2026-04-15

### Added
- **Mouse back/forward navigation** — mouse buttons 4/5 (browser back/forward) now select the previous/next file in the browser
- **File browser auto-sizes to longest filename** — the right panel widens to fit the longest filename/subdirectory without truncation (clamped to 800 px max)

### Fixed
- **DSO labels with suffixes** — catalog entries like `NGC0061A` or `IC0080 NED01` previously rendered as `NGC 0` / `IC 0`; they now display as `NGC 61A` and `IC 80 NED01`
- **File tooltip glyph** — replaced `←/→` arrow glyphs (missing in the bundled font) with `Left/Right` in the file-list hover tooltip

## [1.0.0] – 2026-04-14

First stable release.

### Changed
- **Removed 1.25× Windows zoom workaround** — no longer needed with egui 0.32's text rendering; UI now uses the same scale on all platforms
- **Filename removed from menu bar** — already shown in the title bar alongside `[X/N]`

## [0.6.4] – 2026-04-14

### Changed
- **Upgraded to egui/eframe 0.32** (from 0.28) — four minor versions of font rendering, text shaping, and widget improvements; the winit 0.30.x type inference regression that previously pinned us to 0.28 is fixed on Rust 1.93
- **Windows zoom factor** — bumped UI scale to 1.25× on Windows to compensate for egui's grayscale anti-aliasing (no ClearType support)

### Fixed
- **Windows font rendering** — reverted from Segoe UI Semibold to Regular; combined with the 0.32 text rendering improvements and 1.25× scale, text is noticeably crisper on Windows

## [0.6.3] – 2026-04-14

### Added
- **Header copy context menu** — right-clicking on a FITS header row in the left panel shows a context menu with options to copy the key, the value, or `key = value`
- **Version in title bar** — the window title now includes the application version (e.g. `fastfits 0.6.3 — image.fits [3/12]`)

### Changed
- **Windows font weight** — use Segoe UI Semibold instead of Regular on Windows; the heavier strokes anti-alias better under egui's grayscale rendering (no ClearType)

## [0.6.2] – 2026-04-14

### Added
- **Native font on Windows** — uses Segoe UI (Windows' system UI font) for better on-screen text rendering on Windows; falls back to bundled Open Sans if Segoe UI is unavailable; Linux and macOS continue to use Open Sans

## [0.6.1] – 2026-04-14
- **File counter** — title bar and Files heading now show `[X/N]` (current index / total files in directory)
- **Histogram hover tooltip** — hovering over the histogram shows per-channel min, max, median, sigma, and STF black/white points (in AutoStretch mode)
- **Right-click feedback** — right-clicking on an image without WCS headers shows a transient warning message instead of silently doing nothing

### Changed
- **Line histogram** — histogram draws as thin R/G/B lines instead of filled bars for a cleaner look; STF marker lines removed (values available in hover tooltip)
- **Histogram cached with images** — histogram data is stored in the LRU cache alongside images; navigating back to a previously viewed file restores the histogram instantly
- **Stable right panel layout** — histogram and Star FWHM sections reserve their space during loading, preventing layout jumps when switching files or stretch modes
- **Stretch toggle no longer recomputes histogram** — toggling Auto/Linear is now instant since the histogram bins are stretch-independent

## [0.6.0] – 2026-04-13

### Added
- **LRU image cache** — recently viewed images (up to 8) are kept in an LRU cache; navigating back to a previously viewed file is near-instant; adjacent files (next/previous) are preloaded in the background so forward/backward navigation is significantly faster
- **Loading spinner** — a centered animated spinner replaces the static "Loading…" text while files are being read
- **1:1 zoom button** — a **1:1** button in the bottom bar provides quick access to 100% zoom (same as pressing `0`)
- **Seeing estimator** — the right panel now shows the atmospheric seeing measured from stellar PSFs in the current image; stars are detected as local maxima above sky background + 8σ (histogram-based), their FWHM is measured via a half-maximum profile walk along horizontal and vertical axes (no fitting), elongated sources are rejected, and the median FWHM ± MAD-based error are reported; when WCS headers are present the result is shown in arcseconds ("Seeing: 2.3″ ± 0.2″   18 stars") and in pixels ("FWHM: 3.8 px ± 0.3 px"); images without WCS show only the pixel result with star count; the computation runs in a background thread and the panel shows "Seeing: measuring…" while it runs; the result clears automatically when navigating to another file; flat fields and images with fewer than 3 usable stars show nothing
- **Sky markers** — right-click anywhere on the image to place a coloured circle annotation; markers are stored in equatorial coordinates (RA/Dec via WCS) so they remain correctly positioned across zoom, pan, rotation, and file navigation; right-click an existing marker to remove it; up to 8 markers per session with a fixed colour palette (red, green, blue, yellow, orange, purple, cyan, amber); marker radius scales with zoom level; requires valid WCS headers; no-WCS images are unaffected
- **Hover pixel info overlay** — pixel value and RA/Dec are now shown as a floating tooltip near the cursor instead of in the bottom bar; the label (dark semi-transparent background, monospace text) appears below-right of the cursor and flips left automatically when near the right edge of the image; pixel coordinates are no longer shown (value and sky position are sufficient)
- **Raw Bayer view** — for Bayer-pattern images a **Raw** toggle appears in the menu bar; when active, the raw single-channel sensor data is displayed without debayering (grayscale); the original ADU value at the exact sensor pixel is shown in the hover overlay alongside the debayered R/G/B values; switching between raw and debayered is instantaneous (both representations are kept in memory after load)
- **Directory navigation** — the file browser now shows subdirectories (with `/` suffix) and a `..` entry to go to the parent directory; clicking a directory navigates into it; the full directory path is displayed above the file list

### Fixed
- **3D FITS cubes (e.g. Siril RGB stacks)** — files with NAXIS3=3 were rendered incorrectly (width/channels swapped) because the fitsio crate reverses shape to C order; the channel menu showed thousands of "?" entries instead of R/G/B/RGB
- **Float / compressed FITS images (e.g. Siril master bias/dark/flat)** — files with `BITPIX=-32` stored as tile-compressed extensions were displayed as all-black because the raw FITS `BITPIX` header reflects the binary-table storage format (`8`), not the image data type; Bayer detection and `bitdepth_max` derivation now use the `image_type` reported by cfitsio instead, correctly identifying float data and skipping debayering
- **Hover pixel value for float data** — values in the 0–1 range (normalised calibration frames) were displayed as `0` due to integer rounding; float images now show 4 decimal places (e.g. `val=0.0622`)
- **Texture rebuild timing** — toggling Clip or switching files via the right panel no longer flashes "No file selected" for one frame; texture rebuild now runs after all UI panels have processed their interactions

### Changed
- **Right panel layout** — "Files" heading moved below the histogram and seeing info, directly above the file list

## [0.5.1] – 2026-03-04

### Added
- **North-up / East-left orientation** — press `N` or click **N↑** in the menu bar to rotate the display so North is up and East is to the left (standard astronomical convention); requires valid WCS headers; the rotation angle is derived from the CD-matrix inverse; a horizontal flip is applied automatically when needed so East is always to the left; WCS grid lines and DSO catalogue circles remain correctly positioned on the rotated image; hover pixel coordinates and RA/Dec display correctly under rotation; zoom-to-cursor (scroll wheel) works as normal; images without WCS headers are unaffected; the toggle is persisted across sessions

- **Clipping overlay** — press `C` or click **Clip** in the menu bar to highlight overexposed (saturated) pixels in red; for integer images the threshold is the sensor ceiling (`bitdepth_max`, e.g. 65535 for 16-bit); for float images the data maximum is used; exports (JPEG/PNG) are unaffected by the overlay; the toggle is persisted across sessions

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
