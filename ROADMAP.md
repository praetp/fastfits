# Roadmap

Potential improvements, grouped by theme. No particular order or priority.

## Image rendering

- Additional stretch modes: `sqrt`, `log`, `asinh` (common in astronomy viewers like DS9)
- Per-channel brightness/contrast sliders
- Color maps / LUTs (grayscale, inferno, viridis, …) for mono images

## Navigation & inspection

- **WCS support** — show RA/Dec coordinates alongside pixel `(x, y)` on hover (requires parsing `CRPIX`, `CRVAL`, `CD` / `CDELT` headers)
- **Multi-HDU browser** — navigate between HDUs in a single file (some FITS files contain multiple image extensions)

## Zoom & pan

- Keyboard nudge for pan (`W`/`A`/`S`/`D` or arrow keys when zoomed in)
- Zoom to 1:1 centred on cursor (not just image center)

## File handling

- Subdirectory traversal toggle in the file browser
- Recent files list

## Export

- Save current stretched/rendered view as PNG / JPEG

## UX polish

- Loading progress bar for large files (currently just "Loading…")
- Resizable panels remember their size between sessions (persist to a config file)
- Thumbnail strip in the file browser
