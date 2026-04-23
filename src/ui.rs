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
            show_grid:         self.show_grid,
            show_dso:          self.show_dso,
            stretch:           self.stretch,
            demosaic_mode:     self.demosaic_mode,
            show_histogram:    self.show_histogram,
            show_clipping:     self.show_clipping,
            show_north_up:     self.show_north_up,
            welcome_dismissed: self.welcome_dismissed,
            last_dir:          Some(self.current_dir.clone()),
            ui_zoom:           self.ui_zoom,
            header_decimal:    self.header_decimal,
        });
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_background_loads(ctx);

        let open_file  = ctx.input(|i| i.key_pressed(egui::Key::O) && i.modifiers.command);
        let export_img = ctx.input(|i| i.key_pressed(egui::Key::E) && i.modifiers.command);
        // Suppress single-key shortcuts while a text field has focus.
        let typing = ctx.wants_keyboard_input();
        let zoomed = self.zoom.is_some();
        let mouse_next = ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Extra2));
        let mouse_prev = ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Extra1));
        let go_next    = mouse_next || (!typing && ctx.input(|i| i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowDown)));
        let go_prev    = mouse_prev || (!typing && ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)  || i.key_pressed(egui::Key::ArrowUp)));
        let go_first   = !typing && ctx.input(|i| i.key_pressed(egui::Key::Home));
        let go_last    = !typing && ctx.input(|i| i.key_pressed(egui::Key::End));
        let go_back_10 = !typing && ctx.input(|i| i.key_pressed(egui::Key::PageUp));
        let go_fwd_10  = !typing && ctx.input(|i| i.key_pressed(egui::Key::PageDown));
        const NUDGE: f32 = 50.0;
        let nudge = if !typing { ctx.input(|i| {
            if !zoomed { return egui::Vec2::ZERO; }
            let mut d = egui::Vec2::ZERO;
            if i.key_pressed(egui::Key::A) { d.x += NUDGE; }
            if i.key_pressed(egui::Key::D) { d.x -= NUDGE; }
            if i.key_pressed(egui::Key::W) { d.y += NUDGE; }
            if i.key_pressed(egui::Key::S) { d.y -= NUDGE; }
            d
        })} else { egui::Vec2::ZERO };
        let toggle_stretch    = !typing && ctx.input(|i| i.key_pressed(egui::Key::T));
        let zoom_in    = !typing && ctx.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals));
        let zoom_out   = !typing && ctx.input(|i| i.key_pressed(egui::Key::Minus));
        let zoom_reset = !typing && ctx.input(|i| i.key_pressed(egui::Key::Num0));
        let zoom_fit   = !typing && ctx.input(|i| i.key_pressed(egui::Key::F));
        let do_delete  = !typing && ctx.input(|i| i.key_pressed(egui::Key::Delete));
        let toggle_help      = !typing && ctx.input(|i| i.key_pressed(egui::Key::Questionmark));
        let toggle_prefs     = !typing && ctx.input(|i| i.key_pressed(egui::Key::Comma));
        let toggle_histogram = !typing && ctx.input(|i| i.key_pressed(egui::Key::H));
        let toggle_about     = !typing && ctx.input(|i| i.key_pressed(egui::Key::I));
        let toggle_grid      = !typing && ctx.input(|i| i.key_pressed(egui::Key::G));
        let toggle_dso       = !typing && ctx.input(|i| i.key_pressed(egui::Key::B));
        let toggle_clipping  = !typing && ctx.input(|i| i.key_pressed(egui::Key::C));
        let toggle_raw_bayer = !typing && ctx.input(|i| i.key_pressed(egui::Key::R));
        let toggle_north_up  = !typing && ctx.input(|i| i.key_pressed(egui::Key::N));
        let toggle_headers   = !typing && ctx.input(|i| i.key_pressed(egui::Key::L));
        let close_popup      = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let do_quit          = !typing && ctx.input(|i| i.key_pressed(egui::Key::Q));
        let toggle_focus_analysis = !typing && ctx.input(|i| i.key_pressed(egui::Key::K));

        if go_next    { self.select_next(); }
        if go_prev    { self.select_prev(); }
        if go_first   { self.select_first(); }
        if go_last    { self.select_last(); }
        if go_fwd_10  { self.select_skip(10); }
        if go_back_10 { self.select_skip(-10); }
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
        if toggle_raw_bayer {
            if self.image.as_ref().is_some_and(|img| img.is_bayer) {
                self.show_raw_bayer = !self.show_raw_bayer;
                self.texture = None;
            }
        }
        if toggle_north_up  { self.show_north_up  = !self.show_north_up; }
        if toggle_headers   { self.show_headers   = !self.show_headers; }
        if toggle_stretch {
            self.stretch = match self.stretch {
                Stretch::AutoStretch => Stretch::Linear,
                Stretch::Linear      => Stretch::AutoStretch,
            };
            self.texture  = None;
        }
        if do_quit { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
        if close_popup {
            self.show_help           = false;
            self.show_prefs          = false;
            self.show_about          = false;
            self.show_welcome        = false;
            self.show_focus_analysis = false;
            if typing { ctx.memory_mut(|m| m.stop_text_input()); }
        }

        let n = self.files.len();
        let ver = env!("CARGO_PKG_VERSION");
        let title = match self.selected.and_then(|i| self.files.get(i)) {
            Some(p) => format!("fastfits {} — {} [{}/{}]",
                ver,
                p.file_name().unwrap_or_default().to_string_lossy(),
                self.selected.unwrap() + 1, n),
            None    => format!("fastfits {}", ver),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        self.show_welcome_window(ctx);
        self.show_help_window(ctx);
        if self.show_prefs_window(ctx) { self.reload_image(); }
        self.show_about_window(ctx);
        self.show_focus_analysis_window(ctx);

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

        if let Some(rx) = &self.focus_analysis_rx {
            if let Ok(result) = rx.try_recv() {
                self.focus_analysis_rx = None;
                self.focus_analysis_running = false;
                self.focus_analysis_progress = None;
                self.focus_analysis = Some(result);
            }
        }

        let (go_prev_btn, go_next_btn, do_delete_btn) = self.show_bottom_bar(ctx);
        if go_prev_btn   { self.select_prev(); }
        if go_next_btn   { self.select_next(); }
        if do_delete_btn { self.delete_selected(); }

        let (open_btn, export_jpg_btn, export_png_btn, focus_btn) = self.show_menu_bar(ctx);
        if focus_btn || toggle_focus_analysis { self.trigger_focus_analysis(ctx); }
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
        // Rebuild texture just before drawing the center panel so that any
        // texture = None set by the bottom bar, right panel, or left panel
        // (e.g. toggling Clip, switching files) is resolved this frame.
        if self.image.is_some() && self.texture.is_none() {
            self.rebuild_texture(ctx);
        }
        self.show_center_panel(ctx);
    }
}

