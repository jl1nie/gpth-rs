use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use zip::ZipArchive;

use crate::media::Media;
use crate::ThrottledProgress;

/// Assign output paths, then write files.
pub fn write_output(
    media: &[Media],
    zip_paths: &[String],
    output_dir: &Path,
    divide_to_dates: bool,
    album_dest: Option<&str>,
    album_link: bool,
    progress: &ThrottledProgress,
) -> anyhow::Result<Vec<PathBuf>> {
    fs::create_dir_all(output_dir)?;

    // Phase 1: Assign destination paths (sequential - needs collision tracking)
    // Use counters per base path to avoid O(n²) worst case
    let mut name_counters: HashMap<PathBuf, u32> = HashMap::new();
    let mut used_paths: HashSet<PathBuf> = HashSet::new();
    let mut assignments: Vec<PathBuf> = Vec::with_capacity(media.len());

    for m in media {
        let sub_dir = if divide_to_dates {
            match &m.date {
                Some(dt) => {
                    let year = dt.format("%Y").to_string();
                    let month = dt.format("%m").to_string();
                    output_dir.join(&year).join(&month)
                }
                None => output_dir.join("date-unknown"),
            }
        } else {
            output_dir.to_path_buf()
        };

        fs::create_dir_all(&sub_dir)?;

        let base_dest = sub_dir.join(&m.filename);
        let counter = name_counters.entry(base_dest.clone()).or_insert(0);

        let dest = if *counter == 0 && !base_dest.exists() && !used_paths.contains(&base_dest) {
            base_dest
        } else {
            let stem = Path::new(&m.filename)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("file");
            let ext = Path::new(&m.filename)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            // Start from the current counter value (avoid re-checking already used numbers)
            loop {
                *counter += 1;
                let new_name = if ext.is_empty() {
                    format!("{}({})", stem, counter)
                } else {
                    format!("{}({}).{}", stem, counter, ext)
                };
                let candidate = sub_dir.join(&new_name);
                if !used_paths.contains(&candidate) && !candidate.exists() {
                    break candidate;
                }
            }
        };

        used_paths.insert(dest.clone());
        assignments.push(dest);
    }

    // Phase 2: Write files in parallel
    let total = media.len() as u64;
    let counter = AtomicU64::new(0);

    let num_threads = rayon::current_num_threads();
    let work: Vec<(usize, &Media, &PathBuf)> = media
        .iter()
        .zip(assignments.iter())
        .enumerate()
        .map(|(i, (m, d))| (i, m, d))
        .collect();

    use std::collections::HashMap;
    let mut by_zip: HashMap<usize, Vec<(usize, &Media, &PathBuf)>> = HashMap::new();
    for &(i, m, d) in &work {
        by_zip.entry(m.zip_index).or_default().push((i, m, d));
    }

    for (zip_idx, entries) in &by_zip {
        let chunk_size = (entries.len() + num_threads - 1) / num_threads;
        let chunks: Vec<&[(usize, &Media, &PathBuf)]> = entries.chunks(chunk_size).collect();
        let zip_path = &zip_paths[*zip_idx];

        std::thread::scope(|s| -> anyhow::Result<()> {
            let handles: Vec<_> = chunks
                .into_iter()
                .map(|chunk| {
                    let counter = &counter;
                    let zip_path = zip_path;
                    let progress = &progress;
                    s.spawn(move || -> anyhow::Result<()> {
                        let file = File::open(zip_path)?;
                        let mut archive = ZipArchive::new(file)?;

                        for &(_i, m, dest) in chunk {
                            let mut entry = archive.by_name(&m.zip_path)?;
                            let mut out_file = io::BufWriter::new(File::create(dest)?);
                            io::copy(&mut entry, &mut out_file)?;

                            if let Some(dt) = &m.date {
                                if let Some(local) = dt.and_local_timezone(chrono::Local).single() {
                                    let ft = filetime::FileTime::from_unix_time(local.timestamp(), 0);
                                    filetime::set_file_mtime(dest, ft).ok();
                                }
                            }

                            let current = counter.fetch_add(1, Ordering::Relaxed);
                            progress.report("write", current, total, "Writing files");
                        }
                        Ok(())
                    })
                })
                .collect();

            for h in handles {
                h.join().unwrap()?;
            }
            Ok(())
        })?;
    }

    // Phase 3: Album output (if --album-dest album)
    if album_dest == Some("album") {
        write_album_folders(media, &assignments, output_dir, album_link)?;
    }

    Ok(assignments)
}

/// Write album folders under `<output>/albums/<album_name>/`
fn write_album_folders(
    media: &[Media],
    assignments: &[PathBuf],
    output_dir: &Path,
    use_symlinks: bool,
) -> anyhow::Result<()> {
    let albums_dir = output_dir.join("albums");
    let mut count = 0u32;
    // Track used paths per album to avoid collisions
    let mut used_by_album: HashMap<String, HashSet<PathBuf>> = HashMap::new();

    for (m, dest) in media.iter().zip(assignments.iter()) {
        for album_name in &m.albums {
            let album_dir = albums_dir.join(album_name);
            fs::create_dir_all(&album_dir)?;

            // Get or create the used paths set for this album
            let used = used_by_album.entry(album_name.clone()).or_default();

            // Resolve filename collision
            let mut album_file = album_dir.join(&m.filename);
            if used.contains(&album_file) || album_file.exists() {
                let stem = Path::new(&m.filename)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("file");
                let ext = Path::new(&m.filename)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                let mut counter = 1u32;
                loop {
                    let new_name = if ext.is_empty() {
                        format!("{}({})", stem, counter)
                    } else {
                        format!("{}({}).{}", stem, counter, ext)
                    };
                    album_file = album_dir.join(&new_name);
                    if !used.contains(&album_file) && !album_file.exists() {
                        break;
                    }
                    counter += 1;
                }
            }
            used.insert(album_file.clone());

            if use_symlinks {
                let rel = pathdiff::diff_paths(dest, &album_dir)
                    .unwrap_or_else(|| dest.to_path_buf());
                #[cfg(unix)]
                std::os::unix::fs::symlink(&rel, &album_file)?;
                #[cfg(windows)]
                std::os::windows::fs::symlink_file(&rel, &album_file)?;
            } else {
                fs::copy(dest, &album_file)?;
            }
            count += 1;
        }
    }

    if count > 0 {
        eprintln!("Wrote {} album file(s) to {}", count, albums_dir.display());
    }
    Ok(())
}
