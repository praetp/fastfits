use crate::fits::{ChannelView, HistogramData, Stretch};

/// Draw a per-channel histogram into the current UI panel.
pub(crate) fn draw_histogram(
    ui: &mut egui::Ui,
    hist: &HistogramData,
    stretch: Stretch,
    view: ChannelView,
) {
    const HIST_HEIGHT: f32 = 80.0;

    let width = ui.available_width();
    let (resp, painter) = ui.allocate_painter(egui::Vec2::new(width, HIST_HEIGHT), egui::Sense::hover());
    let rect = resp.rect;

    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(20));

    let (channel_indices, channel_colors, channel_names): (Vec<usize>, Vec<egui::Color32>, Vec<&str>) = match view {
        ChannelView::Single(c) => {
            let idx = c.min(hist.channels.len().saturating_sub(1));
            let name = match c { 0 => "R", 1 => "G", 2 => "B", _ => "Ch" };
            (vec![idx], vec![egui::Color32::from_rgba_unmultiplied(200, 200, 200, 160)], vec![name])
        }
        ChannelView::Rgb if hist.channels.len() == 1 => {
            (vec![0], vec![egui::Color32::from_rgba_unmultiplied(200, 200, 200, 160)], vec!["Gray"])
        }
        ChannelView::Rgb => (
            vec![0, 1, 2],
            vec![
                egui::Color32::from_rgba_unmultiplied(220,  60,  60, 160),
                egui::Color32::from_rgba_unmultiplied( 60, 180,  60, 160),
                egui::Color32::from_rgba_unmultiplied( 60, 100, 220, 160),
            ],
            vec!["R", "G", "B"],
        ),
    };

    let global_max = channel_indices.iter()
        .filter_map(|&ci| hist.channels.get(ci))
        .flat_map(|ch| ch.bins.iter()).copied()
        .max().unwrap_or(1).max(1);
    let num_bins = hist.channels.first().map(|ch| ch.bins.len()).unwrap_or(256);
    let bin_w = width / num_bins as f32;

    for (&ci, &col) in channel_indices.iter().zip(channel_colors.iter()) {
        let Some(ch) = hist.channels.get(ci) else { continue };
        let stroke = egui::Stroke::new(1.0, col);
        let points: Vec<egui::Pos2> = ch.bins.iter().enumerate().map(|(b, &count)| {
            let h = ((count + 1) as f64).ln() / ((global_max + 1) as f64).ln() * HIST_HEIGHT as f64;
            let x = rect.left() + (b as f32 + 0.5) * bin_w;
            egui::pos2(x, rect.bottom() - h as f32)
        }).collect();
        for pair in points.windows(2) {
            painter.line_segment([pair[0], pair[1]], stroke);
        }
    }

    painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, egui::Color32::from_gray(60)), egui::StrokeKind::Inside);

    // Hover tooltip with statistics.
    resp.on_hover_ui(|ui| {
        let fmt = |v: f32| {
            if v.abs() < 10.0 { format!("{:.2}", v) } else { format!("{:.0}", v) }
        };

        for (&ci, &name) in channel_indices.iter().zip(channel_names.iter()) {
            let Some(ch) = hist.channels.get(ci) else { continue };
            if channel_indices.len() > 1 {
                ui.label(egui::RichText::new(name).strong());
            }
            egui::Grid::new(format!("hist_tip_{ci}")).num_columns(2).spacing([12.0, 1.0]).show(ui, |ui| {
                ui.label("Min:");    ui.label(fmt(ch.data_min)); ui.end_row();
                ui.label("Max:");    ui.label(fmt(ch.data_max)); ui.end_row();
                ui.label("Median:"); ui.label(fmt(ch.median));   ui.end_row();
                ui.label("Sigma:");  ui.label(fmt(ch.sigma));    ui.end_row();
                if stretch == Stretch::AutoStretch {
                    if let Some(bp) = ch.black_frac {
                        let bv = ch.data_min + bp * (ch.data_max - ch.data_min);
                        ui.label("Black pt:"); ui.label(fmt(bv)); ui.end_row();
                    }
                    if let Some(wp) = ch.white_frac {
                        let wv = ch.data_min + wp * (ch.data_max - ch.data_min);
                        ui.label("White pt:"); ui.label(fmt(wv)); ui.end_row();
                    }
                }
            });
            if ci != *channel_indices.last().unwrap_or(&0) {
                ui.separator();
            }
        }
    });
}
