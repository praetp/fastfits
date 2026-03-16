use crate::app::{FastFitsApp, LoadResult, SkyMarker};
use crate::fits::{ChannelView, FitsImage, Stretch, compute_histogram};
use crate::histogram_ui::draw_histogram;
use crate::wcs::{WcsTransform, format_ra, format_dec, clip_segment_to_image};
use std::sync::mpsc;

const MARKER_COLORS: [egui::Color32; 8] = [
    egui::Color32::from_rgb(255,  80,  80),  // red
    egui::Color32::from_rgb( 80, 200,  80),  // green
    egui::Color32::from_rgb( 80, 130, 255),  // blue
    egui::Color32::from_rgb(255, 220,   0),  // yellow
    egui::Color32::from_rgb(255, 140,   0),  // orange
    egui::Color32::from_rgb(200,  80, 200),  // purple
    egui::Color32::from_rgb(  0, 220, 220),  // cyan
    egui::Color32::from_rgb(255, 180, 100),  // amber
];
const MARKER_RADIUS_SCREEN: f32 = 14.0;
const MARKER_HIT_RADIUS: f32   = 18.0;

impl eframe::App for FastFitsApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "prefs", &crate::app::AppPrefs {
            show_grid:      self.show_grid,
            show_dso:       self.show_dso,
            stretch:        self.stretch,
            demosaic_mode:  self.demosaic_mode,
            show_histogram: self.show_histogram,
            show_clipping:  self.show_clipping,
            show_north_up:  self.show_north_up,
        });
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_background_loads(ctx);

        let open_file  = ctx.input(|i| i.key_pressed(egui::Key::O) && i.modifiers.command);
        let export_img = ctx.input(|i| i.key_pressed(egui::Key::E) && i.modifiers.command);
        // Suppress single-key shortcuts while a text field has focus.
        let typing = ctx.wants_keyboard_input();
        let zoomed = self.zoom.is_some();
        let go_next    = !typing && ctx.input(|i| !zoomed && (i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowDown)));
        let go_prev    = !typing && ctx.input(|i| !zoomed && (i.key_pressed(egui::Key::ArrowLeft)  || i.key_pressed(egui::Key::ArrowUp)));
        const NUDGE: f32 = 50.0;
        let nudge = if !typing { ctx.input(|i| {
            if !zoomed { return egui::Vec2::ZERO; }
            let mut d = egui::Vec2::ZERO;
            if i.key_pressed(egui::Key::ArrowLeft)  { d.x -= NUDGE; }
            if i.key_pressed(egui::Key::ArrowRight) { d.x += NUDGE; }
            if i.key_pressed(egui::Key::ArrowUp)    { d.y -= NUDGE; }
            if i.key_pressed(egui::Key::ArrowDown)  { d.y += NUDGE; }
            d
        })} else { egui::Vec2::ZERO };
        let toggle_stretch    = !typing && ctx.input(|i| i.key_pressed(egui::Key::S));
        let zoom_in    = !typing && ctx.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals));
        let zoom_out   = !typing && ctx.input(|i| i.key_pressed(egui::Key::Minus));
        let zoom_reset = !typing && ctx.input(|i| i.key_pressed(egui::Key::Num0));
        let zoom_fit   = !typing && ctx.input(|i| i.key_pressed(egui::Key::F));
        let do_delete  = !typing && ctx.input(|i| i.key_pressed(egui::Key::Delete));
        let toggle_help      = !typing && ctx.input(|i| i.key_pressed(egui::Key::Questionmark));
        let toggle_prefs     = !typing && ctx.input(|i| i.key_pressed(egui::Key::Comma));
        let toggle_histogram = !typing && ctx.input(|i| i.key_pressed(egui::Key::H));
        let toggle_about     = !typing && ctx.input(|i| i.key_pressed(egui::Key::A));
        let toggle_grid      = !typing && ctx.input(|i| i.key_pressed(egui::Key::G));
        let toggle_dso       = !typing && ctx.input(|i| i.key_pressed(egui::Key::D));
        let toggle_clipping  = !typing && ctx.input(|i| i.key_pressed(egui::Key::C));
        let toggle_north_up  = !typing && ctx.input(|i| i.key_pressed(egui::Key::N));
        let close_popup      = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let do_quit          = !typing && ctx.input(|i| i.key_pressed(egui::Key::Q));

        if go_next    { self.select_next(); }
        if go_prev    { self.select_prev(); }
        self.pan_offset += nudge;
        if do_delete  { self.delete_selected(); }
        if zoom_in    { let s = self.zoom.unwrap_or(1.0); self.zoom = Some((s * 1.25).min(32.0)); }
        if zoom_out   { let s = self.zoom.unwrap_or(1.0); self.zoom = Some((s / 1.25).max(0.05)); }
        if zoom_reset { self.zoom = Some(1.0); }
        if zoom_fit   { self.zoom = None; self.pan_offset = egui::Vec2::ZERO; }
        if toggle_help      { self.show_help      = !self.show_help; }
        if toggle_prefs     { self.show_prefs     = !self.show_prefs; }
        if toggle_histogram { self.show_histogram = !self.show_histogram; }
        if toggle_about     { self.show_about     = !self.show_about; }
        if toggle_grid      { self.show_grid      = !self.show_grid; }
        if toggle_dso       { self.show_dso       = !self.show_dso; }
        if toggle_clipping  { self.show_clipping  = !self.show_clipping; self.texture = None; }
        if toggle_north_up  { self.show_north_up  = !self.show_north_up; }
        if toggle_stretch {
            self.stretch = match self.stretch {
                Stretch::AutoStretch => Stretch::Linear,
                Stretch::Linear      => Stretch::AutoStretch,
            };
            self.texture  = None;
            self.histogram = None;
            self.hist_rx  = None;
        }
        if do_quit { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
        if close_popup {
            self.show_help  = false;
            self.show_prefs = false;
            self.show_about = false;
            if typing { ctx.memory_mut(|m| m.stop_text_input()); }
        }

        let title = match self.selected.and_then(|i| self.files.get(i)) {
            Some(p) => format!("fastfits — {}", p.file_name().unwrap_or_default().to_string_lossy()),
            None    => "fastfits".to_string(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        self.show_help_window(ctx);
        if self.show_prefs_window(ctx) { self.reload_image(); }
        self.show_about_window(ctx);

        if self.image.is_some() && self.texture.is_none() {
            self.rebuild_texture(ctx);
        }
        self.maybe_start_histogram();
        if let Some(rx) = &self.hist_rx {
            if let Ok(hist) = rx.try_recv() {
                self.hist_rx  = None;
                self.histogram = Some(hist);
            }
        }

        self.maybe_start_seeing();
        if let Some(rx) = &self.seeing_rx {
            if let Ok(result) = rx.try_recv() {
                self.seeing_rx = None;
                self.seeing = Some(result);
            }
        }

        let (go_prev_btn, go_next_btn, do_delete_btn) = self.show_bottom_bar(ctx);
        if go_prev_btn   { self.select_prev(); }
        if go_next_btn   { self.select_next(); }
        if do_delete_btn { self.delete_selected(); }

        let (open_btn, export_jpg_btn, export_png_btn) = self.show_menu_bar(ctx);
        if open_file || open_btn {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("FITS files", &["fits", "fit", "fz"])
                .set_directory(&self.current_dir)
                .pick_file()
            {
                self.open_path(path);
            }
        }
        if export_img || export_jpg_btn { self.export_jpeg(); }
        if export_png_btn               { self.export_png(); }
        self.show_left_panel(ctx);
        self.show_right_panel(ctx);
        self.show_center_panel(ctx);
    }
}

impl FastFitsApp {
    fn poll_background_loads(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.load_rx {
            if let Ok(result) = rx.try_recv() {
                self.load_rx      = None;
                self.loading_name = None;
                match result {
                    LoadResult::Ok(img) => {
                        self.channel_view = if img.channels >= 3 {
                            ChannelView::Rgb
                        } else {
                            ChannelView::Single(0)
                        };
                        self.wcs = WcsTransform::from_headers(&img.headers);
                        self.image = Some(*img);
                    }
                    LoadResult::Err(e) => {
                        self.load_error = Some(e);
                    }
                }
                ctx.request_repaint();
            }
        }
    }

    /// Kick off a background histogram computation if needed.
    fn maybe_start_histogram(&mut self) {
        if self.image.is_none() || self.histogram.is_some() || self.hist_rx.is_some() {
            return;
        }
        if let Some(img) = &self.image {
            let data         = img.data.clone();
            let width        = img.width;
            let height       = img.height;
            let channels     = img.channels;
            let bitdepth_max = img.bitdepth_max;
            let with_markers = self.stretch == Stretch::AutoStretch;
            let ctx2         = self.ctx.clone();
            let (tx, rx)     = mpsc::channel();
            self.hist_rx     = Some(rx);
            std::thread::spawn(move || {
                let img_shell = FitsImage {
                    width, height, channels, data,
                    headers: vec![], bitdepth_max, is_bayer: false, raw_bayer: None,
                };
                let _ = tx.send(compute_histogram(&img_shell, with_markers));
                ctx2.request_repaint();
            });
        }
    }

    fn show_help_window(&mut self, ctx: &egui::Context) {
        if !self.show_help { return; }
        egui::Window::new("Keyboard shortcuts")
            .open(&mut self.show_help)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::Grid::new("help_grid").striped(true).show(ui, |ui| {
                    let rows: &[(&str, &str)] = &[
                        ("Ctrl+O",             "Open file dialog"),
                        ("Ctrl+E",             "Export current view as JPEG"),
                        ("← / →  or  ↑ / ↓", "Previous / next file  (pan when zoomed)"),
                        ("Delete",             "Move current file to trash"),
                        ("S",                  "Toggle stretch (Auto ↔ Linear)"),
                        ("+  /  -",            "Zoom in / out"),
                        ("0",                  "Zoom to 1:1 (100 %)"),
                        ("F",                  "Zoom to fit"),
                        ("H",                  "Show / hide histogram"),
                        ("G",                  "Show / hide WCS coordinate grid"),
                        ("D",                  "Show / hide DSO catalogue overlay"),
                        ("C",                  "Show / hide clipping overlay (overexposed pixels → red)"),
                        ("N",                  "Rotate image: North up, East left (requires WCS)"),
                        ("A",                  "Show / hide About"),
                        ("?",                  "Show / hide this help"),
                        (",",                  "Show / hide Preferences"),
                        ("Q",                  "Quit"),
                    ];
                    for (key, desc) in rows {
                        ui.label(egui::RichText::new(*key).monospace().strong());
                        ui.label(*desc);
                        ui.end_row();
                    }
                });
            });
    }

    /// Returns true if the image should be reloaded.
    fn show_prefs_window(&mut self, ctx: &egui::Context) -> bool {
        if !self.show_prefs { return false; }
        let mut reload = false;
        egui::Window::new("Preferences")
            .open(&mut self.show_prefs)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let is_bayer = self.image.as_ref().map_or(false, |img| img.is_bayer);
                if is_bayer {
                    ui.label("Demosaic algorithm");
                    ui.horizontal(|ui| {
                        use crate::fits::DemosaicMode;
                        if ui.selectable_label(self.demosaic_mode == DemosaicMode::Bilinear, "Bilinear")
                            .clicked() && self.demosaic_mode != DemosaicMode::Bilinear
                        {
                            self.demosaic_mode = DemosaicMode::Bilinear;
                            reload = true;
                        }
                        if ui.selectable_label(self.demosaic_mode == DemosaicMode::Cubic, "Cubic")
                            .clicked() && self.demosaic_mode != DemosaicMode::Cubic
                        {
                            self.demosaic_mode = DemosaicMode::Cubic;
                            reload = true;
                        }
                    });
                    ui.separator();
                }
            });
        reload
    }

    fn show_about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about { return; }
        egui::Window::new("About fastfits")
            .open(&mut self.show_about)
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
                    ui.label("Author");    ui.label(env!("CARGO_PKG_AUTHORS")); ui.end_row();
                    ui.label("License");  ui.label(env!("CARGO_PKG_LICENSE")); ui.end_row();
                    ui.label("Repository"); ui.hyperlink("https://github.com/praetp/fastfits"); ui.end_row();
                    ui.label("Built");    ui.label(env!("FASTFITS_BUILD_DATE")); ui.end_row();
                    ui.label("Rust");     ui.label(env!("FASTFITS_RUSTC_VERSION")); ui.end_row();
                });
            });
    }

    fn show_menu_bar(&mut self, ctx: &egui::Context) -> (bool, bool, bool) {
        let mut open_clicked = false;
        let mut export_jpg_clicked = false;
        let mut export_png_clicked = false;
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.label(egui::RichText::new("fastfits").strong());
                ui.separator();
                if ui.button("Open…").on_hover_text("Open a FITS file  [Ctrl+O]").clicked() {
                    open_clicked = true;
                }
                let has_image = self.image.is_some();
                if ui.add_enabled(has_image, egui::Button::new("Export JPG"))
                    .on_hover_text("Save current view as JPEG  [Ctrl+E]").clicked()
                {
                    export_jpg_clicked = true;
                }
                if ui.add_enabled(has_image, egui::Button::new("Export PNG"))
                    .on_hover_text("Save current view as PNG").clicked()
                {
                    export_png_clicked = true;
                }
                ui.separator();
                if let Some(idx) = self.selected {
                    if let Some(f) = self.files.get(idx) {
                        ui.label(f.file_name().unwrap_or_default().to_string_lossy().as_ref());
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("About").on_hover_text("About fastfits  [A]").clicked() {
                        self.show_about = !self.show_about;
                    }
                    if ui.button("?").on_hover_text("Show keyboard shortcuts  [?]").clicked() {
                        self.show_help = !self.show_help;
                    }
                    if ui.button("Prefs").on_hover_text("Preferences  [,]").clicked() {
                        self.show_prefs = !self.show_prefs;
                    }
                    if ui.selectable_label(self.show_histogram, "Hist")
                        .on_hover_text("Show / hide histogram  [H]").clicked()
                    {
                        self.show_histogram = !self.show_histogram;
                    }
                    {
                        let has_wcs = self.wcs.is_some();
                        let tip_grid = if has_wcs {
                            "Show / hide WCS coordinate grid  [G]"
                        } else {
                            "No WCS headers in this file — coordinate grid unavailable"
                        };
                        if ui.add_enabled(has_wcs, egui::SelectableLabel::new(self.show_grid, "Grid"))
                            .on_hover_text(tip_grid)
                            .on_disabled_hover_text(tip_grid)
                            .clicked()
                        {
                            self.show_grid = !self.show_grid;
                        }
                        let tip_dso = if has_wcs {
                            "Show / hide DSO catalogue overlay  [D]"
                        } else {
                            "No WCS headers in this file — DSO overlay unavailable"
                        };
                        if ui.add_enabled(has_wcs, egui::SelectableLabel::new(self.show_dso, "DSO"))
                            .on_hover_text(tip_dso)
                            .on_disabled_hover_text(tip_dso)
                            .clicked()
                        {
                            self.show_dso = !self.show_dso;
                        }
                        let tip_eq = if has_wcs {
                            "Rotate: North up, East left  [N]"
                        } else {
                            "No WCS headers in this file — equatorial orientation unavailable"
                        };
                        if ui.add_enabled(has_wcs, egui::SelectableLabel::new(self.show_north_up, "Equatorial orientation"))
                            .on_hover_text(tip_eq)
                            .on_disabled_hover_text(tip_eq)
                            .clicked()
                        {
                            self.show_north_up = !self.show_north_up;
                        }
                    }
                    if ui.selectable_label(self.show_clipping, "Clip")
                        .on_hover_text("Show / hide clipping overlay  [C]").clicked()
                    {
                        self.show_clipping = !self.show_clipping;
                        self.texture = None;
                    }
                    ui.separator();
                    self.draw_stretch_and_channels(ui);
                });
            });
        });
        (open_clicked, export_jpg_clicked, export_png_clicked)
    }

    fn draw_stretch_and_channels(&mut self, ui: &mut egui::Ui) {
        let zoom_str = match self.zoom {
            None => {
                // Compute the actual fit scale from the last known image rect.
                if let (Some(img), Some(rect)) = (&self.image, self.image_screen_rect) {
                    let s = (rect.width() / img.width as f32)
                        .min(rect.height() / img.height as f32);
                    format!("{:.0}%", s * 100.0)
                } else {
                    "Fit".to_string()
                }
            }
            Some(s) => format!("{:.0}%", s * 100.0),
        };
        ui.label(zoom_str).on_hover_text("Zoom  [+] [-] [0=1:1] [F=fit]");
        if ui.button("Fit").on_hover_text("Zoom to fit  [F]").clicked() {
            self.zoom = None;
            self.pan_offset = egui::Vec2::ZERO;
        }
        ui.label("Zoom:").on_hover_text("Zoom  [+] [-] [0=1:1] [F=fit]");
        ui.separator();

        if let Some(img) = &self.image {
            if img.is_bayer {
                if ui.selectable_label(self.show_raw_bayer, "Raw")
                    .on_hover_text("Show raw Bayer sensor data without debayering")
                    .clicked()
                {
                    self.show_raw_bayer = !self.show_raw_bayer;
                    self.texture = None;
                }
                ui.separator();
            }
            if img.channels >= 3 && !self.show_raw_bayer {
                for ch in (0..img.channels).rev() {
                    let label = match ch { 0 => "R", 1 => "G", 2 => "B", _ => "?" };
                    let tip = match ch {
                        0 => "Show red channel only",
                        1 => "Show green channel only",
                        2 => "Show blue channel only",
                        _ => "Show channel",
                    };
                    if ui.selectable_label(self.channel_view == ChannelView::Single(ch), label)
                        .on_hover_text(tip).clicked()
                    {
                        self.channel_view = ChannelView::Single(ch);
                        self.texture = None;
                    }
                }
                if ui.selectable_label(self.channel_view == ChannelView::Rgb, "RGB")
                    .on_hover_text("Show composite RGB").clicked()
                {
                    self.channel_view = ChannelView::Rgb;
                    self.texture = None;
                }
                ui.label("Channel:");
                ui.separator();
            }
        }

        let stretch_label = match self.stretch {
            Stretch::AutoStretch => "Auto",
            Stretch::Linear      => "Linear",
        };
        if ui.selectable_label(true, stretch_label)
            .on_hover_text("Toggle stretch mode  [S]").clicked()
        {
            self.stretch = match self.stretch {
                Stretch::AutoStretch => Stretch::Linear,
                Stretch::Linear      => Stretch::AutoStretch,
            };
            self.texture   = None;
            self.histogram = None;
            self.hist_rx   = None;
        }
        ui.label("Stretch:").on_hover_text("Toggle stretch mode  [S]");
        ui.separator();
    }

    fn show_bottom_bar(&mut self, ctx: &egui::Context) -> (bool, bool, bool) {
        let has_files    = !self.files.is_empty();
        let has_selected = self.selected.is_some();
        let btn_size     = egui::vec2(100.0, 32.0);
        let mut go_prev      = false;
        let mut go_next      = false;
        let mut do_del       = false;
        let mut clear_status = false;

        egui::TopBottomPanel::bottom("nav_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            let full_width = ui.available_width();
            let third      = full_width / 3.0;
            let row_height = btn_size.y;

            ui.horizontal(|ui| {
                // LEFT: spacer to keep the centre buttons centred.
                ui.allocate_exact_size(egui::vec2(third, row_height), egui::Sense::hover());

                // CENTRE: nav + delete buttons, horizontally centered.
                ui.allocate_ui_with_layout(
                    egui::vec2(third, row_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.horizontal(|ui| {
                            if ui.add_enabled(has_files, egui::Button::new("< Prev").min_size(btn_size))
                                .on_hover_text("Previous file  [Left / Up]").clicked()
                            {
                                go_prev = true;
                            }
                            if ui.add_enabled(has_files, egui::Button::new("Next >").min_size(btn_size))
                                .on_hover_text("Next file  [Right / Down]").clicked()
                            {
                                go_next = true;
                            }
                            ui.separator();
                            if ui.add_enabled(has_selected, egui::Button::new("Delete").min_size(btn_size))
                                .on_hover_text("Move file to trash  [Del]").clicked()
                            {
                                do_del = true;
                            }
                        });
                    },
                );

                // RIGHT: delete-error status, right-aligned.
                ui.allocate_ui_with_layout(
                    egui::vec2(third, row_height),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if let Some(msg) = &self.delete_status {
                            if ui.small_button("x").clicked() { clear_status = true; }
                            ui.label(egui::RichText::new(msg).color(egui::Color32::RED));
                        }
                    },
                );
            });
            ui.add_space(4.0);
        });

        if clear_status { self.delete_status = None; }
        (go_prev, go_next, do_del)
    }

    fn show_left_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("headers_panel")
            .resizable(true)
            .min_width(100.0)
            .max_width(500.0)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.heading("Headers");
                ui.separator();
                ui.horizontal(|ui| {
                    // Always reserve button space so TextEdit width stays constant.
                    let btn = ui.add_visible(
                        !self.header_filter.is_empty(),
                        egui::Button::new("✕").small(),
                    );
                    if btn.clicked() { self.header_filter.clear(); }
                    ui.add(egui::TextEdit::singleline(&mut self.header_filter)
                        .hint_text("Search…")
                        .desired_width(ui.available_width()));
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(img) = &self.image {
                        let needle = self.header_filter.to_ascii_lowercase();
                        for (k, v) in &img.headers {
                            if !needle.is_empty()
                                && !k.to_ascii_lowercase().contains(&needle)
                                && !v.to_ascii_lowercase().contains(&needle)
                            {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                ui.add(egui::Label::new(egui::RichText::new(k).strong().monospace()).selectable(true));
                                ui.add(egui::Label::new(egui::RichText::new(v).monospace()).selectable(true));
                            });
                        }
                    } else {
                        ui.label("(no file loaded)");
                    }
                });
            });
    }

    fn show_right_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("file_browser")
            .resizable(true)
            .min_width(100.0)
            .max_width(500.0)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.heading("Files");
                ui.separator();

                if self.show_histogram {
                    if let Some(hist) = &self.histogram {
                        draw_histogram(ui, hist, self.stretch, self.channel_view);
                        ui.separator();
                    }
                }

                if self.image.is_some() {
                    egui::Grid::new("seeing_grid").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
                        match &self.seeing {
                            None => {
                                // Thread not yet started or still running
                                ui.label(egui::RichText::new("Seeing:").strong());
                                ui.label("measuring…");
                                ui.end_row();
                                ui.label(egui::RichText::new("FWHM:").strong());
                                ui.label("measuring…");
                                ui.end_row();
                            }
                            Some(None) => {
                                // Computation finished but no usable stars found
                                ui.label(egui::RichText::new("Seeing:").strong());
                                ui.label("— (no stars detected)");
                                ui.end_row();
                                ui.label(egui::RichText::new("FWHM:").strong());
                                ui.label("— (no stars detected)");
                                ui.end_row();
                            }
                            Some(Some(s)) => {
                                let (samp_color, samp_label) = sampling_quality(s.fwhm_px);
                                ui.label(egui::RichText::new("Seeing:").strong());
                                if let (Some(fsec), Some(esec)) = (s.fwhm_arcsec, s.error_arcsec) {
                                    let (see_color, see_label) = seeing_quality(fsec);
                                    ui.label(egui::RichText::new(
                                        format!("{:.1}″ ± {:.1}″   {} stars [{}]", fsec, esec, s.star_count, see_label)
                                    ).color(see_color)).on_hover_ui(|ui| seeing_tooltip(ui));
                                } else {
                                    // No WCS — report in pixels; arcsec conversion unavailable.
                                    // Color by sampling since we have no atmospheric quality info.
                                    ui.label(egui::RichText::new(
                                        format!("{:.1} px ± {:.1} px   {} stars [{}]", s.fwhm_px, s.error_px, s.star_count, samp_label)
                                    ).color(samp_color)).on_hover_ui(|ui| sampling_tooltip(ui));
                                }
                                ui.end_row();
                                ui.label(egui::RichText::new("FWHM:").strong());
                                ui.label(egui::RichText::new(
                                    format!("{:.1} px ± {:.1} px [{}]", s.fwhm_px, s.error_px, samp_label)
                                ).color(samp_color)).on_hover_ui(|ui| sampling_tooltip(ui));
                                ui.end_row();
                            }
                        }
                    });
                    ui.separator();
                }

                let dir_label = self.current_dir
                    .file_name().unwrap_or(self.current_dir.as_os_str())
                    .to_string_lossy().to_string();
                ui.small(dir_label);
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut clicked = None;
                    for (i, path) in self.files.iter().enumerate() {
                        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        let is_selected = self.selected == Some(i);
                        if ui.selectable_label(is_selected, &name)
                            .on_hover_text("Open file  [←/→ to navigate]  [Del to trash]")
                            .clicked()
                        {
                            clicked = Some(i);
                        }
                    }
                    if let Some(i) = clicked { self.select(i); }
                });
            });
    }

    fn show_center_panel(&mut self, ctx: &egui::Context) {
        // Read input before borrowing self into the closure.
        let pointer_pos  = ctx.input(|i| i.pointer.hover_pos());
        let scroll_delta = ctx.input(|i| i.smooth_scroll_delta);
        let right_clicked_pos: Option<egui::Pos2> = ctx.input(|i| {
            if i.pointer.button_clicked(egui::PointerButton::Secondary) {
                i.pointer.interact_pos()
            } else {
                None
            }
        });

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

            let img_size  = texture.size_vec2();
            let available = ui.available_size();

            // Compute zoom factor and display size.
            let (display_size, zoom_factor) = match self.zoom {
                None => {
                    let s = (available.x / img_size.x).min(available.y / img_size.y);
                    (img_size * s, s)
                }
                Some(s) => (img_size * s, s),
            };
            // Autofit → always centered, no pan.
            if self.zoom.is_none() {
                self.pan_offset = egui::Vec2::ZERO;
            }

            // Allocate the full panel as an interactive surface.
            let (resp, painter) =
                ui.allocate_painter(available, egui::Sense::click_and_drag());
            let panel_rect = resp.rect;

            // Zoom-to-cursor via scroll wheel.
            if scroll_delta.y != 0.0 {
                if let Some(pos) = pointer_pos {
                    let inside = self.image_screen_rect.map_or(false, |r| r.contains(pos));
                    if inside {
                        let old_zoom = zoom_factor;
                        let new_zoom = (old_zoom * (1.1f32).powf(scroll_delta.y / 50.0))
                            .clamp(0.05, 32.0);
                        self.zoom = Some(new_zoom);
                        // Shift pan so the pixel under the cursor stays fixed.
                        let cursor_rel = pos - panel_rect.center() - self.pan_offset;
                        self.pan_offset += cursor_rel * (1.0 - new_zoom / old_zoom);
                    }
                }
            }

            // Drag to pan.
            if resp.dragged() {
                self.pan_offset += resp.drag_delta();
            }

            // Clamp pan so the image stays reachable.
            let max_pan = (display_size + available) * 0.5;
            self.pan_offset = self.pan_offset.clamp(-max_pan, max_pan);

            let image_rect = egui::Rect::from_center_size(
                panel_rect.center() + self.pan_offset, display_size,
            );

            // North-up rotation parameters.
            let (north_angle, east_flip) = if self.show_north_up {
                if let Some(wcs) = &self.wcs {
                    (wcs.north_up_angle() as f32, wcs.east_needs_flip())
                } else { (0.0, false) }
            } else { (0.0, false) };
            let rotating = north_angle.abs() > 1e-4 || east_flip;

            let img_cx = img_size.x / 2.0;
            let img_cy = img_size.y / 2.0;
            let image_center_screen = panel_rect.center() + self.pan_offset;

            // Maps a pixel (col, row) → screen Pos2, applying optional flip + rotation.
            let pixel_to_screen = |col: f32, row: f32| -> egui::Pos2 {
                let mut rel = egui::vec2(col - img_cx, row - img_cy) * zoom_factor;
                if east_flip { rel.x = -rel.x; }
                let cos = north_angle.cos();
                let sin = north_angle.sin();
                let r = egui::vec2(rel.x * cos - rel.y * sin, rel.x * sin + rel.y * cos);
                image_center_screen + r
            };

            // Draw the image (rotated mesh or plain rect).
            if rotating {
                use egui::epaint::{Mesh, Vertex};
                let mut mesh = Mesh::with_texture(texture.id());
                let (u0, u1) = if east_flip { (1.0f32, 0.0f32) } else { (0.0f32, 1.0f32) };
                let corners = [
                    (0.0f32,        0.0f32,        u0, 0.0f32),
                    (img_cx * 2.0,  0.0,           u1, 0.0),
                    (img_cx * 2.0,  img_cy * 2.0,  u1, 1.0),
                    (0.0,           img_cy * 2.0,  u0, 1.0),
                ];
                for (c, r, u, v) in &corners {
                    mesh.vertices.push(Vertex {
                        pos: pixel_to_screen(*c, *r),
                        uv: egui::pos2(*u, *v),
                        color: egui::Color32::WHITE,
                    });
                }
                mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                painter.add(egui::Shape::mesh(mesh));
            } else {
                painter.image(
                    texture.id(),
                    image_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }

            // Compute AABB of the (possibly rotated) image for hit-testing.
            let aabb = if rotating {
                let pts = [
                    (0.0f32,       0.0f32),
                    (img_cx * 2.0, 0.0),
                    (img_cx * 2.0, img_cy * 2.0),
                    (0.0,          img_cy * 2.0),
                ].map(|(c, r)| pixel_to_screen(c, r));
                let xs = pts.map(|p| p.x);
                let ys = pts.map(|p| p.y);
                egui::Rect::from_min_max(
                    egui::pos2(xs.into_iter().fold(f32::INFINITY,     f32::min),
                               ys.into_iter().fold(f32::INFINITY,     f32::min)),
                    egui::pos2(xs.into_iter().fold(f32::NEG_INFINITY, f32::max),
                               ys.into_iter().fold(f32::NEG_INFINITY, f32::max)),
                )
            } else {
                image_rect
            };
            self.image_screen_rect = Some(aabb);

            // Right-click: add or remove a sky marker.
            if let (Some(pos), Some(wcs)) = (right_clicked_pos, self.wcs.as_ref()) {
                if aabb.contains(pos) {
                    // Inverse transform: screen → pixel coords.
                    let cursor_rel = (pos - image_center_screen) / zoom_factor;
                    let cos = north_angle.cos();
                    let sin = north_angle.sin();
                    let mut unrot = egui::vec2(
                         cursor_rel.x * cos + cursor_rel.y * sin,
                        -cursor_rel.x * sin + cursor_rel.y * cos,
                    );
                    if east_flip { unrot.x = -unrot.x; }
                    let col = unrot.x + img_cx;
                    let row = unrot.y + img_cy;
                    if col >= 0.0 && row >= 0.0 && col < img_size.x && row < img_size.y {
                        if let Some((ra, dec)) = wcs.pixel_to_sky(col as f64, row as f64) {
                            // Check if click is near an existing marker (in screen space).
                            let wcs_ref = self.wcs.as_ref().unwrap();
                            let hit = self.markers.iter().position(|m| {
                                if let Some((mc, mr)) = wcs_ref.sky_to_pixel(m.ra, m.dec) {
                                    let sp = pixel_to_screen(mc as f32, mr as f32);
                                    sp.distance(pos) < MARKER_HIT_RADIUS
                                } else {
                                    false
                                }
                            });
                            if let Some(idx) = hit {
                                self.markers.remove(idx);
                            } else if self.markers.len() < 8 {
                                let color_idx = self.markers.len();
                                self.markers.push(SkyMarker { ra, dec, color_idx });
                            }
                        }
                    }
                }
            }

            // WCS grid overlay.
            if self.show_grid {
                if let (Some(wcs), Some(img)) = (&self.wcs, &self.image) {
                    let (ra_lines, dec_lines) = wcs.grid_lines(img.width, img.height, 64);
                    let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 100, 180));
                    let label_color = egui::Color32::from_rgba_unmultiplied(255, 255, 100, 220);
                    let iw = img.width as f64;
                    let ih = img.height as f64;
                    for line in ra_lines.iter().chain(dec_lines.iter()) {
                        for seg in &line.segments {
                            for pair in seg.windows(2) {
                                let Some((c0, c1)) = clip_segment_to_image(pair[0], pair[1], iw, ih) else { continue };
                                let p0 = pixel_to_screen(c0.0 as f32, c0.1 as f32);
                                let p1 = pixel_to_screen(c1.0 as f32, c1.1 as f32);
                                painter.line_segment([p0, p1], stroke);
                            }
                        }
                        if let Some(lp) = line.label_pos {
                            let lp_clamped = (
                                lp.0.clamp(0.0, iw - 1.0),
                                lp.1.clamp(0.0, ih - 1.0),
                            );
                            let sp = pixel_to_screen(lp_clamped.0 as f32, lp_clamped.1 as f32);
                            painter.text(
                                sp,
                                egui::Align2::CENTER_CENTER,
                                &line.label,
                                egui::FontId::proportional(11.0),
                                label_color,
                            );
                        }
                    }
                }
            }

            // DSO catalogue overlay.
            if self.show_dso {
                if let (Some(wcs), Some(img)) = (&self.wcs, &self.image) {
                    let scale_arcsec = wcs.pixel_scale_deg * 3600.0;
                    for (entry, col_px, row_px) in crate::dso::visible_objects(
                        crate::dso::catalogue(), wcs, img.width, img.height, 50.0,
                    ) {
                        let sc = pixel_to_screen(col_px as f32, row_px as f32);
                        // maj_ax_arcmin is diameter; *30 = half in arcsec → half-radius in px
                        let r = if scale_arcsec > 0.0 {
                            (entry.maj_ax_arcmin * 30.0 / scale_arcsec as f32) * zoom_factor
                        } else {
                            0.0
                        }.max(5.0);
                        let color = entry.dso_type.color();
                        painter.circle_stroke(sc, r, egui::Stroke::new(1.2, color));
                        painter.text(
                            sc - egui::vec2(0.0, r + 2.0),
                            egui::Align2::CENTER_BOTTOM,
                            &entry.name,
                            egui::FontId::proportional(10.0),
                            color,
                        );
                    }
                }
            }

            // Sky marker overlay.
            if let Some(wcs) = &self.wcs {
                for marker in &self.markers {
                    if let Some((mc, mr)) = wcs.sky_to_pixel(marker.ra, marker.dec) {
                        let sp = pixel_to_screen(mc as f32, mr as f32);
                        if aabb.contains(sp) {
                            let color = MARKER_COLORS[marker.color_idx % 8];
                            let r = MARKER_RADIUS_SCREEN * zoom_factor;
                            painter.circle_stroke(sp, r, egui::Stroke::new(2.0, color));
                        }
                    }
                }
            }

            // Crosshair overlay.
            if let Some(pos) = pointer_pos {
                if aabb.contains(pos) {
                    let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 120));
                    // Horizontal line across the image at cursor y.
                    painter.line_segment(
                        [egui::pos2(aabb.min.x, pos.y), egui::pos2(aabb.max.x, pos.y)],
                        stroke,
                    );
                    // Vertical line across the image at cursor x.
                    painter.line_segment(
                        [egui::pos2(pos.x, aabb.min.y), egui::pos2(pos.x, aabb.max.y)],
                        stroke,
                    );
                }
            }

            // Pixel value under cursor.
            self.hover_pixel_info = None;
            if let (Some(pos), Some(img)) = (pointer_pos, &self.image) {
                if aabb.contains(pos) {
                    // Inverse transform: screen → pixel coords.
                    let cursor_rel = (pos - image_center_screen) / zoom_factor;
                    let cos = north_angle.cos();
                    let sin = north_angle.sin();
                    // Inverse rotation (transpose):
                    let mut unrot = egui::vec2(
                         cursor_rel.x * cos + cursor_rel.y * sin,
                        -cursor_rel.x * sin + cursor_rel.y * cos,
                    );
                    if east_flip { unrot.x = -unrot.x; }
                    let col = (unrot.x + img_cx).floor();
                    let row = (unrot.y + img_cy).floor();
                    let x = (col as usize).min(img.width.saturating_sub(1));
                    let y = (row as usize).min(img.height.saturating_sub(1));
                    let npix = img.width * img.height;
                    let idx  = y * img.width + x;
                    let raw_val = img.raw_bayer.as_ref().map(|r| r[idx]);
                    let fmt = |v: f32| fmt_pixel(v, img.bitdepth_max);
                    let pixel_str = if self.show_raw_bayer {
                        format!("raw={}", fmt(raw_val.unwrap_or(img.data[idx])))
                    } else {
                        match self.channel_view {
                            ChannelView::Single(c) => {
                                format!("val={}", fmt(img.data[c * npix + idx]))
                            }
                            ChannelView::Rgb if img.channels == 3 => {
                                let rgb = format!("R={} G={} B={}",
                                    fmt(img.data[idx]),
                                    fmt(img.data[npix + idx]),
                                    fmt(img.data[2 * npix + idx]));
                                match raw_val {
                                    Some(r) => format!("{rgb}  raw={}", fmt(r)),
                                    None     => rgb,
                                }
                            }
                            ChannelView::Rgb => format!("val={}", fmt(img.data[idx])),
                        }
                    };
                    let sky_str = self.wcs.as_ref()
                        .and_then(|wcs| wcs.pixel_to_sky(x as f64 + 0.5, y as f64 + 0.5))
                        .map(|(ra, dec)| format!("  RA {} Dec {}", format_ra(ra), format_dec(dec)))
                        .unwrap_or_default();
                    self.hover_pixel_info = Some(format!("{pixel_str}{sky_str}"));

                    // Render as a floating overlay near the cursor.
                    if let Some(info) = &self.hover_pixel_info {
                        let font_id  = egui::FontId::monospace(12.0);
                        let bg_color = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180);
                        let padding  = egui::vec2(5.0, 3.0);
                        let offset   = egui::vec2(16.0, 16.0);
                        let galley   = ctx.fonts(|f| {
                            f.layout_no_wrap(info.clone(), font_id, egui::Color32::WHITE)
                        });
                        // Default: below-right of cursor; flip left if near the right edge.
                        let mut label_pos = pos + offset;
                        if label_pos.x + galley.size().x + padding.x > aabb.max.x {
                            label_pos.x = pos.x - galley.size().x - padding.x - offset.x * 0.5;
                        }
                        let bg_rect = egui::Rect::from_min_size(
                            label_pos - padding,
                            galley.size() + padding * 2.0,
                        );
                        painter.rect_filled(bg_rect, 3.0, bg_color);
                        painter.galley(label_pos, galley, egui::Color32::WHITE);
                    }
                }
            }
        });
    }
}

