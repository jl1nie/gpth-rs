use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

use rayon::prelude::*;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::media::Media;
use crate::ThrottledProgress;

const MAX_HASH_SIZE: u64 = 64 * 1024 * 1024; // 64 MiB

/// Compute SHA-256 hashes for media that share sizes, then remove duplicates.
pub fn deduplicate(mut media: Vec<Media>, zip_files: &[String], progress: &ThrottledProgress) -> anyhow::Result<Vec<Media>> {
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
        let total = needs_hash.len() as u64;

        // Batch-read entries grouped by zip, then hash in parallel from memory
        let mut by_zip: HashMap<usize, Vec<usize>> = HashMap::new();
        for &idx in &needs_hash {
            by_zip.entry(media[idx].zip_index).or_default().push(idx);
        }

        let mut entry_bytes: Vec<(usize, Vec<u8>)> = Vec::with_capacity(needs_hash.len());
        for (zip_idx, media_indices) in &by_zip {
            let Ok(file) = File::open(&zip_files[*zip_idx]) else { continue };
            let Ok(mut archive) = ZipArchive::new(file) else { continue };

            for &midx in media_indices {
                let Ok(mut entry) = archive.by_name(&media[midx].zip_path) else { continue };
                let mut bytes = Vec::with_capacity(entry.size() as usize);
                if entry.read_to_end(&mut bytes).is_ok() {
                    entry_bytes.push((midx, bytes));
                }
            }
        }

        // Parallel hash from memory
        let counter = std::sync::atomic::AtomicU64::new(0);
        let hashes: Vec<(usize, String)> = entry_bytes
            .par_iter()
            .map(|(idx, bytes)| {
                let hash = hex::encode(Sha256::digest(bytes));
                let current = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                progress.report("dedup", current, total, "Hashing duplicates");
                (*idx, hash)
            })
            .collect();

        for (idx, hash) in hashes {
            media[idx].hash = Some(hash);
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