impl FastFitsApp {
    fn poll_background_loads(&mut self, ctx: &egui::Context) {
        // Poll the primary (user-requested) load.
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
                        if let Some(idx) = self.selected {
                            self.trigger_preloads(idx);
                        }
                    }
                    LoadResult::Err(e) => {
                        self.load_error = Some(e);
                    }
                }
                ctx.request_repaint();
            }
        }

        // Poll background preloads — completed images go into the cache.
        self.preload_rxs.retain_mut(|(idx, rx)| {
            match rx.try_recv() {
                Ok(LoadResult::Ok(img)) => {
                    self.cache.insert(*idx, *img, None);
                    false
                }
                Ok(LoadResult::Err(_)) => false,
                Err(mpsc::TryRecvError::Empty) => true,
                Err(mpsc::TryRecvError::Disconnected) => false,
            }
        });
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

    fn shortcut_rows() -> &'static [(&'static str, &'static str)] {
        &[
            ("Ctrl+O",                    "Open file dialog"),
            ("Ctrl+E",                    "Export current view as JPEG"),
            ("Left / Right or Up / Down", "Previous / next file"),
            ("Mouse Back / Forward",      "Previous / next file"),
            ("W / A / S / D",             "Pan viewport (when zoomed in)"),
            ("Home / End",                "Jump to first / last file"),
            ("PageUp / PageDown",         "Skip back / forward 10 files"),
            ("Delete",                    "Move current file to trash"),
            ("T",                         "Toggle stretch (Auto / Linear)"),
            ("+  /  -",                   "Zoom in / out"),
            ("0",                         "Zoom to 1:1 (100 %)"),
            ("F",                         "Zoom to fit"),
            ("L",                         "Show / hide FITS headers panel"),
            ("H",                         "Show / hide histogram"),
            ("G",                         "Show / hide WCS coordinate grid"),
            ("B",                         "Show / hide DSO catalogue overlay"),
            ("C",                         "Show / hide clipping overlay (overexposed pixels red)"),
            ("R",                         "Show / hide raw Bayer sensor data (Bayer images only)"),
            ("N",                         "Rotate image: North up, East left (requires WCS)"),
            ("K",                         "Focus temperature compensation analysis"),
            ("I",                         "Show / hide About / Info"),
            ("?",                         "Show / hide this help"),
            (",",                         "Show / hide Preferences"),
            ("Q",                         "Quit"),
        ]
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
                    for (key, desc) in Self::shortcut_rows() {
                        ui.label(egui::RichText::new(*key).monospace().strong());
                        ui.label(*desc);
                        ui.end_row();
                    }
                });
            });
    }

    fn show_welcome_window(&mut self, ctx: &egui::Context) {
        if !self.show_welcome { return; }
        let mut still_open = true;
        let mut close_clicked = false;
        egui::Window::new("Welcome to fastfits")
            .open(&mut still_open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("fastfits is a fast viewer for FITS astronomy images.");
                ui.add_space(4.0);
                ui.label("Pass a file or directory on the command line, or drop one on the window.");
                ui.label("The right panel lists FITS files in the current directory; the left panel shows headers.");
                ui.label("Press ? any time to reopen this shortcut list.");
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Keyboard shortcuts").strong());
                egui::Grid::new("welcome_grid").striped(true).show(ui, |ui| {
                    for (key, desc) in Self::shortcut_rows() {
                        ui.label(egui::RichText::new(*key).monospace().strong());
                        ui.label(*desc);
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.welcome_dismissed, "Don't show this again");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Got it").clicked() { close_clicked = true; }
                    });
                });
            });
        if !still_open || close_clicked {
            self.show_welcome = false;
        }
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

                ui.label("UI scale");
                ui.horizontal(|ui| {
                    let resp = ui.add(
                        egui::Slider::new(&mut self.ui_zoom, 0.5..=2.0)
                            .step_by(0.05)
                            .suffix("×"),
                    );
                    if resp.changed() {
                        ctx.set_zoom_factor(self.ui_zoom);
                    }
                    if ui.button("Reset").clicked() && (self.ui_zoom - 1.0).abs() > f32::EPSILON {
                        self.ui_zoom = 1.0;
                        ctx.set_zoom_factor(1.0);
                    }
                });
                ui.small("Applied on top of the OS display scale.");
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

    fn show_menu_bar(&mut self, ctx: &egui::Context) -> (bool, bool, bool, bool) {
        let mut open_clicked = false;
        let mut export_jpg_clicked = false;
        let mut export_png_clicked = false;
        let mut focus_analysis_clicked = false;
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
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
                    .on_hover_text("Save current view as PNG  [Ctrl+E]").clicked()
                {
                    export_png_clicked = true;
                }
                ui.separator();
                let focus_btn_tip = "Focus temperature compensation analysis  [K]";
                let focus_btn = if self.focus_analysis_running {
                    ui.add_enabled(false, egui::Button::new("⏳ Focus T°C"))
                        .on_disabled_hover_text(focus_btn_tip)
                } else {
                    ui.add_enabled(!self.files.is_empty(), egui::Button::selectable(self.show_focus_analysis, "Focus T°C"))
                        .on_hover_text(focus_btn_tip)
                };
                if focus_btn.clicked() {
                    focus_analysis_clicked = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("About").on_hover_text("About fastfits  [I]").clicked() {
                        self.show_about = !self.show_about;
                    }
                    if ui.button("?").on_hover_text("Show keyboard shortcuts  [?]").clicked() {
                        self.show_help = !self.show_help;
                    }
                    if ui.button("Prefs").on_hover_text("Preferences  [,]").clicked() {
                        self.show_prefs = !self.show_prefs;
                    }
                    if ui.selectable_label(self.show_headers, "Hdr")
                        .on_hover_text("Show / hide FITS headers panel  [L]").clicked()
                    {
                        self.show_headers = !self.show_headers;
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
                        if ui.add_enabled(has_wcs, egui::Button::selectable(self.show_grid, "Grid"))
                            .on_hover_text(tip_grid)
                            .on_disabled_hover_text(tip_grid)
                            .clicked()
                        {
                            self.show_grid = !self.show_grid;
                        }
                        let tip_dso = if has_wcs {
                            "Show / hide DSO catalogue overlay  [B]"
                        } else {
                            "No WCS headers in this file — DSO overlay unavailable"
                        };
                        if ui.add_enabled(has_wcs, egui::Button::selectable(self.show_dso, "DSO"))
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
                        if ui.add_enabled(has_wcs, egui::Button::selectable(self.show_north_up, "Equatorial orientation"))
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
        (open_clicked, export_jpg_clicked, export_png_clicked, focus_analysis_clicked)
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
        if ui.button("1:1").on_hover_text("Zoom to 100%  [0]").clicked() {
            self.zoom = Some(1.0);
        }
        ui.label("Zoom:").on_hover_text("Zoom  [+] [-] [0=1:1] [F=fit]");
        ui.separator();

        if let Some(img) = &self.image {
            if img.is_bayer {
                if ui.selectable_label(self.show_raw_bayer, "Raw")
                    .on_hover_text("Show raw Bayer sensor data without debayering  [R]")
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
            .on_hover_text("Toggle stretch mode  [T]").clicked()
        {
            self.stretch = match self.stretch {
                Stretch::AutoStretch => Stretch::Linear,
                Stretch::Linear      => Stretch::AutoStretch,
            };
            self.texture   = None;
        }
        ui.label("Stretch:").on_hover_text("Toggle stretch mode  [T]");
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
        if !self.show_headers { return; }
        egui::SidePanel::left("headers_panel")
            .resizable(true)
            .min_width(100.0)
            .max_width(500.0)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Headers");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("X").on_hover_text("Hide headers panel  [L]").clicked() {
                            self.show_headers = false;
                        }
                        let dec_label = if self.header_decimal { "1.23" } else { "1E0" };
                        if ui.selectable_label(self.header_decimal, dec_label)
                            .on_hover_text("Toggle decimal / scientific notation for numeric values")
                            .clicked()
                        {
                            self.header_decimal = !self.header_decimal;
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    // Always reserve button space so TextEdit width stays constant.
                    let btn = ui.add_visible(
                        !self.header_filter.is_empty(),
                        egui::Button::new("X").small(),
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
                            let display_v = if self.header_decimal {
                                format_header_value(v)
                            } else {
                                v.to_string()
                            };
                            ui.horizontal(|ui| {
                                let k_resp = ui.add(
                                    egui::Label::new(egui::RichText::new(k).strong().monospace())
                                        .selectable(true)
                                        .sense(egui::Sense::click()),
                                );
                                let v_resp = ui.add(
                                    egui::Label::new(egui::RichText::new(&display_v).monospace())
                                        .selectable(true)
                                        .sense(egui::Sense::click()),
                                );
                                let menu = |ui: &mut egui::Ui| {
                                    if ui.button("Copy key").clicked() {
                                        ui.ctx().copy_text(k.clone());
                                        ui.close();
                                    }
                                    if ui.button("Copy value").clicked() {
                                        ui.ctx().copy_text(v.clone());
                                        ui.close();
                                    }
                                    if ui.button("Copy key = value").clicked() {
                                        ui.ctx().copy_text(format!("{} = {}", k, v));
                                        ui.close();
                                    }
                                };
                                k_resp.context_menu(menu);
                                v_resp.context_menu(menu);
                            });
                        }
                    } else {
                        ui.label("(no file loaded)");
                    }
                });
            });
    }

    /// Width needed for the file browser so the longest filename/subdir
    /// entry fits without truncation. Clamped to a sensible range.
    fn file_list_fit_width(&self, ctx: &egui::Context) -> f32 {
        let font_id = egui::TextStyle::Button.resolve(&ctx.style());
        let mut longest: f32 = 0.0;
        ctx.fonts(|f| {
            let mut measure = |s: &str| {
                let w = f.layout_no_wrap(s.to_string(), font_id.clone(), egui::Color32::WHITE).size().x;
                if w > longest { longest = w; }
            };
            if self.current_dir.parent().is_some() {
                measure("..");
            }
            for dir in &self.subdirs {
                let name = dir.file_name().unwrap_or_default().to_string_lossy();
                measure(&format!("{}/", name));
            }
            for path in &self.files {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                measure(&name);
            }
        });
        // Add padding for selectable_label insets, scrollbar, and panel margin.
        (longest + 48.0).clamp(160.0, 800.0)
    }

    fn show_right_panel(&mut self, ctx: &egui::Context) {
        let fit_width = self.file_list_fit_width(ctx);
        egui::SidePanel::right("file_browser")
            .resizable(true)
            .min_width(100.0)
            .max_width(800.0)
            .default_width(fit_width)
            .show(ctx, |ui| {
                if self.show_histogram {
                    if let Some(hist) = &self.histogram {
                        draw_histogram(ui, hist, self.stretch, self.channel_view);
                    } else {
                        // Reserve space so the layout doesn't jump when histogram arrives.
                        let width = ui.available_width();
                        ui.allocate_space(egui::vec2(width, 80.0));
                    }
                    ui.separator();
                }

                {
                    let is_light = self.image.as_ref().map(|img| {
                        img.headers.iter()
                            .find(|(k, _)| k == "IMAGETYP")
                            .map(|(_, v)| v.trim().to_ascii_lowercase().contains("light"))
                            .unwrap_or(true)
                    });

                    egui::Grid::new("seeing_grid").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
                        ui.label(egui::RichText::new("Star FWHM:").strong());
                        match is_light {
                            None if self.selected.is_some() => { ui.label("measuring…"); }
                            None => { ui.label("—"); }
                            Some(false) => { ui.label("N/A (not a light frame)"); }
                            Some(true) => match &self.seeing {
                                None => { ui.label("measuring…"); }
                                Some(None) => { ui.label("— (no stars detected)"); }
                                Some(Some(s)) => {
                                    let (samp_color, samp_label) = sampling_quality(s.fwhm_px);
                                    if let (Some(fsec), Some(esec)) = (s.fwhm_arcsec, s.error_arcsec) {
                                        let (see_color, see_label) = seeing_quality(fsec);
                                        let resp = ui.horizontal(|ui| {
                                            ui.label(format!("{:.1}\" +/- {:.1}\" / {:.1} px   {} stars [{}]", fsec, esec, s.fwhm_px, s.star_count, see_label));
                                            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                            ui.painter().circle_filled(rect.center(), 5.0, see_color);
                                        });
                                        resp.response.on_hover_ui(|ui| seeing_tooltip(ui));
                                    } else {
                                        let resp = ui.horizontal(|ui| {
                                            ui.label(format!("{:.1} px +/- {:.1} px   {} stars [{}]", s.fwhm_px, s.error_px, s.star_count, samp_label));
                                            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                            ui.painter().circle_filled(rect.center(), 5.0, samp_color);
                                        });
                                        resp.response.on_hover_ui(|ui| sampling_tooltip(ui));
                                    }
                                }
                            },
                        }
                        ui.end_row();

                        ui.label(egui::RichText::new("Roundness:").strong());
                        match is_light {
                            None if self.selected.is_some() => { ui.label("measuring…"); }
                            None => { ui.label("—"); }
                            Some(false) => { ui.label("N/A"); }
                            Some(true) => match &self.seeing {
                                None => { ui.label("measuring…"); }
                                Some(None) => { ui.label("— (no stars detected)"); }
                                Some(Some(s)) => {
                                    let r = s.roundness;
                                    let (color, label) = roundness_quality(r);
                                    let resp = ui.horizontal(|ui| {
                                        ui.label(format!("{:.2}  [{}]", r, label));
                                        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                        ui.painter().circle_filled(rect.center(), 5.0, color);
                                    });
                                    resp.response.on_hover_text("Median axis ratio (b/a) of detected stars.\n1.00 = perfect circle; lower = elongated stars (tracking error, wind, coma).");
                                }
                            },
                        }
                        ui.end_row();
                    });
                    ui.separator();
                }

                let file_count_label = if self.files.is_empty() {
                    "Files".to_string()
                } else {
                    let idx = self.selected.map(|i| i + 1).unwrap_or(0);
                    format!("Files [{}/{}]", idx, self.files.len())
                };
                ui.heading(file_count_label);
                ui.small(self.current_dir.to_string_lossy());
                ui.separator();

                let scroll_to = self.scroll_to_selected;
                self.scroll_to_selected = false;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut nav_dir: Option<std::path::PathBuf> = None;

                    // ".." to go to parent directory.
                    if let Some(parent) = self.current_dir.parent() {
                        if ui.selectable_label(false, "..").on_hover_text("Go to parent directory").clicked() {
                            nav_dir = Some(parent.to_path_buf());
                        }
                    }

                    // Subdirectories.
                    for dir in &self.subdirs {
                        let name = dir.file_name().unwrap_or_default().to_string_lossy();
                        let label = format!("{}/", name);
                        if ui.selectable_label(false, &label)
                            .on_hover_text("Open directory")
                            .clicked()
                        {
                            nav_dir = Some(dir.clone());
                        }
                    }

                    // FITS files.
                    let mut clicked = None;
                    for (i, path) in self.files.iter().enumerate() {
                        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        let is_selected = self.selected == Some(i);
                        let resp = ui.selectable_label(is_selected, &name)
                            .on_hover_text("Open file  [Left/Right to navigate]  [Del to trash]");
                        if is_selected && scroll_to {
                            resp.scroll_to_me(Some(egui::Align::Center));
                        }
                        if resp.clicked() {
                            clicked = Some(i);
                        }
                    }
                    if let Some(i) = clicked { self.select(i); }
                    if let Some(dir) = nav_dir { self.open_dir(dir); }
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
                if let Some(name) = &self.loading_name {
                    ui.vertical_centered(|ui| {
                        let available = ui.available_height();
                        ui.add_space(available / 2.0 - 20.0);
                        ui.spinner();
                        ui.label(format!("Loading {}…", name));
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("No file selected");
                    });
                }
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
            if let Some(pos) = right_clicked_pos {
                if aabb.contains(pos) && self.wcs.is_none() {
                    self.marker_status = Some((
                        "Cannot place marker: no WCS coordinates in this file".to_string(),
                        std::time::Instant::now(),
                    ));
                }
            }
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
                        // Default: below-right of cursor; flip to keep the box visible.
                        // Clamp to the visible area (intersection of image and panel).
                        let visible = aabb.intersect(panel_rect);
                        let box_w = galley.size().x + padding.x * 2.0;
                        let box_h = galley.size().y + padding.y * 2.0;
                        let mut label_pos = pos + offset;
                        if label_pos.x + galley.size().x + padding.x > visible.max.x {
                            label_pos.x = (pos.x - box_w - offset.x * 0.5).max(visible.min.x + padding.x);
                        }
                        if label_pos.y + galley.size().y + padding.y > visible.max.y {
                            label_pos.y = (pos.y - box_h - offset.y * 0.5).max(visible.min.y + padding.y);
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

            // Transient marker status message (auto-dismissed after 3 seconds).
            if let Some((msg, when)) = &self.marker_status {
                if when.elapsed().as_secs_f32() < 3.0 {
                    let font_id = egui::FontId::proportional(14.0);
                    let bg = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200);
                    let padding = egui::vec2(10.0, 6.0);
                    let galley = ctx.fonts(|f| {
                        f.layout_no_wrap(msg.clone(), font_id, egui::Color32::from_rgb(255, 200, 80))
                    });
                    let label_pos = egui::pos2(
                        aabb.center().x - galley.size().x / 2.0,
                        aabb.max.y - 40.0,
                    );
                    let bg_rect = egui::Rect::from_min_size(
                        label_pos - padding,
                        galley.size() + padding * 2.0,
                    );
                    painter.rect_filled(bg_rect, 4.0, bg);
                    painter.galley(label_pos, galley, egui::Color32::WHITE);
                    ctx.request_repaint();
                } else {
                    self.marker_status = None;
                }
            }
        });
    }

    fn trigger_focus_analysis(&mut self, ctx: &egui::Context) {
        if self.focus_analysis_running || self.files.is_empty() { return; }
        let files = self.files.clone();
        let demosaic = self.demosaic_mode;
        let (tx, rx) = mpsc::channel();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        self.focus_analysis_rx = Some(rx);
        self.focus_analysis_running = true;
        self.focus_analysis_progress = Some(progress.clone());
        self.focus_analysis_total = files.len();
        self.show_focus_analysis = true;
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let result = crate::focus_analysis::run_focus_analysis(files, demosaic, progress, ctx2.clone());
            let _ = tx.send(result);
            ctx2.request_repaint();
        });
    }

    fn show_focus_analysis_window(&mut self, ctx: &egui::Context) {
        if !self.show_focus_analysis { return; }
        let mut open = self.show_focus_analysis;
        egui::Window::new("Focus Temperature Compensation")
            .open(&mut open)
            .resizable(true)
            .default_size([650.0, 480.0])
            .show(ctx, |ui| {
                if self.focus_analysis_running {
                    let done = self.focus_analysis_progress
                        .as_ref()
                        .map(|p| p.load(std::sync::atomic::Ordering::Relaxed))
                        .unwrap_or(0);
                    let total = self.focus_analysis_total;
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("Scanning files… {}/{}", done, total));
                    });
                    let frac = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                    ui.add(egui::ProgressBar::new(frac).show_percentage());
                    return;
                }
                let Some(result) = &self.focus_analysis else {
                    ui.label("Press K or click the toolbar button to run the analysis.");
                    return;
                };

                if result.points.len() < 2 {
                    ui.label("Not enough qualifying data (need ≥ 2 images with roundness ≥ 0.9 and FOCUSTEM/FOCUSPOS headers).");
                    ui.separator();
                    Self::focus_analysis_skip_summary(ui, result);
                    return;
                }

                let r2_color = if result.r_squared >= 0.9 {
                    egui::Color32::from_rgb(80, 200, 80)
                } else if result.r_squared >= 0.7 {
                    egui::Color32::from_rgb(220, 180, 0)
                } else {
                    egui::Color32::from_rgb(220, 80, 80)
                };
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!(
                        "N = {}  |  slope = {:.1} steps/°C  |  ",
                        result.points.len(), result.slope)).strong());
                    ui.label(egui::RichText::new(format!("R² = {:.3}", result.r_squared))
                        .strong().color(r2_color));
                });
                Self::focus_analysis_skip_summary(ui, result);
                ui.separator();

                // Collect data for the plot.
                let pts: Vec<(f64, f64, f32, String)> = result.points.iter()
                    .map(|p| (p.temp, p.focuspos, p.fwhm_px, p.filename.clone()))
                    .collect();
                let slope     = result.slope;
                let intercept = result.intercept;

                if let Some(fname) = draw_focus_scatter(ui, &pts, slope, intercept) {
                    if let Some(idx) = self.files.iter().position(|p| {
                        p.file_name().map(|n| n.to_string_lossy().as_ref() == fname).unwrap_or(false)
                    }) {
                        self.select(idx);
                    }
                }
            });
        self.show_focus_analysis = open;
    }

    fn focus_analysis_skip_summary(ui: &mut egui::Ui, result: &crate::focus_analysis::FocusAnalysisResult) {
        ui.label(egui::RichText::new(format!(
            "Scanned {} files — skipped {} (no headers), {} (roundness < 0.9), {} (no stars detected)",
            result.n_scanned,
            result.n_skipped_headers,
            result.n_skipped_roundness,
            result.n_skipped_no_stars,
        )).small().weak());
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

/// Star FWHM quality based on arcsec value (color + bracket label only).
/// Lower arcsec = sharper stars. Includes atmosphere + optics + tracking.
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
        (egui::Color32::from_rgb(220, 180, 0), "adequate")
    } else {
        (egui::Color32::from_rgb(220, 80, 80), "undersampled")
    }
}

