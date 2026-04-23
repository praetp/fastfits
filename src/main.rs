mod app;
mod cache;
mod dso;
mod fits;
mod focus_analysis;
mod histogram_ui;
mod seeing;
mod ui;
mod wcs;

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fastfits", about = "Fast FITS file viewer")]
struct Args {
    /// FITS file or directory to open (defaults to current directory)
    path: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title(concat!("fastfits ", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    };

    eframe::run_native(
        "fastfits",
        options,
        Box::new(move |cc| Ok(Box::new(app::FastFitsApp::new(cc, args.path)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))
}
