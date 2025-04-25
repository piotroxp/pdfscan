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
    fn process_loaded_document(&mut self) {
        if self.loading {
            // Check if document has been loaded by the background thread
            let doc_option = {
                let mut document_loaded = self.document_loaded.lock().unwrap();
                document_loaded.take()
            };
            
            if let Some(doc) = doc_option {
                // Update state with the loaded document
                self.document = Some(doc.clone());
                
                // Set a default number of pages (in a real app, we'd get this from the PDF)
                self.total_pages = 1;
                
                // Load first page text
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
        self.process_loaded_document();
        
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
                    });
                });
            
            // Main content area for the PDF
            if let Some(_doc) = &self.document {
                // Display the PDF content in a scrollable area
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
                            
                            // Show the text
                            ui.allocate_rect(content_rect, egui::Sense::hover());
                            
                            // Add text with proper padding
                            let text_rect = content_rect.shrink(20.0);
                            ui.put(text_rect, egui::Label::new(&page_data.text).wrap(true));
                        } else {
                            ui.vertical_centered(|ui| {
                                ui.add_space(50.0);
                                ui.label("Loading page content...");
                                ui.add_space(50.0);
                            });
                        }
                    });
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