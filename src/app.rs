use crate::cache::ImageCache;
use crate::fits::{ChannelView, DemosaicMode, FitsImage, HistogramData, Stretch};
use crate::seeing::SeeingResult;
use crate::wcs::WcsTransform;
use std::path::PathBuf;
use std::time::Instant;

/// A sky-coordinate annotation placed by right-clicking on the image.
/// Stored in equatorial (RA/Dec) so it persists across zoom, pan, and rotation.
#[derive(Clone, Copy)]
pub struct SkyMarker {
    pub ra: f64,        // degrees
    pub dec: f64,       // degrees
    pub color_idx: usize, // indexes MARKER_COLORS in ui.rs
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AppPrefs {
    pub show_grid:         bool,
    pub show_dso:          bool,
    pub stretch:           Stretch,
    pub demosaic_mode:     DemosaicMode,
    pub show_histogram:    bool,
    pub show_clipping:     bool,
    pub show_north_up:     bool,
    #[serde(default)]
    pub welcome_dismissed: bool,
    #[serde(default)]
    pub last_dir:          Option<PathBuf>,
    #[serde(default = "default_ui_zoom")]
    pub ui_zoom:           f32,
    #[serde(default = "default_true")]
    pub header_decimal:    bool,
}

fn default_ui_zoom() -> f32 { 1.0 }
fn default_true() -> bool { true }

impl Default for AppPrefs {
    fn default() -> Self {
        Self {
            show_grid:         false,
            show_dso:          false,
            stretch:           Stretch::AutoStretch,
            demosaic_mode:     DemosaicMode::Bilinear,
            show_histogram:    true,
            show_clipping:     false,
            show_north_up:     false,
            welcome_dismissed: false,
            last_dir:          None,
            ui_zoom:           1.0,
            header_decimal:    true,
        }
    }
}
use egui::TextureHandle;
use std::sync::mpsc;

pub enum LoadResult {
    Ok(Box<FitsImage>),
    Err(String),
}

pub struct FastFitsApp {
    /// egui context, stored so background threads can call request_repaint()
    pub ctx: egui::Context,

    /// Directory being browsed
    pub current_dir: PathBuf,
    /// Sorted list of FITS files in current_dir
    pub files: Vec<PathBuf>,
    /// Sorted list of subdirectories in current_dir
    pub subdirs: Vec<PathBuf>,
    /// Index into `files` of the currently selected file
    pub selected: Option<usize>,

    /// Currently loaded image (None if nothing loaded yet or on error)
    pub image: Option<FitsImage>,
    /// Cached egui texture for the current image/stretch/view combo
    pub texture: Option<TextureHandle>,
    /// Error message to show instead of an image
    pub load_error: Option<String>,

    /// Receiver for in-flight background load; None when idle
    pub load_rx: Option<mpsc::Receiver<LoadResult>>,

    /// Current stretch mode
    pub stretch: Stretch,
    /// Current channel view
    pub channel_view: ChannelView,

    /// Zoom: None = autofit, Some(s) = explicit scale factor
    pub zoom: Option<f32>,

    /// Result of the last delete attempt (shown briefly in the status bar)
    pub delete_status: Option<String>,
    /// Transient message shown when right-click cannot place a marker (auto-dismissed)
    pub marker_status: Option<(String, Instant)>,
    /// Whether the left headers panel is visible
    pub show_headers: bool,
    /// Whether the keyboard shortcuts help popup is open
    pub show_help: bool,
    /// Whether the first-run welcome popup is open
    pub show_welcome: bool,
    /// Persistent: user checked "Don't show again" on the welcome popup
    pub welcome_dismissed: bool,
    /// Persistent: UI zoom factor applied on top of native OS DPI (0.5 – 2.0, default 1.0)
    pub ui_zoom: f32,
    /// Whether the Preferences dialog is open
    pub show_prefs: bool,
    /// Whether the About dialog is open
    pub show_about: bool,
    /// Demosaic algorithm for Bayer images
    pub demosaic_mode: DemosaicMode,

