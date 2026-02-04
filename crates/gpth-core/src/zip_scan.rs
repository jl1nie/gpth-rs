use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use chrono::NaiveDateTime;
use encoding_rs::SHIFT_JIS;

use crate::date;
use crate::extras;
use crate::folder_classify;
use crate::media::Media;
use crate::ThrottledProgress;

/// Decode ZIP entry name, trying UTF-8 first, then Shift_JIS
fn decode_zip_name(entry: &zip::read::ZipFile) -> String {
    let raw = entry.name_raw();

    // Try UTF-8 first
    if let Ok(s) = std::str::from_utf8(raw) {
        return s.to_string();
    }

    // Fall back to Shift_JIS (common for Japanese ZIP files)
    let (decoded, _, had_errors) = SHIFT_JIS.decode(raw);
    if !had_errors {
        return decoded.into_owned();
    }

    // Last resort: lossy UTF-8
    String::from_utf8_lossy(raw).into_owned()
}

/// An entry found in an album folder
#[derive(Debug, Clone)]
pub struct AlbumEntry {
    pub filename: String,
    pub zip_path: String,
    pub zip_index: usize,
    pub entry_index: usize,
    pub size: u64,
}

/// Result of scanning all zip files
pub struct ScanResult {
    /// Media files found (in year folders only)
    pub media: Vec<Media>,
    /// JSON dates: media_path (with variants) -> date
    pub json_dates: HashMap<String, NaiveDateTime>,
    /// Album entries: album_name -> list of album entries
    pub album_entries: HashMap<String, Vec<AlbumEntry>>,
}

/// Scan all zip files, collecting media entries and JSON dates
pub fn scan_zips(zip_paths: &[String], skip_extras: bool, scan_albums: bool, progress: &ThrottledProgress) -> anyhow::Result<ScanResult> {
    let mut media = Vec::new();
    let mut json_dates: HashMap<String, NaiveDateTime> = HashMap::new();
    let mut album_entries: HashMap<String, Vec<AlbumEntry>> = HashMap::new();

    for (zip_index, zip_path) in zip_paths.iter().enumerate() {
        let file = File::open(zip_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        let total = archive.len() as u64;

        let zip_name = Path::new(zip_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(zip_path)
            .to_string();

        for i in 0..archive.len() {
            progress.report("scan", i as u64, total, &format!("Scanning {}", zip_name));
            let entry = archive.by_index(i)?;
            let entry_path = decode_zip_name(&entry);

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

            // Parse JSON metadata and register date with all variants
            if entry_path.ends_with(".json") {
                drop(entry);
                let mut json_entry = archive.by_index(i)?;
                let mut bytes = Vec::new();
                json_entry.read_to_end(&mut bytes)?;
                if let Some(dt) = date::json::parse_google_json(&bytes) {
                    date::json::register_json_date(&entry_path, dt, &mut json_dates);
                }
                // bytes dropped here - no longer kept in memory
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

            // Check for album membership
            if scan_albums {
                if let Some(album_name) = folder_classify::extract_album_name(&entry_path) {
                    album_entries.entry(album_name).or_default().push(AlbumEntry {
                        filename: filename.clone(),
                        zip_path: entry_path.clone(),
                        zip_index,
                        entry_index: i,
                        size,
                    });
                    if !folder_classify::is_in_year_folder(&entry_path) {
                        continue;
                    }
                }
            }

            // Only process media files in year folders
            if !folder_classify::is_in_year_folder(&entry_path) {
                continue;
            }

            media.push(Media::new(entry_path, zip_index, i, filename, size));
        }
        progress.report("scan", total, total, &format!("Scanned {}", zip_name));
    }

    Ok(ScanResult {
        media,
        json_dates,
        album_entries,
    })
}