/// Reformat a FITS header value string from scientific to decimal notation if possible.
///
/// Draw a scatter plot of (temp, focuspos) data with a regression line.
///
/// Axes are drawn with tick marks, data points as filled circles, the
/// regression line as a yellow segment, and a hover tooltip per point.
/// Returns the filename of a clicked data point, if any.
fn draw_focus_scatter(
    ui: &mut egui::Ui,
    pts: &[(f64, f64, f32, String)],
    slope: f64,
    intercept: f64,
) -> Option<String> {
    const PAD_L: f32 = 55.0; // left margin for Y axis labels
    const PAD_B: f32 = 35.0; // bottom margin for X axis labels
    const PAD_T: f32 = 8.0;
    const PAD_R: f32 = 8.0;
    const PT_R:  f32 = 5.0;  // data point radius

    let available = ui.available_size();
    let (response, painter) = ui.allocate_painter(available, egui::Sense::click());
    let rect = response.rect;

    // Plot area (inside margins).
    let plot = egui::Rect::from_min_max(
        egui::pos2(rect.left() + PAD_L, rect.top() + PAD_T),
        egui::pos2(rect.right() - PAD_R, rect.bottom() - PAD_B),
    );

    // Data ranges with 5% margin.
    let x_min = pts.iter().map(|(t, _, _, _)| *t).fold(f64::INFINITY, f64::min);
    let x_max = pts.iter().map(|(t, _, _, _)| *t).fold(f64::NEG_INFINITY, f64::max);
    let y_min = pts.iter().map(|(_, fp, _, _)| *fp).fold(f64::INFINITY, f64::min);
    let y_max = pts.iter().map(|(_, fp, _, _)| *fp).fold(f64::NEG_INFINITY, f64::max);
    let xm = (x_max - x_min).max(1.0) * 0.08;
    let ym = (y_max - y_min).max(1.0) * 0.08;
    let (xlo, xhi) = (x_min - xm, x_max + xm);
    let (ylo, yhi) = (y_min - ym, y_max + ym);
    let xspan = (xhi - xlo).max(1e-9);
    let yspan = (yhi - ylo).max(1e-9);

    let to_screen = |x: f64, y: f64| -> egui::Pos2 {
        let px = plot.left() + ((x - xlo) / xspan) as f32 * plot.width();
        let py = plot.bottom() - ((y - ylo) / yspan) as f32 * plot.height();
        egui::pos2(px, py)
    };

    // Background and border.
    painter.rect_filled(plot, 0.0, egui::Color32::from_gray(20));
    painter.rect_stroke(plot, 0.0, egui::Stroke::new(1.0, egui::Color32::from_gray(100)), egui::StrokeKind::Inside);

    // Axis ticks and labels.
    let font = egui::FontId::monospace(10.0);
    let label_color = egui::Color32::from_gray(180);
    let tick_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(60));

    let x_ticks = nice_ticks(xlo, xhi, 6);
    for &tx in &x_ticks {
        let sx = to_screen(tx, ylo);
        painter.line_segment([sx, egui::pos2(sx.x, plot.bottom() + 4.0)],
            egui::Stroke::new(1.0, egui::Color32::from_gray(100)));
        painter.line_segment([sx, egui::pos2(sx.x, plot.top())], tick_stroke);
        painter.text(egui::pos2(sx.x, plot.bottom() + 6.0),
            egui::Align2::CENTER_TOP, format!("{:.1}", tx), font.clone(), label_color);
    }
    let y_ticks = nice_ticks(ylo, yhi, 6);
    for &ty in &y_ticks {
        let sy = to_screen(xlo, ty);
        painter.line_segment([egui::pos2(plot.left() - 4.0, sy.y), sy],
            egui::Stroke::new(1.0, egui::Color32::from_gray(100)));
        painter.line_segment([sy, egui::pos2(plot.right(), sy.y)], tick_stroke);
        painter.text(egui::pos2(plot.left() - 6.0, sy.y),
            egui::Align2::RIGHT_CENTER, format!("{:.0}", ty), font.clone(), label_color);
    }

    // Axis titles.
    let title_font = egui::FontId::proportional(11.0);
    painter.text(egui::pos2(plot.center().x, rect.bottom() - 2.0),
        egui::Align2::CENTER_BOTTOM, "Temperature (°C)", title_font.clone(), label_color);
    // Rotated Y-axis title via a galley.
    let galley = painter.layout_no_wrap("Focus position (steps)".to_string(),
        title_font.clone(), label_color);
    let angle = -std::f32::consts::FRAC_PI_2;
    painter.add(egui::Shape::Text(egui::epaint::TextShape {
        pos: egui::pos2(rect.left() + 2.0, plot.center().y + galley.size().x * 0.5),
        galley,
        underline: egui::Stroke::NONE,
        fallback_color: label_color,
        override_text_color: None,
        opacity_factor: 1.0,
        angle,
    }));

    // Regression line (clipped to plot bounds).
    let rx0 = to_screen(xlo, slope * xlo + intercept);
    let rx1 = to_screen(xhi, slope * xhi + intercept);
    if let Some((p0, p1)) = clip_line_to_rect(rx0, rx1, plot) {
        painter.line_segment([p0, p1],
            egui::Stroke::new(2.0, egui::Color32::from_rgb(200, 200, 60)));
    }

    // FWHM range for color mapping.
    let fwhm_min = pts.iter().map(|(_, _, f, _)| *f).fold(f32::INFINITY, f32::min);
    let fwhm_max = pts.iter().map(|(_, _, f, _)| *f).fold(f32::NEG_INFINITY, f32::max);
    let fwhm_span = (fwhm_max - fwhm_min).max(0.01);

    let hover_pos = response.hover_pos();
    let clicked   = response.clicked();
    let mut tooltip:      Option<String> = None;
    let mut clicked_name: Option<String> = None;

    for (tx, fp, fwhm, fname) in pts {
        let sp = to_screen(*tx, *fp);
        // 0 = sharpest (green), 1 = softest (red).
        let t = ((fwhm - fwhm_min) / fwhm_span).clamp(0.0, 1.0);
        let color = fwhm_color(t);
        painter.circle_filled(sp, PT_R, color);
        painter.circle_stroke(sp, PT_R, egui::Stroke::new(1.0, egui::Color32::from_gray(200)));

        if hover_pos.is_some_and(|hp| hp.distance(sp) < PT_R + 4.0) {
            tooltip = Some(format!(
                "{}\nT = {:.2} °C\nPos = {:.0}\nFWHM = {:.2} px\n(click to open)",
                fname, tx, fp, fwhm,
            ));
            if clicked {
                clicked_name = Some(fname.clone());
            }
        }
    }

    // Legend: FWHM color scale.
    {
        let lx = plot.right() - 6.0;
        let ly0 = plot.top() + 6.0;
        let ly1 = ly0 + 60.0;
        let lw = 8.0;
        let steps = 30u32;
        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;
            let y0 = ly0 + t * (ly1 - ly0);
            let y1 = ly0 + (t + 1.0 / (steps - 1) as f32) * (ly1 - ly0);
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(lx - lw, y0), egui::pos2(lx, y1)),
                0.0, fwhm_color(t),
            );
        }
        painter.text(egui::pos2(lx - lw / 2.0, ly0 - 1.0),
            egui::Align2::CENTER_BOTTOM, "sharp", egui::FontId::monospace(9.0), label_color);
        painter.text(egui::pos2(lx - lw / 2.0, ly1 + 1.0),
            egui::Align2::CENTER_TOP,    "soft",  egui::FontId::monospace(9.0), label_color);
    }

    if let Some(tip) = tooltip {
        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("focus_tip"), |ui| {
            ui.label(tip);
        });
    }

    // Change cursor to a pointer when hovering a point.
    if hover_pos.is_some_and(|hp| pts.iter().any(|(tx, fp, _, _)| hp.distance(to_screen(*tx, *fp)) < PT_R + 4.0)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    clicked_name
}

