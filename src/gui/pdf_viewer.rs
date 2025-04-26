use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::convert::TryFrom;

use egui::{Context, Ui, Vec2, RichText, Color32, TextureHandle};
use lopdf::Document;
use image::{ImageBuffer, Rgba};
use pdfium_render::prelude::*;

/// PDF viewer component that renders PDFs using Pdfium
pub struct PdfViewer {
    current_pdf_path: Option<PathBuf>,
    document: Option<Arc<Document>>,
    pdfium: Option<Pdfium>,
    pdfium_document: Option<Arc<PdfDocumentWrapper>>,
    current_page: usize,
    total_pages: usize,
    zoom: f32,
    pages: HashMap<usize, PageData>,
    page_textures: HashMap<usize, TextureHandle>,
    document_title: String,
    outline: Vec<OutlineItem>,
    text_data: Arc<Mutex<String>>,
    loading: bool,
    document_loaded: Arc<Mutex<Option<Arc<Document>>>>,
    // View mode settings
    show_text_panel: bool,
    view_mode: ViewMode,
}

/// Wrapper around PdfDocument to make it shareable between threads
struct PdfDocumentWrapper {
    document: PdfDocument<'static>,
}

// This is safe because Pdfium handles its own thread safety
unsafe impl Send for PdfDocumentWrapper {}
unsafe impl Sync for PdfDocumentWrapper {}

/// View modes for the PDF viewer
#[derive(PartialEq, Clone, Copy)]
enum ViewMode {
    Rendered,
    TextOnly,
}

/// Page data
struct PageData {
    text: String,
    size: Vec2,
}

/// Outline item
struct OutlineItem {
    title: String,
    page: usize,
    level: usize,
    children: Vec<OutlineItem>,
}

impl PdfViewer {
    pub fn new() -> Self {
        // Try to initialize pdfium once at startup
        let pdfium = match Pdfium::bind_to_system_library() {
            Ok(bindings) => {
                println!("Successfully initialized Pdfium");
                Some(Pdfium::new(bindings))
            },
            Err(err) => {
                eprintln!("Failed to initialize Pdfium: {}", err);
                None
            }
        };

        Self {
            current_pdf_path: None,
            document: None,
            pdfium,
            pdfium_document: None,
            current_page: 0,
            total_pages: 0,
            zoom: 1.0,
            pages: HashMap::new(),
            page_textures: HashMap::new(),
            document_title: String::new(),
            outline: Vec::new(),
            text_data: Arc::new(Mutex::new(String::new())),
            loading: false,
            document_loaded: Arc::new(Mutex::new(None)),
            // Initialize new fields
            show_text_panel: false,
            view_mode: ViewMode::Rendered,
        }
    }
    
    /// Load a PDF file
    pub fn load_pdf(&mut self, path: &Path) {
        self.loading = true;
        self.current_pdf_path = Some(path.to_path_buf());
        
        // Create a clone for the async task
        let path_clone = path.to_path_buf();
        let text_data = self.text_data.clone();
        let document_loaded = self.document_loaded.clone();
        
        // Reset state
        self.document = None;
        self.pdfium_document = None;
        self.current_page = 0;
        self.total_pages = 0;
        self.pages.clear();
        self.page_textures.clear();
        self.document_title = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        
        // Load the PDF in a separate thread
        std::thread::spawn(move || {
            // Load with lopdf for structure parsing (optional, for compatibility)
            let lopdf_result = Document::load(&path_clone);
            
            // Extract text for search and analysis
            match extract_text_from_pdf(&path_clone) {
                Ok(text) => {
                    let mut text_data = text_data.lock().unwrap();
                    *text_data = text;
                },
                Err(e) => {
                    eprintln!("Error extracting text: {}", e);
                }
            }
            
            if let Ok(document) = lopdf_result {
                // Store the loaded document in the shared mutex
                let doc = Arc::new(document);
                let mut document_loaded = document_loaded.lock().unwrap();
                *document_loaded = Some(doc);
            } else {
                eprintln!("Error loading PDF with lopdf (optional)");
            }
        });
    }
    
