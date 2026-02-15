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

    let (channel_indices, channel_colors): (Vec<usize>, Vec<egui::Color32>) = match view {
        ChannelView::Single(c) => {
            let idx = c.min(hist.channels.len().saturating_sub(1));
            (vec![idx], vec![egui::Color32::from_rgba_unmultiplied(200, 200, 200, 160)])
        }
        ChannelView::Rgb if hist.channels.len() == 1 => {
            (vec![0], vec![egui::Color32::from_rgba_unmultiplied(200, 200, 200, 160)])
        }
        ChannelView::Rgb => (
            vec![0, 1, 2],
            vec![
                egui::Color32::from_rgba_unmultiplied(220,  60,  60, 160),
                egui::Color32::from_rgba_unmultiplied( 60, 180,  60, 160),
                egui::Color32::from_rgba_unmultiplied( 60, 100, 220, 160),
            ],
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
        for (b, &count) in ch.bins.iter().enumerate() {
            let bar_h = ((count + 1) as f64).ln() / ((global_max + 1) as f64).ln() * HIST_HEIGHT as f64;
            let x0 = rect.left() + b as f32 * bin_w;
            let bar_rect = egui::Rect::from_min_max(
                egui::pos2(x0, rect.bottom() - bar_h as f32),
                egui::pos2(x0 + bin_w.max(1.0), rect.bottom()),
            );
            painter.rect_filled(bar_rect, 0.0, col);
        }
    }

    if stretch == Stretch::AutoStretch {
        let marker_colors: Vec<egui::Color32> = if hist.channels.len() == 1
            || matches!(view, ChannelView::Single(_))
        {
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

    painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, egui::Color32::from_gray(60)));
}
