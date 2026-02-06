use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;

use zip::ZipArchive;

use crate::media::Media;
use crate::ThrottledProgress;

/// Recursively scan directory for existing files with sizes (for fast exists/size checks).
/// Returns HashMap<path, size> to avoid repeated stat() calls.
fn scan_existing_files(dir: &Path) -> HashMap<PathBuf, u64> {
    let mut files = HashMap::new();
    scan_existing_files_recursive(dir, &mut files);
    files
}

fn scan_existing_files_recursive(dir: &Path, files: &mut HashMap<PathBuf, u64>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_existing_files_recursive(&path, files);
        } else if let Ok(meta) = entry.metadata() {
            files.insert(path, meta.len());
        }
    }
}

/// Assign output paths, then write files.
/// Result of the write phase.
pub struct WriteResult {
    pub assignments: Vec<PathBuf>,
    pub files_skipped: u64,
}

pub fn write_output(
    media: &[Media],
    zip_paths: &[String],
    output_dir: &Path,
    divide_to_dates: bool,
    album_dest: Option<&str>,
    album_link: bool,
    force: bool,
    progress: &ThrottledProgress,
    checkpoint_saver: Option<&mut crate::checkpoint::CheckpointSaver>,
    cancel_token: Option<&crate::checkpoint::CancellationToken>,
) -> anyhow::Result<WriteResult> {
    fs::create_dir_all(output_dir)?;

    // Get already written files from checkpoint (if resuming)
    // Map: zip_path -> output_path
    let already_written: HashMap<String, PathBuf> = checkpoint_saver
        .as_ref()
        .map(|s| s.get_written_map())
        .unwrap_or_default();

    // Phase 1: Assign destination paths (sequential - needs collision tracking)
    // Use counters per base path to avoid O(n²) worst case
    let mut name_counters: HashMap<PathBuf, u32> = HashMap::new();
    let mut used_paths: HashSet<PathBuf> = HashSet::new();
    let mut created_dirs: HashSet<PathBuf> = HashSet::new();
    let mut assignments: Vec<PathBuf> = Vec::with_capacity(media.len());

    let mut skip_indices: HashSet<usize> = HashSet::new();

    // Pre-populate used_paths with checkpoint files (fast, no I/O)
    for path in already_written.values() {
        used_paths.insert(path.clone());
    }

    // Pre-scan existing files in output directory to avoid repeated exists()/stat() calls
    // Skip scanning if:
    // - force mode (overwrite all)
    // - checkpoint has written files (they're already tracked)
    let existing_files: HashMap<PathBuf, u64> = if force {
        // Force mode - skip all existence checks, overwrite everything
        HashMap::new()
    } else if !already_written.is_empty() {
        // Resuming from checkpoint - skip scan, checkpoint tracks all written files
        HashMap::new()
    } else if output_dir.exists() {
        scan_existing_files(output_dir)
    } else {
        HashMap::new()
    };

    for (idx, m) in media.iter().enumerate() {
        // Fast path: if file was already written (from checkpoint), use saved path
        if let Some(saved_path) = already_written.get(&m.zip_path) {
            skip_indices.insert(idx);
            assignments.push(saved_path.clone());
            continue;
        }

        // Compute destination directory
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

        // Create directory only once per unique path
        if !created_dirs.contains(&sub_dir) {
            fs::create_dir_all(&sub_dir)?;
            created_dirs.insert(sub_dir.clone());
        }

        let base_dest = sub_dir.join(&m.filename);
        let counter = name_counters.entry(base_dest.clone()).or_insert(0);

        let can_use_base = *counter == 0 && !used_paths.contains(&base_dest);
        
        // Check existing file using pre-scanned cache (O(1), no I/O)
        let existing_size = existing_files.get(&base_dest).copied();
        let existing_is_same = can_use_base && existing_size == Some(m.size);

        // Skip if existing file has same size (already written in previous run)
        if existing_is_same {
            skip_indices.insert(idx);
        }

        let dest = if existing_is_same || (can_use_base && existing_size.is_none()) {
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
                // Use cache for existence check (O(1), no I/O)
                let candidate_exists = existing_files.contains_key(&candidate);
                if !used_paths.contains(&candidate) && !candidate_exists {
                    break candidate;
                }
            }
        };

        used_paths.insert(dest.clone());
        assignments.push(dest);
    }

    // Phase 2: Write files in parallel (skip unchanged files and checkpoint files)
    let work_count = media.len() - skip_indices.len();
    let total = work_count as u64;
    let write_counter = AtomicU64::new(0);

    let num_threads = rayon::current_num_threads();
    let work: Vec<(usize, &Media, &PathBuf)> = media
        .iter()
        .zip(assignments.iter())
        .enumerate()
        .filter(|(i, _)| !skip_indices.contains(i))
        .map(|(i, (m, d))| (i, m, d))
        .collect();

    // For checkpoint tracking, we need thread-safe collection of written files
    use std::sync::Mutex;
    let written_files: Mutex<Vec<(String, PathBuf, u64)>> = Mutex::new(Vec::new());
    let cancelled = std::sync::atomic::AtomicBool::new(false);

    let mut by_zip: HashMap<usize, Vec<(usize, &Media, &PathBuf)>> = HashMap::new();
    for &(i, m, d) in &work {
        by_zip.entry(m.zip_index).or_default().push((i, m, d));
    }

    for (zip_idx, entries) in &by_zip {
        // Check cancellation before processing each zip
        if let Some(token) = cancel_token {
            if token.check().is_err() {
                cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
                break;
            }
        }

        let chunk_size = (entries.len() + num_threads - 1) / num_threads;
        let chunks: Vec<&[(usize, &Media, &PathBuf)]> = entries.chunks(chunk_size).collect();
        let zip_path = &zip_paths[*zip_idx];

        std::thread::scope(|s| -> anyhow::Result<()> {
            let handles: Vec<_> = chunks
                .into_iter()
                .map(|chunk| {
                    let write_counter = &write_counter;
                    let zip_path = zip_path;
                    let progress = &progress;
                    let written_files = &written_files;
                    let cancel_token = cancel_token;
                    let cancelled = &cancelled;
                    s.spawn(move || -> anyhow::Result<()> {
                        let file = File::open(zip_path)?;
                        let mut archive = ZipArchive::new(file)?;

                        for &(_i, m, dest) in chunk {
                            // Check for cancellation
                            if let Some(token) = cancel_token {
                                if token.check().is_err() {
                                    cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
                                    return Ok(());
                                }
                            }

                            let mut entry = archive.by_index(m.entry_index)?;
                            let mut out_file = io::BufWriter::new(File::create(dest)?);
                            io::copy(&mut entry, &mut out_file)?;

                            if let Some(dt) = &m.date {
                                if let Some(local) = dt.and_local_timezone(chrono::Local).single() {
                                    let ft = filetime::FileTime::from_unix_time(local.timestamp(), 0);
                                    filetime::set_file_mtime(dest, ft).ok();
                                }
                            }

                            // Track written file for checkpoint
                            written_files.lock().unwrap().push((
                                m.zip_path.clone(),
                                dest.clone(),
                                m.size,
                            ));

                            let current = write_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

    // Update checkpoint with written files
    if let Some(saver) = checkpoint_saver {
        let files = written_files.into_inner().unwrap();
        for (zip_path, output_path, size) in files {
            saver.mark_written(&zip_path, &output_path, size);
        }
        // Force save if cancelled
        if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            saver.force_save();
            return Err(crate::checkpoint::CancelledError.into());
        }
    }

    // Phase 3: Album output (if --album-dest album)
    if album_dest == Some("album") {
        write_album_folders(media, &assignments, output_dir, album_link)?;
    }

    Ok(WriteResult {
        assignments,
        files_skipped: skip_indices.len() as u64,
    })
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