/// Format a pixel value for the hover overlay.
///
/// Integer data (bitdepth_max > 0): round to integer (values are large ADU counts).
/// Float data (bitdepth_max == 0): show 4 significant figures (values may be < 1.0).
fn fmt_pixel(v: f32, bitdepth_max: f32) -> String {
    if bitdepth_max > 0.0 {
        format!("{:.0}", v)
    } else {
        format!("{:.4}", v)
    }
}

/// Atmospheric seeing quality based on arcsec FWHM (color + bracket label only).
/// Lower arcsec = better atmosphere. Independent of imaging setup.
fn seeing_quality(fwhm_arcsec: f32) -> (egui::Color32, &'static str) {
    if fwhm_arcsec < 2.0 {
        (egui::Color32::from_rgb(80, 200, 80), "excellent")
    } else if fwhm_arcsec < 3.0 {
        (egui::Color32::from_rgb(80, 200, 80), "good")
    } else if fwhm_arcsec < 5.0 {
        (egui::Color32::from_rgb(220, 180, 0), "fair")
    } else {
        (egui::Color32::from_rgb(220, 80, 80), "poor")
    }
}

/// PSF sampling quality based on FWHM in pixels (color + bracket label only).
/// Higher px = better sampled. Property of imaging setup, not atmosphere.
fn sampling_quality(fwhm_px: f32) -> (egui::Color32, &'static str) {
    if fwhm_px >= 3.5 {
        (egui::Color32::from_rgb(80, 200, 80), "well sampled")
    } else if fwhm_px >= 2.5 {
        (egui::Color32::from_rgb(220, 180, 0), "marginal")
    } else {
        (egui::Color32::from_rgb(220, 80, 80), "undersampled")
    }
}

