use crate::app::{FastFitsApp, LoadResult};
use crate::fits::{ChannelView, FitsImage, Stretch, compute_histogram};
use crate::histogram_ui::draw_histogram;
use std::sync::mpsc;

impl eframe::App for FastFitsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_background_loads(ctx);

        let open_file  = ctx.input(|i| i.key_pressed(egui::Key::O) && i.modifiers.command);
        let go_next    = ctx.input(|i| i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowDown));
        let go_prev    = ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)  || i.key_pressed(egui::Key::ArrowUp));
        let toggle_stretch    = ctx.input(|i| i.key_pressed(egui::Key::S));
        let zoom_in    = ctx.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals));
        let zoom_out   = ctx.input(|i| i.key_pressed(egui::Key::Minus));
        let zoom_reset = ctx.input(|i| i.key_pressed(egui::Key::Num0));
        let zoom_fit   = ctx.input(|i| i.key_pressed(egui::Key::F));
        let do_delete  = ctx.input(|i| i.key_pressed(egui::Key::Delete));
        let toggle_help      = ctx.input(|i| i.key_pressed(egui::Key::Questionmark));
        let toggle_prefs     = ctx.input(|i| i.key_pressed(egui::Key::Comma));
        let toggle_histogram = ctx.input(|i| i.key_pressed(egui::Key::H));
        let toggle_about     = ctx.input(|i| i.key_pressed(egui::Key::A));
        let close_popup      = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        if go_next    { self.select_next(); }
        if go_prev    { self.select_prev(); }
        if do_delete  { self.delete_selected(); }
        if zoom_in    { let s = self.zoom.unwrap_or(1.0); self.zoom = Some((s * 1.25).min(32.0)); }
        if zoom_out   { let s = self.zoom.unwrap_or(1.0); self.zoom = Some((s / 1.25).max(0.05)); }
        if zoom_reset { self.zoom = Some(1.0); }
        if zoom_fit   { self.zoom = None; self.pan_offset = egui::Vec2::ZERO; }
        if toggle_help      { self.show_help      = !self.show_help; }
        if toggle_prefs     { self.show_prefs     = !self.show_prefs; }
        if toggle_histogram { self.show_histogram = !self.show_histogram; }
        if toggle_about     { self.show_about     = !self.show_about; }
        if toggle_stretch {
            self.stretch = match self.stretch {
                Stretch::AutoStretch => Stretch::Linear,
                Stretch::Linear      => Stretch::AutoStretch,
            };
            self.texture  = None;
            self.histogram = None;
            self.hist_rx  = None;
        }
        if close_popup {
            self.show_help  = false;
            self.show_prefs = false;
            self.show_about = false;
        }

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

        let (go_prev_btn, go_next_btn, do_delete_btn) = self.show_bottom_bar(ctx);
        if go_prev_btn   { self.select_prev(); }
        if go_next_btn   { self.select_next(); }
        if do_delete_btn { self.delete_selected(); }

        let open_btn = self.show_menu_bar(ctx);
        if open_file || open_btn {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("FITS files", &["fits", "fit", "fz"])
                .set_directory(&self.current_dir)
                .pick_file()
            {
                self.open_path(path);
            }
        }
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
                    headers: vec![], bitdepth_max, is_bayer: false,
                };
                let _ = tx.send(compute_histogram(&img_shell, with_markers));
                ctx2.request_repaint();
            });
        }
    }

    fn show_help_window(&mut self, ctx: &egui::Context) {
        if !self.show_help { return; }
        egui::Window::new("Keyboard shortcuts")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::Grid::new("help_grid").striped(true).show(ui, |ui| {
                    let rows: &[(&str, &str)] = &[
                        ("Ctrl+O",             "Open file dialog"),
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

    /// Returns true if the image should be reloaded.
    fn show_prefs_window(&mut self, ctx: &egui::Context) -> bool {
        if !self.show_prefs { return false; }
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
                if ui.button("Close  [,]").clicked() {
                    self.show_prefs = false;
                }
            });
        reload
    }

    fn show_about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about { return; }
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
                    ui.label("Author");    ui.label(env!("CARGO_PKG_AUTHORS")); ui.end_row();
                    ui.label("License");  ui.label(env!("CARGO_PKG_LICENSE")); ui.end_row();
                    ui.label("Repository"); ui.hyperlink("https://github.com/praetp/fastfits"); ui.end_row();
                    ui.label("Built");    ui.label(env!("FASTFITS_BUILD_DATE")); ui.end_row();
                    ui.label("Rust");     ui.label(env!("FASTFITS_RUSTC_VERSION")); ui.end_row();
                });
                ui.separator();
                if ui.button("Close  [A]").clicked() {
                    self.show_about = false;
                }
            });
    }

    fn show_menu_bar(&mut self, ctx: &egui::Context) -> bool {
        let mut open_clicked = false;
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.label(egui::RichText::new("fastfits").strong());
                ui.separator();
                if ui.button("Open…").on_hover_text("Open a FITS file  [Ctrl+O]").clicked() {
                    open_clicked = true;
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
                    ui.separator();
                    self.draw_stretch_and_channels(ui);
                });
            });
        });
        open_clicked
    }

    fn draw_stretch_and_channels(&mut self, ui: &mut egui::Ui) {
        let zoom_str = match self.zoom {
            None    => "Fit".to_string(),
            Some(s) => format!("{:.0}%", s * 100.0),
        };
        ui.label(zoom_str).on_hover_text("Zoom  [+] [-] [0=1:1] [F=fit]");
        ui.label("Zoom:").on_hover_text("Zoom  [+] [-] [0=1:1] [F=fit]");
        ui.separator();

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
        let has_files = !self.files.is_empty();
        let btn_size  = egui::vec2(100.0, 32.0);
        let mut go_prev = false;
        let mut go_next = false;
        let mut do_del  = false;

        egui::TopBottomPanel::bottom("nav_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                // Pixel info on the left.
                if let Some(info) = &self.hover_pixel_info {
                    ui.monospace(info);
                }

                // Nav / delete buttons pushed to the right.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(msg) = &self.delete_status.clone() {
                        ui.label(egui::RichText::new(msg).color(egui::Color32::RED));
                        if ui.small_button("x").clicked() { self.delete_status = None; }
                        ui.separator();
                    }
                    if ui.add_enabled(self.selected.is_some(), egui::Button::new("Delete").min_size(btn_size))
                        .on_hover_text("Move file to trash  [Del]").clicked()
                    {
                        do_del = true;
                    }
                    ui.separator();
                    if ui.add_enabled(has_files, egui::Button::new("Next >").min_size(btn_size))
                        .on_hover_text("Next file  [Right / Down]").clicked()
                    {
                        go_next = true;
                    }
                    if ui.add_enabled(has_files, egui::Button::new("< Prev").min_size(btn_size))
                        .on_hover_text("Previous file  [Left / Up]").clicked()
                    {
                        go_prev = true;
                    }
                });
            });
            ui.add_space(4.0);
        });
        (go_prev, go_next, do_del)
    }

    fn show_left_panel(&self, ctx: &egui::Context) {
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
    }

    fn show_right_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("file_browser")
            .resizable(true)
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

            painter.image(
                texture.id(),
                image_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            self.image_screen_rect = Some(image_rect);

            // Crosshair overlay.
            if let Some(pos) = pointer_pos {
                if image_rect.contains(pos) {
                    let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 120));
                    // Horizontal line across the image at cursor y.
                    painter.line_segment(
                        [egui::pos2(image_rect.min.x, pos.y), egui::pos2(image_rect.max.x, pos.y)],
                        stroke,
                    );
                    // Vertical line across the image at cursor x.
                    painter.line_segment(
                        [egui::pos2(pos.x, image_rect.min.y), egui::pos2(pos.x, image_rect.max.y)],
                        stroke,
                    );
                }
            }

            // Pixel value under cursor.
            self.hover_pixel_info = None;
            if let (Some(pos), Some(img)) = (pointer_pos, &self.image) {
                if image_rect.contains(pos) {
                    let px = ((pos - image_rect.min) / zoom_factor).floor();
                    let x  = (px.x as usize).min(img.width.saturating_sub(1));
                    let y  = (px.y as usize).min(img.height.saturating_sub(1));
                    let npix = img.width * img.height;
                    let idx  = y * img.width + x;
                    self.hover_pixel_info = Some(match self.channel_view {
                        ChannelView::Single(c) => {
                            format!("({x}, {y})  val={:.0}", img.data[c * npix + idx])
                        }
                        ChannelView::Rgb if img.channels == 3 => {
                            format!("({x}, {y})  R={:.0} G={:.0} B={:.0}",
                                img.data[idx],
                                img.data[npix + idx],
                                img.data[2 * npix + idx])
                        }
                        ChannelView::Rgb => {
                            format!("({x}, {y})  val={:.0}", img.data[idx])
                        }
                    });
                }
            }
        });
    }
}

