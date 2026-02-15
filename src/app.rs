use crate::fits::{compute_histogram, ChannelView, DemosaicMode, FitsImage, HistogramData, Stretch};
use egui::TextureHandle;
use std::path::PathBuf;
use std::sync::mpsc;

enum LoadResult {
    Ok(Box<FitsImage>),
    Err(String),
}

pub struct FastFitsApp {
    /// egui context, stored so background threads can call request_repaint()
    ctx: egui::Context,

    /// Directory being browsed
    current_dir: PathBuf,
    /// Sorted list of FITS files in current_dir
    files: Vec<PathBuf>,
    /// Index into `files` of the currently selected file
    selected: Option<usize>,

    /// Currently loaded image (None if nothing loaded yet or on error)
    image: Option<FitsImage>,
    /// Cached egui texture for the current image/stretch/view combo
    texture: Option<TextureHandle>,
    /// Error message to show instead of an image
    load_error: Option<String>,

    /// Receiver for in-flight background load; None when idle
    load_rx: Option<mpsc::Receiver<LoadResult>>,

    /// Current stretch mode
    stretch: Stretch,
    /// Current channel view
    channel_view: ChannelView,

    /// Zoom: None = autofit, Some(s) = explicit scale factor
    zoom: Option<f32>,

    /// Result of the last delete attempt (shown briefly in the status bar)
    delete_status: Option<String>,
    /// Whether the keyboard shortcuts help popup is open
    show_help: bool,
    /// Whether the Preferences dialog is open
    show_prefs: bool,
    /// Whether the About dialog is open
    show_about: bool,
    /// Demosaic algorithm for Bayer images
    demosaic_mode: DemosaicMode,

    /// Filename being loaded (shown in center panel while loading)
    loading_name: Option<String>,

    /// Whether the histogram panel is visible (default: true)
    show_histogram: bool,
    /// Precomputed histogram for the current image (invalidated when image changes)
    histogram: Option<HistogramData>,
    /// Receiver for an in-flight background histogram computation; None when idle
    hist_rx: Option<mpsc::Receiver<HistogramData>>,
}

impl FastFitsApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, start_path: PathBuf) -> Self {
        let (current_dir, selected, files) = if start_path.is_file() {
            let dir = start_path
                .parent()
                .unwrap_or(&start_path)
                .to_path_buf();
            let files = collect_fits_files(&dir);
            let selected = files.iter().position(|f| f == &start_path);
            (dir, selected, files)
        } else {
            let files = collect_fits_files(&start_path);
            let selected = if files.is_empty() { None } else { Some(0) };
            (start_path, selected, files)
        };

