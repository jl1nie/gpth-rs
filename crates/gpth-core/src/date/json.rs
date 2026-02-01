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

/// Build a map of media_filename -> DateTime from all JSON entries in zip
/// json_entries: map of zip_path -> json_bytes for all .json files
pub fn build_json_date_map(
    json_entries: &HashMap<String, Vec<u8>>,
) -> HashMap<String, NaiveDateTime> {
    let mut result = HashMap::new();

    for (json_path, json_bytes) in json_entries {
        if let Some(date) = parse_google_json(json_bytes) {
            let json_name = Path::new(json_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if let Some(media_name) = json_name.strip_suffix(".json") {
                let dir = Path::new(json_path)
                    .parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or("");
                let media_path = if dir.is_empty() {
                    media_name.to_string()
                } else {
                    format!("{}/{}", dir, media_name)
                };
                result.insert(media_path, date);
            }
        }
    }

    result
}

/// Try multiple filename transformations to find matching JSON date
pub fn find_json_date(
    zip_path: &str,
    filename: &str,
    json_dates: &HashMap<String, NaiveDateTime>,
    tryhard: bool,
) -> Option<NaiveDateTime> {
    let dir = Path::new(zip_path)
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

    let methods: Vec<Box<dyn Fn(&str) -> String>> = {
        let mut v: Vec<Box<dyn Fn(&str) -> String>> = vec![
            Box::new(|s: &str| s.to_string()),
            Box::new(shorten_name),
            Box::new(bracket_swap),
            Box::new(|s: &str| extras::remove_extra(s)),
            Box::new(no_extension),
        ];
        if tryhard {
            v.push(Box::new(remove_extra_regex));
            v.push(Box::new(remove_digit));
        }
        v
    };

    for method in &methods {
        let transformed = method(filename);
        let path = make_path(&transformed);
        if let Some(date) = json_dates.get(&path) {
            return Some(*date);
        }
    }

    // If not found with basic methods, try tryhard
    if !tryhard {
        for method in [remove_extra_regex, remove_digit] {
            let transformed = method(filename);
            let path = make_path(&transformed);
            if let Some(date) = json_dates.get(&path) {
                return Some(*date);
            }
        }
    }

    None
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
