use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

use egui::{Context, Ui, RichText, Color32, TextEdit};

use super::pdf_viewer::PdfViewer;
use crate::search;

/// Search panel component
pub struct SearchPanel {
    search_query: String,
    search_results: Vec<SearchResult>,
    search_paths: Vec<PathBuf>,
    case_sensitive: bool,
    search_in_current: bool,
    search_in_directory: bool,
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

impl SearchPanel {
    pub fn new() -> Self {
        Self {
            search_query: String::new(),
            search_results: Vec::new(),
            search_paths: Vec::new(),
            case_sensitive: false,
            search_in_current: true,
            search_in_directory: false,
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
        
        if ui.add(text_edit).changed() {
            // Could implement live search here
        }
        
        ui.add_space(5.0);
        
        // Search options
        ui.checkbox(&mut self.case_sensitive, "Case sensitive");
        
        ui.add_space(10.0);
        
        // Search scope
        ui.label(RichText::new("Search scope:").strong());
        ui.radio_value(&mut self.search_in_current, true, "Current document");
        
        let has_current = pdf_viewer.current_pdf().is_some();
        if !has_current {
            ui.label(RichText::new("No document open").italics().color(Color32::GRAY));
        }
        
        ui.radio_value(&mut self.search_in_directory, true, "Directory");
        
        if self.search_in_directory {
            ui.horizontal(|ui| {
                let dir_name = match &self.directory_path {
                    Some(path) => path.to_string_lossy().to_string(),
                    None => "Select directory...".to_string(),
                };
                
                if ui.button(&dir_name).clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.directory_path = Some(path);
                    }
                }
            });
        }
        
        if self.search_in_directory {
            ui.checkbox(&mut self.create_zip, "Create ZIP with results");
        }
        
        ui.add_space(15.0);
        
        // Search button
        let button_text = if self.is_searching {
            "Searching..."
        } else {
            "Search"
        };
        
        if ui.button(button_text).clicked() && !self.is_searching && !self.search_query.is_empty() {
            self.perform_search(pdf_viewer);
        }
    }
    
    /// Perform a search operation
    fn perform_search(&mut self, pdf_viewer: &PdfViewer) {
        self.is_searching = true;
        self.search_results.clear();
        
        // Search in current document
        if self.search_in_current {
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
        else if self.search_in_directory {
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
            ui.heading("PDF Search");
            
            // Search input at the top
            ui.horizontal(|ui| {
                let text_edit = TextEdit::singleline(&mut self.search_query)
                    .hint_text("Search in PDFs...")
                    .desired_width(ui.available_width() - 100.0);
                
                ui.add(text_edit);
                
                if ui.button("Search").clicked() && !self.search_query.is_empty() {
                    self.perform_search(pdf_viewer);
                }
            });
            
            ui.add_space(10.0);
            
            // Results section
            ui.heading("Results");
            
            if self.search_results.is_empty() {
                if self.is_searching {
                    ui.label("Searching...");
                } else if !self.search_query.is_empty() {
                    ui.label("No results found");
                } else {
                    ui.label("Enter a search query to begin");
                }
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for result in &self.search_results {
                        ui.collapsing(format!("{} ({} matches)", result.file_name, result.match_count), |ui| {
                            for (i, m) in result.matches.iter().enumerate() {
                                let text = m.text.replace(&self.search_query, &format!("<<{}>>", &self.search_query));
                                ui.label(format!("{}. ...{}...", i + 1, text));
                                
                                if ui.button("Open").clicked() {
                                    pdf_viewer.load_pdf(&result.file_path);
                                    // In a real implementation, we would also jump to this specific match
                                }
                                
                                ui.separator();
                            }
                        });
                    }
                });
            }
        });
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