mod models;
mod providers;

use models::{ArchiveRequest, ArchiveResult, DeleteRequest, DeleteResult, ScanResult};
use providers::ProviderRegistry;

#[tauri::command]
async fn scan_sessions() -> Result<ScanResult, String> {
    tauri::async_runtime::spawn_blocking(|| ProviderRegistry::built_in().scan())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn delete_sessions(request: DeleteRequest) -> Result<DeleteResult, String> {
    tauri::async_runtime::spawn_blocking(move || ProviderRegistry::built_in().delete(request))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn archive_sessions(request: ArchiveRequest) -> Result<ArchiveResult, String> {
    tauri::async_runtime::spawn_blocking(move || ProviderRegistry::built_in().archive(request))
        .await
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            scan_sessions,
            delete_sessions,
            archive_sessions
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
