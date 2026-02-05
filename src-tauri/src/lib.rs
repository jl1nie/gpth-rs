use gpth_core::{ProcessOptions, Progress};
use tauri::Emitter;

#[tauri::command]
async fn run_process(
    options: ProcessOptions,
    window: tauri::Window,
) -> Result<String, String> {
    let handle = std::thread::spawn(move || {
        let cb = move |stage: &str, current: u64, total: u64, message: &str| {
            let _ = window.emit(
                "progress",
                Progress {
                    stage: stage.to_string(),
                    current,
                    total,
                    message: message.to_string(),
                },
            );
        };
        gpth_core::process(&options, &cb)
    });

    let result = handle
        .join()
        .map_err(|_| "Processing thread panicked".to_string())?
        .map_err(|e| e.to_string())?;

    Ok(format!(
        "{} media files processed, {} duplicates removed, {} files written, {} skipped",
        result.total_media, result.duplicates_removed, result.files_written, result.files_skipped
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![run_process])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
