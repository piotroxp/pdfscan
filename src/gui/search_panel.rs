use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Context, Ui, RichText, Color32, TextEdit, Key};

use super::pdf_viewer::PdfViewer;

/// Search panel component
pub struct SearchPanel {
    search_query: String,
    search_results: Vec<SearchResult>,
    search_paths: Vec<PathBuf>,
    case_sensitive: bool,
    search_scope: SearchScope,
    directory_path: Option<PathBuf>,
    is_searching: bool,
    create_zip: bool,
}

/// Search result
struct SearchResult {
    file_path: PathBuf,
    file_name: String,
    match_count: usize,
    matches: Vec<MatchResult>,
}

/// Match within a file
struct MatchResult {
    text: String,
    position: usize,
}

#[derive(PartialEq)]
enum SearchScope {
    CurrentDocument,
    Directory,
}

impl SearchPanel {
    pub fn new() -> Self {
        Self {
            search_query: String::new(),
            search_results: Vec::new(),
            search_paths: Vec::new(),
            case_sensitive: false,
            search_scope: SearchScope::CurrentDocument,
            directory_path: None,
            is_searching: false,
            create_zip: false,
        }
    }
    
    /// Show search options in the sidebar
    pub fn show_options(&mut self, ui: &mut Ui, pdf_viewer: &PdfViewer) {
        ui.heading("Search Options");
        
        // Search query
        ui.label("Search for:");
        let text_edit = TextEdit::singleline(&mut self.search_query)
            .hint_text("Enter search text...")
            .desired_width(ui.available_width());
        
        ui.add(text_edit);
        
        ui.add_space(5.0);
        
        // Search options
        ui.checkbox(&mut self.case_sensitive, "Case sensitive");
        
        ui.add_space(10.0);
        
        // Search scope
        ui.label(RichText::new("Search scope:").strong());
        
        let has_current = pdf_viewer.current_pdf().is_some();
        
        // Properly use radio buttons with an enum
        let current_doc_response = ui.radio(self.search_scope == SearchScope::CurrentDocument, "Current document");
        if current_doc_response.clicked() {
            self.search_scope = SearchScope::CurrentDocument;
        }
        
        if !has_current {
            ui.horizontal(|ui| {
                ui.label(RichText::new("(No document open)").italics().color(Color32::GRAY));
            });
        }
        
        let dir_response = ui.radio(self.search_scope == SearchScope::Directory, "Directory");
        if dir_response.clicked() {
            self.search_scope = SearchScope::Directory;
        }
        
        if self.search_scope == SearchScope::Directory {
            ui.horizontal(|ui| {
                ui.label("   ");  // Indent
                if ui.button("📁 Select...").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.directory_path = Some(path);
                    }
                }
            });
            
            // Show selected directory
            if let Some(path) = &self.directory_path {
                ui.group(|ui| {
                    ui.label(RichText::new("Selected directory:").strong());
                    ui.label(path.to_string_lossy().to_string());
                });
            } else {
                ui.label(RichText::new("No directory selected").italics());
            }
            