/// Map a normalised FWHM value (0=sharp/green … 1=soft/red) to a colour.
fn fwhm_color(t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        // Green → Yellow
        let u = t * 2.0;
        egui::Color32::from_rgb(
            (80.0  + u * (220.0 - 80.0))  as u8,
            (200.0 + u * (180.0 - 200.0)) as u8,
            80,
        )
    } else {
        // Yellow → Red
        let u = (t - 0.5) * 2.0;
        egui::Color32::from_rgb(
            220,
            (180.0 + u * (80.0 - 180.0)) as u8,
            80,
        )
    }
}

/// Generate `~n` nicely-spaced tick values covering [lo, hi].
fn nice_ticks(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    let span = hi - lo;
    if span <= 0.0 { return vec![lo]; }
    let rough = span / n as f64;
    let mag = rough.log10().floor();
    let frac = rough / 10f64.powf(mag);
    let nice_frac = if frac < 1.5 { 1.0 } else if frac < 3.5 { 2.0 } else if frac < 7.5 { 5.0 } else { 10.0 };
    let step = nice_frac * 10f64.powf(mag);
    let start = (lo / step).ceil() * step;
    let mut ticks = Vec::new();
    let mut v = start;
    while v <= hi + step * 0.01 {
        ticks.push(v);
        v += step;
    }
    ticks
}

