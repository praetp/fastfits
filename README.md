# fastfits

A fast desktop viewer for [FITS](https://fits.gsfc.nasa.gov/) astronomy image files, built with Rust and [egui](https://github.com/emilk/egui).

![fastfits screenshot](assets/screenshot.png)

## Features

- **File browser** — lists all `.fits` / `.fit` / `.fz` files in the current directory; click or use arrow keys to navigate
- **File open dialog** — click **Open…** in the menu bar or press `Ctrl+O` to pick any FITS file via a native file dialog; the browser switches to that file's directory automatically
- **Image rendering** — autostretch (PixInsight STF algorithm: median-based sky estimation, one-sided σ noise, MTF midtone placement) and linear (min/max) stretch modes
- **Histogram panel** — per-channel histogram with R/G/B overlapping bars; AutoStretch marker lines show black point, midtone, and white point; toggle with `H`
- **Multi-channel support** — composite RGB view or individual R/G/B channel views for colour images; single-channel for mono
- **Bayer debayering** — RGGB Bayer-patterned single-plane FITS files are automatically demosaiced; choose Cubic or Bilinear algorithm via **Preferences** (`,`)
- **Zoom** — fit-to-window (default), zoom in/out, or 1:1 pixel view; scroll wheel zooms into the point under the cursor; drag to pan when zoomed in
- **Pixel value on hover** — status bar shows `(x, y)  R=… G=… B=…` (or `val=…` for mono) while the cursor is over the image
- **Crosshair overlay** — semi-transparent crosshair follows the cursor over the image for precise pointing
- **FITS header inspector** — left panel shows all header key/value pairs alphabetically
- **File deletion** — move the current file to the system trash (with fallback to permanent delete); auto-advances to the next file
- **Keyboard-driven** — every action has a keyboard shortcut (press `?` for the full list)

## Keyboard shortcuts

| Key | Action |
|---|---|
| `←` / `↑` | Previous file |
| `→` / `↓` | Next file |
| `Delete` | Move current file to trash |
| `S` | Toggle stretch mode (Auto ↔ Linear) |
| `+` / `-` | Zoom in / out |
| `0` | Zoom to 1:1 (100%) |
| `Ctrl+O` | Open file dialog |
| `F` | Zoom to fit (resets pan) |
| `Scroll` | Zoom in/out centred on cursor |
| `H` | Show / hide histogram |
| `A` | Show / hide About |
| `?` | Show / hide keyboard shortcuts |
| `,` | Show / hide Preferences |
| `Escape` | Close help / preferences popup |

## Building

### System dependency

`libcfitsio` must be installed before building:

```bash
# Debian / Ubuntu
sudo apt install libcfitsio-dev

# Fedora / RHEL
sudo dnf install cfitsio-devel

# Arch
sudo pacman -S cfitsio
```

### Compile and run

```bash
# Debug build
cargo build

# Release build (recommended for performance)
cargo build --release

# Run directly
cargo run --release -- path/to/file.fits
cargo run --release -- path/to/directory/
cargo run --release            # defaults to current directory
```

The compiled binary is at `target/release/fastfits`.

## Pre-built binaries

Every tagged release publishes pre-built binaries on the [GitHub Releases](https://github.com/praetp/fastfits/releases) page:

| Platform | File |
|---|---|
| Linux x86-64 | `fastfits-linux-x86_64` |
| Linux arm64 | `fastfits-linux-arm64` |
| Windows x86-64 | `fastfits-windows-x86_64.exe` |
| macOS arm64 (Apple Silicon) | `fastfits-macos-arm64` |

Download the binary for your platform, make it executable (`chmod +x` on Linux/macOS), and run it directly — no `libcfitsio` installation required.

## Usage

```
fastfits [PATH]
```

`PATH` can be:
- a single `.fits` / `.fit` / `.fz` file — opens that file and browses its directory
- a directory — opens the first FITS file found in that directory
- omitted — defaults to the current working directory