/// Tooltip for the Seeing row: full colour-coded scale for atmospheric quality.
fn seeing_tooltip(ui: &mut egui::Ui) {
    let green  = egui::Color32::from_rgb(80, 200, 80);
    let yellow = egui::Color32::from_rgb(220, 180, 0);
    let red    = egui::Color32::from_rgb(220, 80, 80);

    ui.label(egui::RichText::new("Atmospheric seeing").strong());
    ui.label("How much the atmosphere blurs starlight (lower = better).\nIndependent of your telescope or camera.");
    ui.separator();
    ui.label(egui::RichText::new("Algorithm").italics());
    ui.label(
        "Stars are detected as strict local maxima in a 5×5 neighbourhood, above 8σ sky \
         background, rejecting saturated and elongated sources. For each star, horizontal \
         and vertical 41-pixel profiles are background-subtracted and walked outward from \
         the peak until the signal drops to half-maximum; the crossing is interpolated for \
         sub-pixel accuracy. The FWHM is the mean of both axes. The reported value is the \
         median over all accepted stars, converted to arcseconds via the WCS pixel scale; \
         the uncertainty is the MAD × 1.4826 (≈ robust 1σ)."
    );
    ui.separator();
    egui::Grid::new("seeing_tip_grid").num_columns(3).spacing([8.0, 2.0]).show(ui, |ui| {
        ui.label(egui::RichText::new("●").color(green));
        ui.label("< 2″");
        ui.label(egui::RichText::new("excellent").color(green));
        ui.end_row();
        ui.label(egui::RichText::new("●").color(green));
        ui.label("2 – 3″");
        ui.label(egui::RichText::new("good").color(green));
        ui.end_row();
        ui.label(egui::RichText::new("●").color(yellow));
        ui.label("3 – 5″");
        ui.label(egui::RichText::new("fair").color(yellow));
        ui.end_row();
        ui.label(egui::RichText::new("●").color(red));
        ui.label("≥ 5″");
        ui.label(egui::RichText::new("poor").color(red));
        ui.end_row();
    });
}