/// Clip a line segment to a rectangle. Returns None if fully outside.
fn clip_line_to_rect(p0: egui::Pos2, p1: egui::Pos2, rect: egui::Rect) -> Option<(egui::Pos2, egui::Pos2)> {
    // Cohen-Sutherland outcode.
    let code = |p: egui::Pos2| -> u8 {
        let mut c = 0u8;
        if p.x < rect.left()   { c |= 1; }
        if p.x > rect.right()  { c |= 2; }
        if p.y < rect.top()    { c |= 4; }
        if p.y > rect.bottom() { c |= 8; }
        c
    };
    let intersect = |pa: egui::Pos2, pb: egui::Pos2, edge: u8| -> egui::Pos2 {
        let dx = pb.x - pa.x;
        let dy = pb.y - pa.y;
        match edge {
            1 => egui::pos2(rect.left(),   pa.y + dy * (rect.left()   - pa.x) / dx),
            2 => egui::pos2(rect.right(),  pa.y + dy * (rect.right()  - pa.x) / dx),
            4 => egui::pos2(pa.x + dx * (rect.top()    - pa.y) / dy, rect.top()),
            8 => egui::pos2(pa.x + dx * (rect.bottom() - pa.y) / dy, rect.bottom()),
            _ => pa,
        }
    };
    let (mut a, mut b) = (p0, p1);
    let (mut ca, mut cb) = (code(a), code(b));
    loop {
        if ca | cb == 0 { return Some((a, b)); }
        if ca & cb != 0 { return None; }
        let c = if ca != 0 { ca } else { cb };
        let bit = c & (-(c as i8) as u8); // lowest set bit
        let p = intersect(a, b, bit);
        if ca != 0 { a = p; ca = code(a); } else { b = p; cb = code(b); }
    }
}