    /// Filename being loaded (shown in center panel while loading)
    pub loading_name: Option<String>,

    /// Whether the histogram panel is visible (default: true)
    pub show_histogram: bool,
    /// Precomputed histogram for the current image (invalidated when image changes)
    pub histogram: Option<HistogramData>,
    /// Receiver for an in-flight background histogram computation; None when idle
    pub hist_rx: Option<mpsc::Receiver<HistogramData>>,

    /// Pan offset in screen pixels relative to center; reset to ZERO on fit/new file
    pub pan_offset: egui::Vec2,
    /// Image rect on screen last frame (used for zoom-to-cursor pivot)
    pub image_screen_rect: Option<egui::Rect>,
    /// Pixel value(s) under the cursor, formatted for the status bar
    pub hover_pixel_info: Option<String>,

    /// Whether the WCS coordinate grid overlay is shown
    pub show_grid: bool,
    /// Whether the DSO catalogue overlay is shown
    pub show_dso: bool,
    /// Whether the clipping/overexposure overlay is shown (saturated pixels → red)
    pub show_clipping: bool,
    /// Whether to rotate the display so North is up and East is left
    pub show_north_up: bool,
    /// WCS transform for the currently loaded image (None if no valid WCS headers)
    pub wcs: Option<WcsTransform>,

    /// Current text in the header search/filter box
    pub header_filter: String,

    /// Sky-position annotations placed by right-clicking (max 8, session-only).
    pub markers: Vec<SkyMarker>,

    /// Show the raw single-channel Bayer data instead of the debayered RGB image.
    pub show_raw_bayer: bool,
    /// Whether to reformat numeric header values from scientific to decimal notation.
    pub header_decimal: bool,

    /// Atmospheric seeing estimate for the current image.
    /// `None` = not yet attempted; `Some(None)` = attempted, < 3 stars; `Some(Some(r))` = result.
    pub seeing: Option<Option<SeeingResult>>,
    /// Receiver for an in-flight background seeing computation; None when idle.
    pub seeing_rx: Option<mpsc::Receiver<Option<SeeingResult>>>,

    /// LRU cache of recently loaded FITS images (capacity 8).
    pub cache: ImageCache,
    /// Receivers for in-flight background preloads of adjacent files.
    pub preload_rxs: Vec<(usize, mpsc::Receiver<LoadResult>)>,

    /// Set to true when the selected file changes so the file list scrolls it into view.
    pub scroll_to_selected: bool,

    /// Whether the focus temperature compensation window is open.
    pub show_focus_analysis: bool,
    /// True while the background scan is running.
    pub focus_analysis_running: bool,
    /// Receiver for the completed focus analysis result.
    pub focus_analysis_rx: Option<mpsc::Receiver<crate::focus_analysis::FocusAnalysisResult>>,
    /// Most recent completed focus analysis result.
    pub focus_analysis: Option<crate::focus_analysis::FocusAnalysisResult>,
    /// Number of files completed so far in the current scan (shared with worker thread).
    pub focus_analysis_progress: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    /// Total files to scan in the current run.
    pub focus_analysis_total: usize,
    /// Cancellation flag sent to the background scan thread.
    pub focus_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Filenames manually excluded from the focus regression by right-clicking in the plot.
    pub focus_excluded: std::collections::HashSet<String>,
    /// Set each frame by overlay windows that consume a secondary click, to prevent
    /// the center panel from also processing the same right-click.
    pub overlay_consumed_right_click: bool,

