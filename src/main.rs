mod app;
mod fits;

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

    let start_path = args
        .path
        .unwrap_or_else(|| std::env::current_dir().expect("cannot determine current directory"));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("fastfits"),
        ..Default::default()
    };

    eframe::run_native(
        "fastfits",
        options,
        Box::new(|cc| Ok(Box::new(app::FastFitsApp::new(cc, start_path)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))
}
