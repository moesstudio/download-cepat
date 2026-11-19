use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidIncomingIntent {
    pub kind: String,
    pub value: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidExportResult {
    pub content_uri: String,
    pub file_path: Option<String>,
    pub display_path: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidExportEntry {
    pub relative_subpath: String,
    pub content_uri: String,
    pub file_path: Option<String>,
    pub display_path: String,
}

static PENDING_INTENTS: OnceLock<Mutex<Vec<AndroidIncomingIntent>>> = OnceLock::new();
static PAUSE_ALL_HANDLER: OnceLock<Mutex<Option<Box<dyn Fn() + Send + Sync>>>> = OnceLock::new();

#[allow(dead_code)]
pub fn set_pause_all_handler<F>(handler: F)
where
    F: Fn() + Send + Sync + 'static,
{
    let store = PAUSE_ALL_HANDLER.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = store.lock() {
        *guard = Some(Box::new(handler));
    }
}

#[allow(dead_code)]
fn trigger_pause_all_handler() {
    if let Some(store) = PAUSE_ALL_HANDLER.get() {
        if let Ok(guard) = store.lock() {
            if let Some(ref f) = *guard {
                f();
            }
        }
    }
}

#[allow(dead_code)]
pub fn push_pending_intent(intent: AndroidIncomingIntent) {
    if intent.value.trim().is_empty() {
        return;
    }
    let store = PENDING_INTENTS.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut guard) = store.lock() {
        let already = guard
            .iter()
            .any(|item| item.kind == intent.kind && item.value == intent.value);
        if !already {
            guard.push(intent);
        }
    }
}

pub fn take_rust_pending_intents() -> Vec<AndroidIncomingIntent> {
    let Some(store) = PENDING_INTENTS.get() else {
        return Vec::new();
    };
    if let Ok(mut guard) = store.lock() {
        std::mem::take(&mut *guard)
    } else {
        Vec::new()
    }
}

#[cfg(target_os = "android")]
mod platform {
    use super::*;
    use tauri::{
        ipc::{Channel, InvokeResponseBody},
        plugin::PluginHandle,
        AppHandle, Emitter, Manager, Runtime,
    };

    pub struct AndroidBridgeState<R: Runtime> {
        pub handle: PluginHandle<R>,
    }

    static ACTIVE_EVENT_CHANNEL: OnceLock<Mutex<Option<Channel<InvokeResponseBody>>>> =
        OnceLock::new();

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ConsumeIntentsResponse {
        #[serde(default)]
        intents: Vec<AndroidIncomingIntent>,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct MessageResponse {
        message: String,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ClipboardResponse {
        text: String,
    }

    pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
        tauri::plugin::Builder::new("dc-android")
            .setup(|app, api| {
                let handle = api.register_android_plugin(
                    "com.moesstudio.downloadcepat",
                    "DownloadBridgePlugin",
                )?;
                app.manage(AndroidBridgeState { handle });

                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) = register_event_channel(&app_handle).await {
                        log::warn!("Failed registering Android event channel: {}", err);
                    }
                });
                Ok(())
            })
            .build()
    }

    async fn register_event_channel<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
        let Some(state) = app.try_state::<AndroidBridgeState<R>>() else {
            return Err("AndroidBridgeState not initialized".to_string());
        };
        let app_for_cb = app.clone();
        let channel = Channel::new(move |body: InvokeResponseBody| {
            let json_val: Option<serde_json::Value> = match body {
                InvokeResponseBody::Json(raw) => serde_json::from_str(&raw).ok(),
                InvokeResponseBody::Raw(bytes) => serde_json::from_slice(&bytes).ok(),
            };
            if let Some(val) = json_val {
                match val.get("event").and_then(|v| v.as_str()) {
                    Some("incoming_intent") => {
                        let kind = val
                            .get("kind")
                            .and_then(|v| v.as_str())
                            .unwrap_or("url")
                            .to_string();
                        let value = val
                            .get("value")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if !value.is_empty() {
                            let intent = AndroidIncomingIntent { kind, value };
                            push_pending_intent(intent.clone());
                            let _ = app_for_cb.emit("android-incoming-intent", intent);
                        }
                    }
                    Some("notification_action") => {
                        let action = val
                            .get("action")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if action == "pause_all" {
                            trigger_pause_all_handler();
                        }
                        let _ = app_for_cb.emit(
                            "android-notification-action",
                            serde_json::json!({ "action": action }),
                        );
                    }
                    _ => {}
                }
            }
            Ok(())
        });

        let store = ACTIVE_EVENT_CHANNEL.get_or_init(|| Mutex::new(None));
        if let Ok(mut guard) = store.lock() {
            *guard = Some(channel.clone());
        }

        state
            .handle
            .run_mobile_plugin_async::<serde_json::Value>(
                "registerEventChannel",
                serde_json::json!({ "channel": channel }),
            )
            .await
            .map(|_| ())
            .map_err(|e| format!("registerEventChannel failed: {}", e))
    }

    pub async fn update_foreground_service<R: Runtime>(
        app: &AppHandle<R>,
        active_count: usize,
        total_speed_bps: u64,
        overall_progress_pct: f64,
        title: &str,
    ) -> Result<(), String> {
        let Some(state) = app.try_state::<AndroidBridgeState<R>>() else {
            return Ok(());
        };
        state
            .handle
            .run_mobile_plugin_async::<serde_json::Value>(
                "updateForegroundService",
                serde_json::json!({
                    "activeCount": active_count,
                    "totalSpeedBps": total_speed_bps,
                    "progressPercent": overall_progress_pct.clamp(0.0, 100.0),
                    "title": title,
                }),
            )
            .await
            .map(|_| ())
            .map_err(|e| format!("updateForegroundService failed: {}", e))
    }

    pub async fn export_to_public_downloads<R: Runtime>(
        app: &AppHandle<R>,
        staging_path: &std::path::Path,
        relative_subpath: &str,
        mime_type: Option<&str>,
    ) -> Result<AndroidExportResult, String> {
        let state = app
            .try_state::<AndroidBridgeState<R>>()
            .ok_or_else(|| "AndroidBridgeState unavailable".to_string())?;
        state
            .handle
            .run_mobile_plugin_async::<AndroidExportResult>(
                "exportToDownloads",
                serde_json::json!({
                    "stagingPath": staging_path.display().to_string(),
                    "relativeSubpath": relative_subpath,
                    "mimeType": mime_type,
                }),
            )
            .await
            .map_err(|e| format!("exportToDownloads failed: {}", e))
    }

    pub async fn open_content_or_file<R: Runtime>(
        app: &AppHandle<R>,
        path_or_uri: &str,
        folder_mode: bool,
    ) -> Result<String, String> {
        let state = app
            .try_state::<AndroidBridgeState<R>>()
            .ok_or_else(|| "AndroidBridgeState unavailable".to_string())?;
        let resp: MessageResponse = state
            .handle
            .run_mobile_plugin_async(
                "openFile",
                serde_json::json!({
                    "path": path_or_uri,
                    "folderMode": folder_mode,
                }),
            )
            .await
            .map_err(|e| format!("{}", e))?;
        Ok(resp.message)
    }

    pub async fn share_content_or_file<R: Runtime>(
        app: &AppHandle<R>,
        path_or_uri: &str,
    ) -> Result<String, String> {
        let state = app
            .try_state::<AndroidBridgeState<R>>()
            .ok_or_else(|| "AndroidBridgeState unavailable".to_string())?;
        let resp: MessageResponse = state
            .handle
            .run_mobile_plugin_async(
                "shareFile",
                serde_json::json!({
                    "path": path_or_uri,
                }),
            )
            .await
            .map_err(|e| format!("{}", e))?;
        Ok(resp.message)
    }

    pub async fn open_external_url<R: Runtime>(
        app: &AppHandle<R>,
        url: &str,
    ) -> Result<String, String> {
        let state = app
            .try_state::<AndroidBridgeState<R>>()
            .ok_or_else(|| "AndroidBridgeState unavailable".to_string())?;
        let resp: MessageResponse = state
            .handle
            .run_mobile_plugin_async(
                "openExternalUrl",
                serde_json::json!({ "url": url }),
            )
            .await
            .map_err(|e| format!("{}", e))?;
        Ok(resp.message)
    }

    pub async fn install_apk_update<R: Runtime>(
        app: &AppHandle<R>,
        apk_path: &std::path::Path,
    ) -> Result<String, String> {
        let state = app
            .try_state::<AndroidBridgeState<R>>()
            .ok_or_else(|| "AndroidBridgeState unavailable".to_string())?;
        let resp: MessageResponse = state
            .handle
            .run_mobile_plugin_async(
                "installApk",
                serde_json::json!({ "apkPath": apk_path.display().to_string() }),
            )
            .await
            .map_err(|e| format!("{}", e))?;
        Ok(resp.message)
    }

    pub async fn compute_content_checksums<R: Runtime>(
        app: &AppHandle<R>,
        path_or_uri: &str,
    ) -> Result<core_engine::model::FileChecksums, String> {
        let state = app
            .try_state::<AndroidBridgeState<R>>()
            .ok_or_else(|| "AndroidBridgeState unavailable".to_string())?;
        state
            .handle
            .run_mobile_plugin_async::<core_engine::model::FileChecksums>(
                "computeChecksums",
                serde_json::json!({ "path": path_or_uri }),
            )
            .await
            .map_err(|e| format!("{}", e))
    }

    pub async fn consume_pending_intents<R: Runtime>(
        app: &AppHandle<R>,
    ) -> Vec<AndroidIncomingIntent> {
        let mut combined = take_rust_pending_intents();
        if let Some(state) = app.try_state::<AndroidBridgeState<R>>() {
            if let Ok(resp) = state
                .handle
                .run_mobile_plugin_async::<ConsumeIntentsResponse>(
                    "consumePendingIntents",
                    serde_json::json!({}),
                )
                .await
            {
                for item in resp.intents {
                    if !item.value.trim().is_empty()
                        && !combined
                            .iter()
                            .any(|x| x.kind == item.kind && x.value == item.value)
                    {
                        combined.push(item);
                    }
                }
            }
        }
        combined
    }

    pub async fn read_clipboard_text<R: Runtime>(app: &AppHandle<R>) -> Result<String, String> {
        let Some(state) = app.try_state::<AndroidBridgeState<R>>() else {
            return Ok(String::new());
        };
        let resp: ClipboardResponse = state
            .handle
            .run_mobile_plugin_async("readClipboardText", serde_json::json!({}))
            .await
            .map_err(|e| format!("{}", e))?;
        Ok(resp.text)
    }
}