    /// Sorted list of file indices where each imaging session starts.
    /// Computed in the background; empty until the first result arrives.
    pub sessions: Vec<usize>,
    /// Receiver for the background session-computation result.
    pub sessions_rx: Option<mpsc::Receiver<Vec<usize>>>,
}

impl FastFitsApp {
    pub fn new(cc: &eframe::CreationContext<'_>, cli_path: Option<PathBuf>) -> Self {
        let prefs: AppPrefs = cc.storage
            .and_then(|s| eframe::get_value(s, "prefs"))
            .unwrap_or_default();

        setup_fonts(&cc.egui_ctx);
        cc.egui_ctx.set_zoom_factor(prefs.ui_zoom.clamp(0.5, 2.0));

        // Resolve start path: CLI arg wins; else fall back to last-used dir from prefs
        // (if still a valid directory); else current working directory.
        let start_path = cli_path
            .or_else(|| prefs.last_dir.as_ref().filter(|p| p.is_dir()).cloned())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        // Resolve to an absolute path before any chdir so relative paths stay valid.
        let start_path = start_path.canonicalize().unwrap_or(start_path);

        let mut initial_error: Option<String> = None;
        let (current_dir, selected, files) = if start_path.is_file() {
            let dir = start_path.parent().unwrap_or(&start_path).to_path_buf();
            let files = collect_fits_files(&dir);
            let selected = files.iter().position(|f| f == &start_path);
            if selected.is_none() {
                initial_error = Some(format!(
                    "Not a FITS file: {}", start_path.display()
                ));
            }
            (dir, selected, files)
        } else if start_path.is_dir() {
            let files = collect_fits_files(&start_path);
            let selected = if files.is_empty() { None } else { Some(0) };
            (start_path, selected, files)
        } else {
            initial_error = Some(format!("Path not found: {}", start_path.display()));
            let cwd = std::env::current_dir().unwrap_or_default();
            let files = collect_fits_files(&cwd);
            let selected = if files.is_empty() { None } else { Some(0) };
            (cwd, selected, files)
        };

        let _ = std::env::set_current_dir(&current_dir);
        let subdirs = collect_subdirs(&current_dir);

        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            current_dir,
            files,
            subdirs,
            selected,
            image: None,
            texture: None,
            load_error: initial_error,
            load_rx: None,
            stretch: prefs.stretch,
            channel_view: ChannelView::Rgb,
            zoom: None,
            delete_status: None,
            marker_status: None,
            show_headers: true,
            show_help: false,
            show_welcome: !prefs.welcome_dismissed,
            welcome_dismissed: prefs.welcome_dismissed,
            ui_zoom: prefs.ui_zoom.clamp(0.5, 2.0),
            show_prefs: false,
            show_about: false,
            demosaic_mode: prefs.demosaic_mode,
            loading_name: None,
            show_histogram: prefs.show_histogram,
            histogram: None,
            hist_rx: None,
            pan_offset: egui::Vec2::ZERO,
            image_screen_rect: None,
            hover_pixel_info: None,
            show_grid: prefs.show_grid,
            show_dso: prefs.show_dso,
            show_clipping: prefs.show_clipping,
            show_north_up: prefs.show_north_up,
            wcs: None,
            header_filter: String::new(),
            markers: Vec::new(),
            show_raw_bayer: false,
            header_decimal: prefs.header_decimal,
            seeing: None,
            seeing_rx: None,
            cache: ImageCache::new(8),
            preload_rxs: Vec::new(),
            scroll_to_selected: selected.is_some(),
            show_focus_analysis: false,
            focus_analysis_running: false,
            focus_analysis_rx: None,
            focus_analysis: None,
            focus_analysis_progress: None,
            focus_analysis_total: 0,
            focus_cancel: None,
            focus_excluded: std::collections::HashSet::new(),
            overlay_consumed_right_click: false,
            sessions: vec![],
            sessions_rx: None,
        };
        app.start_session_computation();
        app.load_selected();
        app
    }

