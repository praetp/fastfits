# Roadmap

Potential improvements, grouped by theme. No particular order or priority.

## Image rendering

- Additional stretch modes: `sqrt`, `log`, `asinh` (common in astronomy viewers like DS9)
- Per-channel brightness/contrast sliders
- Color maps / LUTs (grayscale, inferno, viridis, …) for mono images

## Navigation & inspection

- **Multi-HDU browser** — navigate between HDUs in a single file (some FITS files contain multiple image extensions)

## Zoom & pan

## File handling

- Subdirectory traversal toggle in the file browser
- Recent files list (last N opened files, persisted across sessions)

## Export

## UX polish

- Cache stretched RGBA texture alongside raw image data for truly instant switching (currently stretch is recomputed on each view)
- Resizable panels remember their size between sessions (persist to a config file)
- Thumbnail strip in the file browser
