use unicode_normalization::UnicodeNormalization;

/// Localized "edited" suffixes (lowercase)
const EXTRA_FORMATS: &[&str] = &[
    "-edited",      // EN
    "-effects",     // EN
    "-smile",       // EN
    "-mix",         // EN
    "-edytowane",   // PL
    "-bearbeitet",  // DE
    "-bewerkt",     // NL
    "-編集済み",     // JA
    "-modificato",  // IT
    "-modifié",     // FR
    "-ha editado",  // ES
    "-editat",      // CA
];

/// Check if a filename (without extension) matches an "extra" pattern
pub fn is_extra(filename_without_ext: &str) -> bool {
    let name: String = filename_without_ext.to_lowercase().nfc().collect();
    EXTRA_FORMATS.iter().any(|extra| name.ends_with(extra))
}

/// Remove extra suffix from filename if present (for JSON matching)
pub fn remove_extra(filename: &str) -> String {
    let normalized: String = filename.nfc().collect();
    for extra in EXTRA_FORMATS {
        if let Some(pos) = normalized.to_lowercase().rfind(extra) {
            let mut result = normalized.clone();
            result.replace_range(pos..pos + extra.len(), "");
            return result;
        }
    }
    normalized
}
