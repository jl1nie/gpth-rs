/// Compare gpth-rs output with reference output (e.g. Dart version's ALL_PHOTOS)
/// Usage: compare <reference_dir> <test_dir>
///
/// Checks:
/// 1. File count match
/// 2. Every file in reference exists in test (by name, searching subdirs)
/// 3. File content matches (SHA-256)
/// 4. File modified time matches (within 1 second tolerance)
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use sha2::{Digest, Sha256};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: compare <reference_dir> <test_dir>");
        std::process::exit(1);
    }

    let ref_dir = Path::new(&args[1]);
    let test_dir = Path::new(&args[2]);

    eprintln!("Reference: {}", ref_dir.display());
    eprintln!("Test:      {}", test_dir.display());

    // Collect all files from both dirs
    let ref_files = collect_files(ref_dir)?;
    let test_files = collect_files(test_dir)?;

    eprintln!("Reference files: {}", ref_files.len());
    eprintln!("Test files:      {}", test_files.len());

    // Parallel hash computation for all files
    eprintln!("Hashing all files (parallel)...");
    let ref_hashes: Vec<(String, PathBuf, String, Option<i64>)> = ref_files
        .par_iter()
        .map(|(rel, abs)| {
            let hash = file_hash(abs).unwrap_or_default();
            let mtime = file_mtime(abs);
            (rel.clone(), abs.clone(), hash, mtime)
        })
        .collect();

    let test_hashes: Vec<(String, PathBuf, String, Option<i64>)> = test_files
        .par_iter()
        .map(|(rel, abs)| {
            let hash = file_hash(abs).unwrap_or_default();
            let mtime = file_mtime(abs);
            (rel.clone(), abs.clone(), hash, mtime)
        })
        .collect();

    eprintln!("Hashing done. Comparing...");

    // Build test lookup: filename -> Vec<(rel, hash, mtime)>
    let mut test_by_name: HashMap<String, Vec<(&str, &str, Option<i64>)>> = HashMap::new();
    for (rel, _, hash, mtime) in &test_hashes {
        let name = Path::new(rel)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        test_by_name
            .entry(name.to_string())
            .or_default()
            .push((rel, hash, *mtime));
    }

    let mut missing = Vec::new();
    let mut content_mismatch = Vec::new();
    let mut date_mismatch = Vec::new();
    let mut matched = 0;

    for (ref_rel, _, ref_hash, ref_mtime) in &ref_hashes {
        let filename = Path::new(ref_rel)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        let candidates = test_by_name.get(filename);
        if candidates.is_none() || candidates.unwrap().is_empty() {
            missing.push(ref_rel.clone());
            continue;
        }

        let mut found = false;
        for &(test_rel, test_hash, test_mtime) in candidates.unwrap() {
            if ref_hash == test_hash {
                found = true;
                matched += 1;

                if let (Some(r), Some(t)) = (ref_mtime, test_mtime) {
                    let diff = (r - t).abs();
                    if diff > 1 {
                        date_mismatch.push((ref_rel.clone(), test_rel.to_string(), *r, t));
                    }
                }
                break;
            }
        }

        if !found {
            content_mismatch.push(ref_rel.clone());
        }
    }

    // Files only in test
    let ref_names: std::collections::HashSet<String> = ref_hashes
        .iter()
        .filter_map(|(rel, _, _, _)| {
            Path::new(rel)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
        .collect();

    let extra: Vec<_> = test_hashes
        .iter()
        .filter(|(rel, _, _, _)| {
            let name = Path::new(rel)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            !ref_names.contains(name)
        })
        .collect();
    let extra_in_test = extra.len();

    // Report
    println!("=== Comparison Results ===");
    println!("Reference files: {}", ref_hashes.len());
    println!("Test files:      {}", test_hashes.len());
    println!();
    println!("Content matched: {}", matched);
    println!("Missing in test: {}", missing.len());
    println!("Content mismatch (same name, diff hash): {}", content_mismatch.len());
    println!("Date mismatch (>1s): {}", date_mismatch.len());
    println!("Extra in test (not in ref): {}", extra_in_test);

    if !missing.is_empty() {
        println!("\n--- Missing files ---");
        for f in &missing {
            println!("  {}", f);
        }
    }

    if !content_mismatch.is_empty() {
        println!("\n--- Content mismatches ---");
        for f in &content_mismatch {
            println!("  {}", f);
        }
    }

    if !date_mismatch.is_empty() {
        println!("\n--- Date mismatches (>1s) ---");
        for (ref_path, test_path, ref_t, test_t) in &date_mismatch[..date_mismatch.len().min(20)]
        {
            println!(
                "  ref: {} (mtime={}) vs test: {} (mtime={}), diff={}s",
                ref_path, ref_t, test_path, test_t,
                (ref_t - test_t).abs()
            );
        }
        if date_mismatch.len() > 20 {
            println!("  ... and {} more", date_mismatch.len() - 20);
        }
    }

    if extra_in_test > 0 {
        println!("\n--- Extra in test (first 20) ---");
        for (rel, _, _, _) in extra.iter().take(20) {
            println!("  {}", rel);
        }
    }

    if missing.is_empty() && content_mismatch.is_empty() {
        println!("\nAll files matched by content!");
    }

    Ok(())
}

fn collect_files(dir: &Path) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut result = Vec::new();
    collect_files_recursive(dir, dir, &mut result)?;
    Ok(result)
}

fn collect_files_recursive(
    base: &Path,
    dir: &Path,
    result: &mut Vec<(String, PathBuf)>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(base, &path, result)?;
        } else {
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            result.push((rel, path));
        }
    }
    Ok(())
}

fn file_hash(path: &Path) -> anyhow::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn file_mtime(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}
