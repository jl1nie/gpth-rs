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

    /// Resume from checkpoint if available
    #[arg(long)]
    resume: bool,

    /// Ignore existing checkpoint and start fresh
    #[arg(long, conflicts_with = "resume")]
    no_resume: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let t_total = std::time::Instant::now();

    let options = gpth_core::ProcessOptions {
        zip_files: cli.zip_files,
        output: cli.output.clone(),
        divide_to_dates: cli.divide_to_dates,
        skip_extras: cli.skip_extras,
        no_guess: cli.no_guess,
        albums: cli.albums,
        album_dest: cli.album_dest,
        album_link: cli.album_link,
        album_json: cli.album_json,
    };

    // Set up cancellation token and Ctrl+C handler
    let cancel_token = gpth_core::CancellationToken::new();
    let token_clone = cancel_token.clone();
    
    ctrlc::set_handler(move || {
        eprintln!("\nInterrupted! Saving checkpoint...");
        token_clone.cancel();
    })?;

    // Determine resume mode
    let resume = if cli.no_resume {
        // Delete existing checkpoint if --no-resume
        let _ = gpth_core::Checkpoint::delete(&cli.output);
        false
    } else {
        cli.resume
    };

    let control = gpth_core::ProcessControl::new()
        .with_resume(resume)
        .with_cancel_token(cancel_token);

    let result = gpth_core::process_with_control(&options, &control, &|stage, current, total, message| {
        eprint!("\r[{}] {}/{} {}        ", stage, current + 1, total, message);
    });

    eprintln!(); // Clear the progress line

    match result {
        Ok(result) => {
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
        Err(e) => {
            if e.downcast_ref::<gpth_core::CancelledError>().is_some() {
                eprintln!("Processing interrupted. Checkpoint saved.");
                eprintln!("Run with --resume to continue from where you left off.");
                std::process::exit(130); // Standard exit code for Ctrl+C
            } else {
                Err(e)
            }
        }
    }
}
