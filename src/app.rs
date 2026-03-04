use crate::fits::{ChannelView, DemosaicMode, FitsImage, HistogramData, Stretch};
use crate::wcs::WcsTransform;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AppPrefs {
    pub show_grid:      bool,
    pub show_dso:       bool,
    pub stretch:        Stretch,
    pub demosaic_mode:  DemosaicMode,
    pub show_histogram: bool,
    pub show_clipping:  bool,
    pub show_north_up:  bool,
}

impl Default for AppPrefs {
    fn default() -> Self {
        Self {
            show_grid:      false,
            show_dso:       false,
            stretch:        Stretch::AutoStretch,
            demosaic_mode:  DemosaicMode::Bilinear,
            show_histogram: true,
            show_clipping:  false,
            show_north_up:  false,
        }
    }
}
use egui::TextureHandle;
use std::path::PathBuf;
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
    /// Whether the keyboard shortcuts help popup is open
    pub show_help: bool,
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
}

impl FastFitsApp {
    pub fn new(cc: &eframe::CreationContext<'_>, start_path: PathBuf) -> Self {
        let prefs: AppPrefs = cc.storage
            .and_then(|s| eframe::get_value(s, "prefs"))
            .unwrap_or_default();

        let (current_dir, selected, files) = if start_path.is_file() {
            let dir = start_path.parent().unwrap_or(&start_path).to_path_buf();
            let files = collect_fits_files(&dir);
            let selected = files.iter().position(|f| f == &start_path);
            (dir, selected, files)
        } else {
            let files = collect_fits_files(&start_path);
            let selected = if files.is_empty() { None } else { Some(0) };
            (start_path, selected, files)
        };

        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            current_dir,
            files,
            selected,
            image: None,
            texture: None,
            load_error: None,
            load_rx: None,
            stretch: prefs.stretch,
            channel_view: ChannelView::Rgb,
            zoom: None,
            delete_status: None,
            show_help: false,
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
        };
        app.load_selected();
        app
    }

    /// Load (or reload) the currently selected file.
    pub fn load_selected(&mut self) {
        self.texture = None;
        self.histogram = None;
        self.hist_rx = None;
        self.load_error = None;
        self.image = None;
        self.wcs = None;
        self.pan_offset = egui::Vec2::ZERO;
        self.image_screen_rect = None;
        self.hover_pixel_info = None;

        let Some(idx) = self.selected else { return };
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
            }
            Err(e) => {
                self.load_error = Some(format!("{e:#}"));
            }
        }
    }

    /// Rebuild the egui texture from the current image + stretch + channel_view.
    pub fn rebuild_texture(&mut self, ctx: &egui::Context) {
        let Some(img) = &self.image else { return };
        let rgba = img.to_rgba(self.stretch, self.channel_view, self.show_clipping);
        let color_image = egui::ColorImage::from_rgba_unmultiplied([img.width, img.height], &rgba);
        self.texture = Some(ctx.load_texture(
            "fits_image",
            color_image,
            egui::TextureOptions::LINEAR,
        ));
    }

    pub fn select(&mut self, idx: usize) {
        if self.selected == Some(idx) { return; }
        self.selected = Some(idx);
        self.zoom = None;
        self.pan_offset = egui::Vec2::ZERO;
        self.image_screen_rect = None;
        self.hover_pixel_info = None;
        self.image = None;
        self.texture = None;
        self.histogram = None;
        self.hist_rx = None;
        self.load_error = None;
        self.load_rx = None;
        self.wcs = None;

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
                self.image = None;
                self.texture = None;
                self.histogram = None;
                self.hist_rx = None;
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

    /// Open a FITS file chosen via the file-open dialog (or any external path).
    /// Updates the current directory to the file's parent and reloads the file list.
    pub fn open_path(&mut self, path: std::path::PathBuf) {
        let dir = path.parent().unwrap_or(path.as_path()).to_path_buf();
        self.current_dir = dir.clone();
        self.files = collect_fits_files(&dir);
        self.selected = None;
        self.image = None;
        self.texture = None;
        self.histogram = None;
        self.hist_rx = None;
        self.load_rx = None;
        self.load_error = None;
        self.pan_offset = egui::Vec2::ZERO;
        self.image_screen_rect = None;
        self.hover_pixel_info = None;
        if let Some(idx) = self.files.iter().position(|f| f == &path) {
            self.select(idx);
        }
    }

    /// Reload the current image (e.g. after a settings change like demosaic mode).
    pub fn reload_image(&mut self) {
        self.image = None;
        self.texture = None;
        self.histogram = None;
        self.hist_rx = None;
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