/// Only applies to values that look like floats. Very large/small exponents (outside
/// 1e-9..1e12) are left as-is to avoid unreadably long strings.
pub fn format_header_value(v: &str) -> String {
    let trimmed = v.trim();
    let Ok(val) = trimmed.parse::<f64>() else { return v.to_string() };

    if val == 0.0 { return "0".to_string(); }

    let abs = val.abs();
    if abs < 1e-9 || abs >= 1e12 { return v.to_string(); }

    // Number of decimal places to show ~8 significant figures.
    let mag = abs.log10().floor() as i32;
    let decimal_places = (8 - 1 - mag).max(0) as usize;
    let formatted = format!("{:.prec$}", val, prec = decimal_places);

    if formatted.contains('.') {
        formatted.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        formatted
    }
}

/// Star shape quality based on median axis ratio (b/a). 1.0 = round, lower = elongated.
fn roundness_quality(ratio: f32) -> (egui::Color32, &'static str) {
    if ratio >= 0.90 {
        (egui::Color32::from_rgb(80, 200, 80), "round")
    } else if ratio >= 0.75 {
        (egui::Color32::from_rgb(220, 180, 0), "slightly elongated")
    } else {
        (egui::Color32::from_rgb(220, 80, 80), "elongated")
    }
}

/// Tooltip for the Seeing row: full colour-coded scale for atmospheric quality.
fn seeing_tooltip(ui: &mut egui::Ui) {
    let green  = egui::Color32::from_rgb(80, 200, 80);
    let yellow = egui::Color32::from_rgb(220, 180, 0);
    let red    = egui::Color32::from_rgb(220, 80, 80);

    ui.label(egui::RichText::new("Star FWHM (arcsec)").strong());
    ui.label("Total PSF width including atmosphere, optics, and tracking\n(lower = sharper stars).");
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
        ui.label(egui::RichText::new("adequate  —  near Nyquist limit").color(yellow));
        ui.end_row();
        ui.label(egui::RichText::new("●").color(red));
        ui.label("< 2.5 px");
        ui.label(egui::RichText::new("undersampled  —  FWHM is a lower bound").color(red));
        ui.end_row();
    });
}
