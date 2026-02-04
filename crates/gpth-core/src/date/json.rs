use chrono::NaiveDateTime;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use crate::extras;

static BRACKET_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(\d+\)\.").unwrap());
static EXTRA_REGEX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<extra>-[A-Za-zÀ-ÖØ-öø-ÿ]+(\(\d\))?)\.\w+$").unwrap());
static DIGIT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(\d\)\.").unwrap());

/// Parse Google's JSON metadata and extract photoTakenTime
pub fn parse_google_json(json_bytes: &[u8]) -> Option<NaiveDateTime> {
    let data: serde_json::Value = serde_json::from_slice(json_bytes).ok()?;
    let ts_str = data
        .get("photoTakenTime")?
        .get("timestamp")?
        .as_str()
        .or_else(|| data["photoTakenTime"]["timestamp"].as_i64().map(|_| ""))?;

    let epoch = if ts_str.is_empty() {
        data["photoTakenTime"]["timestamp"].as_i64()?
    } else {
        ts_str.parse::<i64>().ok()?
    };

    // Convert UTC epoch to local naive datetime
    let utc = chrono::DateTime::from_timestamp(epoch, 0)?;
    Some(utc.with_timezone(&chrono::Local).naive_local())
}

/// Register a JSON date with all filename transformation variants.
/// This allows O(1) lookup later instead of trying multiple transformations.
pub fn register_json_date(
    json_path: &str,
    date: NaiveDateTime,
    json_dates: &mut HashMap<String, NaiveDateTime>,
) {
    let json_name = Path::new(json_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    let Some(media_name) = json_name.strip_suffix(".json") else {
        return;
    };

    let dir = Path::new(json_path)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("");

    let make_path = |name: &str| -> String {
        if dir.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", dir, name)
        }
    };

    // Register all transformation variants
    let transformations: &[fn(&str) -> String] = &[
        |s| s.to_string(),
        shorten_name,
        bracket_swap,
        |s| extras::remove_extra(s),
        no_extension,
        remove_extra_regex,
        remove_digit,
    ];

    for transform in transformations {
        let key = make_path(&transform(media_name));
        json_dates.entry(key).or_insert(date);
    }
}

/// Find JSON date for a media file (simple O(1) lookup)
pub fn find_json_date(
    zip_path: &str,
    json_dates: &HashMap<String, NaiveDateTime>,
) -> Option<NaiveDateTime> {
    json_dates.get(zip_path).copied()
}

fn shorten_name(filename: &str) -> String {
    let max_len = 51 - ".json".len();
    if format!("{}.json", filename).len() > 51 {
        let mut end = max_len;
        while end > 0 && !filename.is_char_boundary(end) {
            end -= 1;
        }
        filename[..end].to_string()
    } else {
        filename.to_string()
    }
}

fn bracket_swap(filename: &str) -> String {
    if let Some(m) = BRACKET_RE.find_iter(filename).last() {
        let bracket = m.as_str().replace('.', "");
        if let Some(pos) = filename.rfind(&bracket) {
            let mut result = String::with_capacity(filename.len());
            result.push_str(&filename[..pos]);
            result.push_str(&filename[pos + bracket.len()..]);
            result.push_str(&bracket);
            return result;
        }
    }
    filename.to_string()
}

fn no_extension(filename: &str) -> String {
    Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename)
        .to_string()
}

fn remove_extra_regex(filename: &str) -> String {
    let matches: Vec<_> = EXTRA_REGEX_RE.find_iter(filename).collect();
    if matches.len() == 1 {
        if let Some(caps) = EXTRA_REGEX_RE.captures(filename) {
            if let Some(extra) = caps.name("extra") {
                let mut result = filename.to_string();
                result.replace_range(extra.start()..extra.end(), "");
                return result;
            }
        }
    }
    filename.to_string()
}

fn remove_digit(filename: &str) -> String {
    DIGIT_RE.replace_all(filename, ".").to_string()
}