    /// Process loaded document (should be called from the UI thread)
    fn process_loaded_document(&mut self, ctx: &Context) {
        if self.loading {
            // Check if document has been loaded by the background thread
            let doc_option = {
                let mut document_loaded = self.document_loaded.lock().unwrap();
                document_loaded.take()
            };
            
            if let Some(doc) = doc_option {
                // Update state with the loaded document
                self.document = Some(doc.clone());
                
                // Try to load the document with Pdfium for rendering
                if let Some(path) = &self.current_pdf_path {
                    if let Some(pdfium) = &self.pdfium {
                        match pdfium.load_pdf_from_file(path, None) {
                            Ok(pdfium_doc) => {
                                // Get the number of pages
                                self.total_pages = pdfium_doc.pages().len() as usize;
                                
                                // Try to extract title from document information
                                let metadata = pdfium_doc.metadata();
                                if let Ok(title) = metadata.title() {
                                    if !title.is_empty() {
                                        self.document_title = title;
                                    }
                                }
                                
                                // Store document for rendering
                                // We need to use a nasty trick to convert lifetimes
                                let document: PdfDocument<'static> = unsafe { 
                                    std::mem::transmute(pdfium_doc) 
                                };
                                self.pdfium_document = Some(Arc::new(PdfDocumentWrapper { document }));
                                
                                // Render the first page
                                self.render_page(0, ctx);
                            },
                            Err(e) => {
                                eprintln!("Error loading PDF with Pdfium: {:?}", e);
                                // Fallback to lopdf for page count
                                if let Some(doc) = &self.document {
                                    self.total_pages = doc.get_pages().len();
                                }
                            }
                        }
                    } else {
                        eprintln!("Pdfium library not initialized");
                    }
                }
                
                // Load first page text
                self.extract_page_text(0);
                
                // Document loading complete
                self.loading = false;
            }
        }
    }
    
    /// Render a PDF page using Pdfium
    fn render_page(&mut self, page_num: usize, ctx: &Context) {
        // Check if we already have this page texture
        if self.page_textures.contains_key(&page_num) {
            return;
        }
        
        if let Some(pdfium_doc) = &self.pdfium_document {
            // Convert usize to u16 for pdfium's page index
            let page_index = match u16::try_from(page_num) {
                Ok(index) => index,
                Err(_) => {
                    eprintln!("Page number too large: {}", page_num);
                    self.render_fallback_page(page_num, ctx);
                    return;
                }
            };
            
            // Get the page with error handling
            let page_result = pdfium_doc.document.pages().get(page_index);
            
            match page_result {
                Ok(page) => {
                    let result = std::panic::catch_unwind(|| {
                        // Get page dimensions (in points)
                        let width_points = page.width();
                        let height_points = page.height();
                        
                        // Create an image buffer at a reasonable resolution
                        let scale = 2.0;  // Scaling factor for better resolution
                        let width_px = (width_points.value * scale) as i32;
                        let height_px = (height_points.value * scale) as i32;
                        
                        // Create a new image buffer with white background
                        let mut img = ImageBuffer::<Rgba<u8>, Vec<u8>>::new(
                            width_px as u32, 
                            height_px as u32
                        );
                        for pixel in img.pixels_mut() {
                            *pixel = Rgba([255, 255, 255, 255]);
                        }
                        
                        // Render the page to a bitmap
                        let config = PdfRenderConfig::new()
                            .set_target_width(width_px)
                            .set_target_height(height_px);
                            
                        match page.render_with_config(&config) {
                            Ok(bitmap) => {
                                // Get the bitmap data using raw_pixels() which is the correct method in pdfium-render 0.8.30
                                let bitmap_width = bitmap.width() as u32;
                                let bitmap_height = bitmap.height() as u32;
                                let bitmap_data = bitmap.raw_pixels();
                                
                                // Copy bitmap data to our image buffer
                                for y in 0..height_px as u32 {
                                    for x in 0..width_px as u32 {
                                        if x < bitmap_width && y < bitmap_height {
                                            let idx = (y * bitmap_width + x) as usize * 4;
                                            
                                            if idx + 3 < bitmap_data.len() {
                                                let r = bitmap_data[idx];
                                                let g = bitmap_data[idx + 1];
                                                let b = bitmap_data[idx + 2];
                                                let a = bitmap_data[idx + 3];
                                                
                                                img.put_pixel(x, y, Rgba([r, g, b, a]));
                                            }
                                        }
                                    }
                                }
                                
                                // Convert to egui texture
                                let size = [width_px as usize, height_px as usize];
                                let pixels = img.into_raw();
                                
                                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                    size,
                                    &pixels
                                );
                                
                                // Load as texture
                                let texture = ctx.load_texture(
                                    format!("pdf_page_{}", page_num),
                                    color_image,
                                    egui::TextureOptions::default()
                                );
                                
                                Some((texture, Vec2::new(width_points.value as f32, height_points.value as f32)))
                            },
                            Err(e) => {
                                eprintln!("Error rendering page: {:?}", e);
                                None
                            }
                        }
                    });
                    
                    if let Ok(Some((texture, size))) = result {
                        // Store texture for reuse
                        self.page_textures.insert(page_num, texture);
                        
                        // Also extract text for this page
                        let mut page_text = String::new();
                        
                        // Try to extract text from the page
                        if let Ok(page_text_obj) = page.text() {
                            // Get text from the page
                            page_text = page_text_obj.to_string();
                        }
                        
                        // Store page data with text and size
                        self.pages.insert(page_num, PageData { 
                            text: page_text, 
                            size,
                        });
                    } else {
                        self.render_fallback_page(page_num, ctx);
                    }
                },
                Err(e) => {
                    eprintln!("Error getting page {}: {:?}", page_num, e);
                    self.render_fallback_page(page_num, ctx);
                }
            }
        } else {
            self.render_fallback_page(page_num, ctx);
        }
    }

    /// Render a fallback page when Pdfium rendering fails
    fn render_fallback_page(&mut self, page_num: usize, ctx: &Context) {
        // Create a simple placeholder page
        let width_px = 612;  // Standard letter width in pixels at 72 DPI
        let height_px = 792; // Standard letter height
        
        let mut img = ImageBuffer::<Rgba<u8>, Vec<u8>>::new(width_px, height_px);
        
        // Fill with white background
        for x in 0..width_px {
            for y in 0..height_px {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        
        // Draw a gray border
        for x in 0..width_px {
            img.put_pixel(x, 0, Rgba([200, 200, 200, 255]));
            img.put_pixel(x, height_px - 1, Rgba([200, 200, 200, 255]));
        }
        
        for y in 0..height_px {
            img.put_pixel(0, y, Rgba([200, 200, 200, 255]));
            img.put_pixel(width_px - 1, y, Rgba([200, 200, 200, 255]));
        }
        
        // Draw a colored rectangle in the middle to indicate status
        let rect_size = width_px.min(height_px) / 10;
        let start_x = width_px / 2 - rect_size / 2;
        let start_y = height_px / 2 - rect_size / 2;
        
        for x in start_x..(start_x + rect_size) {
            for y in start_y..(start_y + rect_size) {
                img.put_pixel(x, y, Rgba([200, 100, 100, 255]));
            }
        }
        
        // Convert to egui texture
        let size = [width_px as usize, height_px as usize];
        let pixels = img.into_raw();
        
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            size,
            &pixels
        );
        
        // Load as texture
        let texture = ctx.load_texture(
            format!("pdf_page_{}", page_num),
            color_image,
            egui::TextureOptions::default()
        );
        
        // Store texture for reuse
        self.page_textures.insert(page_num, texture);
        
        // Extract text if needed
        self.extract_page_text(page_num);
    }
    
    /// Extract text from a specific page
    fn extract_page_text(&mut self, page_num: usize) {
        if self.pages.contains_key(&page_num) {
            return; // Already loaded
        }
        
        // First try to get text from Pdfium
        if let Some(pdfium_doc) = &self.pdfium_document {
            // Convert usize to u16 for pdfium's page index
            if let Ok(page_index) = u16::try_from(page_num) {
                if let Ok(page) = pdfium_doc.document.pages().get(page_index) {
                    let mut page_text = String::new();
                    
                    // Try to extract text from the page
                    if let Ok(page_text_obj) = page.text() {
                        // Get text from the page
                        page_text = page_text_obj.to_string();
                    }
                    
                    let width_points = page.width();
                    let height_points = page.height();
                    let size = Vec2::new(width_points.value as f32, height_points.value as f32);
                    
                    // Store in page data
                    self.pages.insert(page_num, PageData { text: page_text, size });
                    return;
                }
            }
        }
        
        // Fallback to our loaded text data
        self.load_page_text(page_num);
    }
    
    /// Load page text content (fallback for when Pdfium extraction fails)
    fn load_page_text(&mut self, page_num: usize) {
        // If we already have page data, skip
        if self.pages.contains_key(&page_num) {
            return;
        }
        
        // Default page size
        let size = Vec2::new(612.0, 792.0); // Letter size
        
        // Get text content (from the already extracted text)
        let text = {
            let text_data = self.text_data.lock().unwrap();
            text_data.clone()
        };
        
        // For a real implementation, we'd extract text for the specific page
        // For now, we'll just show all the text on the first page (fallback method)
        if page_num == 0 {
            self.pages.insert(page_num, PageData { text, size });
        } else {
            self.pages.insert(page_num, PageData { 
                text: format!("Page {} content", page_num + 1),
                size 
            });
        }
    }
    
    /// Get the current PDF path
    pub fn current_pdf(&self) -> Option<&PathBuf> {
        self.current_pdf_path.as_ref()
    }
    
    /// Get the PDF text
    pub fn text(&self) -> String {
        let text_data = self.text_data.lock().unwrap();
        text_data.clone()
    }
    
    /// Show the PDF viewer
    pub fn show(&mut self, ui: &mut Ui, ctx: &Context) {
        // Process any loaded document
        self.process_loaded_document(ctx);
        
        // Split the PDF viewer into top controls and content
        ui.vertical(|ui| {
            // Top panel with controls
            egui::TopBottomPanel::top("pdf_controls")
                .resizable(false)
                .frame(egui::Frame::none().fill(ui.style().visuals.panel_fill))
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Document title
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if !self.document_title.is_empty() {
                                ui.label(RichText::new(&self.document_title).strong().heading());
                            }
                        });
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Open PDF button
                            if ui.button("📂 Open PDF...").clicked() {
                                if let Some(path) = Self::open_file_dialog() {
                                    self.load_pdf(&path);
                                }
                            }
                        });
                    });
                    
                    // Navigation controls
                    ui.horizontal(|ui| {
                        // Page navigation
                        if ui.add_enabled(self.current_page > 0, egui::Button::new("◀ Previous")).clicked() {
                            self.current_page = self.current_page.saturating_sub(1);
                            // Pre-render the page (will be cached if already rendered)
                            if self.view_mode == ViewMode::Rendered {
                                self.render_page(self.current_page, ctx);
                            } else {
                                self.extract_page_text(self.current_page);
                            }
                        }
                        
                        let total_pages = self.total_pages.max(1);
                        ui.label(format!("Page {} of {}", self.current_page + 1, total_pages));
                        
                        if ui.add_enabled(self.current_page < self.total_pages.saturating_sub(1), 
                                        egui::Button::new("Next ▶")).clicked() {
                            self.current_page = (self.current_page + 1).min(self.total_pages.saturating_sub(1));
                            // Pre-render the page (will be cached if already rendered)
                            if self.view_mode == ViewMode::Rendered {
                                self.render_page(self.current_page, ctx);
                            } else {
                                self.extract_page_text(self.current_page);
                            }
                        }
                        
                        // Zoom controls
                        ui.separator();
                        
                        if ui.add_enabled(self.zoom > 0.2, egui::Button::new("🔍-")).clicked() {
                            self.zoom = (self.zoom - 0.1).max(0.1);
                        }
                        
                        ui.label(format!("{:.0}%", self.zoom * 100.0));
                        
                        if ui.add_enabled(self.zoom < 3.0, egui::Button::new("🔍+")).clicked() {
                            self.zoom = (self.zoom + 0.1).min(3.0);
                        }

                        // View mode toggle
                        ui.separator();
                        
                        // Option to toggle text panel
                        if ui.checkbox(&mut self.show_text_panel, "Show Text").clicked() {
                            // Toggle was clicked, no additional action needed
                        }
                        
                        // View mode options
                        ui.label("View:");
                        if ui.radio(self.view_mode == ViewMode::Rendered, "Rendered").clicked() {
                            self.view_mode = ViewMode::Rendered;
                            // Ensure the current page is rendered
                            self.render_page(self.current_page, ctx);
                        }
                        if ui.radio(self.view_mode == ViewMode::TextOnly, "Text Only").clicked() {
                            self.view_mode = ViewMode::TextOnly;
                            // Ensure we have the text content loaded
                            if !self.pages.contains_key(&self.current_page) {
                                self.extract_page_text(self.current_page);
                            }
                        }
                    });
                });
            
            // Main content area for the PDF
            if self.document.is_some() || self.pdfium_document.is_some() {
                match self.view_mode {
                    ViewMode::Rendered => {
                        if self.show_text_panel {
                            // Split view with rendered PDF and text
                            egui::SidePanel::right("text_panel")
                                .resizable(true)
                                .default_width(350.0)
                                .width_range(200.0..=600.0)
                                .show_inside(ui, |ui| {
                                    ui.heading("Extracted Text");
                                    ui.separator();
                                    
                                    if let Some(page_data) = self.pages.get(&self.current_page) {
                                        egui::ScrollArea::vertical()
                                            .id_source("text_panel_scroll")
                                            .show(ui, |ui| {
                                                if !page_data.text.is_empty() {
                                                    ui.label(&page_data.text);
                                                } else {
                                                    ui.label("No text content available");
                                                }
                                            });
                                    } else {
                                        ui.label("Loading text content...");
                                    }
                                });
                        }
                        
                        // Display the rendered PDF content
                        egui::CentralPanel::default().show_inside(ui, |ui| {
                            egui::ScrollArea::both()
                                .auto_shrink([false; 2])
                                .id_source("pdf_content")
                                .show(ui, |ui| {
                                    // Check if we have a rendered page texture
                                    if let Some(texture) = self.page_textures.get(&self.current_page) {
                                        // Calculate scaled size based on zoom
                                        let size = texture.size_vec2() * self.zoom;
                                        
                                        // Center the page in the view
                                        ui.vertical_centered(|ui| {
                                            // Create an image with the proper size
                                            let image = egui::Image::new(texture)
                                                .fit_to_exact_size(size);
                                            ui.add(image);
                                        });
                                    } else {
                                        // Render the page if not available
                                        self.render_page(self.current_page, ctx);
                                        
                                        ui.vertical_centered(|ui| {
                                            ui.add_space(50.0);
                                            ui.label("Rendering page...");
                                            ui.add_space(50.0);
                                        });
                                    }
                                });
                        });
                    },
                    ViewMode::TextOnly => {
                        // Display the PDF content in a scrollable area with text
                        egui::CentralPanel::default().show_inside(ui, |ui| {
                            egui::ScrollArea::both()
                                .auto_shrink([false; 2])
                                .id_source("pdf_content")
                                .show(ui, |ui| {
                                    if let Some(page_data) = self.pages.get(&self.current_page) {
                                        // Calculate the size of the text display
                                        let text_height = page_data.text.lines().count() as f32 * 18.0;
                                        let content_rect = egui::Rect::from_min_size(
                                            ui.cursor().min,
                                            Vec2::new(
                                                page_data.size.x * self.zoom, 
                                                text_height.max(page_data.size.y * self.zoom)
                                            )
                                        );
                                        
                                        // Create a "page" with white background
                                        ui.painter().rect_filled(content_rect, 4.0, Color32::WHITE);
                                        ui.painter().rect_stroke(content_rect, 4.0, egui::Stroke::new(1.0, Color32::GRAY));
                                        
                                        // Show the text in a PDF-like format
                                        ui.allocate_rect(content_rect, egui::Sense::hover());
                                        
                                        // Create a more readable PDF-like layout
                                        let text_rect = content_rect.shrink(20.0);
                                        
                                        // Display PDF content with better formatting
                                        if !page_data.text.is_empty() {
                                            let mut paragraphs = Vec::new();
                                            let mut current_paragraph = String::new();
                                            
                                            for line in page_data.text.lines() {
                                                let trimmed = line.trim();
                                                if trimmed.is_empty() {
                                                    if !current_paragraph.is_empty() {
                                                        paragraphs.push(current_paragraph);
                                                        current_paragraph = String::new();
                                                    }
                                                } else {
                                                    if !current_paragraph.is_empty() {
                                                        current_paragraph.push(' ');
                                                    }
                                                    current_paragraph.push_str(trimmed);
                                                }
                                            }
                                            
                                            if !current_paragraph.is_empty() {
                                                paragraphs.push(current_paragraph);
                                            }
                                            
                                            let mut current_y = text_rect.min.y;
                                            let line_height = 20.0;
                                            
                                            for paragraph in paragraphs {
                                                let paragraph_rect = egui::Rect::from_min_max(
                                                    egui::pos2(text_rect.min.x, current_y),
                                                    egui::pos2(text_rect.max.x, current_y + line_height * 5.0)
                                                );
                                                
                                                ui.put(paragraph_rect, egui::Label::new(&paragraph).wrap(true));
                                                current_y += line_height * 2.0;
                                            }
                                        } else {
                                            ui.put(text_rect, egui::Label::new("No text content available"));
                                        }
                                    } else {
                                        ui.vertical_centered(|ui| {
                                            ui.add_space(50.0);
                                            ui.label("Loading page content...");
                                            ui.add_space(50.0);
                                        });
                                    }
                                });
                        });
                    }
                }
            } else if self.loading {
                // Show loading indicator
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.label("Loading PDF...");
                    ui.add_space(100.0);
                });
            } else {
                // Show welcome screen when no document is loaded
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.heading("Welcome to PDFScan");
                    ui.add_space(20.0);
                    ui.label("To get started, open a PDF document.");
                    ui.add_space(20.0);
                    if ui.button("📂 Open PDF...").clicked() {
                        if let Some(path) = Self::open_file_dialog() {
                            self.load_pdf(&path);
                        }
                    }
                    ui.add_space(100.0);
                });
            }
        });
    }
    
    /// Show the document outline in the sidebar
    pub fn show_outline(&self, ui: &mut Ui) {
        if self.outline.is_empty() {
            ui.label("No outline available");
            return;
        }
        
        ui.heading("Document Outline");
        
        for item in &self.outline {
            self.show_outline_item(ui, item);
        }
    }
    
    /// Recursively show an outline item and its children
    fn show_outline_item(&self, ui: &mut Ui, item: &OutlineItem) {
        ui.horizontal(|ui| {
            // Indent based on level
            ui.add_space(item.level as f32 * 10.0);
            
            // Highlight if this is the current page
            let text = if item.page == self.current_page {
                RichText::new(&item.title).strong().color(ui.visuals().selection.stroke.color)
            } else {
                RichText::new(&item.title)
            };
            
            if ui.link(text).clicked() {
                // In a real implementation, this would scroll to the page
                // For now, we just set it as the current page
                // self.current_page = item.page;
            }
        });
        
        // Show children
        for child in &item.children {
            self.show_outline_item(ui, child);
        }
    }

    /// Open a file dialog
    fn open_file_dialog() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("PDF Files", &["pdf"])
            .pick_file()
    }
}

/// Extract text from a PDF file using the pdf-extract library
fn extract_text_from_pdf(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let bytes = std::fs::read(path)?;
    let text = pdf_extract::extract_text_from_mem(&bytes)?;
    Ok(text)
} 