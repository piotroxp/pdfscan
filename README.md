# pdfscan
Scanning for PDFs with specific content in Rust

# Description
````
Usage: app [options]
-s <search phrase>   Set the search phrase
-d <directory>       Add a search directory
-z                   Enable zip mode
-h                   Display this help message
````

A thread is created per directory to scan through it, looking for the search phrases.

One can save the results to a zip by using zip mode.