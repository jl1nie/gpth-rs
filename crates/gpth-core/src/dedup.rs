use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::media::Media;
use crate::ThrottledProgress;

/// Buffer size for streaming hash (64 KB)
const HASH_BUFFER_SIZE: usize = 64 * 1024;

/// Result of deduplication
pub struct DedupResult {
    pub media: Vec<Media>,
    pub warnings: Vec<String>,
}

/// Compute SHA-256 hash using streaming to avoid loading entire file into memory
fn compute_streaming_hash<R: Read>(mut reader: R) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; HASH_BUFFER_SIZE];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Compute SHA-256 hashes for media that share sizes, then remove duplicates.
/// Uses streaming hash to minimize memory usage - no file size limit.
pub fn deduplicate(mut media: Vec<Media>, zip_files: &[String], progress: &ThrottledProgress) -> anyhow::Result<DedupResult> {
    let mut warnings = Vec::new();

    // Group by size
    let mut size_groups: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, m) in media.iter().enumerate() {
        size_groups.entry(m.size).or_default().push(i);
    }

    // Only hash files that share a size with at least one other file (no size limit with streaming)
    let needs_hash: Vec<usize> = size_groups
        .values()
        .filter(|indices| indices.len() > 1)
        .flatten()
        .copied()
        .collect();

    if !needs_hash.is_empty() {
        let total = needs_hash.len() as u64;
        let counter = AtomicU64::new(0);

        // Group by zip for efficient reading
        let mut by_zip: HashMap<usize, Vec<usize>> = HashMap::new();
        for &idx in &needs_hash {
            by_zip.entry(media[idx].zip_index).or_default().push(idx);
        }

        // Process each ZIP file with parallel threads (each thread opens its own archive)
        let num_threads = rayon::current_num_threads();
        let mut all_hashes: Vec<(usize, String)> = Vec::new();
        let mut skipped_count = 0usize;

        for (zip_idx, media_indices) in &by_zip {
            let zip_path = &zip_files[*zip_idx];

            // Split work across threads
            let chunk_size = (media_indices.len() + num_threads - 1) / num_threads;
            let chunks: Vec<&[usize]> = media_indices.chunks(chunk_size).collect();

            let chunk_results: Vec<(Vec<(usize, String)>, usize)> = std::thread::scope(|s| {
                let handles: Vec<_> = chunks
                    .into_iter()
                    .map(|chunk| {
                        let media = &media;
                        let zip_path = zip_path;
                        let counter = &counter;
                        let progress = progress;
                        s.spawn(move || -> (Vec<(usize, String)>, usize) {
                            let mut results = Vec::new();
                            let mut skipped = 0usize;

                            let file = match File::open(zip_path) {
                                Ok(f) => f,
                                Err(_) => {
                                    skipped = chunk.len();
                                    return (results, skipped);
                                }
                            };
                            let mut archive = match ZipArchive::new(file) {
                                Ok(a) => a,
                                Err(_) => {
                                    skipped = chunk.len();
                                    return (results, skipped);
                                }
                            };

                            for &midx in chunk {
                                let m = &media[midx];
                                match archive.by_name(&m.zip_path) {
                                    Ok(entry) => {
                                        match compute_streaming_hash(entry) {
                                            Ok(hash) => results.push((midx, hash)),
                                            Err(_) => skipped += 1,
                                        }
                                    }
                                    Err(_) => skipped += 1,
                                }
                                let current = counter.fetch_add(1, Ordering::Relaxed);
                                progress.report("dedup", current, total, "Hashing duplicates");
                            }
                            (results, skipped)
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });

            for (hashes, skipped) in chunk_results {
                all_hashes.extend(hashes);
                skipped_count += skipped;
            }
        }

        if skipped_count > 0 {
            warnings.push(format!("Skipped {} files during dedup hashing", skipped_count));
        }

        for (idx, hash) in all_hashes {
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

    Ok(DedupResult { media, warnings })
}
