use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::media::Media;

const MAX_HASH_SIZE: u64 = 64 * 1024 * 1024; // 64 MiB

/// Compute SHA-256 hashes for media that share sizes, then remove duplicates.
pub fn deduplicate(mut media: Vec<Media>, zip_files: &[String]) -> anyhow::Result<Vec<Media>> {
    // Group by size
    let mut size_groups: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, m) in media.iter().enumerate() {
        size_groups.entry(m.size).or_default().push(i);
    }

    // Only hash files that share a size with at least one other file and are <= 64MiB
    let needs_hash: Vec<usize> = size_groups
        .values()
        .filter(|indices| indices.len() > 1)
        .flatten()
        .copied()
        .filter(|&i| media[i].size <= MAX_HASH_SIZE)
        .collect();

    if !needs_hash.is_empty() {
        let pb = ProgressBar::new(needs_hash.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{bar:40}] {pos}/{len} hashing duplicates")
                .unwrap(),
        );

        // Parallel hash computation
        let hashes: Vec<(usize, Option<String>)> = needs_hash
            .par_iter()
            .map(|&idx| {
                let m = &media[idx];
                let hash = compute_hash_from_zip(&zip_files[m.zip_index], &m.zip_path).ok();
                pb.inc(1);
                (idx, hash)
            })
            .collect();

        pb.finish_and_clear();

        for (idx, hash) in hashes {
            media[idx].hash = hash;
        }
    }

    // Remove duplicates: group by (size, hash), keep best
    let mut hash_groups: HashMap<(u64, Option<String>), Vec<usize>> = HashMap::new();
    for (i, m) in media.iter().enumerate() {
        if m.hash.is_some() {
            hash_groups
                .entry((m.size, m.hash.clone()))
                .or_default()
                .push(i);
        }
    }

    let mut remove_indices: Vec<usize> = Vec::new();
    for indices in hash_groups.values() {
        if indices.len() <= 1 {
            continue;
        }
        let mut sorted = indices.clone();
        sorted.sort_by(|&a, &b| {
            media[a]
                .date_accuracy
                .cmp(&media[b].date_accuracy)
                .then_with(|| media[a].filename.len().cmp(&media[b].filename.len()))
        });
        remove_indices.extend_from_slice(&sorted[1..]);
    }

    remove_indices.sort_unstable();
    remove_indices.dedup();
    for &idx in remove_indices.iter().rev() {
        media.swap_remove(idx);
    }

    Ok(media)
}

fn compute_hash_from_zip(zip_path: &str, entry_path: &str) -> anyhow::Result<String> {
    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut entry = archive.by_name(entry_path)?;

    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = entry.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}
