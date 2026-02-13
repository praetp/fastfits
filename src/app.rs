use crate::fits::{ChannelView, FitsImage, Stretch};
use egui::TextureHandle;
use std::path::PathBuf;

pub struct FastFitsApp {
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

    /// Current stretch mode
    stretch: Stretch,
    /// Current channel view
    channel_view: ChannelView,

    /// Zoom: None = autofit, Some(s) = explicit scale factor
    zoom: Option<f32>,
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
            current_dir,
            files,
            selected,
            image: None,
            texture: None,
            load_error: None,
            stretch: Stretch::AutoStretch,
            channel_view: ChannelView::Rgb,
            zoom: None,
        };
        app.load_selected();
        app
    }

    /// Load (or reload) the currently selected file.
    fn load_selected(&mut self) {
        self.texture = None;
        self.load_error = None;
        self.image = None;

        let Some(idx) = self.selected else { return };
        let Some(path) = self.files.get(idx).cloned() else { return };

        match FitsImage::load(&path) {
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
        self.load_selected();
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
}

impl eframe::App for FastFitsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

        if go_next { self.select_next(); }
        if go_prev { self.select_prev(); }
        if toggle_stretch {
            self.stretch = match self.stretch {
                Stretch::AutoStretch => Stretch::Linear,
                Stretch::Linear => Stretch::AutoStretch,
            };
            self.texture = None;
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

        // Ensure texture is built
        if self.image.is_some() && self.texture.is_none() {
            self.rebuild_texture(ctx);
        }

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
                    // Stretch toggle
                    let stretch_label = match self.stretch {
                        Stretch::AutoStretch => "Auto",
                        Stretch::Linear => "Linear",
                    };
                    if ui.selectable_label(true, stretch_label).clicked() {
                        self.stretch = match self.stretch {
                            Stretch::AutoStretch => Stretch::Linear,
                            Stretch::Linear => Stretch::AutoStretch,
                        };
                        self.texture = None;
                    }
                    ui.label("Stretch:");
                    ui.separator();

                    // Channel selector (only for multi-channel images)
                    if let Some(img) = &self.image {
                        if img.channels >= 3 {
                            for ch in (0..img.channels).rev() {
                                let label = match ch { 0 => "R", 1 => "G", 2 => "B", _ => "?" };
                                if ui.selectable_label(self.channel_view == ChannelView::Single(ch), label).clicked() {
                                    self.channel_view = ChannelView::Single(ch);
                                    self.texture = None;
                                }
                            }
                            if ui.selectable_label(self.channel_view == ChannelView::Rgb, "RGB").clicked() {
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
                    ui.label(zoom_str);
                    ui.label("Zoom:");
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

        // Right panel: file browser
        egui::SidePanel::right("file_browser")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.heading("Files");
                ui.separator();
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
                        if ui.selectable_label(is_selected, &name).clicked() {
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
                    ui.label("No file selected");
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
