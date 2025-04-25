use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

use egui::{Context, Ui, Vec2, ScrollArea, RichText, Color32};
use lopdf::{Document, Object};

use crate::extract;

/// PDF viewer component
pub struct PdfViewer {
    current_pdf_path: Option<PathBuf>,
    document: Option<Arc<Document>>,
    current_page: usize,
    total_pages: usize,
    zoom: f32,
    pages: HashMap<usize, PageData>,
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
            current_page: 0,
            total_pages: 0,
            zoom: 1.0,
            pages: HashMap::new(),
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
        self.current_page = 0;
        self.total_pages = 0;
        self.pages.clear();
        self.document_title = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        
        // Load the PDF in a separate thread
        std::thread::spawn(move || {
            match Document::load(&path_clone) {
                Ok(document) => {
                    // Get the number of pages
                    let page_count = document.get_pages().len();
                    
                    // Extract text
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
                    eprintln!("Error loading PDF: {}", e);
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
                
                // Get the number of pages from the document
                self.total_pages = doc.get_pages().len();
                
                // Load first page
                self.load_page_text(0);
                
                // Document loading complete
                self.loading = false;
            }
        }
    }
    
    /// Load page text content
    fn load_page_text(&mut self, page_num: usize) {
        if let Some(doc) = &self.document {
            if self.pages.contains_key(&page_num) {
                return; // Already loaded
            }
            
            // Default page size
            let size = Vec2::new(612.0, 792.0); // Letter size
            
            // Get text content (from the already extracted text)
            let text = {
                let text_data = self.text_data.lock().unwrap();
                text_data.clone()
            };
            
            // For a real implementation, we'd extract text for the specific page
            // For now, we'll just show all the text on the first page
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
                            self.load_page_text(self.current_page);
                        }
                        
                        let total_pages = self.total_pages.max(1);
                        ui.label(format!("Page {} of {}", self.current_page + 1, total_pages));
                        
                        if ui.add_enabled(self.current_page < self.total_pages.saturating_sub(1), 
                                        egui::Button::new("Next ▶")).clicked() {
                            self.current_page = (self.current_page + 1).min(self.total_pages.saturating_sub(1));
                            self.load_page_text(self.current_page);
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
                        }
                        if ui.radio(self.view_mode == ViewMode::TextOnly, "Text Only").clicked() {
                            self.view_mode = ViewMode::TextOnly;
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
                                    // Rendered PDF view
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
                                            
                                            let font_size = 14.0 * self.zoom;
                                            let line_height = font_size * 1.5;
                                            let mut y_offset = content_rect.min.y;
                                            
                                            // Limit to first few paragraphs for performance
                                            for (i, paragraph) in paragraphs.iter().take(15).enumerate() {
                                                // Wrap text to fit in the content area
                                                let max_width = content_rect.width();
                                                let text = paragraph;
                                                
                                                // For simplicity, we'll just draw the text and let it clip
                                                painter.text(
                                                    egui::pos2(content_rect.min.x, y_offset),
                                                    egui::Align2::LEFT_TOP,
                                                    text,
                                                    egui::FontId::proportional(font_size),
                                                    Color32::BLACK
                                                );
                                                
                                                y_offset += line_height * 2.0;
                                                
                                                // Add a small separator between paragraphs
                                                if i < paragraphs.len() - 1 {
                                                    painter.line_segment(
                                                        [
                                                            egui::pos2(content_rect.min.x, y_offset - line_height * 0.5),
                                                            egui::pos2(content_rect.min.x + 100.0 * self.zoom, y_offset - line_height * 0.5)
                                                        ],
                                                        egui::Stroke::new(1.0, Color32::LIGHT_GRAY)
                                                    );
                                                }
                                            }
                                            
                                            // Indicate that there's more content if necessary
                                            if paragraphs.len() > 15 {
                                                painter.text(
                                                    egui::pos2(content_rect.center().x, content_rect.max.y - 20.0 * self.zoom),
                                                    egui::Align2::CENTER_BOTTOM,
                                                    "...",
                                                    egui::FontId::proportional(16.0 * self.zoom),
                                                    Color32::DARK_GRAY
                                                );
                                            }
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
                                        
                                        // Draw page footer
                                        let footer_rect = egui::Rect::from_min_max(
                                            egui::pos2(rect.min.x, rect.max.y - 30.0 * self.zoom),
                                            rect.max
                                        );
                                        
                                        painter.text(
                                            footer_rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            "PDFScan Viewer",
                                            egui::FontId::proportional(12.0 * self.zoom),
                                            Color32::DARK_GRAY
                                        );
                                    } else {
                                        // Load page data if not available
                                        self.load_page_text(self.current_page);
                                        
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