    /// Load (or reload) the currently selected file.
    pub fn load_selected(&mut self) {
        self.texture = None;
        self.histogram = None;
        self.hist_rx = None;
        self.seeing = None;
        self.seeing_rx = None;
        self.load_error = None;
        self.image = None;
        self.wcs = None;
        self.pan_offset = egui::Vec2::ZERO;
        self.image_screen_rect = None;
        self.hover_pixel_info = None;

        let Some(idx) = self.selected else { return };

        // Check cache first.
        if let Some((img, hist)) = self.cache.take(idx) {
            self.channel_view = if img.channels >= 3 {
                ChannelView::Rgb
            } else {
                ChannelView::Single(0)
            };
            self.wcs = WcsTransform::from_headers(&img.headers);
            self.histogram = hist;
            self.image = Some(img);
            self.trigger_preloads(idx);
            return;
        }

        let Some(path) = self.files.get(idx).cloned() else { return };

        match FitsImage::load(&path, self.demosaic_mode) {
            Ok(img) => {
                self.channel_view = if img.channels >= 3 {
                    ChannelView::Rgb
                } else {
                    ChannelView::Single(0)
                };
                self.wcs = WcsTransform::from_headers(&img.headers);
                self.image = Some(img);
                self.trigger_preloads(idx);
            }
            Err(e) => {
                self.load_error = Some(format!("{e:#}"));
            }
        }
    }

    /// Rebuild the egui texture from the current image + stretch + channel_view.
    pub fn rebuild_texture(&mut self, ctx: &egui::Context) {
        let Some(img) = &self.image else { return };
        let rgba = if self.show_raw_bayer && img.raw_bayer.is_some() {
            img.to_rgba_raw(self.stretch, self.show_clipping)
        } else {
            img.to_rgba(self.stretch, self.channel_view, self.show_clipping)
        };
        let color_image = egui::ColorImage::from_rgba_unmultiplied([img.width, img.height], &rgba);
        self.texture = Some(ctx.load_texture(
            "fits_image",
            color_image,
            egui::TextureOptions::LINEAR,
        ));
    }

    pub fn select(&mut self, idx: usize) {
        if self.selected == Some(idx) { return; }

        // Stash current image + histogram back into cache before switching.
        if let (Some(old_idx), Some(old_image)) = (self.selected, self.image.take()) {
            self.cache.insert(old_idx, old_image, self.histogram.take());
        }

        self.selected = Some(idx);
        self.scroll_to_selected = true;
        self.image_screen_rect = None;
        self.hover_pixel_info = None;
        self.image = None;
        self.texture = None;
        self.histogram = None;
        self.hist_rx = None;
        self.seeing = None;
        self.seeing_rx = None;
        self.load_error = None;
        self.load_rx = None;
        self.wcs = None;

        // Cache hit — use the preloaded image immediately.
        if let Some((img, hist)) = self.cache.take(idx) {
            self.channel_view = if img.channels >= 3 {
                ChannelView::Rgb
            } else {
                ChannelView::Single(0)
            };
            self.wcs = WcsTransform::from_headers(&img.headers);
            self.histogram = hist;
            self.image = Some(img);
            self.loading_name = None;
            self.trigger_preloads(idx);
            return;
        }

        // Cache miss — spawn background load.
        self.loading_name = self.files.get(idx)
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned());

        let Some(path) = self.files.get(idx).cloned() else { return };
        let (tx, rx) = mpsc::channel();
        self.load_rx = Some(rx);