        let mut app = Self {
            ctx: _cc.egui_ctx.clone(),
            current_dir,
            files,
            selected,
            image: None,
            texture: None,
            load_error: None,
            load_rx: None,
            stretch: Stretch::AutoStretch,
            channel_view: ChannelView::Rgb,
            zoom: None,
            delete_status: None,
            show_help: false,
            show_prefs: false,
            show_about: false,
            demosaic_mode: DemosaicMode::Bilinear,
            loading_name: None,
            show_histogram: true,
            histogram: None,
            hist_rx: None,
        };
        app.load_selected();
        app
    }

    /// Load (or reload) the currently selected file.
    fn load_selected(&mut self) {
        self.texture = None;
        self.histogram = None;
        self.hist_rx = None;
        self.load_error = None;
        self.image = None;

        let Some(idx) = self.selected else { return };
        let Some(path) = self.files.get(idx).cloned() else { return };

        match FitsImage::load(&path, self.demosaic_mode) {
            Ok(img) => {
                // Reset channel view based on the new image's channel count
                self.channel_view = if img.channels >= 3 {
                    ChannelView::Rgb
                } else {
                    ChannelView::Single(0)
                };
                self.image = Some(img);
            }
            Err(e) => {
                self.load_error = Some(format!("{e:#}"));
            }
        }
    }

    /// Rebuild the egui texture from the current image + stretch + channel_view.
    fn rebuild_texture(&mut self, ctx: &egui::Context) {
        let Some(img) = &self.image else { return };
        let rgba = img.to_rgba(self.stretch, self.channel_view);
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [img.width, img.height],
            &rgba,
        );
        self.texture = Some(ctx.load_texture(
            "fits_image",
            color_image,
            egui::TextureOptions::LINEAR,
        ));
    }

    fn select(&mut self, idx: usize) {
        if self.selected == Some(idx) { return; }
        self.selected = Some(idx);
        self.zoom = None;
        self.image = None;
        self.texture = None;
        self.histogram = None;
        self.hist_rx = None;
        self.load_error = None;
        self.load_rx = None; // drop any in-flight load

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
                Err(e) => LoadResult::Err(format!("{e:#}")),
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn select_next(&mut self) {
        if self.files.is_empty() { return; }
        let next = self.selected.map(|i| (i + 1) % self.files.len()).unwrap_or(0);
        self.select(next);
    }

    fn select_prev(&mut self) {
        if self.files.is_empty() { return; }
        let prev = self.selected.map(|i| {
            if i == 0 { self.files.len() - 1 } else { i - 1 }
        }).unwrap_or(0);
        self.select(prev);
    }

    /// Delete the currently selected file (trash if available, else permanent).
    /// Auto-advances to the next file.
    fn delete_selected(&mut self) {
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
                    // Stay at same index (now pointing to next file), or step back at end
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
    /// Reload the current image (e.g. after a settings change like demosaic mode).
    fn reload_image(&mut self) {
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
}

impl eframe::App for FastFitsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll background load result
        if let Some(rx) = &self.load_rx {
            if let Ok(result) = rx.try_recv() {
                self.load_rx = None;
                self.loading_name = None;
                match result {
                    LoadResult::Ok(img) => {
                        self.channel_view = if img.channels >= 3 {
                            ChannelView::Rgb
                        } else {
                            ChannelView::Single(0)
                        };
                        self.image = Some(*img);
                    }
                    LoadResult::Err(e) => {
                        self.load_error = Some(e);
                    }
                }
            }
        }

        // Keyboard shortcuts
        ctx.input(|i| {
            use egui::Key;
            if i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowDown) {
                // handled below after borrow ends
            }
            if i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowUp) {
                // handled below
            }
        });
        // Re-check in a non-borrowing way
        let go_next = ctx.input(|i| {
            i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowDown)
        });
        let go_prev = ctx.input(|i| {
            i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowUp)
        });
        let toggle_stretch = ctx.input(|i| i.key_pressed(egui::Key::S));
        let zoom_in = ctx.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals));
        let zoom_out = ctx.input(|i| i.key_pressed(egui::Key::Minus));
        let zoom_reset = ctx.input(|i| i.key_pressed(egui::Key::Num0));
        let zoom_fit = ctx.input(|i| i.key_pressed(egui::Key::F));
        let do_delete = ctx.input(|i| i.key_pressed(egui::Key::Delete));
        let toggle_help = ctx.input(|i| i.key_pressed(egui::Key::Questionmark));
        let toggle_prefs = ctx.input(|i| i.key_pressed(egui::Key::Comma));
        let toggle_histogram = ctx.input(|i| i.key_pressed(egui::Key::H));
        let toggle_about = ctx.input(|i| i.key_pressed(egui::Key::A));
        let close_popup = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        let mut go_next_btn = false;
        let mut go_prev_btn = false;
        let mut do_delete_btn = false;


        if go_next { self.select_next(); }
        if go_prev { self.select_prev(); }
        if toggle_stretch {
            self.stretch = match self.stretch {
                Stretch::AutoStretch => Stretch::Linear,
                Stretch::Linear => Stretch::AutoStretch,
            };
            self.texture = None;
            self.histogram = None;
            self.hist_rx = None;
        }
        if zoom_in {
            let s = self.zoom.unwrap_or(1.0);
            self.zoom = Some((s * 1.25).min(32.0));
        }
        if zoom_out {
            let s = self.zoom.unwrap_or(1.0);
            self.zoom = Some((s / 1.25).max(0.05));
        }
        if zoom_reset {
            self.zoom = Some(1.0);
        }
        if zoom_fit {
            self.zoom = None;
        }
        if do_delete {
            self.delete_selected();
        }
        if toggle_help {
            self.show_help = !self.show_help;
        }
        if toggle_prefs {
            self.show_prefs = !self.show_prefs;
        }
        if toggle_histogram {
            self.show_histogram = !self.show_histogram;
        }
        if toggle_about {
            self.show_about = !self.show_about;
        }
        if close_popup {
            self.show_help = false;
            self.show_prefs = false;
            self.show_about = false;
        }

        // Help popup
        if self.show_help {
            egui::Window::new("Keyboard shortcuts")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("help_grid").striped(true).show(ui, |ui| {
                        let rows: &[(&str, &str)] = &[
                            ("← / →  or  ↑ / ↓", "Previous / next file"),
                            ("Delete",             "Move current file to trash"),
                            ("S",                  "Toggle stretch (Auto ↔ Linear)"),
                            ("+  /  -",            "Zoom in / out"),
                            ("0",                  "Zoom to 1:1 (100 %)"),
                            ("F",                  "Zoom to fit"),
                            ("H",                  "Show / hide histogram"),
                            ("A",                  "Show / hide About"),
                            ("?",                  "Show / hide this help"),
                            (",",                  "Show / hide Preferences"),
                        ];
                        for (key, desc) in rows {
                            ui.label(egui::RichText::new(*key).monospace().strong());
                            ui.label(*desc);
                            ui.end_row();
                        }
                    });
                    ui.separator();
                    if ui.button("Close  [?]").clicked() {
                        self.show_help = false;
                    }
                });
        }

        // Preferences dialog
        if self.show_prefs {
            let mut reload = false;
            egui::Window::new("Preferences")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    let is_bayer = self.image.as_ref().map_or(false, |img| img.is_bayer);
                    if is_bayer {
                        ui.label("Demosaic algorithm");
                        ui.horizontal(|ui| {
                            if ui.selectable_label(
                                self.demosaic_mode == DemosaicMode::Bilinear, "Bilinear"
                            ).clicked() && self.demosaic_mode != DemosaicMode::Bilinear {
                                self.demosaic_mode = DemosaicMode::Bilinear;
                                reload = true;
                            }
                            if ui.selectable_label(
                                self.demosaic_mode == DemosaicMode::Cubic, "Cubic"
                            ).clicked() && self.demosaic_mode != DemosaicMode::Cubic {
                                self.demosaic_mode = DemosaicMode::Cubic;
                                reload = true;
                            }
                        });
                        ui.separator();
                    }
                    if ui.button("Close  [,]").clicked() {
                        self.show_prefs = false;
                    }
                });
            if reload { self.reload_image(); }
        }

        // About dialog
        if self.show_about {
            egui::Window::new("About fastfits")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("fastfits");
                        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    });
                    ui.separator();
                    egui::Grid::new("about_grid").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                        ui.label("Author");
                        ui.label(env!("CARGO_PKG_AUTHORS"));
                        ui.end_row();
                        ui.label("License");
                        ui.label(env!("CARGO_PKG_LICENSE"));
                        ui.end_row();
                        ui.label("Repository");
                        ui.hyperlink("https://github.com/praetp/fastfits");
                        ui.end_row();
                        ui.label("Built");
                        ui.label(env!("FASTFITS_BUILD_DATE"));
                        ui.end_row();
                        ui.label("Rust");
                        ui.label(env!("FASTFITS_RUSTC_VERSION"));
                        ui.end_row();
                    });
                    ui.separator();
                    if ui.button("Close  [A]").clicked() {
                        self.show_about = false;
                    }
                });
        }

        // Ensure texture is built
        if self.image.is_some() && self.texture.is_none() {
            self.rebuild_texture(ctx);
        }

        // Poll background histogram result
        if let Some(rx) = &self.hist_rx {
            if let Ok(hist) = rx.try_recv() {
                self.hist_rx = None;
                self.histogram = Some(hist);
            }
        }

        // Kick off background histogram computation once the image is ready and
        // no computation is already in flight.
        if self.image.is_some() && self.histogram.is_none() && self.hist_rx.is_none() {
            if let Some(img) = &self.image {
                // Clone the data the background thread needs — same cost as a memcpy,
                // but the UI thread is unblocked immediately after.
                let data = img.data.clone();
                let width = img.width;
                let height = img.height;
                let channels = img.channels;
                let bitdepth_max = img.bitdepth_max;
                let with_markers = self.stretch == Stretch::AutoStretch;
                let ctx2 = self.ctx.clone();
                let (tx, rx) = mpsc::channel();
                self.hist_rx = Some(rx);
                std::thread::spawn(move || {
                    // Reconstruct a minimal FitsImage shell that compute_histogram needs.
                    let img_shell = FitsImage {
                        width,
                        height,
                        channels,
                        data,
                        headers: vec![],
                        bitdepth_max,
                        is_bayer: false,
                    };
                    let _ = tx.send(compute_histogram(&img_shell, with_markers));
                    ctx2.request_repaint();
                });
            }
        }

        // Bottom toolbar: navigation + delete buttons + error status
        let has_files = !self.files.is_empty();
        let btn_size = egui::vec2(100.0, 32.0);
        egui::TopBottomPanel::bottom("nav_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                // Centre the three buttons by offsetting with half the remaining space.
                // 3 buttons * 100 + 2 gaps + 1 separator (≈12) ≈ 320
                let approx_buttons_w = btn_size.x * 3.0
                    + ui.spacing().item_spacing.x * 2.0
                    + 12.0;
                let offset = ((ui.available_width() - approx_buttons_w) / 2.0).max(0.0);
                ui.add_space(offset);

                let prev_btn = ui.add_enabled(
                    has_files,
                    egui::Button::new("< Prev").min_size(btn_size),
                ).on_hover_text("Previous file  [Left / Up]");
                if prev_btn.clicked() { go_prev_btn = true; }

                let next_btn = ui.add_enabled(
                    has_files,
                    egui::Button::new("Next >").min_size(btn_size),
                ).on_hover_text("Next file  [Right / Down]");
                if next_btn.clicked() { go_next_btn = true; }

                ui.separator();

                let del_btn = ui.add_enabled(
                    self.selected.is_some(),
                    egui::Button::new("Delete").min_size(btn_size),
                ).on_hover_text("Move file to trash  [Del]");
                if del_btn.clicked() { do_delete_btn = true; }

                if let Some(msg) = &self.delete_status.clone() {
                    ui.separator();
                    ui.label(egui::RichText::new(msg).color(egui::Color32::RED));
                    if ui.small_button("x").clicked() {
                        self.delete_status = None;
                    }
                }
            });
            ui.add_space(4.0);
        });

        if go_prev_btn { self.select_prev(); }
        if go_next_btn { self.select_next(); }
        if do_delete_btn { self.delete_selected(); }

        // Menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.label(egui::RichText::new("fastfits").strong());
                ui.separator();
                if let Some(idx) = self.selected {
                    if let Some(f) = self.files.get(idx) {
                        ui.label(f.file_name().unwrap_or_default().to_string_lossy().as_ref());
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // About button
                    if ui.button("About").on_hover_text("About fastfits  [A]").clicked() {
                        self.show_about = !self.show_about;
                    }
                    // Help button
                    if ui.button("?").on_hover_text("Show keyboard shortcuts  [?]").clicked() {
                        self.show_help = !self.show_help;
                    }
                    // Prefs button
                    if ui.button("Prefs").on_hover_text("Preferences  [,]").clicked() {
                        self.show_prefs = !self.show_prefs;
                    }
                    // Histogram toggle
                    if ui.selectable_label(self.show_histogram, "Hist")
                        .on_hover_text("Show / hide histogram  [H]")
                        .clicked()
                    {
                        self.show_histogram = !self.show_histogram;
                    }
                    ui.separator();

                    // Stretch toggle
                    let stretch_label = match self.stretch {
                        Stretch::AutoStretch => "Auto",
                        Stretch::Linear => "Linear",
                    };
                    if ui.selectable_label(true, stretch_label)
                        .on_hover_text("Toggle stretch mode  [S]")
                        .clicked()
                    {
                        self.stretch = match self.stretch {
                            Stretch::AutoStretch => Stretch::Linear,
                            Stretch::Linear => Stretch::AutoStretch,
                        };
                        self.texture = None;
                        self.histogram = None;
                        self.hist_rx = None;
                    }
                    ui.label("Stretch:").on_hover_text("Toggle stretch mode  [S]");
                    ui.separator();

                    // Channel selector (only for multi-channel images)
                    if let Some(img) = &self.image {
                        if img.channels >= 3 {
                            for ch in (0..img.channels).rev() {
                                let label = match ch { 0 => "R", 1 => "G", 2 => "B", _ => "?" };
                                let tip = match ch {
                                    0 => "Show red channel only",
                                    1 => "Show green channel only",
                                    2 => "Show blue channel only",
                                    _ => "Show channel",
                                };
                                if ui.selectable_label(self.channel_view == ChannelView::Single(ch), label)
                                    .on_hover_text(tip)
                                    .clicked()
                                {
                                    self.channel_view = ChannelView::Single(ch);
                                    self.texture = None;
                                }
                            }
                            if ui.selectable_label(self.channel_view == ChannelView::Rgb, "RGB")
                                .on_hover_text("Show composite RGB")
                                .clicked()
                            {
                                self.channel_view = ChannelView::Rgb;
                                self.texture = None;
                            }
                            ui.label("Channel:");
                            ui.separator();
                        }
                    }

                    // Zoom info
                    let zoom_str = match self.zoom {
                        None => "Fit".to_string(),
                        Some(s) => format!("{:.0}%", s * 100.0),
                    };
                    ui.label(zoom_str).on_hover_text("Zoom  [+] [-] [0=1:1] [F=fit]");
                    ui.label("Zoom:").on_hover_text("Zoom  [+] [-] [0=1:1] [F=fit]");
                });
            });
        });

        // Left panel: FITS headers
        egui::SidePanel::left("headers_panel")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.heading("Headers");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(img) = &self.image {
                        for (k, v) in &img.headers {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(k).strong().monospace());
                                ui.label(egui::RichText::new(v).monospace());
                            });
                        }
                    } else {
                        ui.label("(no file loaded)");
                    }
                });
            });

        // Right panel: file browser (+ histogram at top)
        egui::SidePanel::right("file_browser")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.heading("Files");
                ui.separator();

                // Histogram (shown above file list when enabled)
                if self.show_histogram {
                    if let Some(hist) = &self.histogram {
                        draw_histogram(ui, hist, self.stretch, self.channel_view);
                        ui.separator();
                    }
                }

                let dir_label = self
                    .current_dir
                    .file_name()
                    .unwrap_or(self.current_dir.as_os_str())
                    .to_string_lossy()
                    .to_string();
                ui.small(dir_label);
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut clicked = None;
                    for (i, path) in self.files.iter().enumerate() {
                        let name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let is_selected = self.selected == Some(i);
                        if ui.selectable_label(is_selected, &name)
                            .on_hover_text("Open file  [←/→ to navigate]  [Del to trash]")
                            .clicked()
                        {
                            clicked = Some(i);
                        }
                    }
                    if let Some(i) = clicked {
                        self.select(i);
                    }
                });
            });

        // Center panel: image viewport
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = &self.load_error {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(err).color(egui::Color32::RED));
                });
                return;
            }

            let Some(texture) = &self.texture else {
                ui.centered_and_justified(|ui| {
                    if let Some(name) = &self.loading_name {
                        ui.label(format!("Loading {}…", name));
                    } else {
                        ui.label("No file selected");
                    }
                });
                return;
            };

            let img_size = texture.size_vec2();
            let available = ui.available_size();

            let display_size = match self.zoom {
                None => {
                    // Autofit: scale to fill available area while preserving aspect ratio
                    let scale = (available.x / img_size.x).min(available.y / img_size.y);
                    img_size * scale
                }
                Some(s) => img_size * s,
            };

            egui::ScrollArea::both().show(ui, |ui| {
                ui.image((texture.id(), display_size));
            });
        });

    }
}

