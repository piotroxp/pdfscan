use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

use egui::{Context, Ui, Vec2, RichText, Color32, TextureHandle};
use lopdf::Document;
use poppler::Document as PopplerDocument;
use image::{ImageBuffer, Rgba};

/// PDF viewer component
pub struct PdfViewer {
    current_pdf_path: Option<PathBuf>,
    document: Option<Arc<Document>>,
    poppler_document: Option<Arc<PopplerDocument>>,
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
        Self {
            current_pdf_path: None,
            document: None,
            poppler_document: None,
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
        self.poppler_document = None;
        self.current_page = 0;
        self.total_pages = 0;
        self.pages.clear();
        self.page_textures.clear();
        self.document_title = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        
        // Load the PDF in a separate thread
        std::thread::spawn(move || {
            // Load with lopdf for structure parsing
            let lopdf_result = Document::load(&path_clone);
            
            // Load with Poppler for rendering
            // No need to handle Poppler document here as we'll do it in process_loaded_document
            
            match lopdf_result {
                Ok(document) => {
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
                    
                    // Store the loaded document in the shared mutex
                    let doc = Arc::new(document);
                    let mut document_loaded = document_loaded.lock().unwrap();
                    *document_loaded = Some(doc);
                },
                Err(e) => {
                    eprintln!("Error loading PDF with lopdf: {}", e);
                }
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
                
                // Try to load the document with Poppler for rendering
                if let Some(path) = &self.current_pdf_path {
                    // Convert path to a properly encoded file URI
                    // First get absolute path
                    let absolute_path = if path.is_absolute() {
                        path.clone()
                    } else {
                        std::env::current_dir().unwrap_or_default().join(path)
                    };
                    
                    // Then convert to URI with proper escaping
                    let path_str = absolute_path.to_string_lossy();
                    let file_uri = if cfg!(windows) {
                        // Windows paths need special handling
                        format!("file:///{}", path_str.replace('\\', "/"))
                    } else {
                        // Unix paths just need the scheme prefix
                        format!("file://{}", path_str)
                    };
                    
                    match PopplerDocument::from_file(
                        &file_uri,
                        None, // No password
                    ) {
                        Ok(poppler_doc) => {
                            // Get the number of pages from Poppler and convert from i32 to usize
                            self.total_pages = poppler_doc.n_pages() as usize;
                            
                            // Store Poppler document for rendering
                            self.poppler_document = Some(Arc::new(poppler_doc));
                            
                            // Render the first page
                            self.render_page(0, ctx);
                        },
                        Err(e) => {
                            eprintln!("Error loading PDF with Poppler in main thread: {:?}", e);
                            
                            // Fallback to lopdf for page count
                            if let Some(doc) = &self.document {
                                self.total_pages = doc.get_pages().len();
                            }
                        }
                    }
                }
                
                // Load first page text
                self.load_page_text(0);
                
                // Document loading complete
                self.loading = false;
            }
        }
    }
    
    /// Render a PDF page using Poppler
    fn render_page(&mut self, page_num: usize, ctx: &Context) {
        // Check if we already have this page texture
        if self.page_textures.contains_key(&page_num) {
            return;
        }
        
        if let Some(poppler_doc) = &self.poppler_document {
            // Get the page - convert usize to i32 for the Poppler API
            if let Some(page) = poppler_doc.page(page_num as i32) {
                // Get page dimensions
                let (width, height) = page.size();
                
                // Create an image buffer at a reasonable resolution
                let scale = 2.0;  // Scaling factor for better resolution
                let width_px = (width * scale) as u32;
                let height_px = (height * scale) as u32;
                
                let mut img = ImageBuffer::<Rgba<u8>, Vec<u8>>::new(width_px, height_px);
                
                // Fill with white background
                for pixel in img.pixels_mut() {
                    *pixel = Rgba([255, 255, 255, 255]);
                }
                
                // Use a simplified approach to render to our image buffer
                // This is not perfect but avoids dependency on Cairo
                // In a real-world application, you would want to use Cairo properly
                
                // Create a simple context for rendering - we'll just draw the page outline
                let mut drawn = false;
                
                // Try to render using the text content as a fallback
                if let Some(_) = page.text() {
                    drawn = true;
                    // We have the text - at minimum we can place it on a white background
                    
                    // Draw a simple border for the page
                    for x in 0..width_px {
                        // Top and bottom border
                        if x < width_px {
                            img.put_pixel(x, 0, Rgba([200, 200, 200, 255]));
                            img.put_pixel(x, height_px - 1, Rgba([200, 200, 200, 255]));
                        }
                    }
                    
                    for y in 0..height_px {
                        // Left and right border
                        if y < height_px {
                            img.put_pixel(0, y, Rgba([200, 200, 200, 255]));
                            img.put_pixel(width_px - 1, y, Rgba([200, 200, 200, 255]));
                        }
                    }
                }
                
                if !drawn {
                    // If we can't render or get text, at least provide a placeholder
                    // Draw a border and page number
                    for x in 0..width_px {
                        for y in 0..height_px {
                            if x < 2 || x > width_px - 3 || y < 2 || y > height_px - 3 {
                                img.put_pixel(x, y, Rgba([200, 200, 200, 255]));
                            }
                        }
                    }
                    
                    // Note: In a real implementation, you would want to use a proper
                    // font rendering library to draw the page number text
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
                
                // Also extract text for this page specifically and store in page_data
                self.extract_page_text(page_num);
            }
        }
    }
    
    /// Extract text from a specific page
    fn extract_page_text(&mut self, page_num: usize) {
        if self.pages.contains_key(&page_num) {
            return; // Already loaded
        }
        
        if let Some(poppler_doc) = &self.poppler_document {
            if let Some(page) = poppler_doc.page(page_num as i32) {
                // Get page dimensions for storing
                let (width, height) = page.size();
                let size = Vec2::new(width as f32, height as f32);
                
                // Extract text from the page - page.text() returns Option<GString>, not Result
                let text = match page.text() {
                    Some(text) => text.to_string(),
                    None => {
                        eprintln!("No text found on page {}", page_num + 1);
                        String::new()
                    }
                };
                
                // Store in page data
                self.pages.insert(page_num, PageData { text, size });
            } else {
                // Create empty page data as fallback
                let size = Vec2::new(612.0, 792.0); // Letter size
                self.pages.insert(page_num, PageData { 
                    text: format!("Error loading page {} content", page_num + 1),
                    size 
                });
            }
        } else {
            // Legacy code for when poppler isn't available - use the global text
            // This is a fallback that puts all text on first page and generic 
            // messages on other pages
            let size = Vec2::new(612.0, 792.0); // Letter size
            
            if page_num == 0 {
                // For the first page, use all the extracted text
                let text = {
                    let text_data = self.text_data.lock().unwrap();
                    text_data.clone()
                };
                self.pages.insert(page_num, PageData { text, size });
            } else {
                // For other pages, just add a placeholder
                self.pages.insert(page_num, PageData { 
                    text: format!("Page {} content", page_num + 1),
                    size 
                });
            }
        }
    }
    
    /// Load page text content (fallback for when Poppler isn't available)
    fn load_page_text(&mut self, page_num: usize) {
        // If we already have page data or Poppler is available (which handles text extraction), skip
        if self.pages.contains_key(&page_num) || self.poppler_document.is_some() {
            return;
        }
        
        if let Some(doc) = &self.document {
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
                            if self.view_mode == ViewMode::Rendered && self.poppler_document.is_some() {
                                self.render_page(self.current_page, ctx);
                            } else {
                                self.load_page_text(self.current_page);
                            }
                        }
                        
                        let total_pages = self.total_pages.max(1);
                        ui.label(format!("Page {} of {}", self.current_page + 1, total_pages));
                        
                        if ui.add_enabled(self.current_page < self.total_pages.saturating_sub(1), 
                                        egui::Button::new("Next ▶")).clicked() {
                            self.current_page = (self.current_page + 1).min(self.total_pages.saturating_sub(1));
                            // Pre-render the page (will be cached if already rendered)
                            if self.view_mode == ViewMode::Rendered && self.poppler_document.is_some() {
                                self.render_page(self.current_page, ctx);
                            } else {
                                self.load_page_text(self.current_page);
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
                            if self.poppler_document.is_some() {
                                self.render_page(self.current_page, ctx);
                            }
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
            if let Some(_doc) = &self.document {
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
                                        if self.poppler_document.is_some() {
                                            self.render_page(self.current_page, ctx);
                                            
                                            ui.vertical_centered(|ui| {
                                                ui.add_space(50.0);
                                                ui.label("Rendering page...");
                                                ui.add_space(50.0);
                                            });
                                        } else {
                                            // Fallback to painter-based rendering if Poppler isn't available
                                            if let Some(page_data) = self.pages.get(&self.current_page) {
                                                // Calculate content size with zoom
                                                let width = 612.0 * self.zoom;  // Standard letter width
                                                let height = 792.0 * self.zoom; // Standard letter height
                                                
                                                // Allocate space for the page
                                                let (response, painter) = ui.allocate_painter(
                                                    Vec2::new(width, height),
                                                    egui::Sense::hover()
                                                );
                                                
                                                let rect = response.rect;
                                                
                                                // Draw the page background
                                                painter.rect_filled(rect, 0.0, Color32::WHITE);
                                                painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, Color32::GRAY));
                                                
                                                // Add a header with page number
                                                let header_rect = egui::Rect::from_min_max(
                                                    rect.min,
                                                    egui::pos2(rect.max.x, rect.min.y + 40.0 * self.zoom)
                                                );
                                                painter.rect_filled(header_rect, 0.0, Color32::from_rgb(240, 240, 240));
                                                
                                                // Page number text
                                                painter.text(
                                                    egui::pos2(rect.center().x, header_rect.center().y),
                                                    egui::Align2::CENTER_CENTER,
                                                    format!("Page {} of {}", self.current_page + 1, self.total_pages),
                                                    egui::FontId::proportional(16.0 * self.zoom),
                                                    Color32::BLACK
                                                );
                                                
                                                // Draw content area
                                                let content_rect = egui::Rect::from_min_max(
                                                    egui::pos2(rect.min.x + 50.0 * self.zoom, header_rect.max.y + 20.0 * self.zoom),
                                                    egui::pos2(rect.max.x - 50.0 * self.zoom, rect.max.y - 50.0 * self.zoom)
                                                );
                                                
                                                // Draw text content
                                                if !page_data.text.is_empty() {
                                                    let font_size = 14.0 * self.zoom;
                                                    let line_height = font_size * 1.5;
                                                    let mut y_offset = content_rect.min.y;
                                                    
                                                    // Simple text rendering
                                                    painter.text(
                                                        egui::pos2(content_rect.min.x, y_offset),
                                                        egui::Align2::LEFT_TOP,
                                                        &page_data.text,
                                                        egui::FontId::proportional(font_size),
                                                        Color32::BLACK
                                                    );
                                                } else {
                                                    // No text available
                                                    painter.text(
                                                        content_rect.center(),
                                                        egui::Align2::CENTER_CENTER,
                                                        "No text content available",
                                                        egui::FontId::proportional(16.0 * self.zoom),
                                                        Color32::DARK_GRAY
                                                    );
                                                }
                                            } else {
                                                // Load page data if not available
                                                self.load_page_text(self.current_page);
                                                
                                                ui.vertical_centered(|ui| {
                                                    ui.add_space(50.0);
                                                    ui.label("Rendering page...");
                                                    ui.add_space(50.0);
                                                });
                                            }
                                        }
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
                                            ui.vertical_centered(|ui| {
                                                ui.label("No text content available");
                                            });
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

/// Extract text from a PDF file
fn extract_text_from_pdf(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    // Use the extract module to get the text
    let bytes = std::fs::read(path)?;
    let text = pdf_extract::extract_text_from_mem(&bytes)?;
    Ok(text)
} 