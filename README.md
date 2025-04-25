# PDFScan

A versatile command-line tool for extracting text from PDFs and searching within PDF documents.

## Features

- **Extract text** from PDF files into a single output file with clear document boundaries
- **Search for text** within PDF files across multiple directories
- Multi-threaded processing for better performance
- Option to create a ZIP archive of matching PDF files
- Robust error handling

## Installation

### Prerequisites

- Rust and Cargo (install from [rustup.rs](https://rustup.rs/))

### Building from Source

```bash
# Clone this repository
git clone https://github.com/username/pdfscan.git
cd pdfscan

# Build the project
cargo build --release

# The binary will be available at ./target/release/pdfscan
```

## Usage

### Text Extraction

Extract text from PDF files and save to a single output file:

```bash
# Extract from specific PDF files
pdfscan extract output.txt file1.pdf file2.pdf

# Extract from all PDFs in a directory
pdfscan extract output.txt /path/to/directory/

# Extract from multiple sources
pdfscan extract output.txt /path/to/directory/ file1.pdf
```

The output file will contain the extracted text with clear document boundaries:

```
[Start of document: file1.pdf]
... extracted text ...
[End of document: file1.pdf]

[Start of document: file2.pdf]
... extracted text ...
[End of document: file2.pdf]
```

### PDF Search

Search for text within PDF files:

```bash
# Search in the home directory
pdfscan search --search-phrase "search term"

# Search in specific directories
pdfscan search --search-phrase "search term" --directories /path1/ /path2/

# Search and create a ZIP file with matching PDFs
pdfscan search --search-phrase "search term" --directories /path/ --zip
```

## Error Handling

PDFScan handles various error conditions gracefully:

- Invalid file paths
- Permission issues
- Corrupted PDF files
- Unicode/encoding challenges

Errors are reported to stderr while the application continues processing other files.

## License

This project is licensed under the terms specified in the LICENSE file.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.