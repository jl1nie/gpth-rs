use chrono::NaiveDateTime;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Media {
    /// Relative path inside the zip
    pub zip_path: String,
    /// Index of the zip file in the input list
    pub zip_index: usize,
    /// Index of this entry within the zip archive (for by_index access)
    pub entry_index: usize,
    /// Just the filename
    pub filename: String,
    /// File size in bytes
    pub size: u64,
    /// SHA-256 hash hex (lazy, None if not computed or >64MiB)
    pub hash: Option<String>,
    /// Extracted date
    pub date: Option<NaiveDateTime>,
    /// Date accuracy (0 = best, higher = less accurate)
    pub date_accuracy: u8,
    /// Album names this media belongs to
    pub albums: Vec<String>,
}

impl Media {
    pub fn new(zip_path: String, zip_index: usize, entry_index: usize, filename: String, size: u64) -> Self {
        Self {
            zip_path,
            zip_index,
            entry_index,
            filename,
            size,
            hash: None,
            date: None,
            date_accuracy: u8::MAX,
            albums: Vec::new(),
        }
    }
}