            ui.checkbox(&mut self.create_zip, "Create ZIP with results");
        }
        
        ui.add_space(15.0);
        
        // Search button
        let button_enabled = (!self.search_query.is_empty()) && 
                          ((self.search_scope == SearchScope::CurrentDocument && has_current) || 
                           (self.search_scope == SearchScope::Directory && self.directory_path.is_some()));
        
        if ui.add_enabled(button_enabled && !self.is_searching,
            egui::Button::new(if self.is_searching { "Searching..." } else { "Search" })
                .min_size(egui::vec2(120.0, 28.0))  // Make button more prominent
                .fill(ui.style().visuals.selection.bg_fill))
            .clicked()
        {
            self.perform_search(pdf_viewer);
        }
    }
    
    /// Perform a search operation
    fn perform_search(&mut self, pdf_viewer: &PdfViewer) {
        self.is_searching = true;
        self.search_results.clear();
        
        // Search in current document
        if self.search_scope == SearchScope::CurrentDocument {
            if let Some(pdf_path) = pdf_viewer.current_pdf() {
                let text = pdf_viewer.text();
                let matches = self.search_in_text(&text);
                
                if !matches.is_empty() {
                    let result = SearchResult {
                        file_path: pdf_path.clone(),
                        file_name: pdf_path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                        match_count: matches.len(),
                        matches,
                    };
                    
                    self.search_results.push(result);
                }
            }
        }
        // Search in directory
        else if self.search_scope == SearchScope::Directory {
            if let Some(dir_path) = &self.directory_path {
                // Clone data for thread
                let dir_path_clone = dir_path.clone();
                let search_query = self.search_query.clone();
                let search_results = Arc::new(Mutex::new(Vec::new()));
                let search_results_clone = search_results.clone();
                
                // Start search in a background thread
                std::thread::spawn(move || {
                    // Use the search module to find matches
                    let matching_pdfs = match search_files_in_directory(&dir_path_clone, &search_query) {
                        Ok(files) => files,
                        Err(e) => {
                            eprintln!("Error searching directory: {}", e);
                            Vec::new()
                        }
                    };
                    
                    // Process results
                    let mut results = Vec::new();
                    for path in matching_pdfs {
                        // Extract file name
                        let file_name = path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        
                        // We don't have detailed match information when searching directories
                        // so we'll just create a single match
                        let match_item = MatchResult {
                            text: format!("Found occurrence in '{}'", file_name),
                            position: 0,
                        };
                        
                        // Create a search result
                        results.push(SearchResult {
                            file_path: path,
                            file_name,
                            match_count: 1, // Just indicate we found a match
                            matches: vec![match_item],
                        });
                    }
                    
                    // Store the results
                    let mut search_results = search_results_clone.lock().unwrap();
                    *search_results = results;
                });
                
                // Wait a bit for results (in a real app, we'd handle this asynchronously)
                std::thread::sleep(std::time::Duration::from_millis(100));
                
                // Get any results so far
                let mut results = search_results.lock().unwrap();
                if !results.is_empty() {
                    self.search_results.append(&mut results);
                }
                
                // Create ZIP file if requested
                if self.create_zip && !self.search_results.is_empty() && self.search_query.len() > 0 {
                    self.create_zip_with_results();
                }
            }
        }
        
        self.is_searching = false;
    }
    
    /// Create a ZIP file with search results
    fn create_zip_with_results(&self) {
        if self.search_results.is_empty() {
            return;
        }
        
        // Get paths to include in the ZIP
        let pdf_paths: Vec<String> = self.search_results.iter()
            .map(|r| r.file_path.to_string_lossy().to_string())
            .collect();
            
        // Use the zip function from the search module
        let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
        let zip_file_name = format!("search_results_{}.zip", timestamp);
        
        if let Err(e) = crate::search::zip_files(&zip_file_name, &pdf_paths) {
            eprintln!("Error creating ZIP file: {}", e);
        } else {
            println!("Created ZIP file with search results: {}", zip_file_name);
        }
    }
    
    /// Search for matches in text
    fn search_in_text(&self, text: &str) -> Vec<MatchResult> {
        let mut matches = Vec::new();
        let query = if self.case_sensitive {
            self.search_query.clone()
        } else {
            self.search_query.to_lowercase()
        };
        
        let search_text = if self.case_sensitive {
            text.to_string()
        } else {
            text.to_lowercase()
        };
        
        // Find all occurrences
        let mut start = 0;
        while let Some(pos) = search_text[start..].find(&query) {
            let actual_pos = start + pos;
            
            // Extract context (a few characters before and after)
            let context_start = actual_pos.saturating_sub(40);
            let context_end = (actual_pos + query.len() + 40).min(text.len());
            let context = text[context_start..context_end].to_string();
            
            matches.push(MatchResult {
                text: context,
                position: actual_pos,
            });
            
            start = actual_pos + query.len();
        }
        
        matches
    }
    
    /// Show the search panel in the main content area
    pub fn show(&mut self, ui: &mut Ui, ctx: &Context, pdf_viewer: &mut PdfViewer) {
        ui.vertical(|ui| {
            // Top search bar
            ui.horizontal(|ui| {
                ui.heading("PDF Search");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("📁 Select Directory").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.directory_path = Some(path);
                            self.search_scope = SearchScope::Directory;
                        }
                    }
                });
            });
            
            ui.separator();
            
            // Split view for directory tree and results
            egui::TopBottomPanel::top("search_top_panel")
                .resizable(true)
                .height_range(50.0..=150.0)
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Search input with Enter key handling
                        let text_edit_response = TextEdit::singleline(&mut self.search_query)
                            .hint_text("Search in PDFs...")
                            .desired_width(ui.available_width() - 20.0)
                            .show(ui);
                        
                        ui.checkbox(&mut self.case_sensitive, "Case sensitive");
                        
                        let button_enabled = !self.search_query.is_empty() && 
                            ((self.search_scope == SearchScope::CurrentDocument && pdf_viewer.current_pdf().is_some()) || 
                            (self.search_scope == SearchScope::Directory && self.directory_path.is_some()));
                        
                        // Check for Enter key press to trigger search
                        if button_enabled && !self.is_searching && 
                           (text_edit_response.response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
                            self.perform_search(pdf_viewer);
                        }
                    });
                    
                    // Search scope selection
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.search_scope, SearchScope::CurrentDocument, "Current document");
                        ui.radio_value(&mut self.search_scope, SearchScope::Directory, "Directory");
                        
                        if self.search_scope == SearchScope::Directory {
                            if let Some(dir) = &self.directory_path {
                                ui.label(dir.to_string_lossy().to_string());
                            } else {
                                ui.label(RichText::new("No directory selected").italics());
                            }
                            
                            ui.checkbox(&mut self.create_zip, "Create ZIP");
                        }
                    });
                });
            
            // Show search results in the central panel
            egui::CentralPanel::default().show_inside(ui, |ui| {
                self.show_results(ui, pdf_viewer, ctx);
            });
        });
    }
    
    /// Show the search results
    fn show_results(&mut self, ui: &mut Ui, pdf_viewer: &mut PdfViewer, ctx: &Context) {
        // Store search query in memory for highlighting
        if !self.search_query.is_empty() {
            ui.memory_mut(|mem| mem.data.insert_temp("search_query".into(), self.search_query.clone()));
        }
        
        if self.is_searching {
            ui.spinner();
            ui.label("Searching...");
            return;
        }
        
        // Show results count
        ui.horizontal(|ui| {
            if self.search_results.is_empty() {
                ui.label(RichText::new("No results found").italics());
            } else {
                let total_matches: usize = self.search_results.iter().map(|r| r.match_count).sum();
                ui.label(RichText::new(format!("{} matches found in {} file(s)", total_matches, self.search_results.len())).strong());
            }
        });
        
        if self.search_results.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                
                if !self.search_query.is_empty() {
                    ui.label("Try different search terms or checking a different location");
                } else {
                    ui.label("Enter a search term and press Enter to search");
                }
                
                ui.add_space(20.0);
            });
        } else {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for result in &self.search_results {
                        // Format header with file name and match count
                        let header = format!(
                            "{} ({} {})", 
                            result.file_name, 
                            result.match_count,
                            if result.match_count == 1 { "match" } else { "matches" }
                        );
                        
                        egui::CollapsingHeader::new(header)
                            .id_source(&result.file_path)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if ui.button("Open PDF").clicked() {
                                        pdf_viewer.load_pdf(&result.file_path);
                                    }
                                    ui.label(RichText::new(&*result.file_path.to_string_lossy()).monospace());
                                });
                                
                                ui.add_space(5.0);
                                
                                // Show matches
                                for (i, m) in result.matches.iter().enumerate() {
                                    ui.group(|ui| {
                                        // Create a highlighted version of the text
                                        let text = if self.search_query.is_empty() {
                                            m.text.clone()
                                        } else {
                                            // Highlight all occurrences of the search query
                                            let parts: Vec<&str> = m.text.split(&self.search_query).collect();
                                            if parts.len() <= 1 {
                                                m.text.clone()
                                            } else {
                                                parts.join(&format!("<<{}>>", &self.search_query))
                                            }
                                        };
                                        
                                        ui.label(format!("{}. ...{}...", i + 1, text));
                                        
                                        if ui.button("Jump to match").clicked() {
                                            // Calculate the approximate page number based on position
                                            let text = pdf_viewer.text();
                                            if !text.is_empty() {
                                                let position_ratio = m.position as f32 / text.len() as f32;
                                                let page = (position_ratio * pdf_viewer.total_pages() as f32).floor() as usize;
                                                
                                                // Jump to the calculated page with search term highlighting
                                                pdf_viewer.jump_to_page(page, Some(&self.search_query), ctx);
                                            } else {
                                                // Just load the PDF if we can't calculate the page
                                                pdf_viewer.load_pdf(&result.file_path);
                                            }
                                        }
                                    });
                                }
                            });
                    }
                });
        }
    }
}

/// Search for PDF files containing the given phrase in a directory
fn search_files_in_directory(dir: &PathBuf, search_phrase: &str) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut results = Vec::new();
    
    // Walk through all files in the directory
    for entry in walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        
        if path.is_file() {
            if let Some(extension) = path.extension() {
                if extension == "pdf" {
                    // Check if PDF contains the search phrase
                    match search_phrase_in_pdf(path, search_phrase) {
                        Ok(true) => {
                            results.push(path.to_path_buf());
                        },
                        Ok(false) => {}, // Phrase not found
                        Err(e) => eprintln!("Error processing {}: {}", path.display(), e),
                    }
                }
            }
        }
    }
    
    Ok(results)
}

/// Check if a PDF file contains the search phrase
fn search_phrase_in_pdf(file_path: &Path, search_phrase: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let bytes = std::fs::read(file_path)?;
    
    let text = pdf_extract::extract_text_from_mem(&bytes)?;
    
    Ok(text.contains(search_phrase))
} 