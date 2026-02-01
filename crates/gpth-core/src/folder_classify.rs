use regex::Regex;
use std::sync::LazyLock;

/// Localized prefixes: "<prefix>YYYY"
const YEAR_FOLDER_PREFIXES: &[&str] = &[
    "Photos from ",      // EN
    "Fotos von ",        // DE
    "Fotos aus ",        // DE (alternate)
    "Photos de ",        // FR
    "Fotos de ",         // ES, PT, CA
    "Foto's uit ",       // NL
    "Foto dal ",         // IT
    "Foto del ",         // IT (alternate)
    "Zdjęcia z ",        // PL
    "Фото за ",          // RU
    "Фотографии за ",    // RU (alternate)
    "Fotky z ",          // CS
    "Fotografii din ",   // RO
    "Foton från ",       // SV
    "Bilder fra ",       // NO
    "Billeder fra ",     // DA
    "Valokuvat ",        // FI
    "Fényképek - ",      // HU
    "Fotoğraflar ",      // TR
];

/// Localized suffixes: "YYYY<suffix>"
const YEAR_FOLDER_SUFFIXES: &[&str] = &[
    " 年の写真",   // JA
    "年のフォト",   // JA (alternate)
    "년의 사진",    // KO
    "年的照片",     // ZH-CN
    "年的相片",     // ZH-TW
];

static YEAR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(20|19|18)\d{2}$").unwrap());

/// Check if a folder name matches a Google Takeout year folder pattern
pub fn is_year_folder(name: &str) -> bool {
    for prefix in YEAR_FOLDER_PREFIXES {
        if let Some(rest) = name.strip_prefix(prefix) {
            if YEAR_RE.is_match(rest) {
                return true;
            }
        }
    }
    for suffix in YEAR_FOLDER_SUFFIXES {
        if let Some(rest) = name.strip_suffix(suffix) {
            if YEAR_RE.is_match(rest) {
                return true;
            }
        }
    }
    false
}

/// Check if a zip entry path is inside a year folder (at any level)
pub fn is_in_year_folder(zip_path: &str) -> bool {
    for component in zip_path.split('/') {
        if is_year_folder(component) {
            return true;
        }
    }
    false
}

/// Extract album name from a zip entry path.
/// Returns Some(album_name) if the path is inside a Google Photos album folder
/// (i.e. `Takeout/Google Photos/<name>/...` where `<name>` is not a year folder).
pub fn extract_album_name(zip_path: &str) -> Option<String> {
    let parts: Vec<&str> = zip_path.split('/').collect();
    for i in 0..parts.len().saturating_sub(2) {
        let p = parts[i];
        if p.starts_with("Google")
            && (p.contains("hoto")
                || p.contains("ото")
                || p.contains("フォト")
                || p.contains("照片")
                || p.contains("사진"))
        {
            let folder_name = parts[i + 1];
            if !folder_name.is_empty() && !is_year_folder(folder_name) {
                if i + 2 < parts.len() {
                    return Some(folder_name.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_year_folders() {
        assert!(is_year_folder("Photos from 2023"));
        assert!(is_year_folder("Fotos von 2021"));
        assert!(is_year_folder("2023 年の写真"));
        assert!(is_year_folder("2023년의 사진"));
        assert!(is_year_folder("2023年的照片"));
        assert!(!is_year_folder("My Vacation"));
        assert!(!is_year_folder("Photos from abcd"));
    }
}
