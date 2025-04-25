use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process;

mod extract;
mod search;

#[derive(Parser)]
#[command(author, version, about = "PDF text extraction and search tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Extract text from PDFs and save to a file
    Extract {
        /// Output text file path
        output_file: String,
        
        /// Input paths (directories or PDF files)
        input_paths: Vec<String>,
    },
    
    /// Search for text in PDF files
    Search {
        /// Text to search for
        #[arg(short, long)]
        search_phrase: String,
        
        /// Directories to search in
        #[arg(short, long, required = false)]
        directories: Vec<PathBuf>,
        
        /// Enable ZIP output of matching files
        #[arg(short, long)]
        zip: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Extract { output_file, input_paths } => {
            extract::run(&output_file, &input_paths)
        },
        Commands::Search { search_phrase, directories, zip } => {
            search::run(&search_phrase, &directories, zip)
        },
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
} 