use std::env;
use std::path::PathBuf;
use lopdf::{Document};
use walkdir::WalkDir;
use std::thread;
use std::sync::{Arc, Mutex};
use std::fs::File;
use std::io::Write;
use zip::write::FileOptions;
use zip::CompressionMethod::Deflated;

fn search_phrase_in_pdf(file_path: &str, search_phrase: &str) -> bool {
    if let Ok(document) = Document::load(file_path) {
        let pages = u32::try_from(document.get_pages().len());
        let mut vec: Vec<u32> = (1..pages.unwrap()).collect();
        let slice: &mut [u32] = &mut vec;
        
        let doc_str = document.extract_text(slice).unwrap();
        if doc_str.contains(search_phrase) {
            return true;
        }
    }

    false
}

fn search_pdf_files(root_path: &str, search_phrase: &str) {
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


                    if search_phrase_in_pdf(path.to_str().unwrap(), search_phrase) {
                        pdf_files.push(path.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }

    for file in pdf_files {
        println!("{}", file);
    }
}

fn print_help() {
    println!("Usage: app [options]");
    println!("-s <search phrase>   Set the search phrase");
    println!("-d <directory>       Add a search directory");
    println!("-z                   Enable zip mode");
    println!("-h                   Display this help message");
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
        let search_phrase_clone = search_phrase.clone();
        let directory_clone = directory.clone();

        handles.push(thread::spawn(move || {
            println!("Thread started to search in: {}", directory_clone.to_str().unwrap());
            search_pdf_files(directory_clone.to_str().unwrap(), &search_phrase_clone);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let locked_results = results.lock().unwrap();
    for result in locked_results.iter() {
        println!("{}", result);
    }

    if zip {
        let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
        let zip_file_name = format!("search_results_{}.zip", timestamp);

        let file = File::create(&zip_file_name).expect("Failed to create ZIP file");
        let mut zip = zip::ZipWriter::new(file);

        let locked_results = results.lock().unwrap();
        for (index, result) in locked_results.iter().enumerate() {
            let options = FileOptions::default()
                .compression_method(Deflated)
                .unix_permissions(0o755); // Set appropriate permissions if required

            let entry_path = format!("result_{}.txt", index);
            zip.start_file(entry_path, options).unwrap();
            zip.write_all(result.as_bytes()).unwrap();
        }

        zip.finish().unwrap();

        println!("Search results have been zipped to: {}", zip_file_name);
    }
}