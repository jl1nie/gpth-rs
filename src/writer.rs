use std::collections::HashSet;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use zip::ZipArchive;

use crate::media::Media;

/// Assign output paths (sequential for collision handling), then write files in parallel.
pub fn write_output(
    media: &[Media],
    zip_paths: &[String],
    output_dir: &Path,
    divide_to_dates: bool,
) -> anyhow::Result<()> {
    fs::create_dir_all(output_dir)?;

    // Phase 1: Assign destination paths (sequential - needs collision tracking)
    let mut used_paths: HashSet<PathBuf> = HashSet::new();
    let mut assignments: Vec<(&Media, PathBuf)> = Vec::with_capacity(media.len());

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

        let mut dest = sub_dir.join(&m.filename);
        if used_paths.contains(&dest) || dest.exists() {
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
                dest = sub_dir.join(&new_name);
                if !used_paths.contains(&dest) && !dest.exists() {
                    break;
                }
                counter += 1;
            }
        }

        used_paths.insert(dest.clone());
        assignments.push((m, dest));
    }

    // Phase 2: Write files in parallel
    let pb = ProgressBar::new(assignments.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:40}] {pos}/{len} writing files")
            .unwrap(),
    );

    assignments
        .par_iter()
        .try_for_each(|(m, dest)| -> anyhow::Result<()> {
            let zip_path = &zip_paths[m.zip_index];
            let file = File::open(zip_path)?;
            let mut archive = ZipArchive::new(file)?;
            let mut entry = archive.by_name(&m.zip_path)?;

            let mut out_file = File::create(dest)?;
            io::copy(&mut entry, &mut out_file)?;

            if let Some(dt) = &m.date {
                // NaiveDateTime is local time; convert back to UTC epoch for mtime
                if let Some(local) = dt.and_local_timezone(chrono::Local).single() {
                    let ft = filetime::FileTime::from_unix_time(local.timestamp(), 0);
                    filetime::set_file_mtime(dest, ft).ok();
                }
            }

            pb.inc(1);
            Ok(())
        })?;

    pb.finish_and_clear();
    eprintln!("Wrote {} files to {}", media.len(), output_dir.display());

    Ok(())
}
