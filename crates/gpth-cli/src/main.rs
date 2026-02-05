use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "gpth-rs-cli", version, about = "Google Photos Takeout Helper - process zip files without extraction")]
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

    /// Process album folders (non-year named folders)
    #[arg(long)]
    albums: bool,

    /// Album file output mode: "year" (merge into date folders) or "album" (albums/<name>/)
    #[arg(long, default_value = "year")]
    album_dest: String,

    /// Use relative symlinks instead of copies for album output (--album-dest album only)
    #[arg(long)]
    album_link: bool,

    /// Output path for albums.json (default: <output>/albums.json)
    #[arg(long)]
    album_json: Option<std::path::PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let t_total = std::time::Instant::now();

    let options = gpth_core::ProcessOptions {
        zip_files: cli.zip_files,
        output: cli.output,
        divide_to_dates: cli.divide_to_dates,
        skip_extras: cli.skip_extras,
        no_guess: cli.no_guess,
        albums: cli.albums,
        album_dest: cli.album_dest,
        album_link: cli.album_link,
        album_json: cli.album_json,
    };

    let result = gpth_core::process(&options, &|stage, current, total, message| {
        eprintln!("\r[{}] {}/{} {}", stage, current + 1, total, message);
    })?;

    eprintln!(
        "Done! {} media files, {} duplicates removed, {} files written, {} skipped ({:.2}s)",
        result.total_media,
        result.duplicates_removed,
        result.files_written,
        result.files_skipped,
        t_total.elapsed().as_secs_f64()
    );

    Ok(())
}