#[cfg(not(target_os = "android"))]
#[allow(dead_code)]
mod platform {
    use super::*;
    use tauri::{AppHandle, Runtime};

    pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
        tauri::plugin::Builder::new("dc-android").build()
    }

    pub async fn update_foreground_service<R: Runtime>(
        _app: &AppHandle<R>,
        _active_count: usize,
        _total_speed_bps: u64,
        _overall_progress_pct: f64,
        _title: &str,
    ) -> Result<(), String> {
        Ok(())
    }

    pub async fn export_to_public_downloads<R: Runtime>(
        _app: &AppHandle<R>,
        staging_path: &std::path::Path,
        relative_subpath: &str,
        _mime_type: Option<&str>,
    ) -> Result<AndroidExportResult, String> {
        Ok(AndroidExportResult {
            content_uri: String::new(),
            file_path: Some(staging_path.display().to_string()),
            display_path: relative_subpath.to_string(),
        })
    }

    pub async fn open_content_or_file<R: Runtime>(
        _app: &AppHandle<R>,
        _path_or_uri: &str,
        _folder_mode: bool,
    ) -> Result<String, String> {
        Err("Not running on Android".to_string())
    }

    pub async fn share_content_or_file<R: Runtime>(
        _app: &AppHandle<R>,
        _path_or_uri: &str,
    ) -> Result<String, String> {
        Err("Share is only supported on Android".to_string())
    }

    pub async fn open_external_url<R: Runtime>(
        _app: &AppHandle<R>,
        _url: &str,
    ) -> Result<String, String> {
        Err("Not running on Android".to_string())
    }

    pub async fn install_apk_update<R: Runtime>(
        _app: &AppHandle<R>,
        _apk_path: &std::path::Path,
    ) -> Result<String, String> {
        Err("APK update is only supported on Android".to_string())
    }

    pub async fn compute_content_checksums<R: Runtime>(
        _app: &AppHandle<R>,
        _path_or_uri: &str,
    ) -> Result<core_engine::model::FileChecksums, String> {
        Err("Content checksum helper is only used on Android".to_string())
    }

    pub async fn consume_pending_intents<R: Runtime>(
        _app: &AppHandle<R>,
    ) -> Vec<AndroidIncomingIntent> {
        take_rust_pending_intents()
    }

    pub async fn read_clipboard_text<R: Runtime>(_app: &AppHandle<R>) -> Result<String, String> {
        Ok(String::new())
    }
}

#[allow(unused_imports)]
pub use platform::{
    compute_content_checksums, consume_pending_intents, export_to_public_downloads, init,
    install_apk_update, open_content_or_file, open_external_url, read_clipboard_text,
    share_content_or_file, update_foreground_service,
};
