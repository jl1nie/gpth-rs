pub mod album_json;
pub mod checkpoint;
pub mod date;
pub mod dedup;
pub mod extras;
pub mod folder_classify;
pub mod media;
pub mod writer;
pub mod zip_scan;

use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};

fn default_album_dest() -> String {
    "year".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessOptions {
    pub zip_files: Vec<String>,
    pub output: PathBuf,
    pub divide_to_dates: bool,
    pub skip_extras: bool,
    pub no_guess: bool,
    #[serde(default)]
    pub albums: bool,
    #[serde(default = "default_album_dest")]
    pub album_dest: String,
    #[serde(default)]
    pub album_link: bool,
    #[serde(default)]
    pub album_json: Option<PathBuf>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub stage: String,
    pub current: u64,
    pub total: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessResult {
    pub total_media: u64,
    pub duplicates_removed: u64,
    pub files_written: u64,
    #[serde(default)]
    pub files_skipped: u64,
    #[serde(default)]
    pub warnings: Vec<String>,
}


/// Control options for process execution (resume, cancellation).
#[derive(Debug, Clone, Default)]
pub struct ProcessControl {
    /// Whether to resume from an existing checkpoint.
    pub resume: bool,
    /// Cancellation token for pause/cancel support.
    pub cancel_token: Option<checkpoint::CancellationToken>,
}

impl ProcessControl {
    /// Create a new ProcessControl with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create ProcessControl with resume enabled.
    pub fn with_resume(mut self, resume: bool) -> Self {
        self.resume = resume;
        self
    }

    /// Create ProcessControl with a cancellation token.
    pub fn with_cancel_token(mut self, token: checkpoint::CancellationToken) -> Self {
        self.cancel_token = Some(token);
        self
    }
}

// Re-export checkpoint types for convenience
pub use checkpoint::{CancellationToken, CancelledError, Checkpoint, CheckpointSaver, CHECKPOINT_FILENAME};

/// Type alias for progress callback
pub type ProgressCallback = dyn Fn(&str, u64, u64, &str) + Send + Sync;

/// Throttled progress reporter — emits at most every 200ms or on completion.
pub struct ThrottledProgress<'a> {
    inner: &'a ProgressCallback,
    last_emit: std::sync::Mutex<Instant>,
}

impl<'a> ThrottledProgress<'a> {
    pub fn new(inner: &'a ProgressCallback) -> Self {
        Self {
            inner,
            last_emit: std::sync::Mutex::new(Instant::now() - std::time::Duration::from_secs(1)),
        }
    }

    pub fn report(&self, stage: &str, current: u64, total: u64, message: &str) {
        let is_done = current + 1 >= total;
        if !is_done {
            let mut last = self.last_emit.lock().unwrap();
            if last.elapsed().as_millis() < 200 {
                return;
            }
            *last = Instant::now();
        }
        (self.inner)(stage, current, total, message);
    }
}

/// Run the full processing pipeline with progress reporting.
pub fn process(
    options: &ProcessOptions,
    progress_callback: &ProgressCallback,
) -> anyhow::Result<ProcessResult> {
    process_with_control(options, &ProcessControl::default(), progress_callback)
}

/// Run the full processing pipeline with progress reporting and control options.
pub fn process_with_control(
    options: &ProcessOptions,
    control: &ProcessControl,
    progress_callback: &ProgressCallback,
) -> anyhow::Result<ProcessResult> {
    let tp = ThrottledProgress::new(progress_callback);

    // Check for cancellation early
    if let Some(ref token) = control.cancel_token {
        token.check()?;
    }

    // Load or create checkpoint
    let mut checkpoint_saver = if control.resume {
        if let Some(existing) = checkpoint::Checkpoint::load(&options.output)? {
            if existing.is_compatible(options)? {
                eprintln!("Resuming from checkpoint: {} files already written", existing.written_files.len());
                Some(checkpoint::CheckpointSaver::from_existing(existing, options.output.clone()))
            } else {
                eprintln!("Checkpoint incompatible with current options, starting fresh");
                let cp = checkpoint::Checkpoint::new(options)?;
                Some(checkpoint::CheckpointSaver::new(cp, options.output.clone()))
            }
        } else {
            let cp = checkpoint::Checkpoint::new(options)?;
            Some(checkpoint::CheckpointSaver::new(cp, options.output.clone()))
        }
    } else {
        // Even without --resume, we create a checkpoint for potential future resume
        let cp = checkpoint::Checkpoint::new(options)?;
        Some(checkpoint::CheckpointSaver::new(cp, options.output.clone()))
    };

    if let Some(ref mut saver) = checkpoint_saver {
        saver.set_stage("scan");
    }

    // Stage 1: Scan all zips
    let scan = zip_scan::scan_zips(&options.zip_files, options.skip_extras, options.albums, &tp)?;
    let mut media_list = scan.media;

    if media_list.is_empty() {
        // Clean up checkpoint on success
        if let Some(mut saver) = checkpoint_saver {
            let _ = saver.mark_completed();
        }
        return Ok(ProcessResult {
            total_media: 0,
            duplicates_removed: 0,
            files_written: 0,
            files_skipped: 0,
            warnings: vec![],
        });
    }

    // Check for cancellation
    if let Some(ref token) = control.cancel_token {
        if token.check().is_err() {
            if let Some(mut saver) = checkpoint_saver {
                saver.force_save();
            }
            return Err(checkpoint::CancelledError.into());
        }
    }

    // Use pre-built JSON date map from scan (already has all variants registered)
    let json_dates = scan.json_dates;

    if let Some(ref mut saver) = checkpoint_saver {
        saver.set_stage("date");
    }

    // Stage 2: Extract dates
    let allow_guess = !options.no_guess;
    let total = media_list.len() as u64;

    // JSON + guess pass (fast, single report)
    for m in media_list.iter_mut() {
        let json_date = date::json::find_json_date(&m.zip_path, &json_dates);

        if let Some(result) = date::extract_date(json_date, None, &m.filename, allow_guess) {
            m.date = Some(result.date);
            m.date_accuracy = result.accuracy;
        }
    }
    tp.report("date", total, total, "JSON/filename dates extracted");

    // EXIF pass
    let exif_targets: Vec<usize> = media_list
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.date.is_none()
                && m.size <= 32 * 1024 * 1024
                && mime_guess::from_path(&m.filename)
                    .first()
                    .map_or(false, |mime| mime.type_() == mime_guess::mime::IMAGE)
        })
        .map(|(i, _)| i)
        .collect();

    if !exif_targets.is_empty() {
        let exif_total = exif_targets.len() as u64;
        let mut by_zip: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
        for &idx in &exif_targets {
            by_zip.entry(media_list[idx].zip_index).or_default().push(idx);
        }

        let num_threads = rayon::current_num_threads();
        let mut all_results: Vec<(usize, Option<date::DateResult>)> = Vec::new();
        let counter = AtomicU64::new(0);

        for (zip_idx, indices) in &by_zip {
            let chunk_size = (indices.len() + num_threads - 1) / num_threads;
            let chunks: Vec<&[usize]> = indices.chunks(chunk_size).collect();
            let zip_path = &options.zip_files[*zip_idx];

            let chunk_results: Vec<Vec<(usize, Option<date::DateResult>)>> =
                std::thread::scope(|s| {
                    let handles: Vec<_> = chunks
                        .into_iter()
                        .map(|chunk| {
                            let media = &media_list;
                            let zip_path = zip_path;
                            let counter = &counter;
                            let tp = &tp;
                            s.spawn(move || -> Vec<(usize, Option<date::DateResult>)> {
                                let Ok(file) = std::fs::File::open(zip_path) else {
                                    return vec![];
                                };
                                let Ok(mut archive) = zip::ZipArchive::new(file) else {
                                    return vec![];
                                };
                                let mut results = Vec::with_capacity(chunk.len());
                                for &midx in chunk {
                                    let m = &media[midx];
                                    let result = archive
                                        .by_index(m.entry_index)
                                        .ok()
                                        .and_then(|mut entry| {
                                            let mut bytes = Vec::with_capacity(entry.size() as usize);
                                            entry.read_to_end(&mut bytes).ok()?;
                                            Some(bytes)
                                        })
                                        .and_then(|bytes| {
                                            date::extract_date(None, Some(&bytes), &m.filename, allow_guess)
                                        });
                                    let current = counter.fetch_add(1, Ordering::Relaxed);
                                    tp.report("date-exif", current, exif_total, "Reading EXIF");
                                    results.push((midx, result));
                                }
                                results
                            })
                        })
                        .collect();
                    handles.into_iter().map(|h| h.join().unwrap()).collect()
                });

            for chunk in chunk_results {
                all_results.extend(chunk);
            }
        }

        for (idx, result) in all_results {
            if let Some(r) = result {
                media_list[idx].date = Some(r.date);
                media_list[idx].date_accuracy = r.accuracy;
            }
        }
    }

    // Check for cancellation
    if let Some(ref token) = control.cancel_token {
        if token.check().is_err() {
            if let Some(mut saver) = checkpoint_saver {
                saver.force_save();
            }
            return Err(checkpoint::CancelledError.into());
        }
    }

    // Stage 2.5: Album merge (O(N+M) using HashMap)
    let album_only_start = media_list.len();
    if options.albums && !scan.album_entries.is_empty() {
        // Build lookup index: (filename, size) -> media index
        let mut media_index: std::collections::HashMap<(String, u64), usize> =
            std::collections::HashMap::with_capacity(media_list.len());
        for (i, m) in media_list.iter().enumerate() {
            media_index.insert((m.filename.clone(), m.size), i);
        }

        for (album_name, entries) in &scan.album_entries {
            for ae in entries {
                // O(1) lookup using HashMap
                if let Some(&idx) = media_index.get(&(ae.filename.clone(), ae.size)) {
                    if !media_list[idx].albums.contains(album_name) {
                        media_list[idx].albums.push(album_name.clone());
                    }
                } else {
                    // Album-only file: add as new Media
                    let mut m = media::Media::new(
                        ae.zip_path.clone(),
                        ae.zip_index,
                        ae.entry_index,
                        ae.filename.clone(),
                        ae.size,
                    );
                    m.albums.push(album_name.clone());
                    // Add to index for subsequent lookups within same album scan
                    media_index.insert((ae.filename.clone(), ae.size), media_list.len());
                    media_list.push(m);
                }
            }
        }
    }

    // Stage 2.6: Extract dates for album-only files
    if album_only_start < media_list.len() {
        // JSON + guess pass for album-only files
        for m in media_list[album_only_start..].iter_mut() {
            let json_date = date::json::find_json_date(&m.zip_path, &json_dates);
            if let Some(result) = date::extract_date(json_date, None, &m.filename, allow_guess) {
                m.date = Some(result.date);
                m.date_accuracy = result.accuracy;
            }
        }

        // EXIF pass for album-only files
        let album_exif_targets: Vec<usize> = media_list[album_only_start..]
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                m.date.is_none()
                    && m.size <= 32 * 1024 * 1024
                    && mime_guess::from_path(&m.filename)
                        .first()
                        .map_or(false, |mime| mime.type_() == mime_guess::mime::IMAGE)
            })
            .map(|(i, _)| album_only_start + i)
            .collect();

        if !album_exif_targets.is_empty() {
            let exif_total = album_exif_targets.len() as u64;
            let mut by_zip: std::collections::HashMap<usize, Vec<usize>> =
                std::collections::HashMap::new();
            for &idx in &album_exif_targets {
                by_zip
                    .entry(media_list[idx].zip_index)
                    .or_default()
                    .push(idx);
            }

            let num_threads = rayon::current_num_threads();
            let mut all_results: Vec<(usize, Option<date::DateResult>)> = Vec::new();
            let counter = AtomicU64::new(0);

            for (zip_idx, indices) in &by_zip {
                let chunk_size = (indices.len() + num_threads - 1) / num_threads;
                let chunks: Vec<&[usize]> = indices.chunks(chunk_size).collect();
                let zip_path = &options.zip_files[*zip_idx];

                let chunk_results: Vec<Vec<(usize, Option<date::DateResult>)>> =
                    std::thread::scope(|s| {
                        let handles: Vec<_> = chunks
                            .into_iter()
                            .map(|chunk| {
                                let media = &media_list;
                                let zip_path = zip_path;
                                let counter = &counter;
                                let tp = &tp;
                                s.spawn(move || -> Vec<(usize, Option<date::DateResult>)> {
                                    let Ok(file) = std::fs::File::open(zip_path) else {
                                        return vec![];
                                    };
                                    let Ok(mut archive) = zip::ZipArchive::new(file) else {
                                        return vec![];
                                    };
                                    let mut results = Vec::with_capacity(chunk.len());
                                    for &midx in chunk {
                                        let m = &media[midx];
                                        let result = archive
                                            .by_index(m.entry_index)
                                            .ok()
                                            .and_then(|mut entry| {
                                                let mut bytes =
                                                    Vec::with_capacity(entry.size() as usize);
                                                entry.read_to_end(&mut bytes).ok()?;
                                                Some(bytes)
                                            })
                                            .and_then(|bytes| {
                                                date::extract_date(
                                                    None,
                                                    Some(&bytes),
                                                    &m.filename,
                                                    allow_guess,
                                                )
                                            });
                                        let current = counter.fetch_add(1, Ordering::Relaxed);
                                        tp.report(
                                            "date-exif-album",
                                            current,
                                            exif_total,
                                            "Reading EXIF (albums)",
                                        );
                                        results.push((midx, result));
                                    }
                                    results
                                })
                            })
                            .collect();
                        handles.into_iter().map(|h| h.join().unwrap()).collect()
                    });

                for chunk in chunk_results {
                    all_results.extend(chunk);
                }
            }

            for (idx, result) in all_results {
                if let Some(r) = result {
                    media_list[idx].date = Some(r.date);
                    media_list[idx].date_accuracy = r.accuracy;
                }
            }
        }
    }

    // Check for cancellation
    if let Some(ref token) = control.cancel_token {
        if token.check().is_err() {
            if let Some(mut saver) = checkpoint_saver {
                saver.force_save();
            }
            return Err(checkpoint::CancelledError.into());
        }
    }

    if let Some(ref mut saver) = checkpoint_saver {
        saver.set_stage("dedup");
    }

    // Stage 3: Deduplicate
    let before = media_list.len();
    let dedup_result = dedup::deduplicate(media_list, &options.zip_files, &tp)?;
    media_list = dedup_result.media;
    let warnings = dedup_result.warnings;
    let duplicates_removed = (before - media_list.len()) as u64;

    // Check for cancellation
    if let Some(ref token) = control.cancel_token {
        if token.check().is_err() {
            if let Some(mut saver) = checkpoint_saver {
                saver.force_save();
            }
            return Err(checkpoint::CancelledError.into());
        }
    }

    if let Some(ref mut saver) = checkpoint_saver {
        saver.set_stage("write");
    }

    // Stage 4: Write output
    let album_dest_opt = if options.albums {
        Some(options.album_dest.as_str())
    } else {
        None
    };
    let write_result = writer::write_output(
        &media_list,
        &options.zip_files,
        &options.output,
        options.divide_to_dates,
        album_dest_opt,
        options.album_link,
        options.force,
        &tp,
        checkpoint_saver.as_mut(),
        control.cancel_token.as_ref(),
    )?;
    let assignments = write_result.assignments;
    let files_skipped = write_result.files_skipped;

    // Write albums.json if any albums exist
    if options.albums {
        let has_albums = media_list.iter().any(|m| !m.albums.is_empty());
        if has_albums {
            let album_json_path = options.album_json.clone()
                .unwrap_or_else(|| options.output.join("albums.json"));
            album_json::write_albums_json(&media_list, &assignments, &options.output, &album_json_path)?;
        }
    }

    // Clean up checkpoint on success
    if let Some(mut saver) = checkpoint_saver {
        let _ = saver.mark_completed();
    }

    Ok(ProcessResult {
        total_media: before as u64,
        duplicates_removed,
        files_written: media_list.len() as u64 - files_skipped,
        files_skipped,
        warnings,
    })
}
