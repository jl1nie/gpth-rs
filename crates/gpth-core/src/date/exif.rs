use chrono::NaiveDateTime;
use exif::{In, Reader, Tag};
use std::io::Cursor;

/// Extract date from EXIF data in raw image bytes.
/// EXIF datetimes have no timezone info - they are local time as-is.
pub fn extract_exif_date(bytes: &[u8]) -> Option<NaiveDateTime> {
    let reader = Reader::new().read_from_container(&mut Cursor::new(bytes)).ok()?;

    let tags = [Tag::DateTimeOriginal, Tag::DateTimeDigitized, Tag::DateTime];

    for tag in &tags {
        if let Some(field) = reader.get_field(*tag, In::PRIMARY) {
            let val = field.display_value().to_string();
            if let Some(dt) = parse_exif_datetime(&val) {
                return Some(dt);
            }
        }
    }

    None
}

fn parse_exif_datetime(s: &str) -> Option<NaiveDateTime> {
    let cleaned = s
        .replace('-', ":")
        .replace('/', ":")
        .replace('\\', ":")
        .replace('.', ":");

    if let Ok(dt) = NaiveDateTime::parse_from_str(&cleaned, "%Y:%m:%d %H:%M:%S") {
        return Some(dt);
    }

    if let Ok(d) = chrono::NaiveDate::parse_from_str(&cleaned.split(' ').next()?, "%Y:%m:%d") {
        return Some(d.and_hms_opt(0, 0, 0)?);
    }

    None
}
