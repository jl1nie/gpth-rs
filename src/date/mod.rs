pub mod exif;
pub mod guess;
pub mod json;

use chrono::NaiveDateTime;

/// Result of date extraction: date + accuracy (0 = best)
pub struct DateResult {
    pub date: NaiveDateTime,
    pub accuracy: u8,
}

/// Extract date using all methods in priority order.
pub fn extract_date(
    json_date: Option<NaiveDateTime>,
    media_bytes: Option<&[u8]>,
    filename: &str,
    allow_guess: bool,
) -> Option<DateResult> {
    // 1. JSON metadata (accuracy 0 - best)
    if let Some(date) = json_date {
        return Some(DateResult { date, accuracy: 0 });
    }

    // 2. EXIF (accuracy 1)
    if let Some(bytes) = media_bytes {
        if let Some(date) = exif::extract_exif_date(bytes) {
            return Some(DateResult { date, accuracy: 1 });
        }
    }

    // 3. Filename guess (accuracy 2)
    if allow_guess {
        if let Some(date) = guess::guess_date_from_filename(filename) {
            return Some(DateResult { date, accuracy: 2 });
        }
    }

    None
}
