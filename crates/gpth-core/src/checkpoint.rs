
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::ProcessOptions;

/// Current checkpoint file format version
const CHECKPOINT_VERSION: u32 = 1;

/// Default checkpoint filename
pub const CHECKPOINT_FILENAME: &str = ".gpth-progress.json";

/// A file that was successfully written to the output directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrittenFile {
    pub zip_path: String,
    pub output_path: PathBuf,
    pub size: u64,
}

/// Checkpoint data stored in .gpth-progress.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub version: u32,
    pub timestamp: DateTime<Utc>,
    pub options_hash: String,
    pub zip_files: Vec<String>,
    pub zip_mtimes: Vec<i64>,
    pub written_files: Vec<WrittenFile>,
    pub last_stage: String,
    pub completed: bool,
}

impl Checkpoint {
    /// Create a new checkpoint for the given options.
    pub fn new(options: &ProcessOptions) -> anyhow::Result<Self> {
        let options_hash = compute_options_hash(options);
        let zip_mtimes = get_zip_mtimes(&options.zip_files)?;

        Ok(Self {
            version: CHECKPOINT_VERSION,
            timestamp: Utc::now(),
            options_hash,
            zip_files: options.zip_files.clone(),
            zip_mtimes,
            written_files: Vec::new(),
            last_stage: String::new(),
            completed: false,
        })
    }

    /// Load checkpoint from output directory.
    pub fn load(output_dir: &Path) -> anyhow::Result<Option<Self>> {
        let path = output_dir.join(CHECKPOINT_FILENAME);
        if !path.exists() {
            return Ok(None);
        }

        let file = File::open(&path)?;
        let reader = BufReader::new(file);
        let checkpoint: Checkpoint = serde_json::from_reader(reader)?;

        Ok(Some(checkpoint))
    }

    /// Save checkpoint to output directory.
    pub fn save(&self, output_dir: &Path) -> anyhow::Result<()> {
        let path = output_dir.join(CHECKPOINT_FILENAME);
        let temp_path = output_dir.join(".gpth-progress.tmp");

        // Write to temp file first, then rename for atomicity
        let file = File::create(&temp_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)?;

        fs::rename(&temp_path, &path)?;
        Ok(())
    }

    /// Delete checkpoint file from output directory.
    pub fn delete(output_dir: &Path) -> anyhow::Result<()> {
        let path = output_dir.join(CHECKPOINT_FILENAME);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Check if this checkpoint is compatible with the given options.
    pub fn is_compatible(&self, options: &ProcessOptions) -> anyhow::Result<bool> {
        // Version check
        if self.version != CHECKPOINT_VERSION {
            return Ok(false);
        }

        // Already completed
        if self.completed {
            return Ok(false);
        }

        // Options hash must match
        let current_hash = compute_options_hash(options);
        if self.options_hash != current_hash {
            return Ok(false);
        }

        // Zip files must match
        if self.zip_files != options.zip_files {
            return Ok(false);
        }

        // Zip mtimes must match (files haven't been modified)
        let current_mtimes = get_zip_mtimes(&options.zip_files)?;
        if self.zip_mtimes != current_mtimes {
            return Ok(false);
        }

        Ok(true)
    }

    /// Mark a file as successfully written.
    pub fn mark_written(&mut self, zip_path: &str, output_path: &Path, size: u64) {
        self.written_files.push(WrittenFile {
            zip_path: zip_path.to_string(),
            output_path: output_path.to_path_buf(),
            size,
        });
        self.timestamp = Utc::now();
    }

    /// Get map of zip_path -> output_path for already written files.
    pub fn get_written_map(&self) -> std::collections::HashMap<String, PathBuf> {
        self.written_files
            .iter()
            .map(|f| (f.zip_path.clone(), f.output_path.clone()))
            .collect()
    }

    /// Update the last stage marker.
    pub fn set_stage(&mut self, stage: &str) {
        self.last_stage = stage.to_string();
        self.timestamp = Utc::now();
    }

    /// Mark processing as completed.
    pub fn mark_completed(&mut self) {
        self.completed = true;
        self.timestamp = Utc::now();
    }
}

/// Compute a hash of the relevant options for compatibility checking.
fn compute_options_hash(options: &ProcessOptions) -> String {
    let mut hasher = Sha256::new();
    // Include options that affect output
    hasher.update(if options.divide_to_dates { b"1" } else { b"0" });
    hasher.update(if options.skip_extras { b"1" } else { b"0" });
    hasher.update(if options.no_guess { b"1" } else { b"0" });
    hasher.update(if options.albums { b"1" } else { b"0" });
    hasher.update(options.album_dest.as_bytes());
    hasher.update(if options.album_link { b"1" } else { b"0" });
    hasher.update(options.output.to_string_lossy().as_bytes());
    hex::encode(hasher.finalize())
}

/// Get modification times for all zip files.
fn get_zip_mtimes(zip_files: &[String]) -> anyhow::Result<Vec<i64>> {
    let mut mtimes = Vec::with_capacity(zip_files.len());
    for path in zip_files {
        let metadata = fs::metadata(path)?;
        let mtime = metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        mtimes.push(mtime);
    }
    Ok(mtimes)
}

/// Token for cooperative cancellation and pause support.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    /// Create a new cancellation token.
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request cancellation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Check if cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Set paused state.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    /// Check if paused.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Check cancellation and wait while paused.
    /// Returns Ok(()) to continue, Err if cancelled.
    pub fn check(&self) -> Result<(), CancelledError> {
        if self.is_cancelled() {
            return Err(CancelledError);
        }

        // Wait while paused
        while self.is_paused() {
            if self.is_cancelled() {
                return Err(CancelledError);
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        Ok(())
    }
}

/// Error indicating the operation was cancelled.
#[derive(Debug, Clone)]
pub struct CancelledError;

impl std::fmt::Display for CancelledError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Operation cancelled")
    }
}