/// Tooltip for the FWHM row: full colour-coded scale for PSF sampling quality.
fn sampling_tooltip(ui: &mut egui::Ui) {
    let green  = egui::Color32::from_rgb(80, 200, 80);
    let yellow = egui::Color32::from_rgb(220, 180, 0);
    let red    = egui::Color32::from_rgb(220, 80, 80);

    ui.label(egui::RichText::new("PSF sampling").strong());
    ui.label("Whether your pixel scale resolves the PSF reliably.\nThis is a property of your setup (focal length + pixel pitch),\nnot the atmosphere. Sharp seeing makes this worse:\ntighter stars land on fewer pixels.");
    ui.separator();
    ui.label(egui::RichText::new("Algorithm").italics());
    ui.label(
        "The FWHM in pixels is measured from the same half-maximum profile walk used for \
         the Seeing row (see its tooltip). Nyquist sampling requires ≥ 2 px per resolution \
         element; the thresholds here use 3.5 px (comfortable margin) and 2.5 px (hard \
         minimum). Note: Bayer-debayered images may read ~0.3–0.5 px wider than reality \
         due to bilinear interpolation."
    );
    ui.separator();
    egui::Grid::new("sampling_tip_grid").num_columns(3).spacing([8.0, 2.0]).show(ui, |ui| {
        ui.label(egui::RichText::new("●").color(green));
        ui.label("≥ 3.5 px");
        ui.label(egui::RichText::new("well sampled  —  Nyquist satisfied").color(green));
        ui.end_row();
        ui.label(egui::RichText::new("●").color(yellow));
        ui.label("2.5 – 3.5 px");
        ui.label(egui::RichText::new("marginal  —  near Nyquist limit").color(yellow));
        ui.end_row();
        ui.label(egui::RichText::new("●").color(red));
        ui.label("< 2.5 px");
        ui.label(egui::RichText::new("undersampled  —  FWHM is a lower bound").color(red));
        ui.end_row();
    });
}
