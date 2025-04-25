use eframe::{NativeOptions, ViewportBuilder, run_native};
use pdfscan::gui::app::PdfScanApp;

mod gui;
mod extract;
mod search;
mod stats;

fn main() -> Result<(), eframe::Error> {
    // Initialize logging
    env_logger::init();
    
    // Set up native options
    let options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([800.0, 600.0])
            .with_title("PDFScan")
            .build(),
        ..Default::default()
    };
    
    // Run the app
    run_native(
        "PDFScan",
        options,
        Box::new(|cc| Box::new(PdfScanApp::new(cc)))
    )
} 