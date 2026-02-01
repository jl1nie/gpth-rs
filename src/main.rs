mod date;
mod dedup;
mod extras;
mod folder_classify;
mod media;
mod writer;
mod zip_scan;

use std::path::PathBuf;

use clap::Parser;
use rayon::prelude::*;

#[derive(Parser)]
#[command(name = "gpth-rs", version, about = "Google Photos Takeout Helper - process zip files without extraction")]
struct Cli {
    /// Google Takeout zip files
    #[arg(required = true)]
    zip_files: Vec<String>,

    /// Output directory
    #[arg(short, long)]
    output: PathBuf,

    /// Organize into YYYY/MM subdirectories
    #[arg(long)]
    divide_to_dates: bool,

    /// Skip -edited, -effects and similar derivative images
    #[arg(long)]
    skip_extras: bool,

    /// Disable date guessing from filenames
    #[arg(long)]
    no_guess: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Stage 1: Scan all zips
    eprintln!("=== Stage 1: Scanning ZIP files ===");
    let scan = zip_scan::scan_zips(&cli.zip_files, cli.skip_extras)?;
    let mut media = scan.media;

    if media.is_empty() {
        eprintln!("No media files found in year folders. Nothing to do.");
        return Ok(());
    }

    // Build JSON date map
    let json_dates = date::json::build_json_date_map(&scan.json_entries);
    eprintln!("Parsed {} JSON date entries", json_dates.len());

    // Stage 2: Extract dates
    eprintln!("=== Stage 2: Extracting dates ===");
    let allow_guess = !cli.no_guess;

    // JSON + guess pass (no I/O needed, already in memory)
    for m in &mut media {
        let json_date = date::json::find_json_date(&m.zip_path, &m.filename, &json_dates, false)
            .or_else(|| {
                date::json::find_json_date(&m.zip_path, &m.filename, &json_dates, true)
            });

        if let Some(result) = date::extract_date(json_date, None, &m.filename, allow_guess) {
            m.date = Some(result.date);
            m.date_accuracy = result.accuracy;
        }
    }

    // EXIF pass (parallel): only for files without date yet, images < 32MiB
    let exif_targets: Vec<usize> = media
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
        eprintln!("Reading EXIF for {} files (parallel)...", exif_targets.len());

        let exif_results: Vec<(usize, Option<date::DateResult>)> = exif_targets
            .par_iter()
            .map(|&idx| {
                let m = &media[idx];
                let result = read_entry_from_zip(&cli.zip_files[m.zip_index], &m.zip_path)
                    .ok()
                    .and_then(|bytes| date::extract_date(None, Some(&bytes), &m.filename, allow_guess));
                (idx, result)
            })
            .collect();

        for (idx, result) in exif_results {
            if let Some(r) = result {
                media[idx].date = Some(r.date);
                media[idx].date_accuracy = r.accuracy;
            }
        }
    }

    let dated = media.iter().filter(|m| m.date.is_some()).count();
    eprintln!("Dates found: {}/{}", dated, media.len());

    // Stage 3: Deduplicate
    eprintln!("=== Stage 3: Deduplicating ===");
    let before = media.len();
    media = dedup::deduplicate(media, &cli.zip_files)?;
    eprintln!("Removed {} duplicates, {} files remaining", before - media.len(), media.len());

    // Stage 4: Write output
    eprintln!("=== Stage 4: Writing output ===");
    writer::write_output(&media, &cli.zip_files, &cli.output, cli.divide_to_dates)?;

    eprintln!("Done!");
    Ok(())
}

fn read_entry_from_zip(zip_path: &str, entry_path: &str) -> anyhow::Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut entry = archive.by_name(entry_path)?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}
