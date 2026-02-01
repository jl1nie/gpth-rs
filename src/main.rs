mod date;
mod dedup;
mod extras;
mod folder_classify;
mod media;
mod writer;
mod zip_scan;

use std::io::Read;
use std::path::PathBuf;

use clap::Parser;

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
    let t_total = std::time::Instant::now();

    // Stage 1: Scan all zips
    eprintln!("=== Stage 1: Scanning ZIP files ===");
    let t = std::time::Instant::now();
    let scan = zip_scan::scan_zips(&cli.zip_files, cli.skip_extras)?;
    eprintln!("  Scan took {:.2}s", t.elapsed().as_secs_f64());
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
    let t = std::time::Instant::now();
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

    // EXIF pass: batch-read from each zip, then extract EXIF in parallel from memory
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
        eprintln!("Reading EXIF for {} files...", exif_targets.len());

        // Group by zip, chunk for parallel decompression + EXIF extraction
        let mut by_zip: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
        for &idx in &exif_targets {
            by_zip.entry(media[idx].zip_index).or_default().push(idx);
        }

        let num_threads = rayon::current_num_threads();
        let mut all_results: Vec<(usize, Option<date::DateResult>)> = Vec::new();

        for (zip_idx, indices) in &by_zip {
            let chunk_size = (indices.len() + num_threads - 1) / num_threads;
            let chunks: Vec<&[usize]> = indices.chunks(chunk_size).collect();
            let zip_path = &cli.zip_files[*zip_idx];

            let chunk_results: Vec<Vec<(usize, Option<date::DateResult>)>> =
                std::thread::scope(|s| {
                    let handles: Vec<_> = chunks
                        .into_iter()
                        .map(|chunk| {
                            let media = &media;
                            let zip_path = zip_path;
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
                                        .by_name(&m.zip_path)
                                        .ok()
                                        .and_then(|mut entry| {
                                            let mut bytes = Vec::with_capacity(entry.size() as usize);
                                            entry.read_to_end(&mut bytes).ok()?;
                                            Some(bytes)
                                        })
                                        .and_then(|bytes| {
                                            date::extract_date(None, Some(&bytes), &m.filename, allow_guess)
                                        });
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
                media[idx].date = Some(r.date);
                media[idx].date_accuracy = r.accuracy;
            }
        }
    }

    let dated = media.iter().filter(|m| m.date.is_some()).count();
    eprintln!("Dates found: {}/{}", dated, media.len());
    eprintln!("  Date extraction took {:.2}s", t.elapsed().as_secs_f64());

    // Stage 3: Deduplicate
    eprintln!("=== Stage 3: Deduplicating ===");
    let t = std::time::Instant::now();
    let before = media.len();
    media = dedup::deduplicate(media, &cli.zip_files)?;
    eprintln!("Removed {} duplicates, {} files remaining", before - media.len(), media.len());
    eprintln!("  Dedup took {:.2}s", t.elapsed().as_secs_f64());

    // Stage 4: Write output
    eprintln!("=== Stage 4: Writing output ===");
    let t = std::time::Instant::now();
    writer::write_output(&media, &cli.zip_files, &cli.output, cli.divide_to_dates)?;
    eprintln!("  Write took {:.2}s", t.elapsed().as_secs_f64());

    eprintln!("Total: {:.2}s", t_total.elapsed().as_secs_f64());
    eprintln!("Done!");
    Ok(())
}
