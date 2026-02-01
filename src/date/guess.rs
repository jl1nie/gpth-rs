use chrono::NaiveDateTime;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

struct DatePattern {
    regex: &'static LazyLock<Regex>,
    format: &'static str,
}

static RE_0: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?P<date>(20|19|18)\d{2}(01|02|03|04|05|06|07|08|09|10|11|12)[0-3]\d-\d{6})").unwrap());
static RE_1: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?P<date>(20|19|18)\d{2}(01|02|03|04|05|06|07|08|09|10|11|12)[0-3]\d_\d{6})").unwrap());
static RE_2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?P<date>(20|19|18)\d{2}-(01|02|03|04|05|06|07|08|09|10|11|12)-[0-3]\d-\d{2}-\d{2}-\d{2})").unwrap());
static RE_3: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?P<date>(20|19|18)\d{2}-(01|02|03|04|05|06|07|08|09|10|11|12)-[0-3]\d-\d{6})").unwrap());
static RE_4: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?P<date>(20|19|18)\d{2}(01|02|03|04|05|06|07|08|09|10|11|12)[0-3]\d{7})").unwrap());
static RE_5: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?P<date>(20|19|18)\d{2}_(01|02|03|04|05|06|07|08|09|10|11|12)_[0-3]\d_\d{2}_\d{2}_\d{2})").unwrap());

static PATTERNS: &[DatePattern] = &[
    DatePattern { regex: &RE_0, format: "%Y%m%d-%H%M%S" },
    DatePattern { regex: &RE_1, format: "%Y%m%d_%H%M%S" },
    DatePattern { regex: &RE_2, format: "%Y-%m-%d-%H-%M-%S" },
    DatePattern { regex: &RE_3, format: "%Y-%m-%d-%H%M%S" },
    DatePattern { regex: &RE_4, format: "%Y%m%d%H%M%S" },
    DatePattern { regex: &RE_5, format: "%Y_%m_%d_%H_%M_%S" },
];

pub fn guess_date_from_filename(filename: &str) -> Option<NaiveDateTime> {
    let basename = Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(filename);

    for pat in PATTERNS {
        if let Some(caps) = pat.regex.captures(basename) {
            if let Some(date_str) = caps.name("date") {
                // For the YYYYMMDDhhmmss pattern, only take first 14 chars
                let s = if pat.format == "%Y%m%d%H%M%S" {
                    &date_str.as_str()[..14.min(date_str.as_str().len())]
                } else {
                    date_str.as_str()
                };
                if let Ok(dt) = NaiveDateTime::parse_from_str(s, pat.format) {
                    return Some(dt);
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
    fn test_guess_patterns() {
        assert!(guess_date_from_filename("Screenshot_20190919-053857.jpg").is_some());
        assert!(guess_date_from_filename("IMG_20190509_154733.jpg").is_some());
        assert!(guess_date_from_filename("signal-2020-10-26-163832.jpg").is_some());
        assert!(guess_date_from_filename("2016_01_30_11_49_15.mp4").is_some());
        assert!(guess_date_from_filename("random_photo.jpg").is_none());
    }
}
