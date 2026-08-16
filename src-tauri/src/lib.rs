mod cleanup;
mod discovery;
mod models;

use models::{DeleteRequest, DeleteResult, ScanResult};

#[tauri::command]
async fn scan_sessions() -> Result<ScanResult, String> {
    tauri::async_runtime::spawn_blocking(discovery::scan_all)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn delete_sessions(request: DeleteRequest) -> Result<DeleteResult, String> {
    tauri::async_runtime::spawn_blocking(move || cleanup::delete(request))
        .await
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            scan_sessions,
            delete_sessions
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