impl std::error::Error for CancelledError {}

/// Manages checkpoint saving with throttling to reduce I/O overhead.
pub struct CheckpointSaver {
    checkpoint: Checkpoint,
    output_dir: PathBuf,
    last_save: Instant,
    files_since_save: usize,
    min_interval: Duration,
    min_files: usize,
}

impl CheckpointSaver {
    /// Create a new checkpoint saver.
    pub fn new(checkpoint: Checkpoint, output_dir: PathBuf) -> Self {
        Self {
            checkpoint,
            output_dir,
            last_save: Instant::now(),
            files_since_save: 0,
            min_interval: Duration::from_secs(5),
            min_files: 100,
        }
    }

    /// Create a checkpoint saver for resuming from an existing checkpoint.
    pub fn from_existing(checkpoint: Checkpoint, output_dir: PathBuf) -> Self {
        Self::new(checkpoint, output_dir)
    }

    /// Mark a file as written and maybe save checkpoint.
    pub fn mark_written(&mut self, zip_path: &str, output_path: &Path, size: u64) {
        self.checkpoint.mark_written(zip_path, output_path, size);
        self.files_since_save += 1;
        self.maybe_save();
    }

    /// Save checkpoint if enough time has passed or enough files processed.
    fn maybe_save(&mut self) {
        let should_save = self.last_save.elapsed() >= self.min_interval
            || self.files_since_save >= self.min_files;
        if should_save {
            self.force_save();
        }
    }

    /// Force save checkpoint to disk.
    pub fn force_save(&mut self) {
        let _ = self.checkpoint.save(&self.output_dir);
        self.last_save = Instant::now();
        self.files_since_save = 0;
    }

    /// Set the current stage.
    pub fn set_stage(&mut self, stage: &str) {
        self.checkpoint.set_stage(stage);
    }

    /// Mark as completed and delete checkpoint file.
    pub fn mark_completed(&mut self) -> anyhow::Result<()> {
        self.checkpoint.mark_completed();
        Checkpoint::delete(&self.output_dir)
    }

    /// Get map of zip_path -> output_path for already written files.
    pub fn get_written_map(&self) -> std::collections::HashMap<String, PathBuf> {
        self.checkpoint.get_written_map()
    }

    /// Get reference to checkpoint.
    pub fn checkpoint(&self) -> &Checkpoint {
        &self.checkpoint
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn test_options() -> ProcessOptions {
        ProcessOptions {
            zip_files: vec!["test.zip".to_string()],
            output: PathBuf::from("/tmp/output"),
            divide_to_dates: true,
            skip_extras: false,
            no_guess: false,
            albums: false,
            album_dest: "year".to_string(),
            album_link: false,
            album_json: None,
        }
    }

    #[test]
    fn test_cancellation_token() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        assert!(!token.is_paused());
        assert!(token.check().is_ok());

        token.cancel();
        assert!(token.is_cancelled());
        assert!(token.check().is_err());
    }

    #[test]
    fn test_checkpoint_save_load() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path();

        // Create a dummy zip file for mtime
        let zip_path = dir_path.join("test.zip");
        File::create(&zip_path).unwrap().write_all(b"test").unwrap();

        let options = ProcessOptions {
            zip_files: vec![zip_path.to_string_lossy().to_string()],
            output: dir_path.to_path_buf(),
            divide_to_dates: true,
            skip_extras: false,
            no_guess: false,
            albums: false,
            album_dest: "year".to_string(),
            album_link: false,
            album_json: None,
        };

        let mut checkpoint = Checkpoint::new(&options).unwrap();
        checkpoint.mark_written("Photos/img.jpg", Path::new("2023/01/img.jpg"), 1024);
        checkpoint.save(dir_path).unwrap();

        let loaded = Checkpoint::load(dir_path).unwrap().unwrap();
        assert_eq!(loaded.version, CHECKPOINT_VERSION);
        assert_eq!(loaded.written_files.len(), 1);
        assert!(!loaded.completed);
    }

    #[test]
    fn test_options_hash_changes() {
        let opts1 = test_options();
        let mut opts2 = test_options();
        opts2.divide_to_dates = false;

        let hash1 = compute_options_hash(&opts1);
        let hash2 = compute_options_hash(&opts2);
        assert_ne!(hash1, hash2);
    }
}
