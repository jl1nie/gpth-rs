use std::sync::{Arc, Mutex};
use gpth_core::{CancellationToken, ProcessControl, ProcessOptions, Progress};
use tauri::{Emitter, State};

/// Shared state for process control
struct ProcessState {
    cancel_token: Mutex<Option<CancellationToken>>,
}

impl Default for ProcessState {
    fn default() -> Self {
        Self {
            cancel_token: Mutex::new(None),
        }
    }
}

#[tauri::command]
async fn run_process(
    options: ProcessOptions,
    force: bool,
    window: tauri::Window,
    state: State<'_, Arc<ProcessState>>,
) -> Result<String, String> {
    // If force mode, delete any existing checkpoint
    if force {
        let _ = gpth_core::Checkpoint::delete(&options.output);
    }

    // Create and store cancellation token
    let cancel_token = CancellationToken::new();
    {
        let mut token_guard = state.cancel_token.lock().unwrap();
        *token_guard = Some(cancel_token.clone());
    }

    let state_clone = state.inner().clone();

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

        // Auto-resume unless force mode
        let control = ProcessControl::new()
            .with_resume(!force)
            .with_cancel_token(cancel_token);

        let result = gpth_core::process_with_control(&options, &control, &cb);

        // Clear the token when done
        {
            let mut token_guard = state_clone.cancel_token.lock().unwrap();
            *token_guard = None;
        }

        result
    });

    let result = handle
        .join()
        .map_err(|_| "Processing thread panicked".to_string())?;

    match result {
        Ok(result) => Ok(format!(
            "{} media files processed, {} duplicates removed, {} files written, {} skipped",
            result.total_media, result.duplicates_removed, result.files_written, result.files_skipped
        )),
        Err(e) => {
            if e.downcast_ref::<gpth_core::CancelledError>().is_some() {
                Err("Processing cancelled. Checkpoint saved.".to_string())
            } else {
                Err(e.to_string())
            }
        }
    }
}

#[tauri::command]
fn pause_process(state: State<'_, Arc<ProcessState>>) -> Result<(), String> {
    let token_guard = state.cancel_token.lock().unwrap();
    if let Some(ref token) = *token_guard {
        token.set_paused(true);
        Ok(())
    } else {
        Err("No active process to pause".to_string())
    }
}

#[tauri::command]
fn resume_process(state: State<'_, Arc<ProcessState>>) -> Result<(), String> {
    let token_guard = state.cancel_token.lock().unwrap();
    if let Some(ref token) = *token_guard {
        token.set_paused(false);
        Ok(())
    } else {
        Err("No active process to resume".to_string())
    }
}

#[tauri::command]
fn cancel_process(state: State<'_, Arc<ProcessState>>) -> Result<(), String> {
    let token_guard = state.cancel_token.lock().unwrap();
    if let Some(ref token) = *token_guard {
        token.cancel();
        Ok(())
    } else {
        Err("No active process to cancel".to_string())
    }
}

#[tauri::command]
fn is_paused(state: State<'_, Arc<ProcessState>>) -> bool {
    let token_guard = state.cancel_token.lock().unwrap();
    token_guard.as_ref().map_or(false, |t| t.is_paused())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(ProcessState::default()))
        .invoke_handler(tauri::generate_handler![
            run_process,
            pause_process,
            resume_process,
            cancel_process,
            is_paused
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