        let ctx = self.ctx.clone();
        let demosaic = self.demosaic_mode;
        std::thread::spawn(move || {
            let result = match FitsImage::load(&path, demosaic) {
                Ok(img) => LoadResult::Ok(Box::new(img)),
                Err(e)  => LoadResult::Err(format!("{e:#}")),
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    /// Spawn background loads for adjacent files if they are not already cached
    /// or in-flight.
    pub fn trigger_preloads(&mut self, current_idx: usize) {
        if self.files.len() <= 1 { return; }

        let neighbors = [
            if current_idx + 1 < self.files.len() { Some(current_idx + 1) } else { None },
            if current_idx > 0 { Some(current_idx - 1) } else { None },
        ];

        for idx in neighbors.into_iter().flatten() {
            if self.cache.contains(idx) { continue; }
            if self.preload_rxs.iter().any(|(i, _)| *i == idx) { continue; }

            let path = self.files[idx].clone();
            let (tx, rx) = mpsc::channel();
            let ctx = self.ctx.clone();
            let demosaic = self.demosaic_mode;
            self.preload_rxs.push((idx, rx));
            std::thread::spawn(move || {
                let result = match FitsImage::load(&path, demosaic) {
                    Ok(img) => LoadResult::Ok(Box::new(img)),
                    Err(e) => LoadResult::Err(format!("{e:#}")),
                };
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        }
    }

    pub fn select_next(&mut self) {
        if self.files.is_empty() { return; }
        let next = self.selected.map(|i| (i + 1) % self.files.len()).unwrap_or(0);
        self.select(next);
    }

    pub fn select_prev(&mut self) {
        if self.files.is_empty() { return; }
        let prev = self.selected.map(|i| {
            if i == 0 { self.files.len() - 1 } else { i - 1 }
        }).unwrap_or(0);
        self.select(prev);
    }

    pub fn select_first(&mut self) {
        if self.files.is_empty() { return; }
        self.select(0);
    }

    pub fn select_last(&mut self) {
        if self.files.is_empty() { return; }
        self.select(self.files.len() - 1);
    }

    /// Jump `delta` files forward (positive) or backward (negative), clamped to bounds.
    pub fn select_skip(&mut self, delta: isize) {
        if self.files.is_empty() { return; }
        let len = self.files.len() as isize;
        let cur = self.selected.map(|i| i as isize).unwrap_or(0);
        let new = (cur + delta).clamp(0, len - 1) as usize;
        self.select(new);
    }

    /// Jump to the first file of the previous session (or the current session's
    /// start if already at a session boundary).
    pub fn select_prev_session_start(&mut self) {
        if self.files.is_empty() || self.sessions.is_empty() { return; }
        let cur = self.selected.unwrap_or(0);
        let sess = self.sessions.partition_point(|&s| s <= cur).saturating_sub(1);
        if cur > self.sessions[sess] {
            // Not at the start of this session yet — go there first.
            self.select(self.sessions[sess]);
        } else if sess > 0 {
            self.select(self.sessions[sess - 1]);
        }
    }

    /// Jump to the first file of the next session.
    pub fn select_next_session_start(&mut self) {
        if self.files.is_empty() || self.sessions.is_empty() { return; }
        let cur = self.selected.unwrap_or(0);
        let sess = self.sessions.partition_point(|&s| s <= cur).saturating_sub(1);
        if sess + 1 < self.sessions.len() {
            self.select(self.sessions[sess + 1]);
        }
    }

    /// Jump to the first file of the current session.
    /// If already there, jump to the first file of the previous session.
    pub fn select_session_first(&mut self) {
        if self.files.is_empty() || self.sessions.is_empty() { return; }
        let cur = self.selected.unwrap_or(0);
        let sess = self.sessions.partition_point(|&s| s <= cur).saturating_sub(1);
        let sess_start = self.sessions[sess];
        if cur == sess_start && sess > 0 {
            self.select(self.sessions[sess - 1]);
        } else {
            self.select(sess_start);
        }
    }

    /// Jump to the last file of the current session.
    /// If already there, jump to the last file of the next session.
    pub fn select_session_last(&mut self) {
        if self.files.is_empty() || self.sessions.is_empty() { return; }
        let cur = self.selected.unwrap_or(0);
        let sess = self.sessions.partition_point(|&s| s <= cur).saturating_sub(1);
        let sess_end = if sess + 1 < self.sessions.len() {
            self.sessions[sess + 1] - 1
        } else {
            self.files.len() - 1
        };
        if cur == sess_end && sess + 1 < self.sessions.len() {
            let next_end = if sess + 2 < self.sessions.len() {
                self.sessions[sess + 2] - 1
            } else {
                self.files.len() - 1
            };
            self.select(next_end);
        } else {
            self.select(sess_end);
        }
    }

    /// Kick off a background thread to compute session boundaries from DATE-OBS headers.
    pub fn start_session_computation(&mut self) {
        self.sessions = vec![];
        let files = self.files.clone();
        let (tx, rx) = mpsc::channel();
        self.sessions_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(compute_sessions(&files));
        });
    }

    /// Delete the currently selected file (trash if available, else permanent).
    /// Auto-advances to the next file.
    pub fn delete_selected(&mut self) {
        let Some(idx) = self.selected else { return };
        let Some(path) = self.files.get(idx).cloned() else { return };

        let result: Result<(), String> = trash::delete(&path)
            .map_err(|e| e.to_string())
            .or_else(|_| std::fs::remove_file(&path).map_err(|e| e.to_string()));

        match result {
            Ok(()) => {
                self.files.remove(idx);
                self.start_session_computation();
                self.cache.clear();
                self.preload_rxs.clear();
                self.image = None;
                self.texture = None;
                self.histogram = None;
                self.hist_rx = None;
                self.seeing = None;
                self.seeing_rx = None;
                self.load_error = None;
                self.delete_status = None;
                if self.files.is_empty() {
                    self.selected = None;
                } else {
                    let new_idx = idx.min(self.files.len() - 1);
                    self.selected = Some(new_idx);
                    self.load_selected();
                }
            }
            Err(e) => {
                self.delete_status = Some(format!("Delete failed: {e}"));
            }
        }
    }

    /// Navigate to a directory, refreshing the file list and subdirectories.
    pub fn open_dir(&mut self, dir: PathBuf) {
        self.current_dir = dir.clone();
        let _ = std::env::set_current_dir(&dir);
        self.files = collect_fits_files(&dir);
        self.subdirs = collect_subdirs(&dir);
        self.start_session_computation();
        self.cache.clear();
        self.preload_rxs.clear();
        self.selected = None;
        self.image = None;
        self.texture = None;
        self.histogram = None;
        self.hist_rx = None;
        self.seeing = None;
        self.seeing_rx = None;
        self.load_rx = None;
        self.load_error = None;
        self.pan_offset = egui::Vec2::ZERO;
        self.image_screen_rect = None;
        self.hover_pixel_info = None;
        if !self.files.is_empty() {
            self.select(0);
        }
    }

    /// Open a FITS file chosen via the file-open dialog (or any external path).
    /// Updates the current directory to the file's parent and reloads the file list.
    pub fn open_path(&mut self, path: std::path::PathBuf) {
        let dir = path.parent().unwrap_or(path.as_path()).to_path_buf();
        self.open_dir(dir);
        if let Some(idx) = self.files.iter().position(|f| f == &path) {
            self.select(idx);
        }
    }

    /// Reload the current image (e.g. after a settings change like demosaic mode).
    pub fn reload_image(&mut self) {
        self.cache.clear();
        self.preload_rxs.clear();
        self.image = None;
        self.texture = None;
        self.histogram = None;
        self.hist_rx = None;
        self.seeing = None;
        self.seeing_rx = None;
        self.load_rx = None;
        if let Some(idx) = self.selected {
            self.selected = None;
            self.select(idx);
        }
    }

    /// Open a JPEG save dialog and export the current view at quality 90.
    pub fn export_jpeg(&self) {
        if self.image.is_none() { return; }
        let stem = self.export_stem();
        let Some(path) = rfd::FileDialog::new()
            .add_filter("JPEG image", &["jpg", "jpeg"])
            .set_file_name(&format!("{stem}_export.jpg"))
            .set_directory(&self.current_dir)
            .save_file()
        else { return };
        let Some(img) = &self.image else { return };
        let rgba = img.to_rgba(self.stretch, self.channel_view, false);
        if let Err(e) = write_jpeg(&rgba, img.width as u32, img.height as u32, &path, 90) {
            eprintln!("Export failed: {e}");
        }
    }

    /// Open a PNG save dialog and export the current view losslessly.
    pub fn export_png(&self) {
        if self.image.is_none() { return; }
        let stem = self.export_stem();
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .set_file_name(&format!("{stem}_export.png"))
            .set_directory(&self.current_dir)
            .save_file()
        else { return };
        let Some(img) = &self.image else { return };
        let rgba = img.to_rgba(self.stretch, self.channel_view, false);
        if let Err(e) = image::save_buffer(&path, &rgba, img.width as u32, img.height as u32,
                                           image::ColorType::Rgba8).map_err(|e| e.to_string()) {
            eprintln!("Export failed: {e}");
        }
    }

    /// Kick off a background seeing estimation if one is not already running or complete.
    /// Only runs for light frames (skips darks, biases, flats).
    pub fn maybe_start_seeing(&mut self) {
        if self.image.is_none() || self.seeing.is_some() || self.seeing_rx.is_some() {
            return;
        }
        let Some(img) = &self.image else { return };
        // Skip non-light frames.
        let is_light = img.headers.iter()
            .find(|(k, _)| k == "IMAGETYP")
            .map(|(_, v)| v.trim().to_ascii_lowercase().contains("light"))
            .unwrap_or(true);
        if !is_light {
            self.seeing = Some(None);
            return;
        }
        let data        = img.data.clone();
        let width       = img.width;
        let height      = img.height;
        let channels    = img.channels;
        let bd_max      = img.bitdepth_max;
        let px_scale    = self.wcs.as_ref().map(|w| w.pixel_scale_deg * 3600.0);
        let ctx2        = self.ctx.clone();
        let (tx, rx)    = mpsc::channel();
        self.seeing_rx  = Some(rx);
        std::thread::spawn(move || {
            let result = crate::seeing::estimate_seeing(&data, width, height, channels, bd_max, px_scale);
            let _ = tx.send(result);
            ctx2.request_repaint();
        });
    }

    fn export_stem(&self) -> String {
        self.selected
            .and_then(|idx| self.files.get(idx))
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "export".to_string())
    }
}

fn write_jpeg(rgba: &[u8], width: u32, height: u32, path: &std::path::Path, quality: u8)
    -> Result<(), String>
{
    use image::ImageEncoder as _;
    // JPEG does not support alpha — convert RGBA → RGB.
    let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(
        std::io::BufWriter::new(file), quality);
    enc.write_image(&rgb, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Try to use Segoe UI on Windows — it's designed for ClearType-style UI
    // rendering and matches native Windows apps. Fall back to bundled Open Sans.
    let (regular_name, bold_name) = if cfg!(target_os = "windows") {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".to_string());
        let regular_path = format!(r"{}\Fonts\segoeui.ttf", windir);
        let bold_path    = format!(r"{}\Fonts\segoeuib.ttf", windir);
        match (std::fs::read(&regular_path), std::fs::read(&bold_path)) {
            (Ok(reg), Ok(bold)) => {
                fonts.font_data.insert(
                    "SegoeUI-Regular".to_owned(),
                    std::sync::Arc::new(egui::FontData::from_owned(reg)),
                );
                fonts.font_data.insert(
                    "SegoeUI-Bold".to_owned(),
                    std::sync::Arc::new(egui::FontData::from_owned(bold)),
                );
                ("SegoeUI-Regular", "SegoeUI-Bold")
            }
            _ => {
                fonts.font_data.insert(
                    "OpenSans-Regular".to_owned(),
                    std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/OpenSans-Regular.ttf"))),
                );
                fonts.font_data.insert(
                    "OpenSans-Bold".to_owned(),
                    std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/OpenSans-Bold.ttf"))),
                );
                ("OpenSans-Regular", "OpenSans-Bold")
            }
        }
    } else {
        fonts.font_data.insert(
            "OpenSans-Regular".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/OpenSans-Regular.ttf"))),
        );
        fonts.font_data.insert(
            "OpenSans-Bold".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/OpenSans-Bold.ttf"))),
        );
        ("OpenSans-Regular", "OpenSans-Bold")
    };

    // Use the selected regular font as the primary proportional font.
    fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap()
        .insert(0, regular_name.to_owned());
    // Register a named Bold family so RichText::strong() uses the real bold weight.
    fonts.families.insert(
        egui::FontFamily::Name("Bold".into()),
        vec![bold_name.to_owned()],
    );
    ctx.set_fonts(fonts);
}

pub fn collect_fits_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new(); };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && matches!(
                    p.extension().and_then(|e| e.to_str())
                        .map(|e| e.to_ascii_lowercase()).as_deref(),
                    Some("fits" | "fit" | "fz")
                )
        })
        .collect();
    files.sort();
    files
}

