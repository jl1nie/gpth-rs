use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use indicatif::{ProgressBar, ProgressStyle};

use crate::extras;
use crate::folder_classify;
use crate::media::Media;

/// Result of scanning all zip files
pub struct ScanResult {
    /// Media files found (in year folders only)
    pub media: Vec<Media>,
    /// JSON metadata: zip_path -> raw bytes
    pub json_entries: HashMap<String, Vec<u8>>,
}

/// Scan all zip files, collecting media entries and JSON metadata
pub fn scan_zips(zip_paths: &[String], skip_extras: bool) -> anyhow::Result<ScanResult> {
    let mut media = Vec::new();
    let mut json_entries = HashMap::new();

    for (zip_index, zip_path) in zip_paths.iter().enumerate() {
        eprintln!("Scanning: {}", zip_path);
        let file = File::open(zip_path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let pb = ProgressBar::new(archive.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{bar:40}] {pos}/{len} scanning {msg}")
                .unwrap(),
        );
        pb.set_message(
            Path::new(zip_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(zip_path)
                .to_string(),
        );

        for i in 0..archive.len() {
            pb.inc(1);
            let entry = archive.by_index(i)?;
            let entry_path = entry.name().to_string();

            // Skip directories
            if entry.is_dir() {
                continue;
            }

            let filename = Path::new(&entry_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            if filename.is_empty() {
                continue;
            }

            // Collect JSON metadata files
            if entry_path.ends_with(".json") {
                drop(entry);
                let mut json_entry = archive.by_index(i)?;
                let mut bytes = Vec::new();
                json_entry.read_to_end(&mut bytes)?;
                json_entries.insert(entry_path, bytes);
                continue;
            }

            // Only process media files in year folders
            if !folder_classify::is_in_year_folder(&entry_path) {
                continue;
            }

            // Check if it's a media file
            let mime = mime_guess::from_path(&filename).first();
            let is_media = match &mime {
                Some(m) => {
                    m.type_() == mime_guess::mime::IMAGE
                        || m.type_() == mime_guess::mime::VIDEO
                        || filename.to_lowercase().ends_with(".mts")
                }
                None => false,
            };

            if !is_media {
                continue;
            }

            // Skip extras if requested
            if skip_extras {
                let stem = Path::new(&filename)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if extras::is_extra(stem) {
                    continue;
                }
            }

            let size = entry.size();
            media.push(Media::new(entry_path, zip_index, filename, size));
        }

        pb.finish_and_clear();
    }

    eprintln!("Found {} media files, {} JSON metadata files", media.len(), json_entries.len());

    Ok(ScanResult {
        media,
        json_entries,
    })
}
