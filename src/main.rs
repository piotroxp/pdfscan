use std::env;
use std::path::PathBuf;
use lopdf::Document;
use walkdir::WalkDir;
use std::thread;
use std::sync::{Arc, Mutex};
use std::fs::File;
use zip::write::FileOptions;
use zip::CompressionMethod::Deflated;
use std::io::{Read, Write};
use std::result::Result as StdResult;
use zip::result::ZipError;

fn search_phrase_in_pdf(file_path: &str, search_phrase: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let bytes = std::fs::read(file_path).map_err(|err| format!("Error occurred while reading the PDF file: {}", err))?;
    let a_string = std::str::from_utf8(&bytes).map_err(|err| format!("Error occurred while parsing the PDF file as UTF-8: {}", err))?;

    Ok(a_string.contains(search_phrase))
}


fn search_pdf_files(root_path: &str, search_phrase: &str, results: Arc<Mutex<Vec<String>>>) {
    let mut pdf_files = Vec::new();

    for entry in WalkDir::new(root_path).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();

        if path.is_file() {
            if let Some(extension) = path.extension() {
                if extension == "pdf" {
                    if search_phrase.is_empty() {
                        pdf_files.push(path.to_string_lossy().into_owned());
                        continue;
                    }

                    if let Ok(contains_phrase) = search_phrase_in_pdf(path.to_str().unwrap(), search_phrase) {
                        if contains_phrase {
                            pdf_files.push(path.to_string_lossy().into_owned());
                        }
                    } else {
                        // Handle the error case, such as logging the error
                        eprintln!("Error occurred while processing PDF file: {}", path.display());
                        continue; // Skip to the next file in case of an error
                    }
                    
                }
            }
        }
    }

    let mut results = results.lock().unwrap();
    results.extend(pdf_files);
}


fn print_help() {
    println!("Usage: app [options]");
    println!("-s <search phrase>   Set the search phrase");
    println!("-d <directory>       Add a search directory");
    println!("-z                   Enable zip mode");
    println!("-h                   Display this help message");
}

fn zip_files(name: &str, files: Vec<&str>) -> StdResult<(), ZipError> {
    let path = std::path::Path::new(name);
    let file = File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);

    for file_path in files {
        let file_name = std::path::Path::new(file_path).file_name().unwrap().to_str().unwrap();
        let mut file_content = Vec::new();
        let mut file = File::open(file_path)?;
        file.read_to_end(&mut file_content)?;

        let options = FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o755); // Set appropriate permissions if required

        zip.start_file(file_name, options)?;
        zip.write_all(&file_content)?;
    }

    zip.finish()?;
    Ok(())
}

fn main() {
    let mut search_phrase = String::new();
    let mut directories: Vec<PathBuf> = Vec::new();

    let args: Vec<String> = env::args().collect();
    let mut zip = false;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-s" => {
                search_phrase = args[i + 1].clone();
                eprintln!("Search phrase: {}", search_phrase);
                i += 2;
            }
            "-d" => {
                let directory = PathBuf::from(args[i + 1].clone());
                eprintln!("Search dir: {}", directory.as_path().display().to_string());
                directories.push(directory);
                i += 2;
            }
            "-z" => {
                eprintln!("Enabled zip mode");
                zip = true;
                break;
            }
            "-h" => {
                print_help();
                return;
            }
            _ => {
                eprintln!("Invalid argument: {}", args[i]);
                return;
            }
        }
    }

    if directories.is_empty() {
        if let Some(home_dir) = dirs::home_dir() {
            directories.push(home_dir);
        } else {
            eprintln!("Unable to determine the user's home directory");
            return;
        }
    }

    let results: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();

    for directory in directories {
        let results_clone = results.clone();
        let search_phrase_clone = search_phrase.clone();
        let directory_clone = directory.clone();

        handles.push(thread::spawn(move || {
            println!("Thread started to search in: {}", directory_clone.to_str().unwrap());
            search_pdf_files(directory_clone.to_str().unwrap(), &search_phrase_clone, results_clone);
        }));
    }

    for handle in handles {
        handle.join();
    }

    let locked_results = results.lock().unwrap();
    for result in locked_results.iter() {
        println!("{}", result);
    }

    if zip {
        let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
        let zip_file_name = format!("search_results_{}.zip", timestamp);
        let files: Vec<&str> = locked_results.iter().map(|result| result.as_str()).collect();
        
        zip_files(&zip_file_name, files).expect("Failed to create ZIP file");

        println!("Search results have been zipped to: {}", zip_file_name);
    }

    return;
}