pub fn collect_subdirs(dir: &std::path::Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new(); };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Read just the first few FITS header blocks to extract DATE-OBS as seconds
/// since a fixed epoch (used only for gap comparisons, not display).
/// Returns None if the file cannot be read or has no DATE-OBS header.
fn read_fits_date_obs_secs(path: &std::path::Path) -> Option<i64> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = [0u8; 2880 * 4];
    let n = f.read(&mut buf).ok()?;
    for record in buf[..n].chunks(80) {
        if record.len() < 8 { break; }
        if &record[..3] == b"END" { break; }
        if record.starts_with(b"DATE-OBS") {
            let val = std::str::from_utf8(record.get(10..).unwrap_or(&[])).ok()?
                .split('/').next()?   // strip FITS comment after '/'
                .trim()
                .trim_matches('\'');  // strip string-value quotes
            return parse_iso_secs(val);
        }
    }
    None
}

fn parse_iso_secs(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date_str, time_str) = s.split_once('T').unwrap_or((s, "00:00:00"));
    let mut dp = date_str.splitn(3, '-');
    let y: i64 = dp.next()?.trim().parse().ok()?;
    let m: i64 = dp.next()?.trim().parse().ok()?;
    let d: i64 = dp.next()?.trim().parse().ok()?;
    // Julian Day Number — epoch-independent, used only for gap detection.
    let jdn = 367 * y - 7 * (y + (m + 9) / 12) / 4 + 275 * m / 9 + d + 1_721_014;
    let time_no_frac = time_str.split('.').next().unwrap_or("00:00:00");
    let mut tp = time_no_frac.splitn(3, ':');
    let h:  i64 = tp.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let mi: i64 = tp.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let sc: i64 = tp.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    Some(jdn * 86_400 + h * 3_600 + mi * 60 + sc)
}

/// Compute session start indices for `files`.
/// A new session begins when the DATE-OBS gap between consecutive known timestamps
/// is at least 6 hours. Files without DATE-OBS do not break sessions.
/// Returns a sorted Vec of file indices where each session starts (always includes 0
/// if `files` is non-empty).
pub fn compute_sessions(files: &[PathBuf]) -> Vec<usize> {
    use rayon::prelude::*;
    if files.is_empty() { return vec![]; }
    const GAP: i64 = 6 * 3_600;

    // Read timestamps in parallel (only first ~12 KB per file, very fast).
    let timestamps: Vec<Option<i64>> = files.par_iter()
        .map(|p| read_fits_date_obs_secs(p))
        .collect();

    let mut sessions = vec![0usize];
    let mut last_ts = timestamps[0];
    for (i, &ts) in timestamps.iter().enumerate().skip(1) {
        if let (Some(prev), Some(curr)) = (last_ts, ts) {
            if curr - prev >= GAP {
                sessions.push(i);
            }
        }
        if ts.is_some() { last_ts = ts; }
    }
    sessions
}