/// Draw a per-channel histogram into the current UI panel.
fn draw_histogram(
    ui: &mut egui::Ui,
    hist: &HistogramData,
    stretch: Stretch,
    view: ChannelView,
) {
    const HIST_HEIGHT: f32 = 80.0;

    let width = ui.available_width();
    let (resp, painter) = ui.allocate_painter(
        egui::Vec2::new(width, HIST_HEIGHT),
        egui::Sense::hover(),
    );
    let rect = resp.rect;

    // Dark background
    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(20));

    // Decide which channels to draw and their display colors (RGBA)
    let channel_indices: Vec<usize>;
    let channel_colors: Vec<egui::Color32>;
    match view {
        ChannelView::Single(c) => {
            // Only the selected channel, shown in gray
            let idx = c.min(hist.channels.len().saturating_sub(1));
            channel_indices = vec![idx];
            channel_colors = vec![egui::Color32::from_rgba_unmultiplied(200, 200, 200, 160)];
        }
        ChannelView::Rgb => {
            if hist.channels.len() == 1 {
                // Grayscale image
                channel_indices = vec![0];
                channel_colors = vec![egui::Color32::from_rgba_unmultiplied(200, 200, 200, 160)];
            } else {
                // RGB — draw R, G, B (back to front so B is on top)
                channel_indices = vec![0, 1, 2];
                channel_colors = vec![
                    egui::Color32::from_rgba_unmultiplied(220,  60,  60, 160),
                    egui::Color32::from_rgba_unmultiplied( 60, 180,  60, 160),
                    egui::Color32::from_rgba_unmultiplied( 60, 100, 220, 160),
                ];
            }
        }
    }

    // Global max count across all displayed channels (for log normalisation)
    let global_max = channel_indices
        .iter()
        .filter_map(|&ci| hist.channels.get(ci))
        .flat_map(|ch| ch.bins.iter())
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);

    let num_bins = hist.channels.first().map(|ch| ch.bins.len()).unwrap_or(256);
    let bin_w = width / num_bins as f32;

    // Draw bars for each channel
    for (&ci, &col) in channel_indices.iter().zip(channel_colors.iter()) {
        let Some(ch) = hist.channels.get(ci) else { continue };
        for (b, &count) in ch.bins.iter().enumerate() {
            let bar_h = ((count + 1) as f64).ln() / ((global_max + 1) as f64).ln()
                * HIST_HEIGHT as f64;
            let bar_h = bar_h as f32;
            let x0 = rect.left() + b as f32 * bin_w;
            let x1 = x0 + bin_w.max(1.0);
            let y0 = rect.bottom() - bar_h;
            let bar_rect = egui::Rect::from_min_max(
                egui::pos2(x0, y0),
                egui::pos2(x1, rect.bottom()),
            );
            painter.rect_filled(bar_rect, 0.0, col);
        }
    }

    // Draw stretch marker lines (only when AutoStretch is active)
    if stretch == Stretch::AutoStretch {
        let marker_colors = if hist.channels.len() == 1 || matches!(view, ChannelView::Single(_)) {
            // Grayscale / single channel: white markers
            vec![egui::Color32::WHITE]
        } else {
            vec![
                egui::Color32::from_rgb(220,  60,  60),
                egui::Color32::from_rgb( 60, 180,  60),
                egui::Color32::from_rgb( 60, 100, 220),
            ]
        };

        for (&ci, &mcol) in channel_indices.iter().zip(marker_colors.iter()) {
            let Some(ch) = hist.channels.get(ci) else { continue };
            for frac_opt in [ch.black_frac, ch.mid_frac, ch.white_frac] {
                if let Some(frac) = frac_opt {
                    let x = rect.left() + frac * width;
                    painter.line_segment(
                        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                        egui::Stroke::new(1.5, mcol),
                    );
                }
            }
        }
    }

    // Border
    painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, egui::Color32::from_gray(60)));
}

fn collect_fits_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && matches!(
                    p.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_ascii_lowercase())
                        .as_deref(),
                    Some("fits" | "fit" | "fz")
                )
        })
        .collect();
    files.sort();
    files
}
