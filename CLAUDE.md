# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`fastfits` is a fast Rust-based desktop GUI viewer and tool for FITS (Flexible Image Transport System) files, the standard format used in astronomy. It uses `egui`/`eframe` for the GUI, `clap` for CLI argument parsing, and `fitsio` (wrapping `libcfitsio`) for FITS file I/O.

**System dependency:** `libcfitsio` must be installed (`apt install libcfitsio-dev` / `dnf install cfitsio-devel`).

**Crate version constraints:** `eframe`/`egui` are pinned to `0.28` because `winit 0.30.x` (used by eframe 0.29+) has a type inference regression with Rust 1.93. Do not upgrade eframe/egui without verifying `winit` compiles.

## Commands

```bash
# Build
cargo build
cargo build --release

# Run
cargo run -- [args]
cargo run --release -- path/to/file.fits

# Test
cargo test
cargo test <test_name>          # Run a single test by name
cargo test -- --nocapture       # Show println! output during tests

# Lint and format
cargo clippy
cargo fmt
cargo fmt --check               # Check formatting without modifying files
```

## Architecture

The project is a Rust workspace or single-crate application structured around:

- **Entry point** (`src/main.rs`): Parses CLI args via `clap`, then launches the `eframe` application or runs headless operations.
- **App state** (`src/app.rs`): The central `eframe::App` struct holding all GUI state, open files, and rendering logic. The `update()` method is the main render loop called each frame.
- **FITS I/O** (`src/fits/`): Wraps `fitsio`/`fitrs` to load HDUs, image data, headers, and tables. Separates file parsing from display logic.
- **UI panels** (`src/ui/`): Individual egui panels — e.g., header inspector, image viewport, HDU tree — composed inside the main `App::update()`.

### Key data flow

```
CLI args (clap) → open file path
  → FITS reader (fitsio/fitrs) → parsed HDUs/images/tables
    → App state (egui textures, cached data)
      → egui panels rendered each frame
```

### egui rendering notes

- Image data must be uploaded to the GPU as `egui::ColorImage` / `egui::TextureHandle`. Re-upload only when the source data changes, not every frame.
- Use `egui::Context::request_repaint()` only when state actually changes to avoid unnecessary CPU usage.
- `eframe::NativeOptions` controls window size, vsync, and the wgpu/glow backend.

### Requirements

**File handling**
- Accepts a single `.fits`/`.fit`/`.fz` file or a directory as a CLI argument; defaults to current directory if no argument given
- File browser (right panel) shows only `.fits`, `.fit`, `.fz` files in the current directory — no subdirectory traversal
- Deleting a file moves it to the system trash if available, otherwise permanently deletes; auto-advances to the next file after deletion

**Layout**
- Left panel: FITS header key/value viewer for the current file
- Center panel: rendered image (fills available space by default)
- Right panel: file browser for the current directory

**Image rendering**
- Two stretch modes: **linear** (raw min/max) and **autostretch** (histogram equalization, following the approach used by Siril/KStars)
- Default zoom is autofit (fill center panel); user can zoom in/out and switch to 1:1 (no zoom)
- For multi-channel files, default view is composite RGB (using debayering); user can switch to individual channels (e.g. R, G, B as grayscale)
- Channels may come from a 3D data cube (axis 3) or separate HDUs

**UX**
- Every user action must have a keyboard shortcut
- Quick file navigation (jump to next/previous file)


### Instructions
- NEVER commit without my permission
- Ask questions first, build step by step, increment progress is preferred.
- You can find some FITS files only or in /astrophotography for testing
- Every edit should compile
- README.md must be updated
- CHANGELOG.md must be updated
- The screenshot must be updated (use the M31 file from the testdata)
