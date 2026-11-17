use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{Emitter, Manager};
use tokio::sync::Notify;
use core_engine::model::{InspectResult, MediaInfo, VideoFormat};

#[derive(Clone)]
enum ActiveControl {
    Http(core_engine::DownloadController),
    YtDlp {
        paused: Arc<std::sync::atomic::AtomicBool>,
        canceled: Arc<std::sync::atomic::AtomicBool>,
        stop_notify: Arc<Notify>,
    },
}

impl ActiveControl {
    fn pause(&self) {
        match self {
            ActiveControl::Http(c) => c.pause(),
            ActiveControl::YtDlp {
                paused,
                stop_notify,
                ..
            } => {
                paused.store(true, std::sync::atomic::Ordering::SeqCst);
                stop_notify.notify_waiters();
            }
        }
    }

    fn cancel(&self) {
        match self {
            ActiveControl::Http(c) => c.cancel(),
            ActiveControl::YtDlp {
                paused,
                canceled,
                stop_notify,
            } => {
                paused.store(false, std::sync::atomic::Ordering::SeqCst);
                canceled.store(true, std::sync::atomic::Ordering::SeqCst);
                stop_notify.notify_waiters();
            }
        }
    }

    fn set_speed_limit(&self, bytes_per_sec: u64) {
        if let ActiveControl::Http(c) = self {
            c.set_speed_limit(bytes_per_sec);
        }
    }
}

fn active_downloads() -> &'static Mutex<HashMap<String, ActiveControl>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, ActiveControl>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn task_targets() -> &'static Mutex<HashMap<String, PathBuf>> {
    static TARGETS: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    TARGETS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn active_ui_language() -> &'static Mutex<String> {
    static LANG: OnceLock<Mutex<String>> = OnceLock::new();
    LANG.get_or_init(|| Mutex::new("en".to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RuntimeDownloadSettings {
    pub default_connections: usize,
    pub max_tasks: usize,
    pub speed_limit_kbs: u64,
    pub proxy_url: Option<String>,
    pub video_save_dir: String,
    pub music_save_dir: String,
    pub torrents_save_dir: String,
}

impl Default for RuntimeDownloadSettings {
    fn default() -> Self {
        Self {
            default_connections: 8,
            max_tasks: 2,
            speed_limit_kbs: 0,
            proxy_url: None,
            video_save_dir: "~/Downloads/Video".to_string(),
            music_save_dir: "~/Downloads/Music".to_string(),
            torrents_save_dir: "~/Downloads/Torrents".to_string(),
        }
    }
}

fn normalize_runtime_settings(mut s: RuntimeDownloadSettings) -> RuntimeDownloadSettings {
    s.default_connections = s.default_connections.clamp(1, 32);
    s.max_tasks = s.max_tasks.clamp(1, 10);
    s.proxy_url = s
        .proxy_url
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    s.video_save_dir = if s.video_save_dir.trim().is_empty() {
        "~/Downloads/Video".to_string()
    } else {
        s.video_save_dir.trim().to_string()
    };
    s.music_save_dir = if s.music_save_dir.trim().is_empty() {
        "~/Downloads/Music".to_string()
    } else {
        s.music_save_dir.trim().to_string()
    };
    s.torrents_save_dir = if s.torrents_save_dir.trim().is_empty() {
        "~/Downloads/Torrents".to_string()
    } else {
        s.torrents_save_dir.trim().to_string()
    };
    s
}

fn resolve_save_dir_for_category<'a>(
    rt: &'a RuntimeDownloadSettings,
    category: &str,
) -> &'a str {
    match category.trim().to_ascii_lowercase().as_str() {
        "video" => rt.video_save_dir.as_str(),
        "music" => rt.music_save_dir.as_str(),
        "torrents" => rt.torrents_save_dir.as_str(),
        "compressed" => "~/Downloads/Compressed",
        "documents" => "~/Downloads/Documents",
        "programs" => "~/Downloads/Programs",
        _ => "~/Downloads",
    }
}

fn runtime_settings_file_path() -> Option<PathBuf> {
    user_home_dir().map(|h| h.join(".download-cepat").join("runtime_settings.json"))
}

fn load_runtime_settings_from_path(path: &Path) -> RuntimeDownloadSettings {
    match std::fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str::<RuntimeDownloadSettings>(&raw) {
            Ok(parsed) => normalize_runtime_settings(parsed),
            Err(_) => RuntimeDownloadSettings::default(),
        },
        Err(_) => RuntimeDownloadSettings::default(),
    }
}

fn save_runtime_settings_to_path(
    path: &Path,
    settings: &RuntimeDownloadSettings,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create runtime settings directory: {}", e))?;
    }
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize runtime settings: {}", e))?;
    std::fs::write(path, json)
        .map_err(|e| format!("Failed to write runtime settings: {}", e))
}

fn runtime_settings_store() -> &'static Mutex<Option<RuntimeDownloadSettings>> {
    static STORE: OnceLock<Mutex<Option<RuntimeDownloadSettings>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(None))
}

fn get_runtime_settings() -> RuntimeDownloadSettings {
    if let Ok(mut guard) = runtime_settings_store().lock() {
        if let Some(ref cached) = *guard {
            return cached.clone();
        }
        let loaded = runtime_settings_file_path()
            .map(|p| load_runtime_settings_from_path(&p))
            .unwrap_or_default();
        *guard = Some(loaded.clone());
        return loaded;
    }
    RuntimeDownloadSettings::default()
}

#[tauri::command]
fn sync_runtime_settings(settings: RuntimeDownloadSettings) -> RuntimeDownloadSettings {
    let normalized = normalize_runtime_settings(settings);
    if let Ok(mut guard) = runtime_settings_store().lock() {
        *guard = Some(normalized.clone());
    }
    if let Some(path) = runtime_settings_file_path() {
        if let Err(e) = save_runtime_settings_to_path(&path, &normalized) {
            log::warn!("Failed to write runtime_settings.json: {}", e);
        }
    }
    normalized
}

fn join_save_dir_and_file(save_dir: &str, file_name: &str) -> String {
    let dir = save_dir.trim();
    if dir.is_empty() {
        return file_name.to_string();
    }
    let sep = if dir.contains('\\') && !dir.starts_with("~/") && !dir.starts_with('/') {
        '\\'
    } else {
        '/'
    };
    let trimmed = dir.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() && (dir == "/" || dir == "\\") {
        return format!("{}{}", dir, file_name);
    }
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        return format!("{}\\{}", trimmed, file_name);
    }
    format!("{}{}{}", trimmed, sep, file_name)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub app_name: String,
    pub version: String,
    pub author: String,
    pub publisher: String,
    pub copyright: String,
    pub license: String,
    pub repository: String,
    pub engine: String,
    pub os: String,
    pub arch: String,
    pub build_profile: String,
    pub tauri_version: String,
}

fn build_info_payload(app_name: Option<String>) -> BuildInfo {
    BuildInfo {
        app_name: app_name.unwrap_or_else(|| "Download Cepat".to_string()),
        version: env!("CARGO_PKG_VERSION").to_string(),
        author: "Ujang Mustamar".to_string(),
        publisher: "Ujang Mustamar".to_string(),
        copyright: "Copyright © 2026 Ujang Mustamar".to_string(),
        license: env!("CARGO_PKG_LICENSE").to_string(),
        repository: env!("CARGO_PKG_REPOSITORY").to_string(),
        engine: "Download Cepat Core Engine (Rust Multi-Chunk HTTP/FTP/Torrent)".to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        build_profile: if cfg!(debug_assertions) {
            "debug".to_string()
        } else {
            "release".to_string()
        },
        tauri_version: tauri::VERSION.to_string(),
    }
}

#[tauri::command]
fn get_build_info(app: tauri::AppHandle) -> BuildInfo {
    build_info_payload(Some(app.package_info().name.clone()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrayLabels {
    show: &'static str,
    add: &'static str,
    pause_all: &'static str,
    quit: &'static str,
    tooltip: &'static str,
}

fn normalize_tray_lang(lang: &str) -> &'static str {
    match lang.trim().to_ascii_lowercase().as_str() {
        "id" => "id",
        "ms" => "ms",
        "zh" | "zh-cn" | "zh-hans" => "zh",
        "ja" => "ja",
        "ko" => "ko",
        "es" => "es",
        "fr" => "fr",
        "de" => "de",
        "pt" | "pt-br" => "pt",
        "ru" => "ru",
        "ar" => "ar",
        "hi" => "hi",
        "vi" => "vi",
        "th" => "th",
        "tr" => "tr",
        _ => "en",
    }
}

fn tray_labels_for_lang(lang: &str) -> TrayLabels {
    match normalize_tray_lang(lang) {
        "id" => TrayLabels {
            show: "Buka Download Cepat",
            add: "Tambah URL Baru...",
            pause_all: "Jeda Semua Unduhan",
            quit: "Keluar",
            tooltip: "Download Cepat — Klik untuk Membuka",
        },
        "ms" => TrayLabels {
            show: "Buka Download Cepat",
            add: "Tambah URL Baharu...",
            pause_all: "Jeda Semua Muat Turun",
            quit: "Keluar",
            tooltip: "Download Cepat — Klik untuk Membuka",
        },
        "zh" => TrayLabels {
            show: "打开 Download Cepat",
            add: "添加新 URL...",
            pause_all: "暂停全部下载",
            quit: "退出",
            tooltip: "Download Cepat — 点击打开",
        },
        "ja" => TrayLabels {
            show: "Download Cepat を開く",
            add: "新しい URL を追加...",
            pause_all: "すべて一時停止",
            quit: "終了",
            tooltip: "Download Cepat — クリックして開く",
        },
        "ko" => TrayLabels {
            show: "Download Cepat 열기",
            add: "새 URL 추가...",
            pause_all: "모든 다운로드 일시중지",
            quit: "종료",
            tooltip: "Download Cepat — 클릭하여 열기",
        },
        "es" => TrayLabels {
            show: "Abrir Download Cepat",
            add: "Añadir nueva URL...",
            pause_all: "Pausar todas las descargas",
            quit: "Salir",
            tooltip: "Download Cepat — Haz clic para abrir",
        },
        "fr" => TrayLabels {
            show: "Ouvrir Download Cepat",
            add: "Ajouter une URL...",
            pause_all: "Tout mettre en pause",
            quit: "Quitter",
            tooltip: "Download Cepat — Cliquer pour ouvrir",
        },
        "de" => TrayLabels {
            show: "Download Cepat öffnen",
            add: "Neue URL hinzufügen...",
            pause_all: "Alle Downloads pausieren",
            quit: "Beenden",
            tooltip: "Download Cepat — Zum Öffnen klicken",
        },
        "pt" => TrayLabels {
            show: "Abrir Download Cepat",
            add: "Adicionar nova URL...",
            pause_all: "Pausar todos os downloads",
            quit: "Sair",
            tooltip: "Download Cepat — Clique para abrir",
        },
        "ru" => TrayLabels {
            show: "Открыть Download Cepat",
            add: "Добавить URL...",
            pause_all: "Приостановить все",
            quit: "Выход",
            tooltip: "Download Cepat — Нажмите, чтобы открыть",
        },
        "ar" => TrayLabels {
            show: "فتح Download Cepat",
            add: "إضافة رابط جديد...",
            pause_all: "إيقاف جميع التنزيلات مؤقتاً",
            quit: "إنهاء",
            tooltip: "Download Cepat — انقر للفتح",
        },
        "hi" => TrayLabels {
            show: "Download Cepat खोलें",
            add: "नया URL जोड़ें...",
            pause_all: "सभी डाउनलोड रोकें",
            quit: "बाहर निकलें",
            tooltip: "Download Cepat — खोलने के लिए क्लिक करें",
        },
        "vi" => TrayLabels {
            show: "Mở Download Cepat",
            add: "Thêm URL mới...",
            pause_all: "Tạm dừng tất cả",
            quit: "Thoát",
            tooltip: "Download Cepat — Nhấp để mở",
        },
        "th" => TrayLabels {
            show: "เปิด Download Cepat",
            add: "เพิ่ม URL ใหม่...",
            pause_all: "หยุดดาวน์โหลดทั้งหมดชั่วคราว",
            quit: "ออก",
            tooltip: "Download Cepat — คลิกเพื่อเปิด",
        },
        "tr" => TrayLabels {
            show: "Download Cepat'ı Aç",
            add: "Yeni URL Ekle...",
            pause_all: "Tüm İndirmeleri Duraklat",
            quit: "Çıkış",
            tooltip: "Download Cepat — Açmak için tıklayın",
        },
        _ => TrayLabels {
            show: "Open Download Cepat",
            add: "Add New URL...",
            pause_all: "Pause All Downloads",
            quit: "Quit",
            tooltip: "Download Cepat — Click to Open",
        },
    }
}

struct TrayMenuState {
    show_item: tauri::menu::MenuItem<tauri::Wry>,
    add_item: tauri::menu::MenuItem<tauri::Wry>,
    pause_item: tauri::menu::MenuItem<tauri::Wry>,
    quit_item: tauri::menu::MenuItem<tauri::Wry>,
}

#[tauri::command]
fn set_tray_language(app: tauri::AppHandle, lang: String) -> Result<(), String> {
    let normalized = normalize_tray_lang(&lang);
    if let Ok(mut guard) = active_ui_language().lock() {
        *guard = normalized.to_string();
    }
    let labels = tray_labels_for_lang(normalized);
    if let Some(state) = app.try_state::<TrayMenuState>() {
        let _ = state.show_item.set_text(labels.show);
        let _ = state.add_item.set_text(labels.add);
        let _ = state.pause_item.set_text(labels.pause_all);
        let _ = state.quit_item.set_text(labels.quit);
    }
    if let Some(tray) = app.tray_by_id("dc-tray") {
        let _ = tray.set_tooltip(Some(labels.tooltip));
    }
    Ok(())
}

fn register_task_target(task_id: &str, target: &Path) {
    if let Ok(mut map) = task_targets().lock() {
        map.insert(task_id.to_string(), target.to_path_buf());
    }
}

fn remove_task_target(task_id: &str) -> Option<PathBuf> {
    task_targets()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(task_id))
}

fn is_ytdlp_stream_fragment_file(file_name: &str, stem: &str) -> bool {
    let prefix = format!("{}.f", stem);
    let Some(rest) = file_name.strip_prefix(&prefix) else {
        return false;
    };
    let Some(format_id) = rest.split('.').next() else {
        return false;
    };
    !format_id.is_empty() && format_id.chars().all(|c| c.is_ascii_digit())
}

fn try_remove_file(path: &Path) -> bool {
    path.is_file() && std::fs::remove_file(path).is_ok()
}

fn cleanup_download_artifacts(target: &Path) -> usize {
    let mut removed_count = 0usize;

    let torrent_meta = core_engine::torrent::torrent_checkpoint_path(target);
    if target.is_dir() && torrent_meta.is_file() {
        if std::fs::remove_dir_all(target).is_ok() {
            removed_count += 1;
        }
    }

    let temp_streams: Vec<PathBuf> = ["temp_stream.mp4", "temp_stream.m4a"]
        .into_iter()
        .map(|ext| target.with_extension(ext))
        .filter(|p| p != target)
        .collect();

    let mut direct_candidates = vec![
        target.to_path_buf(),
        core_engine::downloader::checkpoint_path_for(target),
        torrent_meta,
        PathBuf::from(format!("{}.part", target.display())),
        PathBuf::from(format!("{}.ytdl", target.display())),
    ];
    for temp_stream in &temp_streams {
        direct_candidates.push(temp_stream.clone());
        direct_candidates.push(PathBuf::from(format!("{}.part", temp_stream.display())));
        direct_candidates.push(PathBuf::from(format!("{}.ytdl", temp_stream.display())));
    }
    for candidate in &direct_candidates {
        if try_remove_file(candidate) {
            removed_count += 1;
        }
    }

    if let (Some(parent), Some(target_name)) = (
        target.parent(),
        target.file_name().and_then(|n| n.to_str()),
    ) {
        let stem = target
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();

        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
                    continue;
                };
                let target_frag = name.starts_with(&format!("{}.part", target_name))
                    || name.starts_with(&format!("{}.ytdl", target_name));
                let temp_frag = temp_streams.iter().any(|ts| {
                    let temp_name = ts.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                    !temp_name.is_empty()
                        && (name.starts_with(&format!("{}.part", temp_name))
                            || name.starts_with(&format!("{}.ytdl", temp_name)))
                });
                let stream_split = (!stem.is_empty() && is_ytdlp_stream_fragment_file(&name, stem))
                    || temp_streams.iter().any(|ts| {
                        let temp_stem = ts.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
                        !temp_stem.is_empty() && is_ytdlp_stream_fragment_file(&name, temp_stem)
                    });
                let split_temp = !target_name.is_empty()
                    && (name.starts_with(&format!("{}.vtmp.", target_name))
                        || name.starts_with(&format!("{}.atmp.", target_name))
                        || name.starts_with(&format!("{}.mux.", target_name)));

                if target_frag || temp_frag || stream_split || split_temp {
                    if try_remove_file(&path) {
                        removed_count += 1;
                    }
                }
            }
        }
    }

    removed_count
}

fn register_active_download(task_id: &str, ctrl: ActiveControl) {
    if let Ok(mut map) = active_downloads().lock() {
        map.insert(task_id.to_string(), ctrl);
    }
}

fn unregister_active_download(task_id: &str) {
    if let Ok(mut map) = active_downloads().lock() {
        map.remove(task_id);
    }
}

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn hide_window_std(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn hide_window_tokio(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn sanitize_cli_header(val: Option<&str>) -> Option<String> {
    val.map(|s| s.replace(['\r', '\n', '\0'], " ").trim().to_string())
        .filter(|s| !s.is_empty())
}

fn apply_ytdlp_headers(
    cmd: &mut tokio::process::Command,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
) {
    if let Some(ua) = sanitize_cli_header(user_agent) {
        cmd.arg("--user-agent").arg(ua);
    }
    if let Some(rf) = sanitize_cli_header(referer) {
        cmd.arg("--referer").arg(rf);
    }
    if let Some(ck) = sanitize_cli_header(cookie) {
        cmd.arg("--add-header").arg(format!("Cookie: {}", ck));
    }
}

fn apply_ytdlp_network_options(
    cmd: &mut tokio::process::Command,
    proxy_url: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
) {
    if let Some(proxy) = sanitize_cli_header(proxy_url) {
        cmd.arg("--proxy").arg(proxy);
    }
    if let Some(user) = sanitize_cli_header(auth_username) {
        cmd.arg("--username").arg(user);
        if let Some(pass) = sanitize_cli_header(auth_password) {
            cmd.arg("--password").arg(pass);
        }
    }
}

fn find_executable_in_specific_dir(bin_names: &[&str], dir: &Path) -> Option<PathBuf> {
    let suffix = std::env::consts::EXE_SUFFIX;
    for name in bin_names {
        let candidate = if !suffix.is_empty() && !name.ends_with(suffix) {
            dir.join(format!("{}{}", name, suffix))
        } else {
            dir.join(name)
        };
        if candidate.is_file()
            && std::fs::metadata(&candidate)
                .map(|m| m.len() > 0)
                .unwrap_or(false)
        {
            return Some(candidate);
        }
    }
    None
}

fn find_executable_in_dirs(bin_names: &[&str], extra_dirs: &[PathBuf]) -> Option<PathBuf> {
    let mut search_dirs: Vec<PathBuf> = extra_dirs.to_vec();
    if let Some(path_env) = std::env::var_os("PATH") {
        search_dirs.extend(std::env::split_paths(&path_env));
    }

    for dir in search_dirs {
        if let Some(found) = find_executable_in_specific_dir(bin_names, &dir) {
            return Some(found);
        }
    }
    None
}

fn user_home_dir() -> Option<PathBuf> {
    dirs::home_dir()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

fn user_bin_dir() -> Option<PathBuf> {
    user_home_dir().map(|h| h.join(".download-cepat").join("bin"))
}

fn ytdlp_onedir_dir() -> Option<PathBuf> {
    user_bin_dir().map(|b| b.join("yt-dlp-dir"))
}

fn get_ytdlp_onedir_bin() -> Option<PathBuf> {
    let onedir = ytdlp_onedir_dir()?;
    if !onedir.is_dir() || !onedir.join("_internal").is_dir() {
        return None;
    }
    const ONEDIR_BIN_NAMES: &[&str] = &[
        "yt-dlp",
        "yt-dlp_linux",
        "yt-dlp_linux_aarch64",
        "yt-dlp_linux_armv7l",
        "yt-dlp_arm64",
        "yt-dlp_x86",
    ];
    find_executable_in_specific_dir(ONEDIR_BIN_NAMES, &onedir)
}

fn default_tool_dirs() -> Vec<PathBuf> {
    let mut dirs_list = Vec::new();
    if let Some(home) = user_home_dir() {
        let bin_dir = home.join(".download-cepat").join("bin");
        dirs_list.push(bin_dir.join("yt-dlp-dir"));
        dirs_list.push(bin_dir);
        dirs_list.push(home.join(".download-cepat").join("venv").join("bin"));
        #[cfg(target_os = "windows")]
        dirs_list.push(home.join(".download-cepat").join("venv").join("Scripts"));
        dirs_list.push(home.join(".local").join("bin"));
        dirs_list.push(home.join(".deno").join("bin"));
        dirs_list.push(home.join(".bun").join("bin"));
        #[cfg(target_os = "windows")]
        dirs_list.push(home.join("scoop").join("shims"));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            dirs_list.push(exe_dir.to_path_buf());
            dirs_list.push(exe_dir.join("bin"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        dirs_list.push(PathBuf::from("/opt/homebrew/bin"));
        dirs_list.push(PathBuf::from("/usr/local/bin"));
        dirs_list.push(PathBuf::from("/usr/bin"));
    }

    #[cfg(target_os = "linux")]
    {
        dirs_list.push(PathBuf::from("/usr/local/bin"));
        dirs_list.push(PathBuf::from("/usr/bin"));
        dirs_list.push(PathBuf::from("/snap/bin"));
        dirs_list.push(PathBuf::from("/home/linuxbrew/.linuxbrew/bin"));
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            dirs_list.push(local_app_data.join("Download Cepat").join("bin"));
            dirs_list.push(local_app_data.join("Programs").join("yt-dlp"));
            dirs_list.push(local_app_data.join("Microsoft").join("WinGet").join("Links"));
        }
        if let Some(program_data) = std::env::var_os("ProgramData").map(PathBuf::from) {
            dirs_list.push(program_data.join("chocolatey").join("bin"));
        }
        if let Some(program_files) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
            dirs_list.push(program_files.join("yt-dlp"));
            dirs_list.push(program_files.join("ffmpeg").join("bin"));
            dirs_list.push(program_files.join("nodejs"));
        }
        dirs_list.push(PathBuf::from(r"C:\yt-dlp"));
        dirs_list.push(PathBuf::from(r"C:\ffmpeg\bin"));
    }

    dirs_list
}

fn is_supported_node_bin(path: &Path) -> bool {
    static NODE_SUPPORT_CACHE: OnceLock<Mutex<HashMap<PathBuf, bool>>> = OnceLock::new();
    let cache = NODE_SUPPORT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(&supported) = guard.get(path) {
            return supported;
        }
    }

    let mut cmd = std::process::Command::new(path);
    hide_window_std(&mut cmd);
    cmd.arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let supported = match cmd.output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            core_engine::video_runtime::parse_node_major_version(&text)
                .map(|major| major >= 22)
                .unwrap_or(false)
        }
        _ => false,
    };

    if let Ok(mut guard) = cache.lock() {
        guard.insert(path.to_path_buf(), supported);
    }
    supported
}

fn detect_js_runtime_arg() -> Option<String> {
    let tool_dirs = default_tool_dirs();
    if let Some(path) = find_executable_in_dirs(&["deno"], &tool_dirs) {
        return Some(format!("deno:{}", path.display()));
    }
    if let Some(path) = find_executable_in_dirs(&["qjs", "quickjs"], &tool_dirs) {
        return Some(format!("quickjs:{}", path.display()));
    }
    if let Some(path) = find_executable_in_dirs(&["node"], &tool_dirs) {
        if is_supported_node_bin(&path) {
            return Some(format!("node:{}", path.display()));
        }
    }
    if let Some(path) = find_executable_in_dirs(&["bun"], &tool_dirs) {
        return Some(format!("bun:{}", path.display()));
    }
    None
}

fn apply_ytdlp_runtime_env(cmd: &mut tokio::process::Command) -> bool {
    let mut path_entries = default_tool_dirs();
    if let Some(existing) = std::env::var_os("PATH") {
        for p in std::env::split_paths(&existing) {
            if !path_entries.contains(&p) {
                path_entries.push(p);
            }
        }
    }
    if let Ok(joined) = std::env::join_paths(path_entries) {
        cmd.env("PATH", joined);
    }

    if let Some(js_rt) = detect_js_runtime_arg() {
        cmd.arg("--js-runtimes").arg(js_rt);
        true
    } else {
        false
    }
}

fn get_ytdlp_bin() -> Option<String> {
    if let Some(onedir_bin) = get_ytdlp_onedir_bin() {
        return Some(onedir_bin.display().to_string());
    }
    find_executable_in_dirs(
        &["yt-dlp", "yt-dlp_macos", "yt-dlp_linux", "yt-dlp_linux_aarch64"],
        &default_tool_dirs(),
    )
    .map(|p| p.display().to_string())
}

fn ytdlp_download_url_for(os: &str, arch: &str) -> &'static str {
    match (os, arch) {
        ("windows", _) => "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe",
        ("macos", _) => "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos",
        ("linux", "aarch64") => {
            "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64"
        }
        ("linux", "x86_64") => {
            "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux"
        }
        _ => "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp",
    }
}

fn set_file_executable_if_unix(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

async fn download_tool_file(
    task_name: &str,
    url: &str,
    tmp_path: &Path,
    min_bytes: u64,
    label: &str,
) -> Result<(), String> {
    let _ = std::fs::remove_file(tmp_path);
    let inspector = core_engine::UrlInspector::new()
        .map_err(|e| format!("Gagal menginisialisasi inspektor {}: {}", label, e))?;
    let inspect_info = inspector
        .inspect(url)
        .await
        .map_err(|e| format!("Komponen {} gagal diakses ({})", label, e))?;

    let downloader = core_engine::Downloader::new()
        .map_err(|e| format!("Gagal menginisialisasi pengunduh {}: {}", label, e))?;
    downloader
        .download(
            task_name,
            &inspect_info.final_url,
            tmp_path,
            inspect_info.file_size,
            inspect_info.supports_range,
            4,
            None,
            None,
        )
        .await
        .map_err(|e| {
            let _ = std::fs::remove_file(tmp_path);
            format!(
                "Komponen {} gagal diunduh otomatis ({}). Pastikan koneksi internet aktif.",
                label, e
            )
        })?;

    let downloaded_len = std::fs::metadata(tmp_path).map(|m| m.len()).unwrap_or(0);
    if downloaded_len < min_bytes {
        let _ = std::fs::remove_file(tmp_path);
        return Err(format!(
            "Berkas {} yang diunduh tidak valid atau terpotong ({} byte).",
            label, downloaded_len
        ));
    }
    Ok(())
}

fn ytdlp_provision_lock() -> &'static tokio::sync::Mutex<()> {
    static PROVISION_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    PROVISION_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn install_ytdlp_onedir() -> Result<String, String> {
    let spec = core_engine::video_runtime::ytdlp_onedir_spec(
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
    .ok_or_else(|| "Paket onedir yt-dlp tidak tersedia untuk arsitektur ini.".to_string())?;

    let bin_dir = user_bin_dir()
        .ok_or_else(|| "Direktori home tidak ditemukan untuk memasang yt-dlp.".to_string())?;
    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("Gagal membuat folder binari ({}): {}", bin_dir.display(), e))?;

    let zip_tmp = bin_dir.join("yt-dlp-onedir.zip.download");
    let staging_dir = bin_dir.join("yt-dlp-dir.staging");
    let dest_dir = bin_dir.join("yt-dlp-dir");
    let backup_dir = bin_dir.join("yt-dlp-dir.old");
    let _ = std::fs::remove_dir_all(&staging_dir);

    log::info!("Mengunduh paket yt-dlp terurai (onedir) dari {}", spec.url);
    download_tool_file("ytdlp-onedir-bootstrap", spec.url, &zip_tmp, 5_000_000, "yt-dlp onedir")
        .await?;

    let zip_for_extract = zip_tmp.clone();
    let staging_for_extract = staging_dir.clone();
    let extract_res = tokio::task::spawn_blocking(move || {
        core_engine::video_runtime::extract_zip_archive(&zip_for_extract, &staging_for_extract)
    })
    .await
    .map_err(|e| format!("Gagal mengekstrak arsip yt-dlp onedir: {}", e))?;
    let _ = std::fs::remove_file(&zip_tmp);
    extract_res?;

    let staged_bin = find_executable_in_specific_dir(spec.bin_names, &staging_dir)
        .filter(|p| std::fs::metadata(p).map(|m| m.len() >= 1_000_000).unwrap_or(false))
        .ok_or_else(|| {
            let _ = std::fs::remove_dir_all(&staging_dir);
            "Binari yt-dlp tidak ditemukan setelah ekstraksi arsip onedir.".to_string()
        })?;
    if !staging_dir.join("_internal").is_dir() {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err("Folder _internal yt-dlp tidak ditemukan setelah ekstraksi.".to_string());
    }
    set_file_executable_if_unix(&staged_bin);

    let _ = std::fs::remove_dir_all(&backup_dir);
    if dest_dir.exists() {
        if std::fs::rename(&dest_dir, &backup_dir).is_err() {
            let _ = std::fs::remove_dir_all(&dest_dir);
        }
    }
    if let Err(e) = std::fs::rename(&staging_dir, &dest_dir) {
        if backup_dir.exists() && !dest_dir.exists() {
            let _ = std::fs::rename(&backup_dir, &dest_dir);
        }
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(format!(
            "Gagal menempatkan paket yt-dlp onedir ke {}: {}",
            dest_dir.display(),
            e
        ));
    }
    let _ = std::fs::remove_dir_all(&backup_dir);

    get_ytdlp_onedir_bin()
        .map(|p| p.display().to_string())
        .ok_or_else(|| "Gagal memverifikasi instalasi yt-dlp onedir.".to_string())
}

async fn install_ytdlp_singlefile() -> Result<String, String> {
    let bin_dir = user_bin_dir()
        .ok_or_else(|| "Direktori home tidak ditemukan untuk memasang yt-dlp.".to_string())?;
    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("Gagal membuat folder binari ({}): {}", bin_dir.display(), e))?;

    let bin_name = format!("yt-dlp{}", std::env::consts::EXE_SUFFIX);
    let dest_path = bin_dir.join(&bin_name);
    let tmp_path = bin_dir.join(format!("{}.download", bin_name));

    let url = ytdlp_download_url_for(std::env::consts::OS, std::env::consts::ARCH);
    log::info!("Mengunduh komponen yt-dlp otomatis dari {}", url);
    download_tool_file("ytdlp-bootstrap", url, &tmp_path, 1_000_000, "yt-dlp").await?;
    set_file_executable_if_unix(&tmp_path);

    if dest_path.exists() {
        let _ = std::fs::remove_file(&dest_path);
    }
    std::fs::rename(&tmp_path, &dest_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("Gagal menempatkan yt-dlp ke {}: {}", dest_path.display(), e)
    })?;

    Ok(dest_path.display().to_string())
}

async fn ensure_ytdlp_bin() -> Result<String, String> {
    if let Some(existing) = get_ytdlp_bin() {
        return Ok(existing);
    }

    let _guard = ytdlp_provision_lock().lock().await;
    if let Some(existing) = get_ytdlp_bin() {
        return Ok(existing);
    }

    if core_engine::video_runtime::ytdlp_onedir_spec(std::env::consts::OS, std::env::consts::ARCH)
        .is_some()
    {
        match install_ytdlp_onedir().await {
            Ok(path) => return Ok(path),
            Err(err) => {
                log::warn!(
                    "Pemasangan paket yt-dlp onedir gagal ({}), beralih ke unduhan binari tunggal.",
                    err
                );
            }
        }
    }

    install_ytdlp_singlefile().await
}

async fn upgrade_ytdlp_to_onedir_if_needed() -> Result<String, String> {
    if let Some(existing) = get_ytdlp_onedir_bin() {
        return Ok(existing.display().to_string());
    }
    if core_engine::video_runtime::ytdlp_onedir_spec(std::env::consts::OS, std::env::consts::ARCH)
        .is_none()
    {
        return ensure_ytdlp_bin().await;
    }

    let _guard = ytdlp_provision_lock().lock().await;
    if let Some(existing) = get_ytdlp_onedir_bin() {
        return Ok(existing.display().to_string());
    }
    install_ytdlp_onedir().await
}

async fn ensure_js_runtime_bin() -> Result<Option<String>, String> {
    if let Some(existing) = detect_js_runtime_arg() {
        return Ok(Some(existing));
    }

    static QJS_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    let _guard = QJS_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;

    if let Some(existing) = detect_js_runtime_arg() {
        return Ok(Some(existing));
    }

    let Some(url) = core_engine::video_runtime::quickjs_download_url_for(
        std::env::consts::OS,
        std::env::consts::ARCH,
    ) else {
        return Ok(None);
    };

    let bin_dir = user_bin_dir()
        .ok_or_else(|| "Direktori home tidak ditemukan untuk memasang QuickJS.".to_string())?;
    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("Gagal membuat folder binari ({}): {}", bin_dir.display(), e))?;

    let bin_name = format!("qjs{}", std::env::consts::EXE_SUFFIX);
    let dest_path = bin_dir.join(&bin_name);
    let tmp_path = bin_dir.join(format!("{}.download", bin_name));

    log::info!("Mengunduh runtime JS ringan (QuickJS-NG) dari {}", url);
    download_tool_file("qjs-bootstrap", url, &tmp_path, 500_000, "QuickJS").await?;
    set_file_executable_if_unix(&tmp_path);

    if dest_path.exists() {
        let _ = std::fs::remove_file(&dest_path);
    }
    std::fs::rename(&tmp_path, &dest_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("Gagal menempatkan QuickJS ke {}: {}", dest_path.display(), e)
    })?;

    Ok(Some(format!("quickjs:{}", dest_path.display())))
}

fn is_valid_ffmpeg_candidate(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.len() == 0 {
        return false;
    }
    if let Some(managed_dir) = user_bin_dir() {
        if path.parent() == Some(managed_dir.as_path()) {
            return meta.len() >= 10_000_000;
        }
    }
    true
}

fn find_valid_ffmpeg_in_dirs(extra_dirs: &[PathBuf]) -> Option<PathBuf> {
    let mut search_dirs: Vec<PathBuf> = extra_dirs.to_vec();
    if let Some(path_env) = std::env::var_os("PATH") {
        search_dirs.extend(std::env::split_paths(&path_env));
    }
    let suffix = std::env::consts::EXE_SUFFIX;
    for dir in search_dirs {
        let candidate = if !suffix.is_empty() {
            dir.join(format!("ffmpeg{}", suffix))
        } else {
            dir.join("ffmpeg")
        };
        if is_valid_ffmpeg_candidate(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn get_ffmpeg_bin() -> Option<PathBuf> {
    find_valid_ffmpeg_in_dirs(&default_tool_dirs())
}

fn ffmpeg_provision_lock() -> &'static tokio::sync::Mutex<()> {
    static FFMPEG_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    FFMPEG_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn ensure_ffmpeg_bin() -> Result<PathBuf, String> {
    if let Some(existing) = get_ffmpeg_bin() {
        return Ok(existing);
    }

    let _guard = ffmpeg_provision_lock().lock().await;
    if let Some(existing) = get_ffmpeg_bin() {
        return Ok(existing);
    }

    let spec = core_engine::video_runtime::ffmpeg_download_spec(
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
    .ok_or_else(|| {
        format!(
            "Paket FFmpeg otomatis tidak tersedia untuk arsitektur {}/{}. Pasang FFmpeg di PATH sistem untuk menggabungkan video HD/4K.",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;

    let bin_dir = user_bin_dir()
        .ok_or_else(|| "Direktori home tidak ditemukan untuk memasang FFmpeg.".to_string())?;
    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("Gagal membuat folder binari ({}): {}", bin_dir.display(), e))?;

    let bin_name = format!("ffmpeg{}", std::env::consts::EXE_SUFFIX);
    let dest_path = bin_dir.join(&bin_name);
    let gz_tmp = bin_dir.join(format!("{}.gz.download", bin_name));
    let extract_tmp = bin_dir.join(format!("{}.download", bin_name));
    let _ = std::fs::remove_file(&gz_tmp);
    let _ = std::fs::remove_file(&extract_tmp);

    log::info!("Mengunduh komponen FFmpeg otomatis dari {}", spec.url);
    download_tool_file(
        "ffmpeg-bootstrap",
        spec.url,
        &gz_tmp,
        spec.min_compressed_bytes,
        "FFmpeg",
    )
    .await?;

    let gz_for_extract = gz_tmp.clone();
    let extract_for_blocking = extract_tmp.clone();
    let extract_res = tokio::task::spawn_blocking(move || {
        core_engine::video_runtime::extract_gzip_file(&gz_for_extract, &extract_for_blocking)
    })
    .await
    .map_err(|e| format!("Gagal mengekstrak arsip FFmpeg: {}", e))?;
    let _ = std::fs::remove_file(&gz_tmp);

    let extracted_len = match extract_res {
        Ok(len) => len,
        Err(e) => {
            let _ = std::fs::remove_file(&extract_tmp);
            return Err(e);
        }
    };
    if extracted_len < spec.min_extracted_bytes {
        let _ = std::fs::remove_file(&extract_tmp);
        return Err(format!(
            "Binari FFmpeg hasil ekstraksi tidak valid atau terpotong ({} byte).",
            extracted_len
        ));
    }

    set_file_executable_if_unix(&extract_tmp);
    if dest_path.exists() {
        let _ = std::fs::remove_file(&dest_path);
    }
    std::fs::rename(&extract_tmp, &dest_path).map_err(|e| {
        let _ = std::fs::remove_file(&extract_tmp);
        format!("Gagal menempatkan FFmpeg ke {}: {}", dest_path.display(), e)
    })?;

    Ok(dest_path)
}

fn parse_requested_height_opt(format_id: Option<&str>) -> Option<u32> {
    let fid = format_id?.trim();
    if let Some(h) = fid.strip_suffix("p_mp4") {
        return h.parse::<u32>().ok().filter(|&v| v > 0);
    }
    if let Some(h) = fid.strip_suffix('p') {
        return h.parse::<u32>().ok().filter(|&v| v > 0);
    }
    None
}

async fn ensure_ytdlp_ready_for_video() -> Result<String, String> {
    let (ytdlp_res, qjs_res) = tokio::join!(ensure_ytdlp_bin(), ensure_js_runtime_bin());
    if let Err(err) = qjs_res {
        log::warn!(
            "Runtime QuickJS belum tersedia ({}), lanjut dengan klien cadangan.",
            err
        );
    }
    ytdlp_res
}

fn extract_url_host(raw_url: &str) -> String {
    let lower = raw_url.trim().to_lowercase();
    let without_scheme = lower
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&lower);
    let authority = without_scheme.split(&['/', '?', '#'][..]).next().unwrap_or("");
    authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority)
        .split(':')
        .next()
        .unwrap_or("")
        .to_string()
}

fn host_matches_domain(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{}", domain))
}

fn is_youtube_url(raw_url: &str) -> bool {
    let host = extract_url_host(raw_url);
    host_matches_domain(&host, "youtube.com")
        || host_matches_domain(&host, "youtu.be")
        || host_matches_domain(&host, "youtube-nocookie.com")
}

fn is_video_or_stream_url(raw_url: &str) -> bool {
    if core_engine::torrent::is_torrent_reference(raw_url) {
        return false;
    }
    let lower = raw_url.trim().to_lowercase();
    let without_scheme = lower
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&lower);
    let host = extract_url_host(raw_url);

    const VIDEO_DOMAINS: &[&str] = &[
        "youtube.com",
        "youtu.be",
        "vimeo.com",
        "tiktok.com",
        "instagram.com",
        "twitter.com",
        "x.com",
        "dailymotion.com",
        "dai.ly",
        "twitch.tv",
        "facebook.com",
        "fb.watch",
        "reddit.com",
        "redd.it",
        "bilibili.com",
        "b23.tv",
        "soundcloud.com",
        "bandcamp.com",
        "loom.com",
        "streamable.com",
        "rumble.com",
        "odysee.com",
        "kick.com",
        "ted.com",
    ];
    if VIDEO_DOMAINS.iter().any(|d| host_matches_domain(&host, d)) {
        return true;
    }

    let path_only = without_scheme.split(&['?', '#'][..]).next().unwrap_or("");
    path_only.ends_with(".m3u8")
        || path_only.ends_with(".mpd")
        || lower.contains(".m3u8?")
        || lower.contains(".mpd?")
}

fn sanitize_media_title(raw: &str, fallback: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return fallback.to_string();
    }
    let mut cleaned = core_engine::inspector::sanitize_filename(trimmed);
    if cleaned == "download.bin" && !trimmed.eq_ignore_ascii_case("download.bin") {
        return fallback.to_string();
    }
    if cleaned.chars().count() > 120 {
        cleaned = cleaned.chars().take(120).collect::<String>();
        cleaned = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
        if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
            return fallback.to_string();
        }
    }
    cleaned
}

fn is_direct_manifest_url(raw_url: &str) -> bool {
    let lower = raw_url.trim().to_ascii_lowercase();
    let without_scheme = lower
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&lower);
    let path_only = without_scheme.split(&['?', '#'][..]).next().unwrap_or("");
    path_only.ends_with(".m3u8")
        || path_only.ends_with(".mpd")
        || lower.contains(".m3u8?")
        || lower.contains(".mpd?")
}

#[allow(dead_code)]
async fn inspect_video_url(url: &str) -> Result<InspectResult, String> {
    inspect_video_url_with_headers(url, None, None, None).await
}

#[allow(dead_code)]
async fn inspect_video_url_with_headers(
    url: &str,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
) -> Result<InspectResult, String> {
    inspect_video_url_with_options(url, cookie, referer, user_agent, None, None, None).await
}

fn json_has_playable_video_format(json_val: &serde_json::Value) -> bool {
    json_val["formats"]
        .as_array()
        .map(|arr| {
            arr.iter().any(|f| {
                let ext = f["ext"].as_str().unwrap_or("");
                let protocol = f["protocol"].as_str().unwrap_or("");
                let note = f["format_note"].as_str().unwrap_or("");
                ext != "mhtml"
                    && !protocol.contains("mhtml")
                    && !note.to_lowercase().contains("storyboard")
                    && f["vcodec"].as_str().unwrap_or("none") != "none"
                    && f["height"].as_u64().unwrap_or(0) >= 144
            })
        })
        .unwrap_or(false)
}

fn video_inspect_coordinator() -> &'static core_engine::VideoInspectCoordinator {
    static COORDINATOR: OnceLock<core_engine::VideoInspectCoordinator> = OnceLock::new();
    COORDINATOR.get_or_init(core_engine::VideoInspectCoordinator::new)
}

#[derive(Clone)]
struct CachedRawYtDlpJson {
    json: serde_json::Value,
    stored_at: std::time::Instant,
}

fn raw_ytdlp_json_cache() -> &'static Mutex<HashMap<String, CachedRawYtDlpJson>> {
    static RAW_CACHE: OnceLock<Mutex<HashMap<String, CachedRawYtDlpJson>>> = OnceLock::new();
    RAW_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn raw_video_cache_key(
    url: &str,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
    proxy_url: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
) -> String {
    let canon = core_engine::video_runtime::canonicalize_video_url(url);
    let fp = core_engine::video_runtime::inspect_options_fingerprint(
        cookie,
        referer,
        user_agent,
        proxy_url,
        auth_username,
        auth_password,
    );
    format!("{}::{}", canon, fp)
}

fn get_cached_raw_ytdlp_json(key: &str) -> Option<serde_json::Value> {
    const TTL: std::time::Duration = std::time::Duration::from_secs(90);
    let Ok(mut map) = raw_ytdlp_json_cache().lock() else {
        return None;
    };
    let now = std::time::Instant::now();
    map.retain(|_, v| now.duration_since(v.stored_at) <= TTL);
    map.get(key).map(|entry| entry.json.clone())
}

fn put_cached_raw_ytdlp_json(key: &str, json: &serde_json::Value) {
    const TTL: std::time::Duration = std::time::Duration::from_secs(90);
    const MAX_ENTRIES: usize = 16;
    let Ok(mut map) = raw_ytdlp_json_cache().lock() else {
        return;
    };
    let now = std::time::Instant::now();
    map.retain(|_, v| now.duration_since(v.stored_at) <= TTL);
    while map.len() >= MAX_ENTRIES && !map.contains_key(key) {
        if let Some(oldest_key) = map
            .iter()
            .min_by_key(|(_, v)| v.stored_at)
            .map(|(k, _)| k.clone())
        {
            map.remove(&oldest_key);
        } else {
            break;
        }
    }
    map.insert(
        key.to_string(),
        CachedRawYtDlpJson {
            json: json.clone(),
            stored_at: now,
        },
    );
}

fn invalidate_cached_raw_ytdlp_json(key: &str) {
    if let Ok(mut map) = raw_ytdlp_json_cache().lock() {
        map.remove(key);
    }
}

async fn run_ytdlp_dump_json_with_selector(
    bin: &str,
    url: &str,
    force_fallback_clients: bool,
    format_selector: Option<&str>,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
    proxy_url: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
    timeout_secs: u64,
) -> Result<serde_json::Value, String> {
    let mut cmd = tokio::process::Command::new(bin);
    hide_window_tokio(&mut cmd);
    let has_js_rt = apply_ytdlp_runtime_env(&mut cmd);
    if force_fallback_clients || (!has_js_rt && is_youtube_url(url)) {
        cmd.arg("--extractor-args")
            .arg(core_engine::video_runtime::youtube_fallback_extractor_args());
    }
    cmd.arg("--dump-single-json");
    cmd.arg("--no-playlist");
    cmd.arg("--no-warnings");
    cmd.arg("--no-check-formats");
    if let Some(sel) = format_selector.map(str::trim).filter(|s| !s.is_empty()) {
        cmd.arg("-f").arg(sel);
    }
    apply_ytdlp_headers(&mut cmd, cookie, referer, user_agent);
    apply_ytdlp_network_options(&mut cmd, proxy_url, auth_username, auth_password);
    cmd.arg(url);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        cmd.output(),
    )
    .await
    .map_err(|_| format!("Analisis video (yt-dlp) melebihi batas waktu {} detik.", timeout_secs))?
    .map_err(|e| format!("Gagal menjalankan yt-dlp ({}): {}", bin, e))?;

    if !output.status.success() || output.stdout.is_empty() {
        let err_snippet = String::from_utf8_lossy(&output.stderr);
        let best_line = err_snippet
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| l.starts_with("ERROR:"))
            .or_else(|| {
                err_snippet
                    .lines()
                    .rev()
                    .map(str::trim)
                    .find(|l| !l.is_empty() && !l.starts_with("WARNING:"))
            })
            .unwrap_or("Stream video tidak dapat dianalisis oleh yt-dlp.");
        log::warn!("yt-dlp inspect failed for {}: {}", url, best_line);
        return Err(best_line.to_string());
    }

    serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .map_err(|e| format!("Metadata yt-dlp tidak valid: {}", e))
}

async fn run_ytdlp_dump_json(
    bin: &str,
    url: &str,
    force_fallback_clients: bool,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
    proxy_url: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
    timeout_secs: u64,
) -> Result<serde_json::Value, String> {
    run_ytdlp_dump_json_with_selector(
        bin,
        url,
        force_fallback_clients,
        None,
        cookie,
        referer,
        user_agent,
        proxy_url,
        auth_username,
        auth_password,
        timeout_secs,
    )
    .await
}

async fn inspect_video_url_with_options(
    url: &str,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
    proxy_url: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
) -> Result<InspectResult, String> {
    let fingerprint = core_engine::video_runtime::inspect_options_fingerprint(
        cookie,
        referer,
        user_agent,
        proxy_url,
        auth_username,
        auth_password,
    );

    let url_owned = url.to_string();
    let cookie_owned = cookie.map(String::from);
    let referer_owned = referer.map(String::from);
    let ua_owned = user_agent.map(String::from);
    let proxy_owned = proxy_url.map(String::from);
    let auth_user_owned = auth_username.map(String::from);
    let auth_pass_owned = auth_password.map(String::from);

    video_inspect_coordinator()
        .get_or_inspect(url, &fingerprint, move || async move {
            inspect_video_url_uncached(
                &url_owned,
                cookie_owned.as_deref(),
                referer_owned.as_deref(),
                ua_owned.as_deref(),
                proxy_owned.as_deref(),
                auth_user_owned.as_deref(),
                auth_pass_owned.as_deref(),
            )
            .await
        })
        .await
}

async fn inspect_video_url_uncached(
    url: &str,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
    proxy_url: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
) -> Result<InspectResult, String> {
    if get_ffmpeg_bin().is_none() {
        tokio::spawn(async {
            if let Err(e) = ensure_ffmpeg_bin().await {
                log::warn!("Warmup auto-provisioning FFmpeg saat inspeksi gagal: {}", e);
            }
        });
    }
    let bin = ensure_ytdlp_ready_for_video().await?;
    let is_yt = is_youtube_url(url);
    let has_custom_headers = cookie.is_some() || user_agent.is_some();

    let primary = run_ytdlp_dump_json(
        &bin,
        url,
        false,
        cookie,
        referer,
        user_agent,
        proxy_url,
        auth_username,
        auth_password,
        16,
    )
    .await;

    let json_val = match primary {
        Ok(v) if !is_yt || json_has_playable_video_format(&v) => v,
        Err(c_err) if !is_yt && !has_custom_headers => {
            return Err(c_err);
        }
        fallback_candidate => {
            // Fallback for YouTube session/cookie bot-gate or SABR client mismatch:
            // retry cleanly without browser Cookie/UA headers and with fast fallback clients.
            let retry = run_ytdlp_dump_json(
                &bin,
                url,
                is_yt,
                None,
                referer,
                None,
                proxy_url,
                auth_username,
                auth_password,
                14,
            )
            .await;

            match (retry, fallback_candidate) {
                (Ok(r), Ok(c)) => {
                    if json_has_playable_video_format(&r) || !json_has_playable_video_format(&c) {
                        r
                    } else {
                        c
                    }
                }
                (Ok(r), Err(_)) => r,
                (Err(_), Ok(c)) => c,
                (Err(r_err), Err(c_err)) => {
                    return Err(if !r_err.is_empty() { r_err } else { c_err });
                }
            }
        }
    };

    let raw_title = json_val["title"].as_str().unwrap_or("video");
    let clean_title = sanitize_media_title(raw_title, "video");

    let duration_seconds = json_val["duration"].as_u64().unwrap_or(0);
    let thumbnail_url = json_val["thumbnail"].as_str().map(|s| s.to_string());

    let formats_array = json_val["formats"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| "Daftar format video tidak ditemukan pada tautan ini.".to_string())?;

    // 1. Identify best audio stream
    let mut best_audio_size: u64 = 0;
    let mut best_m4a_size: u64 = 0;

    for f in formats_array {
        let ext = f["ext"].as_str().unwrap_or("");
        let protocol = f["protocol"].as_str().unwrap_or("");
        if ext == "mhtml" || protocol.contains("mhtml") {
            continue;
        }
        let vcodec = f["vcodec"].as_str().unwrap_or("none");
        let acodec = f["acodec"].as_str().unwrap_or("none");

        if vcodec == "none" && acodec != "none" {
            let mut size = f["filesize"].as_u64().or_else(|| f["filesize_approx"].as_u64()).unwrap_or(0);
            if size == 0 && duration_seconds > 0 {
                if let Some(abr) = f["abr"].as_f64().or_else(|| f["tbr"].as_f64()) {
                    if abr > 0.0 {
                        size = ((abr * 1000.0 / 8.0) * duration_seconds as f64) as u64;
                    }
                }
            }
            if size > best_audio_size {
                best_audio_size = size;
            }
            if ext == "m4a" && size > best_m4a_size {
                best_m4a_size = size;
            }
        }
    }

    if best_audio_size == 0 {
        best_audio_size = if duration_seconds > 0 {
            ((160.0 * 1000.0 / 8.0) * duration_seconds as f64) as u64
        } else {
            5_000_000
        };
    }
    if best_m4a_size == 0 {
        best_m4a_size = best_audio_size;
    }

    let best_mp3_size = if duration_seconds > 0 {
        ((256.0 * 1000.0 / 8.0) * duration_seconds as f64) as u64
    } else {
        best_audio_size
    };

    // 2. Collect unique video heights from actual formats provided by the platform
    let valid_resolutions = [144, 240, 360, 480, 720, 1080, 1440, 2160, 4320];
    let mut heights: Vec<u64> = Vec::new();
    for f in formats_array {
        let ext = f["ext"].as_str().unwrap_or("");
        let protocol = f["protocol"].as_str().unwrap_or("");
        let format_note = f["format_note"].as_str().unwrap_or("");
        if ext == "mhtml" || protocol.contains("mhtml") || format_note.to_lowercase().contains("storyboard") {
            continue;
        }
        let vcodec = f["vcodec"].as_str().unwrap_or("none");
        if vcodec != "none" {
            if let Some(h) = f["height"].as_u64() {
                if h > 0 && valid_resolutions.contains(&h) && !heights.contains(&h) {
                    heights.push(h);
                }
            }
        }
    }

    // Fallback for non-standard portrait or HLS resolutions (e.g. 540p, 576p, 1024p, 1280p)
    if heights.is_empty() {
        for f in formats_array {
            let ext = f["ext"].as_str().unwrap_or("");
            let protocol = f["protocol"].as_str().unwrap_or("");
            let format_note = f["format_note"].as_str().unwrap_or("");
            if ext == "mhtml" || protocol.contains("mhtml") || format_note.to_lowercase().contains("storyboard") {
                continue;
            }
            let vcodec = f["vcodec"].as_str().unwrap_or("none");
            if vcodec != "none" {
                if let Some(h) = f["height"].as_u64() {
                    if h >= 144 && !heights.contains(&h) {
                        heights.push(h);
                    }
                }
            }
        }
    }

    heights.sort_by(|a, b| b.cmp(a));

    let mut video_formats: Vec<VideoFormat> = Vec::new();

    for h in heights {
        let matching: Vec<&serde_json::Value> = formats_array
            .iter()
            .filter(|f| {
                let ext = f["ext"].as_str().unwrap_or("");
                let protocol = f["protocol"].as_str().unwrap_or("");
                let format_note = f["format_note"].as_str().unwrap_or("");
                ext != "mhtml"
                    && !protocol.contains("mhtml")
                    && !format_note.to_lowercase().contains("storyboard")
                    && f["vcodec"].as_str().unwrap_or("none") != "none"
                    && f["height"].as_u64() == Some(h)
            })
            .collect();

        if matching.is_empty() {
            continue;
        }

        let chosen = matching
            .iter()
            .find(|f| f["ext"].as_str() == Some("mp4"))
            .unwrap_or(&matching[0]);

        // Calculate accurate video size across all streams for this height
        let mut candidate_sizes: Vec<u64> = Vec::new();
        for m in &matching {
            let mut s = m["filesize"].as_u64().or_else(|| m["filesize_approx"].as_u64()).unwrap_or(0);
            if s == 0 && duration_seconds > 0 {
                let vbr = m["vbr"].as_f64().or_else(|| {
                    let tbr = m["tbr"].as_f64().unwrap_or(0.0);
                    let abr = m["abr"].as_f64().unwrap_or(0.0);
                    if tbr > abr { Some(tbr - abr) } else { None }
                });
                if let Some(kbps) = vbr {
                    if kbps > 0.0 {
                        s = ((kbps * 1000.0 / 8.0) * duration_seconds as f64) as u64;
                    }
                }
            }
            if s > 0 {
                candidate_sizes.push(s);
            }
        }

        let fallback_kbps: f64 = match h {
            4320 => 35000.0,
            2160 => 18000.0,
            1440 => 7000.0,
            1080 => 3200.0,
            720 => 1600.0,
            480 => 850.0,
            360 => 450.0,
            240 => 250.0,
            144 => 120.0,
            _ => 1000.0,
        };

        let v_size = if !candidate_sizes.is_empty() {
            candidate_sizes.iter().sum::<u64>() / candidate_sizes.len() as u64
        } else if duration_seconds > 0 {
            ((fallback_kbps * 1000.0 / 8.0) * duration_seconds as f64) as u64
        } else {
            match h {
                4320 => 450_000_000,
                2160 => 220_000_000,
                1440 => 110_000_000,
                1080 => 65_000_000,
                720 => 28_000_000,
                480 => 15_000_000,
                360 => 9_000_000,
                240 => 5_000_000,
                144 => 3_000_000,
                _ => 15_000_000,
            }
        };

        let acodec = chosen["acodec"].as_str().unwrap_or("none");
        let needs_audio = acodec == "none";
        let total_size = if needs_audio { v_size + best_audio_size } else { v_size };

        let res_label = match h {
            4320 => "8K 4320p".to_string(),
            2160 => "4K 2160p (Ultra HD)".to_string(),
            1440 => "2K 1440p (QHD)".to_string(),
            1080 => "1080p (Full HD)".to_string(),
            720 => "720p (HD)".to_string(),
            480 => "480p (SD Standar)".to_string(),
            360 => "360p (Hemat Kuota)".to_string(),
            240 => "240p (Rendah)".to_string(),
            144 => "144p (Sangat Rendah)".to_string(),
            other => format!("{}p", other),
        };

        let format_id = format!("{}p_mp4", h);
        let vcodec_str = chosen["vcodec"].as_str().unwrap_or("H.264 / AVC").to_string();
        let acodec_str = if needs_audio {
            Some("AAC / Audio Merged".to_string())
        } else {
            Some(acodec.to_string())
        };

        video_formats.push(VideoFormat {
            format_id,
            resolution: res_label,
            extension: "mp4".to_string(),
            file_size_approx: if total_size > 0 { Some(total_size) } else { None },
            video_codec: Some(vcodec_str),
            audio_codec: acodec_str,
            download_url: url.to_string(),
            is_audio_only: false,
        });
    }

    // 3. Add real Audio formats (MP3 & M4A)
    video_formats.push(VideoFormat {
        format_id: "audio_mp3".to_string(),
        resolution: "Audio Only (MP3 Kualitas Terbaik)".to_string(),
        extension: "mp3".to_string(),
        file_size_approx: Some(best_mp3_size),
        video_codec: None,
        audio_codec: Some("MP3 256kbps High Quality".to_string()),
        download_url: url.to_string(),
        is_audio_only: true,
    });

    if best_m4a_size > 0 {
        video_formats.push(VideoFormat {
            format_id: "audio_m4a".to_string(),
            resolution: "Audio Only (M4A Original AAC)".to_string(),
            extension: "m4a".to_string(),
            file_size_approx: Some(best_m4a_size),
            video_codec: None,
            audio_codec: Some("AAC Original Stream".to_string()),
            download_url: url.to_string(),
            is_audio_only: true,
        });
    }

    let top_size = video_formats.first().and_then(|f| f.file_size_approx);

    let raw_key = raw_video_cache_key(
        url,
        cookie,
        referer,
        user_agent,
        proxy_url,
        auth_username,
        auth_password,
    );
    put_cached_raw_ytdlp_json(&raw_key, &json_val);

    Ok(InspectResult {
        url: url.to_string(),
        final_url: url.to_string(),
        file_name: format!("{}.mp4", clean_title),
        file_size: top_size,
        supports_range: true,
        mime_type: Some("video/mp4".to_string()),
        etag: None,
        last_modified: None,
        suggested_category: "video".to_string(),
        media_info: Some(MediaInfo {
            title: clean_title,
            thumbnail_url,
            duration_seconds: if duration_seconds > 0 { Some(duration_seconds) } else { None },
            formats: video_formats,
        }),
        torrent_info: None,
    })
}

fn resolve_path(p: &str) -> PathBuf {
    let trimmed = p.trim();
    if trimmed == "~" {
        if let Some(home) = user_home_dir() {
            return home;
        }
    }
    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix(r#"~\"#))
    {
        if let Some(home) = user_home_dir() {
            let mut out = home;
            for seg in rest.split(['/', '\\']).filter(|s| !s.is_empty()) {
                out.push(seg);
            }
            return out;
        }
    }
    PathBuf::from(trimmed)
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SegmentProgressPayload {
    pub segment_index: usize,
    pub start_byte: u64,
    pub end_byte: u64,
    pub current_byte: u64,
    pub downloaded_bytes: u64,
    pub is_completed: bool,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct DownloadTelemetryPayload {
    pub task_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub speed_bytes_per_sec: u64,
    pub progress_percent: f64,
    pub eta_seconds: Option<u64>,
    pub status: String,
    pub destination_path: String,
    pub segments: Vec<SegmentProgressPayload>,
    pub error: Option<String>,
    #[serde(default)]
    pub peers: Option<usize>,
    #[serde(default)]
    pub seeds: Option<usize>,
}

fn emit_simple_telemetry(
    app: &tauri::AppHandle,
    task_id: &str,
    downloaded_bytes: u64,
    total_bytes: u64,
    speed_bytes_per_sec: u64,
    progress_percent: f64,
    eta_seconds: Option<u64>,
    status: &str,
    target: &Path,
    error: Option<String>,
) {
    let _ = app.emit(
        "download-telemetry",
        DownloadTelemetryPayload {
            task_id: task_id.to_string(),
            downloaded_bytes,
            total_bytes,
            speed_bytes_per_sec,
            progress_percent,
            eta_seconds,
            status: status.to_string(),
            destination_path: target.display().to_string(),
            segments: Vec::new(),
            error,
            peers: None,
            seeds: None,
        },
    );
}

fn parse_size_unit_bytes(token: &str) -> Option<u64> {
    let clean = token.trim().trim_start_matches('~');
    let units: [(&str, f64); 8] = [
        ("GiB", 1024.0 * 1024.0 * 1024.0),
        ("MiB", 1024.0 * 1024.0),
        ("KiB", 1024.0),
        ("GB", 1000.0 * 1000.0 * 1000.0),
        ("MB", 1000.0 * 1000.0),
        ("KB", 1000.0),
        ("B", 1.0),
        ("iB", 1.0),
    ];
    for (suffix, mult) in units {
        if let Some(num_str) = clean.strip_suffix(suffix) {
            if let Ok(v) = num_str.trim().parse::<f64>() {
                if v.is_finite() && v >= 0.0 {
                    return Some((v * mult).round() as u64);
                }
            }
        }
    }
    None
}

fn parse_ytdlp_eta(token: &str) -> Option<u64> {
    let clean = token.trim();
    if clean.eq_ignore_ascii_case("Unknown") || clean.is_empty() {
        return None;
    }
    let parts: Vec<u64> = clean
        .split(':')
        .filter_map(|p| p.trim().parse::<u64>().ok())
        .collect();
    match parts.as_slice() {
        [s] => Some(*s),
        [m, s] => Some(m * 60 + s),
        [h, m, s] => Some(h * 3600 + m * 60 + s),
        _ => None,
    }
}

fn parse_ytdlp_progress_line(line: &str) -> Option<(f64, u64, u64, u64, Option<u64>)> {
    let pct_idx = line.find('%')?;
    let pct_str = line[..pct_idx].split_whitespace().last()?;
    let pct = pct_str.parse::<f64>().ok()?;

    let mut total_bytes: u64 = 10_000_000;
    if let Some(of_pos) = line.find(" of ") {
        let after_of = line[of_pos + 4..].trim_start().trim_start_matches('~').trim_start();
        if let Some(tok) = after_of.split_whitespace().next() {
            if let Some(bytes) = parse_size_unit_bytes(tok) {
                if bytes > 0 {
                    total_bytes = bytes;
                }
            }
        }
    }

    let mut speed_bps: u64 = 0;
    if let Some(at_pos) = line.find(" at ") {
        let after_at = line[at_pos + 4..].trim_start();
        if let Some(tok) = after_at.split_whitespace().next() {
            if let Some(s_tok) = tok.strip_suffix("/s") {
                if let Some(bps) = parse_size_unit_bytes(s_tok) {
                    speed_bps = bps;
                }
            }
        }
    }

    let mut eta: Option<u64> = None;
    if let Some(eta_pos) = line.find("ETA ") {
        let after_eta = line[eta_pos + 4..].trim_start();
        if let Some(tok) = after_eta.split_whitespace().next() {
            eta = parse_ytdlp_eta(tok);
        }
    }

    let downloaded_bytes = ((pct / 100.0) * total_bytes as f64).round() as u64;
    if eta.is_none() && speed_bps > 0 && total_bytes > downloaded_bytes {
        eta = Some(((total_bytes - downloaded_bytes) as f64 / speed_bps as f64).ceil() as u64);
    }

    Some((pct, downloaded_bytes, total_bytes, speed_bps, eta))
}

#[tauri::command]
fn check_file_exists(path: String) -> bool {
    let resolved = resolve_path(&path);
    resolved.exists() && resolved.is_file()
}

#[cfg(any(target_os = "linux", test))]
fn path_to_file_uri(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut encoded = String::from("file://");
    for byte in raw.as_bytes() {
        let b = *byte;
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~') {
            encoded.push(b as char);
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{:02X}", b));
        }
    }
    encoded
}

fn normalize_platform_path(path: PathBuf) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        return PathBuf::from(path.to_string_lossy().replace('/', "\\"));
    }
    #[cfg(not(target_os = "windows"))]
    {
        path
    }
}

#[tauri::command]
fn open_folder(path: String) -> Result<String, String> {
    let mut target = normalize_platform_path(resolve_path(&path));

    // If target is a file (not an existing directory with a dot in its name), reveal in Finder/Explorer/File Manager or open parent directory
    if target.is_file() || (!target.is_dir() && target.extension().is_some()) {
        if target.exists() {
            #[cfg(target_os = "macos")]
            {
                let status = std::process::Command::new("open")
                    .arg("-R")
                    .arg(&target)
                    .status();
                if let Ok(s) = status {
                    if s.success() {
                        return Ok(format!("Menampilkan berkas di Finder: {}", target.display()));
                    }
                }
            }
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                let abs = std::fs::canonicalize(&target).unwrap_or_else(|_| target.clone());
                let norm = abs.to_string_lossy().replace('/', "\\");
                let clean = if let Some(unc) = norm.strip_prefix(r"\\?\UNC\") {
                    format!(r"\\{}", unc)
                } else {
                    norm.strip_prefix(r"\\?\").unwrap_or(&norm).to_string()
                };
                if std::process::Command::new("explorer")
                    .raw_arg(format!("/select,\"{}\"", clean))
                    .spawn()
                    .is_ok()
                {
                    return Ok(format!("Membuka berkas di Explorer: {}", clean));
                }
            }
            #[cfg(target_os = "linux")]
            {
                let abs_target = if target.is_absolute() {
                    target.clone()
                } else {
                    std::env::current_dir()
                        .map(|cwd| cwd.join(&target))
                        .unwrap_or_else(|_| target.clone())
                };
                let uri = path_to_file_uri(&abs_target);
                if let Ok(output) = std::process::Command::new("dbus-send")
                    .args([
                        "--session",
                        "--dest=org.freedesktop.FileManager1",
                        "--print-reply",
                        "--reply-timeout=1500",
                        "--type=method_call",
                        "/org/freedesktop/FileManager1",
                        "org.freedesktop.FileManager1.ShowItems",
                        &format!("array:string:{}", uri),
                        "string:",
                    ])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .output()
                {
                    if output.status.success() {
                        return Ok(format!(
                            "Menampilkan berkas di File Manager: {}",
                            target.display()
                        ));
                    }
                }
            }
        }
        if let Some(parent) = target.parent() {
            target = parent.to_path_buf();
        }
    }

    // Ensure the folder exists
    if !target.exists() {
        let _ = std::fs::create_dir_all(&target);
    }

    open::that(&target)
        .map(|_| format!("Folder berhasil dibuka: {}", target.display()))
        .map_err(|e| format!("Gagal membuka folder ({}): {}", target.display(), e))
}

#[tauri::command]
fn open_file(path: String) -> Result<String, String> {
    let target = normalize_platform_path(resolve_path(&path));

    if !target.exists() {
        // If file is missing, open parent folder to show where it belongs
        if let Some(parent) = target.parent() {
            if !parent.exists() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = open::that(parent);
        }
        return Err(format!(
            "Berkas '{}' belum selesai diunduh atau belum ada di disk. Folder penyimpanan telah dibuka.",
            target.file_name().unwrap_or_default().to_string_lossy()
        ));
    }

    open::that(&target)
        .map(|_| format!("Berkas berhasil dibuka: {}", target.display()))
        .map_err(|e| format!("Gagal membuka berkas ({}): {}", target.display(), e))
}

fn dir_size_bytes(dir: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            total = total.saturating_add(dir_size_bytes(&p));
        } else if let Ok(meta) = entry.metadata() {
            total = total.saturating_add(meta.len());
        }
    }
    total
}

fn path_content_size(path: &Path) -> u64 {
    if path.is_dir() {
        dir_size_bytes(path)
    } else {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }
}

fn resolve_unique_destination(target: &Path) -> PathBuf {
    if !target.exists()
        || core_engine::downloader::checkpoint_path_for(target).exists()
        || core_engine::torrent::torrent_checkpoint_path(target).exists()
    {
        return target.to_path_buf();
    }
    let part_candidate = PathBuf::from(format!("{}.part", target.display()));
    if part_candidate.exists() {
        return target.to_path_buf();
    }
    let parent = target.parent().unwrap_or_else(|| Path::new(""));
    let stem = target
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    let ext = target.extension().and_then(|e| e.to_str());
    for idx in 1..=999 {
        let candidate_name = match ext {
            Some(e) if !e.is_empty() => format!("{} ({}).{}", stem, idx, e),
            _ => format!("{} ({})", stem, idx),
        };
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    target.to_path_buf()
}

#[tauri::command]
fn pause_real_download(task_id: String) -> bool {
    if let Ok(map) = active_downloads().lock() {
        if let Some(ctrl) = map.get(&task_id) {
            ctrl.pause();
            return true;
        }
    }
    false
}

#[tauri::command]
async fn cancel_real_download(task_id: String, destination_path: Option<String>) -> bool {
    let mut signaled_active = false;
    if let Ok(map) = active_downloads().lock() {
        if let Some(ctrl) = map.get(&task_id) {
            ctrl.cancel();
            signaled_active = true;
        }
    }

    if signaled_active {
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            let still_active = active_downloads()
                .lock()
                .map(|m| m.contains_key(&task_id))
                .unwrap_or(false);
            if !still_active {
                break;
            }
        }
    }

    let target_to_clean = remove_task_target(&task_id).or_else(|| {
        destination_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(resolve_path)
    });

    let removed_files = target_to_clean
        .as_deref()
        .map(cleanup_download_artifacts)
        .unwrap_or(0);

    signaled_active || removed_files > 0
}

#[tauri::command]
fn stop_all_real_downloads() -> usize {
    if let Ok(map) = active_downloads().lock() {
        let count = map.len();
        for ctrl in map.values() {
            ctrl.pause();
        }
        return count;
    }
    0
}

fn build_ytdlp_format_selector(
    format_id: Option<&str>,
    target_ext: &str,
    is_audio: bool,
    has_ffmpeg: bool,
) -> String {
    build_ytdlp_format_selector_with_strict(format_id, target_ext, is_audio, has_ffmpeg, false)
}

fn build_ytdlp_format_selector_with_strict(
    format_id: Option<&str>,
    target_ext: &str,
    is_audio: bool,
    has_ffmpeg: bool,
    strict_split: bool,
) -> String {
    let parse_height_selector = |h: u32| -> String {
        if has_ffmpeg && strict_split {
            format!("bestvideo[height<={h}]+bestaudio")
        } else if has_ffmpeg {
            format!("bestvideo[height<={h}]+bestaudio/best[height<={h}]/best")
        } else {
            format!("best[height<={h}][ext=mp4]/best[height<={h}]/best")
        }
    };
    let default_video_selector = if has_ffmpeg && strict_split {
        "bestvideo+bestaudio".to_string()
    } else if has_ffmpeg {
        "bestvideo+bestaudio/best".to_string()
    } else {
        "best[ext=mp4]/best".to_string()
    };
    let default_m4a_selector = if has_ffmpeg {
        "bestaudio[ext=m4a]/bestaudio/18/best".to_string()
    } else {
        "bestaudio[ext=m4a]/best[ext=m4a]/bestaudio/18/best".to_string()
    };

    if let Some(fid) = format_id.map(str::trim).filter(|s| !s.is_empty()) {
        if fid == "audio_mp3" {
            "bestaudio/18/best".to_string()
        } else if fid == "audio_m4a" {
            default_m4a_selector
        } else if let Some(h_str) = fid.strip_suffix("p_mp4") {
            if let Ok(h) = h_str.parse::<u32>() {
                parse_height_selector(h)
            } else {
                default_video_selector
            }
        } else if let Some(h_str) = fid.strip_suffix('p') {
            if let Ok(h) = h_str.parse::<u32>() {
                parse_height_selector(h)
            } else {
                default_video_selector
            }
        } else if is_audio {
            if target_ext == "m4a" {
                default_m4a_selector
            } else {
                "bestaudio/18/best".to_string()
            }
        } else if strict_split
            && has_ffmpeg
            && !fid.eq_ignore_ascii_case("best")
            && !fid.eq_ignore_ascii_case("mp4")
            && fid.chars().any(|c| c.is_ascii_digit())
            && fid
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            format!("{fid}+bestaudio")
        } else {
            default_video_selector
        }
    } else if is_audio {
        if target_ext == "m4a" {
            default_m4a_selector
        } else {
            "bestaudio/18/best".to_string()
        }
    } else {
        default_video_selector
    }
}

fn remove_split_temp_files(target: &Path) {
    if let (Some(parent), Some(target_name)) = (
        target.parent(),
        target.file_name().and_then(|n| n.to_str()),
    ) {
        if target_name.is_empty() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
                    continue;
                };
                if name.starts_with(&format!("{}.vtmp.", target_name))
                    || name.starts_with(&format!("{}.atmp.", target_name))
                    || name.starts_with(&format!("{}.mux.", target_name))
                {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}

async fn wait_child_with_http_ctrl(
    mut child: tokio::process::Child,
    ctrl: Option<&core_engine::DownloadController>,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    match ctrl {
        Some(c) => {
            if c.is_stopped() {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Ok(None);
            }
            tokio::select! {
                res = child.wait() => res.map(Some),
                _ = c.notified() => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    Ok(None)
                }
            }
        }
        None => child.wait().await.map(Some),
    }
}

async fn run_ffmpeg_with_ctrl(
    mut ff_cmd: tokio::process::Command,
    ctrl: Option<&core_engine::DownloadController>,
    output_path: &Path,
) -> Result<(), Option<String>> {
    ff_cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());

    let mut child = match ff_cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Err(Some(format!("Gagal menjalankan FFmpeg: {}", e)));
        }
    };

    let stderr_handle = child.stderr.take().map(|err_pipe| {
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(err_pipe).lines();
            let mut ring: std::collections::VecDeque<String> =
                std::collections::VecDeque::with_capacity(4);
            while let Ok(Some(line)) = reader.next_line().await {
                let clean = line.trim();
                if clean.is_empty() {
                    continue;
                }
                if ring.len() == 4 {
                    ring.pop_front();
                }
                ring.push_back(clean.to_string());
            }
            if ring.is_empty() {
                None
            } else {
                let joined = ring.into_iter().collect::<Vec<_>>().join(" | ");
                let clipped: String = joined.chars().take(360).collect();
                Some(clipped)
            }
        })
    });

    let status_opt = wait_child_with_http_ctrl(child, ctrl).await;
    let err_tail = if let Some(handle) = stderr_handle {
        handle.await.ok().flatten()
    } else {
        None
    };

    match status_opt {
        Ok(None) => {
            let _ = std::fs::remove_file(output_path);
            Err(None)
        }
        Ok(Some(st)) if st.success() => {
            let valid = std::fs::metadata(output_path)
                .map(|m| m.is_file() && m.len() > 0)
                .unwrap_or(false);
            if valid {
                Ok(())
            } else {
                let _ = std::fs::remove_file(output_path);
                Err(Some(match err_tail {
                    Some(msg) => format!("Berkas hasil muxing FFmpeg kosong: {}", msg),
                    None => "Berkas hasil muxing FFmpeg kosong.".to_string(),
                }))
            }
        }
        Ok(Some(st)) => {
            let _ = std::fs::remove_file(output_path);
            let code_str = st
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "sinyal".to_string());
            Err(Some(match err_tail {
                Some(msg) => format!("FFmpeg gagal (kode {}): {}", code_str, msg),
                None => format!("FFmpeg gagal (kode {}).", code_str),
            }))
        }
        Err(e) => {
            let _ = std::fs::remove_file(output_path);
            Err(Some(format!("Proses FFmpeg terputus: {}", e)))
        }
    }
}

/// Converts a downloaded media/audio stream (`src_audio`) into `target` (`.mp3`, `.m4a`, `.flac`, `.wav`).
/// Returns `Ok(true)` on conversion success, `Ok(false)` if interrupted via `controller`, or `Err(msg)`.
async fn convert_audio_stream_to_target(
    src_audio: &Path,
    target: &Path,
    target_ext: &str,
    ffmpeg_path: Option<&Path>,
    controller: Option<&core_engine::DownloadController>,
) -> Result<bool, String> {
    if controller.map_or(false, |c| c.is_stopped()) {
        return Ok(false);
    }

    let mut converted = false;
    if let Some(home) = user_home_dir() {
        let converter_script = home.join(".download-cepat").join("bin").join("converter.py");
        let venv_dir = home.join(".download-cepat").join("venv");
        let python_candidates = [
            venv_dir.join("bin").join("python3"),
            venv_dir.join("Scripts").join("python.exe"),
        ];
        let python_bin = python_candidates
            .into_iter()
            .find(|p| p.is_file())
            .or_else(|| find_executable_in_dirs(&["python3", "python"], &default_tool_dirs()));

        if converter_script.is_file() {
            if let Some(py) = python_bin {
                let mut conv_cmd = tokio::process::Command::new(&py);
                hide_window_tokio(&mut conv_cmd);
                conv_cmd
                    .arg(&converter_script)
                    .arg(src_audio)
                    .arg(target)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                if let Ok(child) = conv_cmd.spawn() {
                    match wait_child_with_http_ctrl(child, controller).await {
                        Ok(None) => return Ok(false),
                        Ok(Some(c_status)) if c_status.success() => {
                            converted = std::fs::metadata(target)
                                .map(|m| m.is_file() && m.len() > 0)
                                .unwrap_or(false);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    if !converted {
        if let Some(ff_path) = ffmpeg_path {
            let mut ff_cmd = tokio::process::Command::new(ff_path);
            hide_window_tokio(&mut ff_cmd);
            ff_cmd.arg("-y").arg("-i").arg(src_audio).arg("-vn");
            match target_ext {
                "mp3" => {
                    ff_cmd.args(["-c:a", "libmp3lame", "-b:a", "256k"]);
                }
                "m4a" => {
                    ff_cmd.args([
                        "-c:a",
                        "aac",
                        "-b:a",
                        "256k",
                        "-movflags",
                        "+faststart",
                    ]);
                }
                "flac" => {
                    ff_cmd.args(["-c:a", "flac"]);
                }
                "wav" => {
                    ff_cmd.args(["-c:a", "pcm_s16le"]);
                }
                _ => {}
            }
            ff_cmd
                .arg(target)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            if let Ok(child) = ff_cmd.spawn() {
                match wait_child_with_http_ctrl(child, controller).await {
                    Ok(None) => return Ok(false),
                    Ok(Some(ff_status)) if ff_status.success() => {
                        converted = std::fs::metadata(target)
                            .map(|m| m.is_file() && m.len() > 0)
                            .unwrap_or(false);
                    }
                    _ => {}
                }
            }
        }
    }

    if !converted {
        #[cfg(target_os = "macos")]
        {
            if controller.map_or(false, |c| c.is_stopped()) {
                return Ok(false);
            }
            let mut m4a_path = target.to_path_buf();
            m4a_path.set_extension("m4a");
            let mut af_cmd = tokio::process::Command::new("afconvert");
            af_cmd
                .arg("-f")
                .arg("m4af")
                .arg("-d")
                .arg("aac")
                .arg(src_audio)
                .arg(&m4a_path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            if let Ok(child) = af_cmd.spawn() {
                match wait_child_with_http_ctrl(child, controller).await {
                    Ok(None) => return Ok(false),
                    Ok(Some(_)) => {
                        if target != m4a_path.as_path() && !target.exists() && m4a_path.exists() {
                            let _ = std::fs::copy(&m4a_path, target);
                        }
                        if std::fs::metadata(target)
                            .map(|m| m.is_file() && m.len() > 0)
                            .unwrap_or(false)
                        {
                            converted = true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if !converted && target_ext == "m4a" && src_audio.is_file() {
        let is_isobmff = std::fs::File::open(src_audio)
            .and_then(|mut f| {
                use std::io::Read;
                let mut hdr = [0u8; 12];
                f.read_exact(&mut hdr).map(|_| &hdr[4..8] == b"ftyp")
            })
            .unwrap_or(false);
        if is_isobmff && move_file_cross_device(src_audio, target).is_ok() {
            converted = std::fs::metadata(target)
                .map(|m| m.is_file() && m.len() > 0)
                .unwrap_or(false);
        }
    }

    let _ = std::fs::remove_file(src_audio);

    if converted {
        Ok(true)
    } else {
        Err(format!(
            "Gagal mengonversi audio ke .{}. Pasang FFmpeg di sistem agar konversi audio (MP3/M4A) dapat berjalan.",
            target_ext
        ))
    }
}

fn resolve_stream_request_headers<'a>(
    stream: &'a core_engine::video_runtime::ResolvedHttpStream,
    is_youtube: bool,
    caller_cookie: Option<&'a str>,
    caller_referer: Option<&'a str>,
    caller_ua: Option<&'a str>,
) -> (Option<&'a str>, Option<&'a str>, Option<&'a str>) {
    let eff_ua = stream.user_agent.as_deref().or(caller_ua);
    let eff_referer = stream.referer.as_deref().or(caller_referer);
    let eff_cookie = if stream.cookie_header.is_some() {
        stream.cookie_header.as_deref()
    } else if is_youtube {
        None
    } else {
        caller_cookie
    };
    (eff_cookie, eff_referer, eff_ua)
}

#[allow(clippy::too_many_arguments)]
async fn inspect_direct_http_stream(
    inspector: &core_engine::inspector::UrlInspector,
    stream: &core_engine::video_runtime::ResolvedHttpStream,
    is_youtube: bool,
    caller_cookie: Option<&str>,
    caller_referer: Option<&str>,
    caller_ua: Option<&str>,
    proxy_url: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
) -> Result<InspectResult, String> {
    let (eff_cookie, eff_referer, eff_ua) = resolve_stream_request_headers(
        stream,
        is_youtube,
        caller_cookie,
        caller_referer,
        caller_ua,
    );
    inspector
        .inspect_with_options(
            &stream.url,
            core_engine::inspector::InspectOptions {
                cookie: eff_cookie,
                referer: eff_referer,
                user_agent: eff_ua,
                proxy_url,
                auth_username,
                auth_password,
            },
        )
        .await
        .map_err(|e| format!("{}", e))
}

enum HttpPhaseOutcome {
    Completed(u64),
    PausedInPhase,
    CanceledInPhase,
    Failed(String),
}

#[allow(clippy::too_many_arguments)]
async fn download_http_stream_phase(
    app: &tauri::AppHandle,
    task_id: &str,
    downloader: &core_engine::downloader::Downloader,
    stream: &core_engine::video_runtime::ResolvedHttpStream,
    resolved_url: &str,
    dest: &Path,
    telemetry_target: &Path,
    known_size: Option<u64>,
    supports_range: bool,
    num_connections: usize,
    is_youtube: bool,
    caller_cookie: Option<&str>,
    caller_referer: Option<&str>,
    caller_ua: Option<&str>,
    proxy_url: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
    speed_limit_bps: Option<u64>,
    prior_bytes: u64,
    combined_total_bytes: u64,
    max_pct_before_mux: f64,
    controller: &core_engine::DownloadController,
) -> HttpPhaseOutcome {
    if controller.is_canceled() {
        return HttpPhaseOutcome::CanceledInPhase;
    }
    if controller.is_paused() {
        return HttpPhaseOutcome::PausedInPhase;
    }

    let (eff_cookie, eff_referer, eff_ua) = resolve_stream_request_headers(
        stream,
        is_youtube,
        caller_cookie,
        caller_referer,
        caller_ua,
    );

    let (tx, mut rx) =
        tokio::sync::broadcast::channel::<core_engine::model::ProgressTelemetry>(128);
    let app_clone = app.clone();
    let target_str = telemetry_target.display().to_string();
    let monitor_handle = tokio::spawn(async move {
        let mut emitted_paused = false;
        while let Ok(telem) = rx.recv().await {
            let segments_payload: Vec<SegmentProgressPayload> = telem
                .segments
                .into_iter()
                .map(|s| SegmentProgressPayload {
                    segment_index: s.segment_index,
                    start_byte: s.start_byte,
                    end_byte: s.end_byte,
                    current_byte: s.current_byte,
                    downloaded_bytes: s.downloaded_bytes,
                    is_completed: s.is_completed,
                })
                .collect();

            let composite = core_engine::video_runtime::compute_split_phase_telemetry(
                prior_bytes,
                telem.downloaded_bytes,
                combined_total_bytes,
                telem.speed_bytes_per_sec,
                max_pct_before_mux,
            );

            match telem.status {
                core_engine::model::TaskStatus::Canceled => {
                    let _ = app_clone.emit(
                        "download-telemetry",
                        DownloadTelemetryPayload {
                            task_id: telem.task_id.clone(),
                            downloaded_bytes: 0,
                            total_bytes: 0,
                            speed_bytes_per_sec: 0,
                            progress_percent: 0.0,
                            eta_seconds: None,
                            status: "canceled".to_string(),
                            destination_path: target_str.clone(),
                            segments: Vec::new(),
                            error: None,
                            peers: None,
                            seeds: None,
                        },
                    );
                    break;
                }
                core_engine::model::TaskStatus::Paused => {
                    emitted_paused = true;
                    let _ = app_clone.emit(
                        "download-telemetry",
                        DownloadTelemetryPayload {
                            task_id: telem.task_id.clone(),
                            downloaded_bytes: composite.downloaded_bytes,
                            total_bytes: composite.total_bytes,
                            speed_bytes_per_sec: 0,
                            progress_percent: composite.progress_percent,
                            eta_seconds: None,
                            status: "paused".to_string(),
                            destination_path: target_str.clone(),
                            segments: segments_payload,
                            error: None,
                            peers: None,
                            seeds: None,
                        },
                    );
                    break;
                }
                core_engine::model::TaskStatus::Error => {
                    // Parent handles fallback cleanly; avoid premature error state in UI.
                    break;
                }
                core_engine::model::TaskStatus::Completed => {
                    // Never emit final "completed" before muxing / audio conversion completes.
                    let _ = app_clone.emit(
                        "download-telemetry",
                        DownloadTelemetryPayload {
                            task_id: telem.task_id.clone(),
                            downloaded_bytes: composite.downloaded_bytes,
                            total_bytes: composite.total_bytes,
                            speed_bytes_per_sec: 0,
                            progress_percent: composite.progress_percent,
                            eta_seconds: composite.eta_seconds,
                            status: "downloading".to_string(),
                            destination_path: target_str.clone(),
                            segments: segments_payload,
                            error: None,
                            peers: None,
                            seeds: None,
                        },
                    );
                    break;
                }
                _ => {
                    let _ = app_clone.emit(
                        "download-telemetry",
                        DownloadTelemetryPayload {
                            task_id: telem.task_id.clone(),
                            downloaded_bytes: composite.downloaded_bytes,
                            total_bytes: composite.total_bytes,
                            speed_bytes_per_sec: telem.speed_bytes_per_sec,
                            progress_percent: composite.progress_percent,
                            eta_seconds: composite.eta_seconds,
                            status: "downloading".to_string(),
                            destination_path: target_str.clone(),
                            segments: segments_payload,
                            error: None,
                            peers: None,
                            seeds: None,
                        },
                    );
                }
            }
        }
        emitted_paused
    });

    let dl_res = downloader
        .download_with_options(
            task_id,
            resolved_url,
            dest,
            known_size,
            supports_range,
            num_connections,
            Some(tx),
            Some(controller.clone()),
            core_engine::downloader::DownloadOptions {
                cookie: eff_cookie,
                referer: eff_referer,
                user_agent: eff_ua,
                speed_limit_bytes_per_sec: speed_limit_bps,
                proxy_url,
                auth_username,
                auth_password,
                allow_url_refresh: true,
            },
        )
        .await;

    let emitted_paused = monitor_handle.await.unwrap_or(false);

    if controller.is_canceled() {
        return HttpPhaseOutcome::CanceledInPhase;
    }
    if controller.is_paused() {
        if !emitted_paused {
            emit_simple_telemetry(
                app,
                task_id,
                prior_bytes,
                combined_total_bytes,
                0,
                if combined_total_bytes > 0 {
                    ((prior_bytes as f64 / combined_total_bytes as f64) * max_pct_before_mux)
                        .clamp(1.0, max_pct_before_mux)
                } else {
                    0.0
                },
                None,
                "paused",
                telemetry_target,
                None,
            );
        }
        return HttpPhaseOutcome::PausedInPhase;
    }

    match dl_res {
        Ok(()) => {
            let written = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
            if written == 0 {
                HttpPhaseOutcome::Failed("Berkas segmen HTTP kosong setelah unduhan.".to_string())
            } else {
                HttpPhaseOutcome::Completed(written)
            }
        }
        Err(e) => HttpPhaseOutcome::Failed(format!("{}", e)),
    }
}

enum DirectPlanOutcome {
    Finished,
    Stopped { paused_already_emitted: bool },
    FallbackToYtDlp(Option<String>),
}

#[allow(clippy::too_many_arguments)]
async fn fetch_video_json_for_download(
    bin: &str,
    url: &str,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
    proxy_url: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
    controller: &core_engine::DownloadController,
) -> Option<Result<serde_json::Value, String>> {
    let is_yt = is_youtube_url(url);
    let has_custom_headers = cookie.is_some() || user_agent.is_some();

    let primary_outcome = tokio::select! {
        res = run_ytdlp_dump_json_with_selector(
            bin,
            url,
            false,
            None,
            cookie,
            referer,
            user_agent,
            proxy_url,
            auth_username,
            auth_password,
            16,
        ) => Some(res),
        _ = controller.notified() => None,
    };
    let primary = primary_outcome?;

    match primary {
        Ok(v) if !is_yt || json_has_playable_video_format(&v) => Some(Ok(v)),
        Err(c_err) if !is_yt && !has_custom_headers => Some(Err(c_err)),
        fallback_candidate => {
            let retry_outcome = tokio::select! {
                res = run_ytdlp_dump_json_with_selector(
                    bin,
                    url,
                    is_yt,
                    None,
                    None,
                    referer,
                    None,
                    proxy_url,
                    auth_username,
                    auth_password,
                    14,
                ) => Some(res),
                _ = controller.notified() => None,
            };
            let retry = retry_outcome?;
            match (retry, fallback_candidate) {
                (Ok(r), Ok(c)) => {
                    if json_has_playable_video_format(&r) || !json_has_playable_video_format(&c) {
                        Some(Ok(r))
                    } else {
                        Some(Ok(c))
                    }
                }
                (Ok(r), Err(_)) => Some(Ok(r)),
                (Err(_), Ok(c)) => Some(Ok(c)),
                (Err(r_err), Err(c_err)) => Some(Err(if !r_err.is_empty() { r_err } else { c_err })),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_direct_http_video_plan(
    app: &tauri::AppHandle,
    task_id: &str,
    target: &Path,
    target_ext: &str,
    is_audio: bool,
    num_connections: usize,
    ffmpeg_path: Option<&Path>,
    is_youtube: bool,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
    proxy_url: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
    speed_limit_bps: Option<u64>,
    resume_mode: bool,
    http_ctrl: &core_engine::DownloadController,
    plan: core_engine::video_runtime::VideoStreamPlan,
) -> DirectPlanOutcome {
    if http_ctrl.is_stopped() {
        return DirectPlanOutcome::Stopped {
            paused_already_emitted: false,
        };
    }

    let downloader = match core_engine::downloader::Downloader::new() {
        Ok(d) => d,
        Err(e) => return DirectPlanOutcome::FallbackToYtDlp(Some(format!("{}", e))),
    };
    let inspector = match core_engine::inspector::UrlInspector::new() {
        Ok(i) => i,
        Err(e) => return DirectPlanOutcome::FallbackToYtDlp(Some(format!("{}", e))),
    };

    if !resume_mode {
        remove_split_temp_files(target);
    }

    let parent = target.parent().unwrap_or_else(|| Path::new(""));
    let target_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("video");

    match plan {
        core_engine::video_runtime::VideoStreamPlan::SingleStream(stream) => {
            let insp_res = tokio::select! {
                res = inspect_direct_http_stream(
                    &inspector,
                    &stream,
                    is_youtube,
                    cookie,
                    referer,
                    user_agent,
                    proxy_url,
                    auth_username,
                    auth_password,
                ) => Some(res),
                _ = http_ctrl.notified() => None,
            };
            let info = match insp_res {
                None => {
                    return DirectPlanOutcome::Stopped {
                        paused_already_emitted: false,
                    }
                }
                Some(Ok(r)) => r,
                Some(Err(e)) => return DirectPlanOutcome::FallbackToYtDlp(Some(e)),
            };

            let known_size = info.file_size.or(stream.filesize);
            let combined_total = known_size.unwrap_or(0);
            let clean_stream_ext = if stream.ext.trim().is_empty() {
                target_ext.to_string()
            } else {
                stream.ext.trim().to_ascii_lowercase()
            };

            if is_audio {
                let atmp = parent.join(format!("{}.atmp.{}", target_name, clean_stream_ext));
                match download_http_stream_phase(
                    app,
                    task_id,
                    &downloader,
                    &stream,
                    &info.final_url,
                    &atmp,
                    target,
                    known_size,
                    info.supports_range,
                    num_connections,
                    is_youtube,
                    cookie,
                    referer,
                    user_agent,
                    proxy_url,
                    auth_username,
                    auth_password,
                    speed_limit_bps,
                    0,
                    combined_total,
                    94.0,
                    http_ctrl,
                )
                .await
                {
                    HttpPhaseOutcome::CanceledInPhase => {
                        return DirectPlanOutcome::Stopped {
                            paused_already_emitted: false,
                        };
                    }
                    HttpPhaseOutcome::PausedInPhase => {
                        return DirectPlanOutcome::Stopped {
                            paused_already_emitted: true,
                        };
                    }
                    HttpPhaseOutcome::Failed(e) => {
                        remove_split_temp_files(target);
                        return DirectPlanOutcome::FallbackToYtDlp(Some(e));
                    }
                    HttpPhaseOutcome::Completed(written) => {
                        emit_simple_telemetry(
                            app,
                            task_id,
                            written,
                            written.max(combined_total),
                            0,
                            95.0,
                            Some(2),
                            "downloading",
                            target,
                            None,
                        );
                        match convert_audio_stream_to_target(
                            &atmp,
                            target,
                            target_ext,
                            ffmpeg_path,
                            Some(http_ctrl),
                        )
                        .await
                        {
                            Ok(true) => return DirectPlanOutcome::Finished,
                            Ok(false) => {
                                return DirectPlanOutcome::Stopped {
                                    paused_already_emitted: false,
                                }
                            }
                            Err(e) => {
                                remove_split_temp_files(target);
                                return DirectPlanOutcome::FallbackToYtDlp(Some(e));
                            }
                        }
                    }
                }
            } else {
                let same_ext =
                    target_ext.is_empty() || clean_stream_ext.eq_ignore_ascii_case(target_ext);
                if same_ext {
                    match download_http_stream_phase(
                        app,
                        task_id,
                        &downloader,
                        &stream,
                        &info.final_url,
                        target,
                        target,
                        known_size,
                        info.supports_range,
                        num_connections,
                        is_youtube,
                        cookie,
                        referer,
                        user_agent,
                        proxy_url,
                        auth_username,
                        auth_password,
                        speed_limit_bps,
                        0,
                        combined_total,
                        99.5,
                        http_ctrl,
                    )
                    .await
                    {
                        HttpPhaseOutcome::Completed(_) => return DirectPlanOutcome::Finished,
                        HttpPhaseOutcome::CanceledInPhase => {
                            return DirectPlanOutcome::Stopped {
                                paused_already_emitted: false,
                            }
                        }
                        HttpPhaseOutcome::PausedInPhase => {
                            return DirectPlanOutcome::Stopped {
                                paused_already_emitted: true,
                            }
                        }
                        HttpPhaseOutcome::Failed(e) => {
                            let target_meta = core_engine::downloader::checkpoint_path_for(target);
                            if target_meta.exists() {
                                let _ = std::fs::remove_file(target);
                                let _ = std::fs::remove_file(&target_meta);
                            }
                            return DirectPlanOutcome::FallbackToYtDlp(Some(e));
                        }
                    }
                } else if let Some(ff_path) = ffmpeg_path {
                    let out_ext = if target_ext.is_empty() {
                        "mp4"
                    } else {
                        target_ext
                    };
                    let vtmp = parent.join(format!("{}.vtmp.{}", target_name, clean_stream_ext));
                    let mux_tmp = parent.join(format!("{}.mux.{}", target_name, out_ext));
                    let written = match download_http_stream_phase(
                        app,
                        task_id,
                        &downloader,
                        &stream,
                        &info.final_url,
                        &vtmp,
                        target,
                        known_size,
                        info.supports_range,
                        num_connections,
                        is_youtube,
                        cookie,
                        referer,
                        user_agent,
                        proxy_url,
                        auth_username,
                        auth_password,
                        speed_limit_bps,
                        0,
                        combined_total,
                        96.0,
                        http_ctrl,
                    )
                    .await
                    {
                        HttpPhaseOutcome::Completed(w) => w,
                        HttpPhaseOutcome::CanceledInPhase => {
                            return DirectPlanOutcome::Stopped {
                                paused_already_emitted: false,
                            }
                        }
                        HttpPhaseOutcome::PausedInPhase => {
                            return DirectPlanOutcome::Stopped {
                                paused_already_emitted: true,
                            }
                        }
                        HttpPhaseOutcome::Failed(e) => {
                            remove_split_temp_files(target);
                            return DirectPlanOutcome::FallbackToYtDlp(Some(e));
                        }
                    };

                    emit_simple_telemetry(
                        app,
                        task_id,
                        written,
                        written.max(combined_total),
                        0,
                        98.0,
                        Some(1),
                        "downloading",
                        target,
                        None,
                    );

                    let _ = std::fs::remove_file(&mux_tmp);
                    let mut ff_cmd = tokio::process::Command::new(ff_path);
                    hide_window_tokio(&mut ff_cmd);
                    ff_cmd.arg("-y").args(["-v", "error"]).arg("-i").arg(&vtmp);
                    let vcodec_l = stream.vcodec.to_ascii_lowercase();
                    let need_video_transcode = out_ext == "mp4"
                        && (vcodec_l.starts_with("vp8") || vcodec_l.starts_with("theora"));
                    if out_ext == "mp4" {
                        let acodec_l = stream.acodec.to_ascii_lowercase();
                        if need_video_transcode {
                            ff_cmd.args([
                                "-c:v",
                                "libx264",
                                "-preset",
                                "veryfast",
                                "-crf",
                                "18",
                                "-pix_fmt",
                                "yuv420p",
                            ]);
                        } else {
                            ff_cmd.args(["-c:v", "copy"]);
                        }
                        if acodec_l == "none" || acodec_l.starts_with("mp4a") || acodec_l == "aac" {
                            ff_cmd.args(["-c:a", "copy"]);
                        } else {
                            ff_cmd.args(["-c:a", "aac", "-b:a", "192k"]);
                        }
                        ff_cmd.args(["-movflags", "+faststart"]);
                    } else {
                        ff_cmd.args(["-c", "copy"]);
                    }
                    ff_cmd.arg(&mux_tmp);

                    let mux_err = match run_ffmpeg_with_ctrl(ff_cmd, Some(http_ctrl), &mux_tmp).await {
                        Ok(()) => {
                            if move_file_cross_device(&mux_tmp, target).is_ok() {
                                remove_split_temp_files(target);
                                return DirectPlanOutcome::Finished;
                            }
                            "Gagal memindahkan berkas video hasil remux.".to_string()
                        }
                        Err(None) => {
                            return DirectPlanOutcome::Stopped {
                                paused_already_emitted: false,
                            };
                        }
                        Err(Some(msg)) => msg,
                    };
                    remove_split_temp_files(target);
                    return DirectPlanOutcome::FallbackToYtDlp(Some(mux_err));
                } else {
                    return DirectPlanOutcome::FallbackToYtDlp(None);
                }
            }
        }

        core_engine::video_runtime::VideoStreamPlan::SplitVideoAudio { video, audio } => {
            let Some(ff_path) = ffmpeg_path else {
                return DirectPlanOutcome::FallbackToYtDlp(None);
            };

            let insp_fut = async {
                tokio::join!(
                    inspect_direct_http_stream(
                        &inspector,
                        &video,
                        is_youtube,
                        cookie,
                        referer,
                        user_agent,
                        proxy_url,
                        auth_username,
                        auth_password,
                    ),
                    inspect_direct_http_stream(
                        &inspector,
                        &audio,
                        is_youtube,
                        cookie,
                        referer,
                        user_agent,
                        proxy_url,
                        auth_username,
                        auth_password,
                    ),
                )
            };
            let (v_res, a_res) = tokio::select! {
                pair = insp_fut => pair,
                _ = http_ctrl.notified() => {
                    return DirectPlanOutcome::Stopped {
                        paused_already_emitted: false,
                    }
                }
            };
            let v_info = match v_res {
                Ok(r) => r,
                Err(e) => return DirectPlanOutcome::FallbackToYtDlp(Some(e)),
            };
            let a_info = match a_res {
                Ok(r) => r,
                Err(e) => return DirectPlanOutcome::FallbackToYtDlp(Some(e)),
            };

            let v_size = v_info.file_size.or(video.filesize);
            let a_size = a_info.file_size.or(audio.filesize);
            let combined_est = v_size.unwrap_or(0).saturating_add(a_size.unwrap_or(0));

            let v_ext = if video.ext.trim().is_empty() {
                "mp4"
            } else {
                video.ext.trim()
            };
            let a_ext = if audio.ext.trim().is_empty() {
                "m4a"
            } else {
                audio.ext.trim()
            };
            let out_ext = if target_ext.is_empty() {
                "mp4"
            } else {
                target_ext
            };

            let vtmp = parent.join(format!("{}.vtmp.{}", target_name, v_ext));
            let atmp = parent.join(format!("{}.atmp.{}", target_name, a_ext));
            let mux_tmp = parent.join(format!("{}.mux.{}", target_name, out_ext));

            // Phase 1: Video track (up to 32 HTTP Range connections)
            let v_meta = core_engine::downloader::checkpoint_path_for(&vtmp);
            let existing_v_len = std::fs::metadata(&vtmp)
                .ok()
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .unwrap_or(0);
            let v_already_done = resume_mode
                && vtmp.is_file()
                && !v_meta.exists()
                && v_size.map_or(false, |sz| sz > 0 && existing_v_len == sz);

            let v_written = if v_already_done {
                existing_v_len
            } else {
                match download_http_stream_phase(
                    app,
                    task_id,
                    &downloader,
                    &video,
                    &v_info.final_url,
                    &vtmp,
                    target,
                    v_size,
                    v_info.supports_range,
                    num_connections,
                    is_youtube,
                    cookie,
                    referer,
                    user_agent,
                    proxy_url,
                    auth_username,
                    auth_password,
                    speed_limit_bps,
                    0,
                    combined_est,
                    96.0,
                    http_ctrl,
                )
                .await
                {
                    HttpPhaseOutcome::Completed(w) => w,
                    HttpPhaseOutcome::CanceledInPhase => {
                        return DirectPlanOutcome::Stopped {
                            paused_already_emitted: false,
                        }
                    }
                    HttpPhaseOutcome::PausedInPhase => {
                        return DirectPlanOutcome::Stopped {
                            paused_already_emitted: true,
                        }
                    }
                    HttpPhaseOutcome::Failed(e) => {
                        remove_split_temp_files(target);
                        return DirectPlanOutcome::FallbackToYtDlp(Some(e));
                    }
                }
            };

            // Phase 2: Audio companion track (up to 32 HTTP Range connections)
            let actual_combined = v_written.saturating_add(a_size.unwrap_or(0));
            let a_meta = core_engine::downloader::checkpoint_path_for(&atmp);
            let existing_a_len = std::fs::metadata(&atmp)
                .ok()
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .unwrap_or(0);
            let a_already_done = resume_mode
                && atmp.is_file()
                && !a_meta.exists()
                && a_size.map_or(false, |sz| sz > 0 && existing_a_len == sz);

            let a_written = if a_already_done {
                existing_a_len
            } else {
                match download_http_stream_phase(
                    app,
                    task_id,
                    &downloader,
                    &audio,
                    &a_info.final_url,
                    &atmp,
                    target,
                    a_size,
                    a_info.supports_range,
                    num_connections,
                    is_youtube,
                    cookie,
                    referer,
                    user_agent,
                    proxy_url,
                    auth_username,
                    auth_password,
                    speed_limit_bps,
                    v_written,
                    actual_combined,
                    98.0,
                    http_ctrl,
                )
                .await
                {
                    HttpPhaseOutcome::Completed(w) => w,
                    HttpPhaseOutcome::CanceledInPhase => {
                        return DirectPlanOutcome::Stopped {
                            paused_already_emitted: false,
                        }
                    }
                    HttpPhaseOutcome::PausedInPhase => {
                        return DirectPlanOutcome::Stopped {
                            paused_already_emitted: true,
                        }
                    }
                    HttpPhaseOutcome::Failed(e) => {
                        remove_split_temp_files(target);
                        return DirectPlanOutcome::FallbackToYtDlp(Some(e));
                    }
                }
            };

            // Phase 3: FFmpeg fast stream-copy mux into destination container
            let total_bytes = v_written.saturating_add(a_written);
            emit_simple_telemetry(
                app,
                task_id,
                total_bytes,
                total_bytes,
                0,
                98.5,
                Some(1),
                "downloading",
                target,
                None,
            );

            let _ = std::fs::remove_file(&mux_tmp);
            let mut ff_cmd = tokio::process::Command::new(ff_path);
            hide_window_tokio(&mut ff_cmd);
            ff_cmd
                .arg("-y")
                .args(["-v", "error"])
                .arg("-i")
                .arg(&vtmp)
                .arg("-i")
                .arg(&atmp)
                .args(["-map", "0:v:0", "-map", "1:a:0"]);

            let vcodec_l = video.vcodec.to_ascii_lowercase();
            let need_video_transcode = out_ext == "mp4"
                && (vcodec_l.starts_with("vp8") || vcodec_l.starts_with("theora"));
            if need_video_transcode {
                ff_cmd.args([
                    "-c:v",
                    "libx264",
                    "-preset",
                    "veryfast",
                    "-crf",
                    "18",
                    "-pix_fmt",
                    "yuv420p",
                ]);
            } else {
                ff_cmd.args(["-c:v", "copy"]);
            }

            let is_mp4_audio = audio.ext.eq_ignore_ascii_case("m4a")
                || audio.acodec.to_ascii_lowercase().starts_with("mp4a")
                || audio.acodec.eq_ignore_ascii_case("aac");
            if out_ext == "mp4" && !is_mp4_audio {
                ff_cmd.args(["-c:a", "aac", "-b:a", "192k"]);
            } else {
                ff_cmd.args(["-c:a", "copy"]);
            }
            if out_ext == "mp4" {
                ff_cmd.args(["-movflags", "+faststart"]);
            }
            ff_cmd.arg(&mux_tmp);

            let mux_err = match run_ffmpeg_with_ctrl(ff_cmd, Some(http_ctrl), &mux_tmp).await {
                Ok(()) => {
                    if move_file_cross_device(&mux_tmp, target).is_ok() {
                        remove_split_temp_files(target);
                        return DirectPlanOutcome::Finished;
                    }
                    "Gagal memindahkan berkas video hasil muxing.".to_string()
                }
                Err(None) => {
                    return DirectPlanOutcome::Stopped {
                        paused_already_emitted: false,
                    };
                }
                Err(Some(msg)) => msg,
            };
            remove_split_temp_files(target);
            DirectPlanOutcome::FallbackToYtDlp(Some(mux_err))
        }
    }
}

#[tauri::command]
async fn start_real_download(
    app: tauri::AppHandle,
    task_id: String,
    url: String,
    destination_path: String,
    num_connections: usize,
    format_id: Option<String>,
    cookie: Option<String>,
    referer: Option<String>,
    user_agent: Option<String>,
    speed_limit_kbs: Option<u64>,
    resume: Option<bool>,
    proxy_url: Option<String>,
    auth_username: Option<String>,
    auth_password: Option<String>,
    allow_url_refresh: Option<bool>,
) -> Result<(), String> {
    start_real_download_with_controller(
        app,
        task_id,
        url,
        destination_path,
        num_connections,
        format_id,
        cookie,
        referer,
        user_agent,
        speed_limit_kbs,
        resume,
        proxy_url,
        auth_username,
        auth_password,
        allow_url_refresh,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn start_real_download_with_controller(
    app: tauri::AppHandle,
    task_id: String,
    url: String,
    destination_path: String,
    num_connections: usize,
    format_id: Option<String>,
    cookie: Option<String>,
    referer: Option<String>,
    user_agent: Option<String>,
    speed_limit_kbs: Option<u64>,
    resume: Option<bool>,
    proxy_url: Option<String>,
    auth_username: Option<String>,
    auth_password: Option<String>,
    allow_url_refresh: Option<bool>,
    pre_controller: Option<core_engine::DownloadController>,
) -> Result<(), String> {
    let resume_mode = resume.unwrap_or(false);
    let raw_target = resolve_path(&destination_path);
    let target = if resume_mode {
        raw_target
    } else {
        resolve_unique_destination(&raw_target)
    };
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let effective_connections = num_connections.clamp(1, 32);

    tauri::async_runtime::spawn(async move {
        register_task_target(&task_id, &target);

        let http_ctrl = match pre_controller {
            Some(c) => c,
            None => {
                let c = core_engine::DownloadController::new();
                register_active_download(&task_id, ActiveControl::Http(c.clone()));
                c
            }
        };

        if http_ctrl.is_canceled() {
            unregister_active_download(&task_id);
            remove_task_target(&task_id);
            cleanup_download_artifacts(&target);
            emit_simple_telemetry(
                &app, &task_id, 0, 0, 0, 0.0, None, "canceled", &target, None,
            );
            return;
        }
        if http_ctrl.is_paused() {
            unregister_active_download(&task_id);
            emit_simple_telemetry(
                &app, &task_id, 0, 0, 0, 0.0, None, "paused", &target, None,
            );
            return;
        }

        let is_video_or_stream = !core_engine::torrent::is_torrent_reference(&url)
            && (is_video_or_stream_url(&url)
                || format_id.as_deref().map_or(false, |f| !f.trim().is_empty()));

        if is_video_or_stream {
            // Emit initial progress immediately
            emit_simple_telemetry(
                &app,
                &task_id,
                0,
                0,
                0,
                2.0,
                None,
                "downloading",
                &target,
                None,
            );

            if get_ffmpeg_bin().is_none() {
                tokio::spawn(async {
                    if let Err(e) = ensure_ffmpeg_bin().await {
                        log::warn!("Warmup auto-provisioning FFmpeg saat mulai unduhan: {}", e);
                    }
                });
            }

            let ytdlp_res = tokio::select! {
                res = ensure_ytdlp_ready_for_video() => Some(res),
                _ = http_ctrl.notified() => None,
            };

            let Some(ytdlp_res) = ytdlp_res else {
                unregister_active_download(&task_id);
                if http_ctrl.is_canceled() {
                    remove_task_target(&task_id);
                    cleanup_download_artifacts(&target);
                    emit_simple_telemetry(
                        &app, &task_id, 0, 0, 0, 0.0, None, "canceled", &target, None,
                    );
                } else {
                    emit_simple_telemetry(
                        &app, &task_id, 0, 0, 0, 0.0, None, "paused", &target, None,
                    );
                }
                return;
            };

            let mut ytdlp_error_detail: Option<String> = ytdlp_res.as_ref().err().cloned();

            if let Ok(bin) = ytdlp_res {
                let target_ext = target
                    .extension()
                    .map(|e| e.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                let is_audio = matches!(target_ext.as_str(), "mp3" | "m4a" | "flac" | "wav")
                    || matches!(format_id.as_deref(), Some("audio_mp3") | Some("audio_m4a"));

                let mut raw_key_opt: Option<String> = None;
                let mut fetched_from_cache = false;
                let mut current_json: Option<serde_json::Value> = None;

                if !is_direct_manifest_url(&url) {
                    let raw_key = raw_video_cache_key(
                        &url,
                        cookie.as_deref(),
                        referer.as_deref(),
                        user_agent.as_deref(),
                        proxy_url.as_deref(),
                        auth_username.as_deref(),
                        auth_password.as_deref(),
                    );
                    let cached_json = get_cached_raw_ytdlp_json(&raw_key);
                    fetched_from_cache = cached_json.is_some();

                    current_json = match cached_json {
                        Some(v) => Some(v),
                        None => {
                            match fetch_video_json_for_download(
                                &bin,
                                &url,
                                cookie.as_deref(),
                                referer.as_deref(),
                                user_agent.as_deref(),
                                proxy_url.as_deref(),
                                auth_username.as_deref(),
                                auth_password.as_deref(),
                                &http_ctrl,
                            )
                            .await
                            {
                                None => {
                                    unregister_active_download(&task_id);
                                    if http_ctrl.is_canceled() {
                                        remove_task_target(&task_id);
                                        cleanup_download_artifacts(&target);
                                        emit_simple_telemetry(
                                            &app, &task_id, 0, 0, 0, 0.0, None, "canceled",
                                            &target, None,
                                        );
                                    } else {
                                        emit_simple_telemetry(
                                            &app, &task_id, 0, 0, 0, 0.0, None, "paused",
                                            &target, None,
                                        );
                                    }
                                    return;
                                }
                                Some(Ok(v)) => {
                                    put_cached_raw_ytdlp_json(&raw_key, &v);
                                    Some(v)
                                }
                                Some(Err(e)) => {
                                    ytdlp_error_detail = Some(e);
                                    None
                                }
                            }
                        }
                    };
                    raw_key_opt = Some(raw_key);
                }

                let split_required_by_video = !is_audio
                    && current_json.as_ref().map_or_else(
                        || parse_requested_height_opt(format_id.as_deref()).map_or(true, |h| h > 360),
                        |j| core_engine::video_runtime::requested_height_requires_split(j, format_id.as_deref()),
                    );
                let conversion_needs_ffmpeg = is_audio && target_ext != "m4a";

                let mut ffmpeg_path = get_ffmpeg_bin();
                if ffmpeg_path.is_none() && (split_required_by_video || conversion_needs_ffmpeg) {
                    emit_simple_telemetry(
                        &app,
                        &task_id,
                        0,
                        0,
                        0,
                        4.0,
                        None,
                        "downloading",
                        &target,
                        None,
                    );

                    let ff_res = tokio::select! {
                        res = ensure_ffmpeg_bin() => Some(res),
                        _ = http_ctrl.notified() => None,
                    };
                    let Some(ff_res) = ff_res else {
                        unregister_active_download(&task_id);
                        if http_ctrl.is_canceled() {
                            remove_task_target(&task_id);
                            cleanup_download_artifacts(&target);
                            emit_simple_telemetry(
                                &app, &task_id, 0, 0, 0, 0.0, None, "canceled", &target, None,
                            );
                        } else {
                            emit_simple_telemetry(
                                &app, &task_id, 0, 0, 0, 0.0, None, "paused", &target, None,
                            );
                        }
                        return;
                    };
                    match ff_res {
                        Ok(p) => ffmpeg_path = Some(p),
                        Err(e) => {
                            if ytdlp_error_detail.is_none() {
                                ytdlp_error_detail = Some(e);
                            }
                        }
                    }
                }
                let has_ffmpeg = ffmpeg_path.is_some();

                if !has_ffmpeg && split_required_by_video {
                    unregister_active_download(&task_id);
                    remove_task_target(&task_id);
                    let msg = ytdlp_error_detail.unwrap_or_else(|| {
                        "FFmpeg diperlukan untuk mengunduh & menggabungkan video resolusi tinggi tanpa turun ke SD."
                            .to_string()
                    });
                    emit_simple_telemetry(
                        &app,
                        &task_id,
                        0,
                        0,
                        0,
                        0.0,
                        None,
                        "error",
                        &target,
                        Some(msg),
                    );
                    return;
                }

                // Fast path: resolve direct HTTP(S) streams (progressive or split DASH video+audio)
                // and run through multi-connection core_engine::Downloader with up to 32 threads!
                if let Some(ref raw_key) = raw_key_opt {
                    for attempt in 0..2 {
                        let Some(ref json_val) = current_json else {
                            break;
                        };
                        let Some(plan) = core_engine::video_runtime::extract_direct_stream_plan(
                            json_val,
                            format_id.as_deref(),
                            &target_ext,
                            is_audio,
                            has_ffmpeg,
                        ) else {
                            break;
                        };

                        let speed_limit_bps = http_ctrl
                            .speed_limit()
                            .or_else(|| speed_limit_kbs.filter(|&k| k > 0).map(|k| k * 1024));

                        match execute_direct_http_video_plan(
                            &app,
                            &task_id,
                            &target,
                            &target_ext,
                            is_audio,
                            effective_connections,
                            ffmpeg_path.as_deref(),
                            is_youtube_url(&url),
                            cookie.as_deref(),
                            referer.as_deref(),
                            user_agent.as_deref(),
                            proxy_url.as_deref(),
                            auth_username.as_deref(),
                            auth_password.as_deref(),
                            speed_limit_bps,
                            resume_mode,
                            &http_ctrl,
                            plan,
                        )
                        .await
                        {
                            DirectPlanOutcome::Finished => {
                                unregister_active_download(&task_id);
                                let file_len = std::fs::metadata(&target)
                                    .ok()
                                    .filter(|m| m.is_file())
                                    .map(|m| m.len())
                                    .unwrap_or(0);
                                remove_task_target(&task_id);
                                if file_len == 0 {
                                    emit_simple_telemetry(
                                        &app,
                                        &task_id,
                                        0,
                                        0,
                                        0,
                                        0.0,
                                        None,
                                        "error",
                                        &target,
                                        Some(
                                            "Berkas hasil unduhan video/audio kosong atau tidak tersimpan ke disk."
                                                .to_string(),
                                        ),
                                    );
                                } else {
                                    emit_simple_telemetry(
                                        &app,
                                        &task_id,
                                        file_len,
                                        file_len,
                                        0,
                                        100.0,
                                        Some(0),
                                        "completed",
                                        &target,
                                        None,
                                    );
                                }
                                return;
                            }
                            DirectPlanOutcome::Stopped {
                                paused_already_emitted,
                            } => {
                                unregister_active_download(&task_id);
                                if http_ctrl.is_canceled() {
                                    remove_task_target(&task_id);
                                    cleanup_download_artifacts(&target);
                                    emit_simple_telemetry(
                                        &app, &task_id, 0, 0, 0, 0.0, None, "canceled", &target,
                                        None,
                                    );
                                } else if !paused_already_emitted {
                                    emit_simple_telemetry(
                                        &app, &task_id, 0, 0, 0, 0.0, None, "paused", &target,
                                        None,
                                    );
                                }
                                return;
                            }
                            DirectPlanOutcome::FallbackToYtDlp(err_msg) => {
                                invalidate_cached_raw_ytdlp_json(raw_key);
                                if let Some(msg) = err_msg {
                                    log::warn!(
                                        "Direct HTTP stream fallback for {} (attempt {}): {}",
                                        url,
                                        attempt + 1,
                                        msg
                                    );
                                    ytdlp_error_detail = Some(msg);
                                }
                                // If initial URL came from short-lived cache, refresh JSON once cleanly.
                                if fetched_from_cache && attempt == 0 {
                                    fetched_from_cache = false;
                                    match fetch_video_json_for_download(
                                        &bin,
                                        &url,
                                        cookie.as_deref(),
                                        referer.as_deref(),
                                        user_agent.as_deref(),
                                        proxy_url.as_deref(),
                                        auth_username.as_deref(),
                                        auth_password.as_deref(),
                                        &http_ctrl,
                                    )
                                    .await
                                    {
                                        None => {
                                            unregister_active_download(&task_id);
                                            if http_ctrl.is_canceled() {
                                                remove_task_target(&task_id);
                                                cleanup_download_artifacts(&target);
                                                emit_simple_telemetry(
                                                    &app, &task_id, 0, 0, 0, 0.0, None,
                                                    "canceled", &target, None,
                                                );
                                            } else {
                                                emit_simple_telemetry(
                                                    &app, &task_id, 0, 0, 0, 0.0, None, "paused",
                                                    &target, None,
                                                );
                                            }
                                            return;
                                        }
                                        Some(Ok(fresh)) => {
                                            put_cached_raw_ytdlp_json(raw_key, &fresh);
                                            current_json = Some(fresh);
                                            continue;
                                        }
                                        Some(Err(_)) => break,
                                    }
                                }
                                break;
                            }
                        }
                    }
                }

                // Fallback path (HLS/DASH manifests or non-Range video CDNs): delegate to yt-dlp -N <conns>
                let paused_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let canceled_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let stop_notify = Arc::new(Notify::new());

                register_active_download(
                    &task_id,
                    ActiveControl::YtDlp {
                        paused: paused_flag.clone(),
                        canceled: canceled_flag.clone(),
                        stop_notify: stop_notify.clone(),
                    },
                );

                // Honor any pause/cancel that raced right before switching control type
                if http_ctrl.is_canceled() {
                    unregister_active_download(&task_id);
                    remove_task_target(&task_id);
                    cleanup_download_artifacts(&target);
                    emit_simple_telemetry(
                        &app, &task_id, 0, 0, 0, 0.0, None, "canceled", &target, None,
                    );
                    return;
                }
                if http_ctrl.is_paused() {
                    unregister_active_download(&task_id);
                    emit_simple_telemetry(
                        &app, &task_id, 0, 0, 0, 0.0, None, "paused", &target, None,
                    );
                    return;
                }

                let tmp_video = if is_audio {
                    if target_ext == "m4a" && !has_ffmpeg {
                        target.with_extension("temp_stream.m4a")
                    } else {
                        target.with_extension("temp_stream.mp4")
                    }
                } else {
                    target.clone()
                };

                let mut cmd = tokio::process::Command::new(&bin);
                hide_window_tokio(&mut cmd);
                let has_js_rt = apply_ytdlp_runtime_env(&mut cmd);
                if !has_js_rt && is_youtube_url(&url) {
                    cmd.arg("--extractor-args")
                        .arg(core_engine::video_runtime::youtube_fallback_extractor_args());
                }
                cmd.arg("--no-playlist");
                cmd.arg("--newline");
                if effective_connections > 1 {
                    cmd.arg("-N").arg(effective_connections.to_string());
                }
                let eff_kbs = http_ctrl
                    .speed_limit()
                    .map(|bps| if bps == 0 { 0 } else { (bps / 1024).max(1) })
                    .or(speed_limit_kbs);
                if let Some(kbs) = eff_kbs.filter(|&k| k > 0) {
                    cmd.arg("--limit-rate").arg(format!("{}K", kbs));
                }
                apply_ytdlp_headers(
                    &mut cmd,
                    cookie.as_deref(),
                    referer.as_deref(),
                    user_agent.as_deref(),
                );
                apply_ytdlp_network_options(
                    &mut cmd,
                    proxy_url.as_deref(),
                    auth_username.as_deref(),
                    auth_password.as_deref(),
                );

                if let Some(ref ff_path) = ffmpeg_path {
                    cmd.arg("--ffmpeg-location").arg(ff_path);
                }

                let fmt_selector = build_ytdlp_format_selector_with_strict(
                    format_id.as_deref(),
                    &target_ext,
                    is_audio,
                    has_ffmpeg,
                    split_required_by_video,
                );

                cmd.arg("-f").arg(&fmt_selector);
                if !is_audio && has_ffmpeg {
                    let merge_ext = match target_ext.as_str() {
                        "mkv" | "webm" => target_ext.as_str(),
                        _ => "mp4",
                    };
                    cmd.arg("--merge-output-format").arg(merge_ext);
                }
                cmd.arg("-o").arg(&tmp_video);
                cmd.arg(&url);
                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());

                match cmd.spawn() {
                    Ok(mut child) => {
                        let stdout = child.stdout.take();
                        let stderr = child.stderr.take();
                        let app_for_lines = app.clone();
                        let tid_for_lines = task_id.clone();
                        let target_for_lines = target.clone();

                        // Read progress lines asynchronously from yt-dlp stdout
                        let line_task = tokio::spawn(async move {
                            if let Some(out) = stdout {
                                use tokio::io::AsyncBufReadExt;
                                let mut reader = tokio::io::BufReader::new(out).lines();
                                while let Ok(Some(line)) = reader.next_line().await {
                                    if let Some((pct, dl_bytes, total_bytes, speed_bps, eta)) =
                                        parse_ytdlp_progress_line(&line)
                                    {
                                        let scaled_pct = (pct * 0.9).min(90.0);
                                        emit_simple_telemetry(
                                            &app_for_lines,
                                            &tid_for_lines,
                                            dl_bytes,
                                            total_bytes,
                                            speed_bps,
                                            scaled_pct,
                                            eta,
                                            "downloading",
                                            &target_for_lines,
                                            None,
                                        );
                                    }
                                }
                            }
                        });

                        // Drain stderr concurrently so yt-dlp cannot block and real error text is captured
                        let err_task = tokio::spawn(async move {
                            let mut last_explicit_err: Option<String> = None;
                            let mut last_other_line: Option<String> = None;
                            if let Some(err_stream) = stderr {
                                use tokio::io::AsyncBufReadExt;
                                let mut reader = tokio::io::BufReader::new(err_stream).lines();
                                while let Ok(Some(line)) = reader.next_line().await {
                                    let trimmed = line.trim();
                                    if trimmed.starts_with("ERROR:") {
                                        last_explicit_err = Some(trimmed.to_string());
                                    } else if !trimmed.is_empty() && !trimmed.starts_with("WARNING:") {
                                        last_other_line = Some(trimmed.to_string());
                                    }
                                }
                            }
                            last_explicit_err.or(last_other_line)
                        });

                        let status = tokio::select! {
                            res = child.wait() => Some(res),
                            _ = stop_notify.notified() => {
                                let _ = child.kill().await;
                                let _ = child.wait().await;
                                None
                            }
                        };
                        let _ = line_task.await;
                        if let Ok(Some(err_msg)) = err_task.await {
                            ytdlp_error_detail = Some(err_msg);
                        }
                        unregister_active_download(&task_id);

                        if canceled_flag.load(std::sync::atomic::Ordering::SeqCst) {
                            remove_task_target(&task_id);
                            cleanup_download_artifacts(&target);
                            emit_simple_telemetry(
                                &app, &task_id, 0, 0, 0, 0.0, None, "canceled", &target, None,
                            );
                            return;
                        }

                        if paused_flag.load(std::sync::atomic::Ordering::SeqCst) {
                            emit_simple_telemetry(
                                &app, &task_id, 0, 0, 0, 0.0, None, "paused", &target, None,
                            );
                            return;
                        }

                        if let Some(Ok(s)) = status {
                            if s.success() {
                                if is_audio {
                                    emit_simple_telemetry(
                                        &app,
                                        &task_id,
                                        9_500_000,
                                        10_000_000,
                                        500_000,
                                        95.0,
                                        Some(2),
                                        "downloading",
                                        &target,
                                        None,
                                    );

                                    if let Err(err_msg) = convert_audio_stream_to_target(
                                        &tmp_video,
                                        &target,
                                        &target_ext,
                                        ffmpeg_path.as_deref(),
                                        None,
                                    )
                                    .await
                                    {
                                        remove_task_target(&task_id);
                                        emit_simple_telemetry(
                                            &app,
                                            &task_id,
                                            0,
                                            0,
                                            0,
                                            0.0,
                                            None,
                                            "error",
                                            &target,
                                            Some(err_msg),
                                        );
                                        return;
                                    }
                                }

                                let file_len = std::fs::metadata(&target)
                                    .ok()
                                    .filter(|m| m.is_file())
                                    .map(|m| m.len())
                                    .unwrap_or(0);
                                remove_task_target(&task_id);
                                if file_len == 0 {
                                    emit_simple_telemetry(
                                        &app,
                                        &task_id,
                                        0,
                                        0,
                                        0,
                                        0.0,
                                        None,
                                        "error",
                                        &target,
                                        Some(
                                            "Berkas hasil unduhan video/audio kosong atau tidak tersimpan ke disk."
                                                .to_string(),
                                        ),
                                    );
                                    return;
                                }
                                emit_simple_telemetry(
                                    &app,
                                    &task_id,
                                    file_len,
                                    file_len,
                                    0,
                                    100.0,
                                    Some(0),
                                    "completed",
                                    &target,
                                    None,
                                );
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        ytdlp_error_detail = Some(format!("Gagal menjalankan yt-dlp: {}", e));
                        unregister_active_download(&task_id);
                    }
                }
            }

            remove_task_target(&task_id);
            unregister_active_download(&task_id);

            // Emitted if video stream extraction or provisioning failed
            emit_simple_telemetry(
                &app,
                &task_id,
                0,
                0,
                0,
                0.0,
                None,
                "error",
                &target,
                Some(ytdlp_error_detail.unwrap_or_else(|| {
                    "Gagal memproses stream video/audio. Pastikan link dapat diakses.".to_string()
                })),
            );
            return;
        }

        // Standard HTTP / HTTPS / FTP / BitTorrent download (reuse http_ctrl so pre-reserved slots work seamlessly)
        let controller = http_ctrl;

        let downloader = match core_engine::downloader::Downloader::new() {
            Ok(d) => d,
            Err(e) => {
                unregister_active_download(&task_id);
                emit_simple_telemetry(
                    &app,
                    &task_id,
                    0,
                    0,
                    0,
                    0.0,
                    None,
                    "error",
                    &target,
                    Some(format!("Gagal inisialisasi downloader: {}", e)),
                );
                return;
            }
        };

        // Normalize bare local .torrent file paths into cached dc-torrent:// references
        let effective_url = {
            let trimmed = url.trim();
            let lower = trimmed.to_ascii_lowercase();
            if core_engine::torrent::is_torrent_reference(trimmed)
                && !lower.starts_with("http://")
                && !lower.starts_with("https://")
                && !lower.starts_with("ftp://")
                && !lower.starts_with("magnet:")
                && !lower.starts_with("dc-torrent://")
                && !lower.starts_with("file://")
            {
                let local_p = resolve_path(trimmed);
                if local_p.is_file() {
                    core_engine::torrent::inspect_local_torrent_file(&local_p)
                        .map(|(dc_ref, _)| dc_ref)
                        .unwrap_or_else(|_| url.clone())
                } else {
                    url.clone()
                }
            } else {
                url.clone()
            }
        };

        // Inspect URL to get real file size, Range support, Torrent info, and reject HTML hijack pages
        let inspector = match core_engine::inspector::UrlInspector::new() {
            Ok(i) => i,
            Err(e) => {
                unregister_active_download(&task_id);
                emit_simple_telemetry(
                    &app,
                    &task_id,
                    0,
                    0,
                    0,
                    0.0,
                    None,
                    "error",
                    &target,
                    Some(format!("Gagal inisialisasi inspektor URL: {}", e)),
                );
                return;
            }
        };

        let inspect_outcome = tokio::select! {
            res = inspector.inspect_with_options(
                &effective_url,
                core_engine::inspector::InspectOptions {
                    cookie: cookie.as_deref(),
                    referer: referer.as_deref(),
                    user_agent: user_agent.as_deref(),
                    proxy_url: proxy_url.as_deref(),
                    auth_username: auth_username.as_deref(),
                    auth_password: auth_password.as_deref(),
                },
            ) => Some(res),
            _ = controller.notified() => None,
        };

        let inspect_res = match inspect_outcome {
            None => {
                unregister_active_download(&task_id);
                if controller.is_canceled() {
                    remove_task_target(&task_id);
                    cleanup_download_artifacts(&target);
                    emit_simple_telemetry(&app, &task_id, 0, 0, 0, 0.0, None, "canceled", &target, None);
                } else {
                    emit_simple_telemetry(&app, &task_id, 0, 0, 0, 0.0, None, "paused", &target, None);
                }
                return;
            }
            Some(Ok(res)) => res,
            Some(Err(e)) => {
                unregister_active_download(&task_id);
                if controller.is_canceled() {
                    remove_task_target(&task_id);
                    cleanup_download_artifacts(&target);
                    emit_simple_telemetry(&app, &task_id, 0, 0, 0, 0.0, None, "canceled", &target, None);
                } else {
                    emit_simple_telemetry(
                        &app,
                        &task_id,
                        0,
                        0,
                        0,
                        0.0,
                        None,
                        "error",
                        &target,
                        Some(format!("{}", e)),
                    );
                }
                return;
            }
        };

        let file_size = inspect_res.file_size;
        let supports_range = inspect_res.supports_range;
        let dl_url = inspect_res.final_url.clone();

        // Emit initial progress event only for fresh downloads (avoid 1% flicker on prebuffered resumes)
        if !resume_mode {
            emit_simple_telemetry(
                &app,
                &task_id,
                0,
                file_size.unwrap_or(0),
                0,
                1.0,
                None,
                "downloading",
                &target,
                None,
            );
        }

        let (tx, mut rx) = tokio::sync::broadcast::channel::<core_engine::model::ProgressTelemetry>(128);

        // Forward telemetry to Tauri frontend (including real ETA, peers/seeds, and per-thread segments!)
        let app_clone = app.clone();
        let target_str = target.display().to_string();
        let monitor_handle = tokio::spawn(async move {
            let mut last_downloaded = 0u64;
            let mut last_total = 0u64;
            while let Ok(telem) = rx.recv().await {
                last_downloaded = telem.downloaded_bytes;
                if let Some(t) = telem.total_bytes {
                    if t > 0 {
                        last_total = t;
                    }
                }
                let is_terminal = matches!(
                    telem.status,
                    core_engine::model::TaskStatus::Completed
                        | core_engine::model::TaskStatus::Error
                        | core_engine::model::TaskStatus::Paused
                        | core_engine::model::TaskStatus::Canceled
                );
                let segments_payload: Vec<SegmentProgressPayload> = telem
                    .segments
                    .into_iter()
                    .map(|s| SegmentProgressPayload {
                        segment_index: s.segment_index,
                        start_byte: s.start_byte,
                        end_byte: s.end_byte,
                        current_byte: s.current_byte,
                        downloaded_bytes: s.downloaded_bytes,
                        is_completed: s.is_completed,
                    })
                    .collect();

                let _ = app_clone.emit(
                    "download-telemetry",
                    DownloadTelemetryPayload {
                        task_id: telem.task_id.clone(),
                        downloaded_bytes: telem.downloaded_bytes,
                        total_bytes: telem.total_bytes.unwrap_or(0),
                        speed_bytes_per_sec: telem.speed_bytes_per_sec,
                        progress_percent: telem.progress_percent as f64,
                        eta_seconds: telem.eta_seconds,
                        status: telem.status.as_str().to_string(),
                        destination_path: target_str.clone(),
                        segments: segments_payload,
                        error: None,
                        peers: telem.peers,
                        seeds: telem.seeds,
                    },
                );
                if is_terminal {
                    break;
                }
            }
            (last_downloaded, last_total)
        });

        let speed_limit_bps = controller
            .speed_limit()
            .or_else(|| speed_limit_kbs.filter(|&k| k > 0).map(|k| k * 1024));
        let dl_opts = core_engine::downloader::DownloadOptions {
            cookie: cookie.as_deref(),
            referer: referer.as_deref(),
            user_agent: user_agent.as_deref(),
            speed_limit_bytes_per_sec: speed_limit_bps,
            proxy_url: proxy_url.as_deref(),
            auth_username: auth_username.as_deref(),
            auth_password: auth_password.as_deref(),
            allow_url_refresh: allow_url_refresh.unwrap_or(false),
        };
        let is_torrent_dl = inspect_res.torrent_info.is_some()
            || core_engine::torrent::is_torrent_reference(&dl_url)
            || core_engine::torrent::is_torrent_reference(&effective_url);

        let download_result = if is_torrent_dl {
            let torrent_source = if core_engine::torrent::is_torrent_reference(&dl_url) {
                &dl_url
            } else {
                &effective_url
            };
            core_engine::torrent::download_torrent(
                &task_id,
                torrent_source,
                &target,
                Some(tx),
                controller.clone(),
                &dl_opts,
            )
            .await
        } else {
            downloader
                .download_with_options(
                    &task_id,
                    &dl_url,
                    &target,
                    file_size,
                    supports_range,
                    effective_connections,
                    Some(tx),
                    Some(controller.clone()),
                    dl_opts,
                )
                .await
        };

        let (last_downloaded, last_total) = monitor_handle.await.unwrap_or((0, 0));
        unregister_active_download(&task_id);

        if controller.is_canceled() {
            remove_task_target(&task_id);
            cleanup_download_artifacts(&target);
            return;
        }

        if controller.is_paused() {
            return;
        }

        match download_result {
            Ok(_) => {
                remove_task_target(&task_id);
                let disk_len = path_content_size(&target);
                let reported_len = if disk_len > 0 {
                    disk_len
                } else {
                    last_total.max(last_downloaded)
                };
                emit_simple_telemetry(
                    &app,
                    &task_id,
                    reported_len,
                    reported_len,
                    0,
                    100.0,
                    Some(0),
                    "completed",
                    &target,
                    None,
                );
            }
            Err(e) => {
                emit_simple_telemetry(
                    &app,
                    &task_id,
                    0,
                    0,
                    0,
                    0.0,
                    None,
                    "error",
                    &target,
                    Some(format!("Unduhan gagal: {}", e)),
                );
            }
        }
    });

    Ok(())
}

#[tauri::command]
fn ensure_file_created(destination_path: String, _file_name: String) -> Result<String, String> {
    let target = resolve_path(&destination_path);
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    Ok(target.display().to_string())
}

#[tauri::command]
async fn inspect_url(
    url: String,
    cookie: Option<String>,
    referer: Option<String>,
    user_agent: Option<String>,
    proxy_url: Option<String>,
    auth_username: Option<String>,
    auth_password: Option<String>,
) -> Result<core_engine::model::InspectResult, String> {
    let trimmed = url.trim();
    if core_engine::torrent::is_torrent_reference(trimmed) {
        let lower = trimmed.to_ascii_lowercase();
        if !lower.starts_with("http://")
            && !lower.starts_with("https://")
            && !lower.starts_with("magnet:")
            && !lower.starts_with("dc-torrent://")
            && !lower.starts_with("file://")
        {
            let local_path = resolve_path(trimmed);
            if local_path.is_file() {
                return core_engine::torrent::inspect_local_torrent_file(&local_path)
                    .map(|(_, res)| res)
                    .map_err(|e| format!("Invalid .torrent file: {}", e));
            }
        }
    }

    if is_video_or_stream_url(&url) {
        match inspect_video_url_with_options(
            &url,
            cookie.as_deref(),
            referer.as_deref(),
            user_agent.as_deref(),
            proxy_url.as_deref(),
            auth_username.as_deref(),
            auth_password.as_deref(),
        )
        .await
        {
            Ok(res) => return Ok(res),
            Err(err) => {
                if !is_direct_manifest_url(&url) {
                    return Err(err);
                }
            }
        }
    }

    let inspector = core_engine::inspector::UrlInspector::new()
        .map_err(|e| format!("Gagal menginisialisasi pemeriksa URL: {}", e))?;
    inspector
        .inspect_with_options(
            &url,
            core_engine::inspector::InspectOptions {
                cookie: cookie.as_deref(),
                referer: referer.as_deref(),
                user_agent: user_agent.as_deref(),
                proxy_url: proxy_url.as_deref(),
                auth_username: auth_username.as_deref(),
                auth_password: auth_password.as_deref(),
            },
        )
        .await
        .map_err(|e| format!("{}", e))
}

#[tauri::command]
fn inspect_torrent_bytes(bytes: Vec<u8>) -> Result<core_engine::model::InspectResult, String> {
    if bytes.is_empty() {
        return Err("Empty .torrent file".to_string());
    }
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Torrent file exceeds 16 MiB safety limit".to_string());
    }
    let dc_ref = core_engine::torrent::cache_torrent_bytes(&bytes)
        .map_err(|e| format!("Failed to cache .torrent metadata: {}", e))?;
    core_engine::torrent::inspect_torrent_bytes(&dc_ref, &bytes)
        .map_err(|e| format!("Invalid .torrent file: {}", e))
}

#[tauri::command]
fn update_download_speed_limit(task_id: Option<String>, limit_kbs: u64) -> usize {
    let total_bps = if limit_kbs == 0 {
        0u64
    } else {
        limit_kbs.saturating_mul(1024)
    };
    let controls: Vec<ActiveControl> = {
        let map = active_downloads().lock().unwrap();
        if let Some(tid) = task_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            map.get(tid).cloned().into_iter().collect()
        } else {
            map.values().cloned().collect()
        }
    };

    if controls.is_empty() {
        return 0;
    }

    let per_task_bps = if task_id.as_deref().map(str::trim).filter(|s| !s.is_empty()).is_some() || total_bps == 0 {
        total_bps
    } else {
        let http_count = controls
            .iter()
            .filter(|c| matches!(c, ActiveControl::Http(_)))
            .count()
            .max(1) as u64;
        (total_bps / http_count).max(1024)
    };

    let mut updated = 0usize;
    for ctrl in &controls {
        if matches!(ctrl, ActiveControl::Http(_)) {
            ctrl.set_speed_limit(per_task_bps);
            updated += 1;
        }
    }
    updated
}

#[tauri::command]
async fn verify_file_checksum(path: String) -> Result<core_engine::model::FileChecksums, String> {
    let target = resolve_path(&path);
    tokio::task::spawn_blocking(move || {
        core_engine::integrity::compute_file_checksums(&target)
            .map_err(|e| format!("Gagal menghitung checksum berkas: {}", e))
    })
    .await
    .map_err(|e| format!("Task checksum gagal: {}", e))?
}

#[tauri::command]
fn check_disk_space(dir: String) -> Result<u64, String> {
    let target = resolve_path(&dir);
    core_engine::integrity::available_disk_space(&target)
        .ok_or_else(|| "Gagal membaca sisa ruang penyimpanan disk".to_string())
}

#[tauri::command]
async fn grab_site_links(
    url: String,
    max_depth: Option<usize>,
    same_host_only: Option<bool>,
    cookie: Option<String>,
    referer: Option<String>,
    user_agent: Option<String>,
    proxy_url: Option<String>,
    auth_username: Option<String>,
    auth_password: Option<String>,
) -> Result<Vec<core_engine::model::GrabbedAsset>, String> {
    core_engine::grabber::grab_site_links(
        &url,
        core_engine::grabber::GrabOptions {
            max_depth: max_depth.unwrap_or(1).clamp(1, 3),
            max_pages: 40,
            max_assets: 500,
            same_host_only: same_host_only.unwrap_or(true),
            cookie: cookie.as_deref(),
            referer: referer.as_deref(),
            user_agent: user_agent.as_deref(),
            proxy_url: proxy_url.as_deref(),
            auth_username: auth_username.as_deref(),
            auth_password: auth_password.as_deref(),
        },
    )
    .await
    .map_err(|e| format!("Site Grabber gagal: {}", e))
}

fn move_file_cross_device(src: &Path, dst: &Path) -> Result<(), String> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    if src.is_dir() {
        copy_dir_recursive(src, dst)
            .map_err(|e| format!("Gagal menyalin direktori ke lokasi baru: {}", e))?;
        std::fs::remove_dir_all(src)
            .map_err(|e| format!("Gagal menghapus direktori lama setelah dipindahkan: {}", e))?;
        return Ok(());
    }
    std::fs::copy(src, dst)
        .map_err(|e| format!("Gagal menyalin berkas ke lokasi baru: {}", e))?;
    std::fs::remove_file(src)
        .map_err(|e| format!("Gagal menghapus berkas lama setelah dipindahkan: {}", e))?;
    Ok(())
}

fn rename_or_move_download_paths(old_target: &Path, new_target: &Path) -> Result<String, String> {
    if old_target == new_target {
        return Ok(new_target.display().to_string());
    }
    if let Some(parent) = new_target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Gagal membuat folder tujuan: {}", e))?;
    }
    if old_target.exists() {
        if new_target.exists() {
            return Err("Berkas dengan nama tujuan sudah ada di folder tersebut.".to_string());
        }
        move_file_cross_device(old_target, new_target)?;
    }

    let old_meta = core_engine::downloader::checkpoint_path_for(old_target);
    let new_meta = core_engine::downloader::checkpoint_path_for(new_target);
    if old_meta.exists() {
        move_file_cross_device(&old_meta, &new_meta)?;
    }

    let old_torrent_meta = core_engine::torrent::torrent_checkpoint_path(old_target);
    let new_torrent_meta = core_engine::torrent::torrent_checkpoint_path(new_target);
    if old_torrent_meta.exists() {
        move_file_cross_device(&old_torrent_meta, &new_torrent_meta)?;
    }

    Ok(new_target.display().to_string())
}

#[tauri::command]
fn rename_or_move_download(old_path: String, new_path: String) -> Result<String, String> {
    let old_target = resolve_path(&old_path);
    let new_target = resolve_path(&new_path);
    rename_or_move_download_paths(&old_target, &new_target)
}

#[tauri::command]
async fn read_clipboard_text() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        return tokio::task::spawn_blocking(|| {
            let output = std::process::Command::new("pbpaste")
                .output()
                .map_err(|e| format!("Gagal membaca clipboard: {}", e))?;
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        })
        .await
        .map_err(|e| format!("Task clipboard gagal: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        return tokio::task::spawn_blocking(|| {
            const CF_UNICODETEXT: u32 = 13;
            #[link(name = "user32")]
            extern "system" {
                fn OpenClipboard(hwnd_new_owner: *mut core::ffi::c_void) -> i32;
                fn CloseClipboard() -> i32;
                fn GetClipboardData(u_format: u32) -> *mut core::ffi::c_void;
            }
            #[link(name = "kernel32")]
            extern "system" {
                fn GlobalLock(hmem: *mut core::ffi::c_void) -> *mut u16;
                fn GlobalUnlock(hmem: *mut core::ffi::c_void) -> i32;
                fn GlobalSize(hmem: *mut core::ffi::c_void) -> usize;
            }
            struct ClipboardGuard;
            impl Drop for ClipboardGuard {
                fn drop(&mut self) {
                    unsafe {
                        CloseClipboard();
                    }
                }
            }
            struct GlobalLockGuard(*mut core::ffi::c_void);
            impl Drop for GlobalLockGuard {
                fn drop(&mut self) {
                    unsafe {
                        GlobalUnlock(self.0);
                    }
                }
            }

            unsafe {
                let mut opened = false;
                for _ in 0..5 {
                    if OpenClipboard(std::ptr::null_mut()) != 0 {
                        opened = true;
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(15));
                }
                if !opened {
                    return Ok(String::new());
                }
                let _clip_guard = ClipboardGuard;
                let handle = GetClipboardData(CF_UNICODETEXT);
                if handle.is_null() {
                    return Ok(String::new());
                }
                let max_units = GlobalSize(handle) / 2;
                if max_units == 0 {
                    return Ok(String::new());
                }
                let ptr = GlobalLock(handle);
                if ptr.is_null() {
                    return Ok(String::new());
                }
                let _lock_guard = GlobalLockGuard(handle);
                let slice = std::slice::from_raw_parts(ptr, max_units);
                let end = slice.iter().position(|&c| c == 0).unwrap_or(max_units);
                Ok(String::from_utf16_lossy(&slice[..end]).trim().to_string())
            }
        })
        .await
        .map_err(|e| format!("Task clipboard gagal: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        return tokio::task::spawn_blocking(|| {
            let candidates: [(&str, &[&str]); 3] = [
                ("wl-paste", &["--no-newline"]),
                ("xclip", &["-o", "-selection", "clipboard"]),
                ("xsel", &["--clipboard", "--output"]),
            ];
            for (bin, args) in candidates {
                if let Ok(output) = std::process::Command::new(bin).args(args).output() {
                    if output.status.success() {
                        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if !text.is_empty() {
                            return Ok(text);
                        }
                    }
                }
            }
            Ok(String::new())
        })
        .await
        .map_err(|e| format!("Task clipboard gagal: {}", e))?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    Ok(String::new())
}

#[tauri::command]
fn read_text_file_preview(path: String) -> Result<String, String> {
    let target = resolve_path(&path);
    let meta = std::fs::metadata(&target)
        .map_err(|e| format!("Tidak dapat membaca metadata berkas: {}", e))?;
    if meta.len() > 2 * 1024 * 1024 {
        return Err("Ukuran berkas daftar URL maksimal 2 MB.".to_string());
    }
    let bytes = std::fs::read(&target)
        .map_err(|e| format!("Gagal membaca berkas teks: {}", e))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[tauri::command]
fn run_post_download_action(app: tauri::AppHandle, action: String) -> Result<String, String> {
    match action.as_str() {
        "none" | "notify_open" => Ok("ok".to_string()),
        "sleep" => {
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("pmset").arg("sleepnow").spawn();
            }
            #[cfg(target_os = "windows")]
            {
                #[link(name = "kernel32")]
                extern "system" {
                    fn LoadLibraryA(lp_lib_file_name: *const u8) -> *mut core::ffi::c_void;
                    fn GetProcAddress(
                        h_module: *mut core::ffi::c_void,
                        lp_proc_name: *const u8,
                    ) -> *mut core::ffi::c_void;
                }
                type SetSuspendStateFn = unsafe extern "system" fn(u8, u8, u8) -> u8;
                unsafe {
                    let module = LoadLibraryA(b"powrprof.dll\0".as_ptr());
                    if module.is_null() {
                        return Err("Gagal memuat powrprof.dll untuk aksi sleep.".to_string());
                    }
                    let proc = GetProcAddress(module, b"SetSuspendState\0".as_ptr());
                    if proc.is_null() {
                        return Err("Fungsi SetSuspendState tidak tersedia di sistem ini.".to_string());
                    }
                    let proc_addr = proc as usize;
                    std::thread::spawn(move || {
                        let set_suspend: SetSuspendStateFn = std::mem::transmute::<
                            *const core::ffi::c_void,
                            SetSuspendStateFn,
                        >(proc_addr as *const core::ffi::c_void);
                        let _ = set_suspend(0, 0, 0);
                    });
                }
            }
            #[cfg(target_os = "linux")]
            {
                let mut slept = false;
                for cmd_name in ["systemctl", "loginctl"] {
                    if let Ok(output) = std::process::Command::new(cmd_name)
                        .arg("suspend")
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output()
                    {
                        if output.status.success() {
                            slept = true;
                            break;
                        }
                    }
                }
                if !slept {
                    return Err(
                        "Gagal mengaktifkan mode sleep (systemctl/loginctl suspend tidak tersedia)."
                            .to_string(),
                    );
                }
            }
            Ok("sleep".to_string())
        }
        "quit" => {
            app.exit(0);
            Ok("quit".to_string())
        }
        other => Err(format!("Aksi pasca-unduhan tidak dikenal: {}", other)),
    }
}

fn resolve_app_executable_path() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(appimage) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
        if appimage.is_file() {
            return Some(appimage);
        }
    }
    std::env::current_exe().ok().filter(|p| p.exists())
}

#[cfg(any(target_os = "linux", test))]
fn escape_desktop_exec_arg(arg: &str) -> String {
    let mut escaped = String::with_capacity(arg.len() + 4);
    for ch in arg.chars() {
        match ch {
            '\\' => escaped.push_str(r"\\"),
            '"' => escaped.push_str(r#"\""#),
            '$' => escaped.push_str(r"\$"),
            '`' => escaped.push_str(r"\`"),
            '%' => escaped.push_str("%%"),
            other => escaped.push(other),
        }
    }
    format!("\"{}\"", escaped)
}

#[cfg(any(target_os = "linux", test))]
fn build_linux_autostart_desktop_entry(exec_path: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Download Cepat\nComment=Pengelola Unduhan Multi-Utas Cepat\nExec={}\nIcon=download-cepat\nTerminal=false\nCategories=Network;FileTransfer;\nX-GNOME-Autostart-enabled=true\n",
        escape_desktop_exec_arg(exec_path)
    )
}

#[cfg(target_os = "linux")]
fn linux_autostart_desktop_path() -> Option<PathBuf> {
    let config_dir = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| user_home_dir().map(|h| h.join(".config")))?;
    Some(config_dir.join("autostart").join("download-cepat.desktop"))
}

#[tauri::command]
fn get_autostart_status() -> bool {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = user_home_dir() {
            let plist = home.join("Library/LaunchAgents/com.moesstudio.downloadcepat.plist");
            return plist.exists();
        }
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        let mut cmd = std::process::Command::new("reg");
        hide_window_std(&mut cmd);
        let status = cmd
            .args([
                "query",
                r#"HKCU\Software\Microsoft\Windows\CurrentVersion\Run"#,
                "/v",
                "Download Cepat",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        return status.map(|s| s.success()).unwrap_or(false);
    }
    #[cfg(target_os = "linux")]
    {
        return linux_autostart_desktop_path().map_or(false, |p| p.is_file());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    false
}

#[tauri::command]
fn set_autostart(enable: bool) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = user_home_dir() {
            let launch_dir = home.join("Library/LaunchAgents");
            let plist = launch_dir.join("com.moesstudio.downloadcepat.plist");

            if enable {
                let _ = std::fs::create_dir_all(&launch_dir);
                let app_path = "/Applications/Download Cepat.app/Contents/MacOS/download-cepat-app";
                let plist_content = format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.moesstudio.downloadcepat</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>"#,
                    app_path
                );

                std::fs::write(&plist, plist_content)
                    .map_err(|e| format!("Gagal menulis berkas startup: {}", e))?;

                let _ = std::process::Command::new("launchctl")
                    .arg("load")
                    .arg("-w")
                    .arg(&plist)
                    .status();

                return Ok(true);
            } else {
                if plist.exists() {
                    let _ = std::process::Command::new("launchctl")
                        .arg("unload")
                        .arg("-w")
                        .arg(&plist)
                        .status();
                    let _ = std::fs::remove_file(&plist);
                }
                return Ok(false);
            }
        }
        return Ok(false);
    }

    #[cfg(target_os = "windows")]
    {
        const RUN_KEY: &str = r#"HKCU\Software\Microsoft\Windows\CurrentVersion\Run"#;
        if enable {
            let exe = resolve_app_executable_path()
                .ok_or_else(|| "Gagal mendeteksi lokasi aplikasi Download Cepat.".to_string())?;
            let value = format!("\"{}\"", normalize_platform_path(exe).display());
            let mut cmd = std::process::Command::new("reg");
            hide_window_std(&mut cmd);
            let status = cmd
                .args([
                    "add",
                    RUN_KEY,
                    "/v",
                    "Download Cepat",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &value,
                    "/f",
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map_err(|e| format!("Gagal mengatur startup Windows: {}", e))?;
            if status.success() {
                return Ok(true);
            }
            return Err("Gagal menyimpan entri startup di Windows Registry.".to_string());
        } else {
            let mut cmd = std::process::Command::new("reg");
            hide_window_std(&mut cmd);
            let _ = cmd
                .args(["delete", RUN_KEY, "/v", "Download Cepat", "/f"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            return Ok(false);
        }
    }

    #[cfg(target_os = "linux")]
    {
        let desktop_path = linux_autostart_desktop_path()
            .ok_or_else(|| "Gagal mendeteksi direktori ~/.config/autostart".to_string())?;
        if enable {
            if let Some(parent) = desktop_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Gagal membuat folder autostart: {}", e))?;
            }
            let exe = resolve_app_executable_path()
                .ok_or_else(|| "Gagal mendeteksi lokasi aplikasi Download Cepat.".to_string())?;
            let content = build_linux_autostart_desktop_entry(&exe.display().to_string());
            std::fs::write(&desktop_path, content)
                .map_err(|e| format!("Gagal menulis berkas autostart: {}", e))?;
            return Ok(true);
        } else {
            if desktop_path.exists() {
                let _ = std::fs::remove_file(&desktop_path);
            }
            return Ok(false);
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = enable;
        Ok(false)
    }
}

pub const OFFICIAL_CHROME_EXTENSION_ID: &str = "lmahadjehdfibjjgcnbajbgapbkghida";
pub const OFFICIAL_CHROME_WEBSTORE_URL: &str = "https://chromewebstore.google.com/detail/lmahadjehdfibjjgcnbajbgapbkghida?utm_source=item-share-cb";

const KNOWN_CHROMIUM_EXTENSION_IDS: &[&str] = &[
    OFFICIAL_CHROME_EXTENSION_ID,
    "caijlihmnclbeecoodjbmfdbnfjbgdnm",
    "gbeagmdkkhbopfcggmeffpbeblalfcpk",
];

fn is_valid_extension_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

fn collect_extension_ids_from_prefs_file(path: &std::path::Path, out: &mut std::collections::BTreeSet<String>) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let Some(settings) = json
        .get("extensions")
        .and_then(|e| e.get("settings"))
        .and_then(|s| s.as_object())
    else {
        return;
    };

    for (ext_id, info) in settings {
        if !is_valid_extension_id(ext_id) {
            continue;
        }
        let manifest_name = info
            .get("manifest")
            .and_then(|m| m.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_lowercase();
        let ext_path = info
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_lowercase();

        if manifest_name.contains("download cepat")
            || manifest_name.contains("downloadcepat")
            || ext_path.contains("download cepat")
            || ext_path.contains("downloadcepat")
        {
            out.insert(ext_id.clone());
        }
    }
}

fn collect_installed_extension_ids(home: &std::path::Path) -> Vec<String> {
    let mut ids = std::collections::BTreeSet::new();
    for id in KNOWN_CHROMIUM_EXTENSION_IDS {
        ids.insert((*id).to_string());
    }

    let mut browser_roots: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        browser_roots.extend([
            home.join("Library/Application Support/Google/Chrome"),
            home.join("Library/Application Support/Microsoft Edge"),
            home.join("Library/Application Support/BraveSoftware/Brave-Browser"),
            home.join("Library/Application Support/Chromium"),
            home.join("Library/Application Support/Vivaldi"),
        ]);
    }
    #[cfg(target_os = "windows")]
    {
        let local_app = std::env::var_os("LOCALAPPDATA")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Local"));
        browser_roots.extend([
            local_app.join(r"Google\Chrome\User Data"),
            local_app.join(r"Microsoft\Edge\User Data"),
            local_app.join(r"BraveSoftware\Brave-Browser\User Data"),
            local_app.join(r"Chromium\User Data"),
            local_app.join(r"Vivaldi\User Data"),
        ]);
    }
    #[cfg(target_os = "linux")]
    {
        let cfg = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        browser_roots.extend([
            cfg.join("google-chrome"),
            cfg.join("chromium"),
            cfg.join("BraveSoftware/Brave-Browser"),
            cfg.join("microsoft-edge"),
            cfg.join("vivaldi"),
            home.join(".var/app/com.google.Chrome/config/google-chrome"),
            home.join(".var/app/org.chromium.Chromium/config/chromium"),
            home.join(".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser"),
            home.join(".var/app/com.microsoft.Edge/config/microsoft-edge"),
            home.join("snap/chromium/current/.config/chromium"),
            home.join("snap/brave/current/.config/BraveSoftware/Brave-Browser"),
        ]);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let _ = home;

    for root in &browser_roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let profile_dir = entry.path();
            if !profile_dir.is_dir() {
                continue;
            }
            collect_extension_ids_from_prefs_file(&profile_dir.join("Preferences"), &mut ids);
            collect_extension_ids_from_prefs_file(&profile_dir.join("Secure Preferences"), &mut ids);
        }
    }

    ids.into_iter().collect()
}

fn resolve_native_host_binary_path() -> String {
    #[cfg(target_os = "macos")]
    {
        let mac_installed =
            std::path::Path::new("/Applications/Download Cepat.app/Contents/MacOS/download-cepat-app");
        if mac_installed.exists() {
            return mac_installed.display().to_string();
        }
    }
    if let Some(exe) = resolve_app_executable_path() {
        return normalize_platform_path(exe).display().to_string();
    }
    #[cfg(target_os = "macos")]
    {
        return "/Applications/Download Cepat.app/Contents/MacOS/download-cepat-app".to_string();
    }
    #[cfg(not(target_os = "macos"))]
    {
        "download-cepat-app".to_string()
    }
}

fn build_chromium_manifest_json(host_path: &str, ext_ids: &[String]) -> String {
    let allowed_origins: Vec<String> = ext_ids
        .iter()
        .filter(|id| is_valid_extension_id(id))
        .map(|id| format!("chrome-extension://{}/", id))
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "name": "com.downloadcepat.nativehost",
        "description": "Download Cepat Native Messaging Host",
        "path": host_path,
        "type": "stdio",
        "allowed_origins": allowed_origins,
    }))
    .unwrap_or_default()
}

fn build_firefox_manifest_json(host_path: &str) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "name": "com.downloadcepat.nativehost",
        "description": "Download Cepat Native Messaging Host",
        "path": host_path,
        "type": "stdio",
        "allowed_extensions": ["downloadcepat@downloadcepat.app"],
    }))
    .unwrap_or_default()
}

/// Configures Chrome, Edge, Brave, Chromium, and Vivaldi External Extensions to detect and prompt
/// users to enable Download Cepat directly from the Chrome Web Store on browser startup.
pub fn sync_external_extensions() -> usize {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let Some(home) = user_home_dir() else { return 0 };
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let json_content = serde_json::to_string_pretty(&serde_json::json!({
        "external_update_url": "https://clients2.google.com/service/update2/crx"
    }))
    .unwrap_or_else(|_| "{\"external_update_url\": \"https://clients2.google.com/service/update2/crx\"}".to_string());

    let mut configured = 0;

    #[cfg(target_os = "macos")]
    {
        let dirs = [
            home.join("Library/Application Support/Google/Chrome/External Extensions"),
            home.join("Library/Application Support/Microsoft Edge/External Extensions"),
            home.join("Library/Application Support/BraveSoftware/Brave-Browser/External Extensions"),
            home.join("Library/Application Support/Chromium/External Extensions"),
            home.join("Library/Application Support/Vivaldi/External Extensions"),
        ];
        for d in &dirs {
            if std::fs::create_dir_all(d).is_ok() {
                let file_path = d.join(format!("{}.json", OFFICIAL_CHROME_EXTENSION_ID));
                if std::fs::write(&file_path, &json_content).is_ok() {
                    configured += 1;
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let cfg = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let standard_dirs = [
            cfg.join("google-chrome/External Extensions"),
            cfg.join("chromium/External Extensions"),
            cfg.join("BraveSoftware/Brave-Browser/External Extensions"),
            cfg.join("microsoft-edge/External Extensions"),
            cfg.join("vivaldi/External Extensions"),
        ];
        for d in &standard_dirs {
            if std::fs::create_dir_all(d).is_ok() {
                let file_path = d.join(format!("{}.json", OFFICIAL_CHROME_EXTENSION_ID));
                if std::fs::write(&file_path, &json_content).is_ok() {
                    configured += 1;
                }
            }
        }

        let sandbox_dirs = [
            home.join(".var/app/com.google.Chrome/config/google-chrome/External Extensions"),
            home.join(".var/app/org.chromium.Chromium/config/chromium/External Extensions"),
            home.join(".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser/External Extensions"),
            home.join(".var/app/com.microsoft.Edge/config/microsoft-edge/External Extensions"),
            home.join("snap/chromium/current/.config/chromium/External Extensions"),
            home.join("snap/brave/current/.config/BraveSoftware/Brave-Browser/External Extensions"),
        ];
        for d in &sandbox_dirs {
            if d.parent().map_or(false, |p| p.exists()) && std::fs::create_dir_all(d).is_ok() {
                let file_path = d.join(format!("{}.json", OFFICIAL_CHROME_EXTENSION_ID));
                if std::fs::write(&file_path, &json_content).is_ok() {
                    configured += 1;
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let ext_id = OFFICIAL_CHROME_EXTENSION_ID;
        let update_url = "https://clients2.google.com/service/update2/crx";
        let reg_paths = [
            format!(r#"HKCU\Software\Google\Chrome\Extensions\{}"#, ext_id),
            format!(r#"HKCU\Software\Microsoft\Edge\Extensions\{}"#, ext_id),
            format!(r#"HKCU\Software\BraveSoftware\Brave-Browser\Extensions\{}"#, ext_id),
            format!(r#"HKCU\Software\Chromium\Extensions\{}"#, ext_id),
            format!(r#"HKCU\Software\Vivaldi\Extensions\{}"#, ext_id),
        ];
        for path in &reg_paths {
            let mut cmd = std::process::Command::new("reg");
            hide_window_std(&mut cmd);
            let status = cmd
                .args(["add", path, "/v", "update_url", "/t", "REG_SZ", "/d", update_url, "/f"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            if status.map_or(false, |s| s.success()) {
                configured += 1;
            }
        }
    }

    configured
}

/// Writes valid Native Messaging host manifests for Chrome, Edge, Brave, Chromium, Vivaldi, and Firefox across macOS, Windows, and Linux.
pub fn sync_native_messaging_hosts() -> Result<usize, String> {
    let home = user_home_dir().ok_or("Gagal mendeteksi direktori home")?;
    let host_path = resolve_native_host_binary_path();
    let ext_ids = collect_installed_extension_ids(&home);
    let chromium_manifest = build_chromium_manifest_json(&host_path, &ext_ids);
    let firefox_manifest = build_firefox_manifest_json(&host_path);

    let mut installed_count = 0;

    #[cfg(target_os = "macos")]
    {
        let chromium_dirs = [
            home.join("Library/Application Support/Google/Chrome/NativeMessagingHosts"),
            home.join("Library/Application Support/Microsoft Edge/NativeMessagingHosts"),
            home.join("Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts"),
            home.join("Library/Application Support/Chromium/NativeMessagingHosts"),
            home.join("Library/Application Support/Vivaldi/NativeMessagingHosts"),
        ];

        for dir in &chromium_dirs {
            if std::fs::create_dir_all(dir).is_ok() {
                let manifest_path = dir.join("com.downloadcepat.nativehost.json");
                if std::fs::write(&manifest_path, &chromium_manifest).is_ok() {
                    installed_count += 1;
                }
            }
        }

        let mozilla_dir = home.join("Library/Application Support/Mozilla/NativeMessagingHosts");
        if std::fs::create_dir_all(&mozilla_dir).is_ok() {
            let manifest_path = mozilla_dir.join("com.downloadcepat.nativehost.json");
            if std::fs::write(&manifest_path, &firefox_manifest).is_ok() {
                installed_count += 1;
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let cfg = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let chromium_dirs = [
            cfg.join("google-chrome/NativeMessagingHosts"),
            cfg.join("chromium/NativeMessagingHosts"),
            cfg.join("BraveSoftware/Brave-Browser/NativeMessagingHosts"),
            cfg.join("microsoft-edge/NativeMessagingHosts"),
            cfg.join("vivaldi/NativeMessagingHosts"),
        ];

        for dir in &chromium_dirs {
            if std::fs::create_dir_all(dir).is_ok() {
                let manifest_path = dir.join("com.downloadcepat.nativehost.json");
                if std::fs::write(&manifest_path, &chromium_manifest).is_ok() {
                    installed_count += 1;
                }
            }
        }

        let sandbox_chromium_dirs = [
            home.join(".var/app/com.google.Chrome/config/google-chrome/NativeMessagingHosts"),
            home.join(".var/app/org.chromium.Chromium/config/chromium/NativeMessagingHosts"),
            home.join(".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser/NativeMessagingHosts"),
            home.join(".var/app/com.microsoft.Edge/config/microsoft-edge/NativeMessagingHosts"),
            home.join("snap/chromium/current/.config/chromium/NativeMessagingHosts"),
            home.join("snap/brave/current/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts"),
        ];
        for dir in &sandbox_chromium_dirs {
            if dir.parent().map_or(false, |p| p.exists()) && std::fs::create_dir_all(dir).is_ok() {
                let manifest_path = dir.join("com.downloadcepat.nativehost.json");
                if std::fs::write(&manifest_path, &chromium_manifest).is_ok() {
                    installed_count += 1;
                }
            }
        }

        let mozilla_dir = home.join(".mozilla/native-messaging-hosts");
        if std::fs::create_dir_all(&mozilla_dir).is_ok() {
            let manifest_path = mozilla_dir.join("com.downloadcepat.nativehost.json");
            if std::fs::write(&manifest_path, &firefox_manifest).is_ok() {
                installed_count += 1;
            }
        }

        let sandbox_mozilla_dirs = [
            home.join(".var/app/org.mozilla.firefox/.mozilla/native-messaging-hosts"),
            home.join("snap/firefox/common/.mozilla/native-messaging-hosts"),
        ];
        for dir in &sandbox_mozilla_dirs {
            if dir.parent().map_or(false, |p| p.exists()) && std::fs::create_dir_all(dir).is_ok() {
                let manifest_path = dir.join("com.downloadcepat.nativehost.json");
                if std::fs::write(&manifest_path, &firefox_manifest).is_ok() {
                    installed_count += 1;
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("LOCALAPPDATA")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Local"));
        let manifest_dir = appdata.join("Download Cepat").join("NativeMessagingHosts");
        if std::fs::create_dir_all(&manifest_dir).is_ok() {
            let chrome_manifest_file = manifest_dir.join("com.downloadcepat.nativehost.json");
            if std::fs::write(&chrome_manifest_file, &chromium_manifest).is_ok() {
                let chrome_manifest_str = normalize_platform_path(chrome_manifest_file).to_string_lossy().into_owned();
                let reg_keys = [
                    r#"HKCU\Software\Google\Chrome\NativeMessagingHosts\com.downloadcepat.nativehost"#,
                    r#"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.downloadcepat.nativehost"#,
                    r#"HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.downloadcepat.nativehost"#,
                    r#"HKCU\Software\Chromium\NativeMessagingHosts\com.downloadcepat.nativehost"#,
                    r#"HKCU\Software\Vivaldi\NativeMessagingHosts\com.downloadcepat.nativehost"#,
                ];
                for k in &reg_keys {
                    let mut cmd = std::process::Command::new("reg");
                    hide_window_std(&mut cmd);
                    let status = cmd
                        .args(["add", k, "/ve", "/t", "REG_SZ", "/d", &chrome_manifest_str, "/f"])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                    if status.map_or(false, |s| s.success()) {
                        installed_count += 1;
                    }
                }
            }

            let firefox_manifest_file = manifest_dir.join("firefox.downloadcepat.nativehost.json");
            if std::fs::write(&firefox_manifest_file, &firefox_manifest).is_ok() {
                let firefox_manifest_str = normalize_platform_path(firefox_manifest_file).to_string_lossy().into_owned();
                let mut cmd = std::process::Command::new("reg");
                hide_window_std(&mut cmd);
                let status = cmd
                    .args([
                        "add",
                        r#"HKCU\Software\Mozilla\NativeMessagingHosts\com.downloadcepat.nativehost"#,
                        "/ve",
                        "/t",
                        "REG_SZ",
                        "/d",
                        &firefox_manifest_str,
                        "/f",
                    ])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                if status.map_or(false, |s| s.success()) {
                    installed_count += 1;
                }
            }
        }
    }

    // Also auto-sync Chrome Web Store external extensions preferences
    let _ = sync_external_extensions();

    Ok(installed_count)
}

fn is_native_messaging_invocation(args: &[String]) -> bool {
    args.iter().skip(1).any(|arg| {
        arg.starts_with("chrome-extension://")
            || arg.starts_with("moz-extension://")
            || arg.contains("downloadcepat@")
            || arg == "--native-messaging-host"
    })
}

fn read_native_message<R: std::io::Read>(reader: &mut R) -> std::io::Result<Option<serde_json::Value>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > 10 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid native message frame length",
        ));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    let parsed: serde_json::Value = serde_json::from_slice(&payload)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(parsed))
}

fn write_native_message<W: std::io::Write>(writer: &mut W, payload: &serde_json::Value) -> std::io::Result<()> {
    let encoded = serde_json::to_vec(payload)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let len = u32::try_from(encoded.len())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Payload too large"))?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&encoded)?;
    writer.flush()
}

fn ensure_bridge_running() {
    let addr: std::net::SocketAddr = "127.0.0.1:18291".parse().unwrap();
    if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300)).is_ok() {
        return;
    }

    #[cfg(target_os = "macos")]
    {
        let app_bundle = std::path::Path::new("/Applications/Download Cepat.app");
        let launched = if app_bundle.exists() {
            std::process::Command::new("open")
                .arg("-a")
                .arg(app_bundle)
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        } else {
            false
        };
        if !launched {
            let _ = std::process::Command::new("open")
                .arg("-a")
                .arg("Download Cepat")
                .status();
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(exe) = resolve_app_executable_path() {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            let _ = std::process::Command::new(exe)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
                .spawn();
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(exe) = resolve_app_executable_path() {
            let _ = std::process::Command::new(exe)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }

    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300)).is_ok() {
            return;
        }
    }
}

fn forward_http_bridge(method: &str, path: &str, body: Option<&str>) -> std::io::Result<serde_json::Value> {
    use std::io::{Read, Write};
    let addr: std::net::SocketAddr = "127.0.0.1:18291".parse().unwrap();
    let mut stream = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3))?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(60)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(5)));

    let body_bytes = body.unwrap_or("").as_bytes();
    let mut req = format!(
        "{} {} HTTP/1.1\r\nHost: 127.0.0.1:18291\r\nConnection: close\r\n",
        method, path
    );
    if !body_bytes.is_empty() || method == "POST" {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));
    }
    req.push_str("\r\n");

    stream.write_all(req.as_bytes())?;
    if !body_bytes.is_empty() {
        stream.write_all(body_bytes)?;
    }
    stream.flush()?;

    let mut raw_resp = Vec::new();
    stream.read_to_end(&mut raw_resp)?;
    let split_pos = raw_resp
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .unwrap_or(0);
    let json_bytes = &raw_resp[split_pos..];
    match serde_json::from_slice::<serde_json::Value>(json_bytes) {
        Ok(v) => Ok(v),
        Err(e) => Ok(serde_json::json!({
            "status": "error",
            "message": format!("Invalid bridge response: {}", e)
        })),
    }
}

/// Handles Chrome/Firefox Native Messaging stdio invocations before launching GUI.
pub fn maybe_run_native_messaging_host() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if !is_native_messaging_invocation(&args) {
        return false;
    }

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    while let Ok(Some(msg)) = read_native_message(&mut reader) {
        ensure_bridge_running();
        let action = msg["action"]
            .as_str()
            .or_else(|| msg["type"].as_str())
            .unwrap_or("ADD_DOWNLOAD")
            .to_uppercase();
        let body = msg.to_string();

        let reply = match action.as_str() {
            "PING" | "STATUS" => forward_http_bridge("GET", "/status", None),
            "INSPECT" | "INSPECT_VIDEO" => forward_http_bridge("POST", "/inspect", Some(&body)),
            "START_DOWNLOAD" | "START_VIDEO_DOWNLOAD" => {
                forward_http_bridge("POST", "/start_download", Some(&body))
            }
            "ADD_LINKS" => forward_http_bridge("POST", "/add_links", Some(&body)),
            _ => forward_http_bridge("POST", "/add_download", Some(&body)),
        }
        .unwrap_or_else(|e| {
            serde_json::json!({
                "status": "error",
                "message": format!("Gagal terhubung ke Download Cepat desktop: {}", e)
            })
        });

        if write_native_message(&mut writer, &reply).is_err() {
            break;
        }
    }

    true
}

#[tauri::command]
fn install_browser_extension_host() -> Result<String, String> {
    let installed_count = sync_native_messaging_hosts()?;
    let ext_count = sync_external_extensions();
    Ok(format!(
        "Konektor native berhasil dipasang ke {} browser dan {} konfigurasi External Extensions aktif.",
        installed_count, ext_count
    ))
}

#[tauri::command]
fn open_external_url(url: String) -> Result<String, String> {
    if url.starts_with("http://") || url.starts_with("https://") {
        open::that(&url).map_err(|e| format!("Gagal membuka URL: {}", e))?;
        Ok(format!("URL dibuka di browser: {}", url))
    } else {
        Err("URL harus berawalan http:// atau https://".to_string())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstallResult {
    pub started_in_place: bool,
    pub message: String,
    pub asset_path: Option<String>,
}

fn macos_app_bundle_for_exe(exe: &Path) -> Option<PathBuf> {
    let macos_dir = exe.parent()?;
    let contents_dir = macos_dir.parent()?;
    let app_dir = contents_dir.parent()?;
    if macos_dir.file_name().and_then(|s| s.to_str()) == Some("MacOS")
        && contents_dir.file_name().and_then(|s| s.to_str()) == Some("Contents")
        && app_dir.extension().and_then(|s| s.to_str()) == Some("app")
    {
        Some(app_dir.to_path_buf())
    } else {
        None
    }
}

fn current_platform_env() -> core_engine::PlatformEnvironment {
    let inside_macos = std::env::current_exe()
        .ok()
        .and_then(|p| macos_app_bundle_for_exe(&p))
        .is_some();
    core_engine::PlatformEnvironment::detect_current(inside_macos)
}

fn emit_update_progress(
    app: &tauri::AppHandle,
    stage: &str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    progress_percent: f64,
    message: &str,
) {
    let _ = app.emit(
        "app-update-progress",
        serde_json::json!({
            "stage": stage,
            "downloadedBytes": downloaded_bytes,
            "totalBytes": total_bytes,
            "progressPercent": progress_percent,
            "message": message,
        }),
    );
}

#[cfg(any(target_os = "windows", test))]
fn validate_windows_cmd_path(path: &Path) -> Result<String, String> {
    let raw = path.to_string_lossy().replace('/', "\\");
    if raw.is_empty()
        || raw.contains('"')
        || raw.contains('%')
        || raw.contains('\r')
        || raw.contains('\n')
        || raw.contains('&')
        || raw.contains('|')
        || raw.contains('<')
        || raw.contains('>')
        || raw.contains('^')
    {
        return Err(format!(
            "Jalur instalasi mengandung karakter tidak aman untuk skrip pembaruan: {}",
            raw
        ));
    }
    Ok(raw)
}

#[cfg(any(target_os = "windows", test))]
fn build_windows_update_cmd(
    installer_path: &Path,
    exe_path: &Path,
    is_msi: bool,
) -> Result<String, String> {
    let installer = validate_windows_cmd_path(installer_path)?;
    let exe = validate_windows_cmd_path(exe_path)?;
    let install_line = if is_msi {
        format!("start /wait \"\" msiexec /i \"{}\" /passive /norestart", installer)
    } else {
        format!("start /wait \"\" \"{}\" /S", installer)
    };
    Ok(format!(
        "@echo off\r\nsetlocal\r\nping -n 4 127.0.0.1 > nul\r\n{}\r\nif exist \"{}\" (\r\n  start \"\" \"{}\"\r\n) else if exist \"%LOCALAPPDATA%\\Download Cepat\\Download Cepat.exe\" (\r\n  start \"\" \"%LOCALAPPDATA%\\Download Cepat\\Download Cepat.exe\"\r\n) else if exist \"%LOCALAPPDATA%\\Download Cepat\\download-cepat-app.exe\" (\r\n  start \"\" \"%LOCALAPPDATA%\\Download Cepat\\download-cepat-app.exe\"\r\n)\r\ndel /f /q \"{}\" > nul 2>&1\r\ndel /f /q \"%~f0\" > nul 2>&1\r\n",
        install_line, exe, exe, installer
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn directory_is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".dc_write_probe_{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(f) => {
            drop(f);
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(target_os = "macos")]
fn find_extracted_app_bundle(root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.extension().and_then(|e| e.to_str()) == Some("app")
                && path.join("Contents").join("MacOS").is_dir()
            {
                return Some(path);
            }
            subdirs.push(path);
        }
    }
    for sub in subdirs {
        if let Ok(nested) = std::fs::read_dir(sub) {
            for entry in nested.flatten() {
                let path = entry.path();
                if path.is_dir()
                    && path.extension().and_then(|e| e.to_str()) == Some("app")
                    && path.join("Contents").join("MacOS").is_dir()
                {
                    return Some(path);
                }
            }
        }
    }
    None
}

#[tauri::command]
async fn check_for_app_update() -> Result<core_engine::AppUpdateInfo, String> {
    let env = current_platform_env();
    core_engine::updater::check_github_release(env!("CARGO_PKG_VERSION"), &env).await
}

fn schedule_app_exit_for_update(app: &tauri::AppHandle) {
    let exit_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let count = stop_all_real_downloads();
        if count > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        }
        exit_handle.exit(0);
    });
}

#[tauri::command]
async fn download_and_install_app_update(app: tauri::AppHandle) -> Result<UpdateInstallResult, String> {
    emit_update_progress(
        &app,
        "checking",
        0,
        None,
        0.0,
        "Memverifikasi rilis terbaru dari GitHub...",
    );

    let env = current_platform_env();
    let info = core_engine::updater::check_github_release(env!("CARGO_PKG_VERSION"), &env).await?;
    if !info.update_available {
        return Err(format!(
            "Download Cepat v{} sudah merupakan versi terbaru.",
            info.current_version
        ));
    }

    let asset_name = info
        .asset_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            "Paket pembaruan otomatis untuk platform Anda belum tersedia pada rilis ini. Silakan unduh langsung dari halaman rilis GitHub."
                .to_string()
        })?;
    let asset_url = info
        .asset_url
        .clone()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "URL paket pembaruan tidak tersedia.".to_string())?;
    let expected_sha = info
        .asset_sha256
        .clone()
        .ok_or_else(|| {
            "Digest SHA-256 tidak tersedia pada aset rilis GitHub sehingga pemasangan otomatis dibatalkan demi keamanan."
                .to_string()
        })?;

    let safe_name = Path::new(&asset_name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download-cepat-update.bin".to_string());

    let update_dir = std::env::temp_dir().join("download-cepat-updates");
    let dest_path = normalize_platform_path(update_dir.join(&safe_name));
    let known_total = info.asset_size;

    let app_for_progress = app.clone();
    let mut last_emit = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(1))
        .unwrap_or_else(std::time::Instant::now);
    let mut last_pct = -1.0f64;

    core_engine::updater::download_verified_asset(
        &asset_url,
        &dest_path,
        &expected_sha,
        &info.current_version,
        true,
        |downloaded, resp_total| {
            let total = resp_total.or(known_total);
            let pct = match total {
                Some(t) if t > 0 => ((downloaded as f64 / t as f64) * 100.0).min(100.0),
                _ => 0.0,
            };
            let now = std::time::Instant::now();
            let done = total.map_or(false, |t| downloaded >= t);
            if done || now.duration_since(last_emit) >= std::time::Duration::from_millis(120) || (pct - last_pct).abs() >= 1.0 {
                last_emit = now;
                last_pct = pct;
                emit_update_progress(
                    &app_for_progress,
                    "downloading",
                    downloaded,
                    total,
                    pct,
                    "Mengunduh paket pembaruan resmi...",
                );
            }
        },
    )
    .await?;

    emit_update_progress(
        &app,
        "installing",
        dest_path.metadata().map(|m| m.len()).unwrap_or(0),
        known_total,
        100.0,
        "Checksum SHA-256 terverifikasi. Memasang pembaruan...",
    );

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

        let exe_path = std::env::current_exe()
            .map_err(|e| format!("Gagal mendeteksi executable Download Cepat: {}", e))?;
        let is_msi = info.install_kind == "windows_msi";
        let script_content = build_windows_update_cmd(&dest_path, &exe_path, is_msi)?;
        let script_path = update_dir.join("dc_apply_update.cmd");
        std::fs::write(&script_path, script_content)
            .map_err(|e| format!("Gagal membuat skrip pembaruan Windows: {}", e))?;

        std::process::Command::new("cmd")
            .args(["/C", &script_path.to_string_lossy().into_owned()])
            .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Gagal menjalankan installer pembaruan: {}", e))?;

        schedule_app_exit_for_update(&app);
        return Ok(UpdateInstallResult {
            started_in_place: true,
            message: format!(
                "Pembaruan v{} berhasil diunduh & diverifikasi SHA-256. Aplikasi akan menutup sebentar untuk memasang pembaruan dan dibuka kembali otomatis.",
                info.latest_version
            ),
            asset_path: Some(dest_path.display().to_string()),
        });
    }

    #[cfg(target_os = "macos")]
    {
        if info.install_kind == "macos_app_tarball" {
            if let Some(target_app) = std::env::current_exe()
                .ok()
                .and_then(|p| macos_app_bundle_for_exe(&p))
            {
                let extract_dir = update_dir.join("extracted");
                let _ = std::fs::remove_dir_all(&extract_dir);
                std::fs::create_dir_all(&extract_dir)
                    .map_err(|e| format!("Gagal membuat folder ekstraksi pembaruan: {}", e))?;

                let tar_status = tokio::process::Command::new("/usr/bin/tar")
                    .arg("-xzf")
                    .arg(&dest_path)
                    .arg("-C")
                    .arg(&extract_dir)
                    .status()
                    .await
                    .map_err(|e| format!("Gagal mengekstrak arsip pembaruan macOS: {}", e))?;
                if !tar_status.success() {
                    return Err("Ekstraksi paket pembaruan macOS gagal.".to_string());
                }

                let new_app = find_extracted_app_bundle(&extract_dir).ok_or_else(|| {
                    "Berkas .app baru tidak ditemukan di dalam arsip pembaruan.".to_string()
                })?;

                let _ = std::process::Command::new("/usr/bin/xattr")
                    .args(["-dr", "com.apple.quarantine"])
                    .arg(&new_app)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();

                let writable = target_app
                    .parent()
                    .map(directory_is_writable)
                    .unwrap_or(false);
                if writable {
                    let pid_str = std::process::id().to_string();
                    let script = r#"
OLD_PID="$1"
NEW_APP="$2"
TARGET_APP="$3"
TAR_FILE="$4"
while kill -0 "$OLD_PID" 2>/dev/null; do sleep 0.2; done
sleep 0.3
rm -rf "${TARGET_APP}.backup"
if [ -e "$TARGET_APP" ]; then
  mv "$TARGET_APP" "${TARGET_APP}.backup" || exit 1
fi
if mv "$NEW_APP" "$TARGET_APP"; then
  rm -rf "${TARGET_APP}.backup"
else
  mv "${TARGET_APP}.backup" "$TARGET_APP" 2>/dev/null
  exit 1
fi
rm -f "$TAR_FILE"
/usr/bin/xattr -dr com.apple.quarantine "$TARGET_APP" 2>/dev/null || true
/usr/bin/open -n "$TARGET_APP"
"#;
                    std::process::Command::new("/bin/sh")
                        .arg("-c")
                        .arg(script)
                        .arg("dc-macos-updater")
                        .arg(&pid_str)
                        .arg(&new_app)
                        .arg(&target_app)
                        .arg(&dest_path)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .map_err(|e| format!("Gagal meluncurkan skrip pembaruan macOS: {}", e))?;

                    schedule_app_exit_for_update(&app);
                    return Ok(UpdateInstallResult {
                        started_in_place: true,
                        message: format!(
                            "Pembaruan v{} berhasil diunduh & diverifikasi SHA-256. Download Cepat akan memuat ulang otomatis.",
                            info.latest_version
                        ),
                        asset_path: Some(dest_path.display().to_string()),
                    });
                }
            }
        }

        let _ = open::that(&dest_path);
        return Ok(UpdateInstallResult {
            started_in_place: false,
            message: format!(
                "Installer v{} ({}) telah diverifikasi SHA-256 dan dibuka. Seret aplikasi ke folder Applications untuk menyelesaikan pembaruan.",
                info.latest_version, safe_name
            ),
            asset_path: Some(dest_path.display().to_string()),
        });
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;

        if info.install_kind == "linux_appimage" {
            let _ = std::fs::set_permissions(&dest_path, std::fs::Permissions::from_mode(0o755));
            if let Some(current_appimage) = std::env::var_os("APPIMAGE")
                .map(PathBuf::from)
                .filter(|p| p.is_file())
            {
                if let Some(app_dir) = current_appimage.parent() {
                    if directory_is_writable(app_dir) {
                        let staged_path = app_dir.join(format!(
                            ".{}.update.tmp",
                            current_appimage
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("Download-Cepat.AppImage")
                        ));
                        let _ = std::fs::remove_file(&staged_path);
                        let moved_staged = if std::fs::rename(&dest_path, &staged_path).is_ok() {
                            true
                        } else if std::fs::copy(&dest_path, &staged_path).is_ok() {
                            let _ = std::fs::remove_file(&dest_path);
                            true
                        } else {
                            false
                        };
                        if moved_staged {
                            let _ = std::fs::set_permissions(
                                &staged_path,
                                std::fs::Permissions::from_mode(0o755),
                            );
                            let pid_str = std::process::id().to_string();
                            let script = r#"
OLD_PID="$1"
STAGED="$2"
TARGET="$3"
while kill -0 "$OLD_PID" 2>/dev/null; do sleep 0.2; done
sleep 0.3
mv -f "$STAGED" "$TARGET" || exit 1
chmod +x "$TARGET"
unset APPIMAGE APPDIR ARGV0 OWD LD_LIBRARY_PATH GDK_PIXBUF_MODULE_FILE GST_PLUGIN_SYSTEM_PATH
nohup "$TARGET" >/dev/null 2>&1 &
"#;
                            std::process::Command::new("/bin/sh")
                                .arg("-c")
                                .arg(script)
                                .arg("dc-appimage-updater")
                                .arg(&pid_str)
                                .arg(&staged_path)
                                .arg(&current_appimage)
                                .env_remove("APPIMAGE")
                                .env_remove("APPDIR")
                                .env_remove("ARGV0")
                                .env_remove("OWD")
                                .env_remove("LD_LIBRARY_PATH")
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .spawn()
                                .map_err(|e| format!("Gagal menjalankan pembaruan AppImage: {}", e))?;

                            schedule_app_exit_for_update(&app);
                            return Ok(UpdateInstallResult {
                                started_in_place: true,
                                message: format!(
                                    "Pembaruan v{} berhasil diunduh & diverifikasi SHA-256. AppImage akan diperbarui dan dijalankan ulang otomatis.",
                                    info.latest_version
                                ),
                                asset_path: Some(current_appimage.display().to_string()),
                            });
                        }
                    }
                }
            }

            let downloads_dest = resolve_path("~/Downloads").join(&safe_name);
            let final_path = if std::fs::copy(&dest_path, &downloads_dest).is_ok() {
                let _ = std::fs::remove_file(&dest_path);
                let _ = std::fs::set_permissions(&downloads_dest, std::fs::Permissions::from_mode(0o755));
                downloads_dest
            } else {
                dest_path
            };
            if let Some(parent) = final_path.parent() {
                let _ = open::that(parent);
            }
            return Ok(UpdateInstallResult {
                started_in_place: false,
                message: format!(
                    "AppImage v{} terverifikasi SHA-256 dan disimpan ke {} (siap dijalankan langsung).",
                    info.latest_version,
                    final_path.display()
                ),
                asset_path: Some(final_path.display().to_string()),
            });
        }

        if info.install_kind == "linux_deb" || info.install_kind == "linux_rpm" {
            let pkexec_available = find_executable_in_dirs(&["pkexec"], &[PathBuf::from("/usr/bin")]).is_some();
            if pkexec_available {
                let exe_path = std::env::current_exe()
                    .map_err(|e| format!("Gagal mendeteksi executable Download Cepat: {}", e))?;
                let pid_str = std::process::id().to_string();
                let script = if info.install_kind == "linux_deb" {
                    r#"
OLD_PID="$1"
PKG="$2"
EXE="$3"
while kill -0 "$OLD_PID" 2>/dev/null; do sleep 0.2; done
sleep 0.3
pkexec /bin/sh -c 'dpkg -i "$1" || apt-get install -f -y' _ "$PKG"
STATUS=$?
rm -f "$PKG"
if [ $STATUS -eq 0 ] && [ -x "$EXE" ]; then
  nohup "$EXE" >/dev/null 2>&1 &
fi
"#
                } else {
                    r#"
OLD_PID="$1"
PKG="$2"
EXE="$3"
while kill -0 "$OLD_PID" 2>/dev/null; do sleep 0.2; done
sleep 0.3
pkexec /bin/sh -c 'if command -v dnf >/dev/null 2>&1; then dnf install -y "$1"; elif command -v zypper >/dev/null 2>&1; then zypper --non-interactive install -y "$1"; else rpm -Uvh "$1"; fi' _ "$PKG"
STATUS=$?
rm -f "$PKG"
if [ $STATUS -eq 0 ] && [ -x "$EXE" ]; then
  nohup "$EXE" >/dev/null 2>&1 &
fi
"#
                };
                std::process::Command::new("/bin/sh")
                    .arg("-c")
                    .arg(script)
                    .arg("dc-linux-pkg-updater")
                    .arg(&pid_str)
                    .arg(&dest_path)
                    .arg(&exe_path)
                    .env_remove("APPIMAGE")
                    .env_remove("APPDIR")
                    .env_remove("ARGV0")
                    .env_remove("OWD")
                    .env_remove("LD_LIBRARY_PATH")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .map_err(|e| format!("Gagal menjalankan pemasang paket Linux: {}", e))?;

                schedule_app_exit_for_update(&app);
                return Ok(UpdateInstallResult {
                    started_in_place: true,
                    message: format!(
                        "Paket v{} terverifikasi SHA-256. Konfirmasi kata sandi sistem bila diminta untuk memasang pembaruan otomatis.",
                        info.latest_version
                    ),
                    asset_path: Some(dest_path.display().to_string()),
                });
            }
        }

        let _ = open::that(&dest_path);
        return Ok(UpdateInstallResult {
            started_in_place: false,
            message: format!(
                "Paket pembaruan v{} telah diunduh dan diverifikasi SHA-256 di {}.",
                info.latest_version,
                dest_path.display()
            ),
            asset_path: Some(dest_path.display().to_string()),
        });
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = open::that(&dest_path);
        Ok(UpdateInstallResult {
            started_in_place: false,
            message: format!(
                "Paket pembaruan v{} diunduh & diverifikasi SHA-256 di {}.",
                info.latest_version,
                dest_path.display()
            ),
            asset_path: Some(dest_path.display().to_string()),
        })
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[tauri::command]
fn open_extension_folder(app: tauri::AppHandle) -> Result<String, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(res_dir) = app.path().resource_dir() {
        candidates.push(res_dir.join("extension"));
        candidates.push(res_dir.join("_up_").join("extension"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("extension"));
            candidates.push(exe_dir.join("resources").join("extension"));
            for ancestor in exe_dir.ancestors().take(4) {
                candidates.push(ancestor.join("extension"));
            }
        }
    }
    if let Some(manifest_ext) = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("extension"))
    {
        candidates.push(manifest_ext);
    }
    candidates.push(resolve_path("~/.download-cepat/extension"));

    let mut found = candidates
        .into_iter()
        .find(|p| p.is_dir() && p.join("manifest.json").is_file())
        .ok_or_else(|| {
            "Folder ekstensi tidak ditemukan di paket aplikasi. Anda juga dapat memasang langsung dari Chrome Web Store."
                .to_string()
        })?;

    #[cfg(target_os = "linux")]
    {
        let found_str = found.to_string_lossy();
        if found_str.starts_with("/tmp/.mount_") || found_str.starts_with("/usr/") {
            let persistent = resolve_path("~/.download-cepat/extension");
            if copy_dir_recursive(&found, &persistent).is_ok()
                && persistent.join("manifest.json").is_file()
            {
                found = persistent;
            }
        }
    }

    found = normalize_platform_path(found);
    open::that(&found)
        .map_err(|e| format!("Gagal membuka folder ekstensi ({}): {}", found.display(), e))?;

    #[cfg(target_os = "macos")]
    return Ok(format!("Folder ekstensi dibuka di Finder: {}", found.display()));
    #[cfg(target_os = "windows")]
    return Ok(format!(
        "Folder ekstensi dibuka di File Explorer: {}",
        found.display()
    ));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return Ok(format!(
        "Folder ekstensi dibuka di File Manager: {}",
        found.display()
    ));
}

fn url_decode(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(val) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                result.push(val);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            result.push(b' ');
            i += 1;
            continue;
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(result).unwrap_or_else(|_| s.to_string())
}

fn format_bytes_label(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

async fn read_full_http_request(stream: &mut tokio::net::TcpStream) -> Option<String> {
    use tokio::io::AsyncReadExt;
    const MAX_REQ_BYTES: usize = 256 * 1024;
    let mut data: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let mut expected_total_len: Option<usize> = None;

    loop {
        if data.len() >= MAX_REQ_BYTES {
            break;
        }
        let read_res = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            stream.read(&mut chunk),
        )
        .await;

        let n = match read_res {
            Ok(Ok(n)) if n > 0 => n,
            _ => break,
        };
        data.extend_from_slice(&chunk[..n]);

        if expected_total_len.is_none() {
            if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                let header_bytes = &data[..pos];
                let header_str = String::from_utf8_lossy(header_bytes);
                let mut content_len: usize = 0;
                for line in header_str.lines() {
                    if let Some((k, v)) = line.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            if let Ok(len) = v.trim().parse::<usize>() {
                                content_len = len;
                            }
                        }
                    }
                }
                expected_total_len = Some(pos + 4 + content_len);
            }
        }

        if let Some(total) = expected_total_len {
            if data.len() >= total {
                break;
            }
        }
    }

    if data.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&data).into_owned())
    }
}

// ============================================================================
// IDM-Style Multi-Window Download Popups & Silent Background Pre-Download
// ============================================================================

const MAX_CONCURRENT_PREBUFFERS: usize = 3;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PopupPrebufferState {
    pub status: String, // "idle" | "inspecting" | "downloading" | "paused" | "completed" | "skipped" | "error"
    pub run_id: u64,
    pub url: String,
    pub file_name: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub speed_bytes_per_sec: u64,
    pub progress_percent: f64,
    pub supports_range: bool,
    pub suggested_category: Option<String>,
    pub mime_type: Option<String>,
    pub eta_seconds: Option<u64>,
    pub error: Option<String>,
    pub torrent_info: Option<core_engine::model::TorrentInfo>,
    pub media_info: Option<MediaInfo>,
}

impl Default for PopupPrebufferState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            run_id: 0,
            url: String::new(),
            file_name: None,
            downloaded_bytes: 0,
            total_bytes: None,
            speed_bytes_per_sec: 0,
            progress_percent: 0.0,
            supports_range: false,
            suggested_category: None,
            mime_type: None,
            eta_seconds: None,
            error: None,
            torrent_info: None,
            media_info: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PopupPrebufferProgressPayload {
    pub popup_id: String,
    pub state: PopupPrebufferState,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PopupWindowSession {
    pub popup_id: String,
    pub mode: String,         // "confirm" | "mini_progress"
    pub source: String,       // "manual" | "browser" | "tray" | "clipboard" | "drop" | "torrent"
    pub initial_mode: String, // "url" | "torrent"
    pub url: String,
    pub file_name: String,
    pub destination_path: String,
    pub category_id: String,
    pub num_connections: usize,
    pub format_id: Option<String>,
    pub resolution: Option<String>,
    pub cookie: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub file_size: Option<u64>,
    pub mime_type: Option<String>,
    pub proxy_url: Option<String>,
    pub auth_username: Option<String>,
    pub auth_password: Option<String>,
    pub speed_limit_bytes_per_sec: Option<u64>,
    pub task_id: Option<String>,
    pub prebuffer: PopupPrebufferState,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDownloadPopupParams {
    pub source: Option<String>,
    pub initial_mode: Option<String>,
    pub url: Option<String>,
    pub file_name: Option<String>,
    pub cookie: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub file_size: Option<u64>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPopupPreDownloadParams {
    pub popup_id: String,
    pub url: String,
    pub cookie: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub force: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PopupPromoteParams {
    pub popup_id: String,
    pub url: String,
    pub file_name: String,
    pub destination_path: String,
    pub category_id: String,
    pub num_connections: usize,
    pub format_id: Option<String>,
    pub resolution: Option<String>,
    pub cookie: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub proxy_url: Option<String>,
    pub auth_username: Option<String>,
    pub auth_password: Option<String>,
    pub speed_limit_kbs: Option<u64>,
    pub file_size: Option<u64>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PopupTaskEventPayload {
    id: String,
    url: String,
    file_name: String,
    destination_path: String,
    category_id: String,
    file_size: Option<u64>,
    downloaded_bytes: u64,
    progress_percent: f64,
    status: String,
    supports_range: bool,
    mime_type: Option<String>,
    num_connections: usize,
    speed_limit_bytes_per_sec: u64,
    format_id: Option<String>,
    resolution: Option<String>,
    cookie: Option<String>,
    referer: Option<String>,
    user_agent: Option<String>,
    proxy_url: Option<String>,
    auth_username: Option<String>,
    auth_password: Option<String>,
    source: String,
}

struct PrebufferRuntime {
    run_id: u64,
    url: String,
    part_path: PathBuf,
    controller: core_engine::DownloadController,
    finished: Arc<std::sync::atomic::AtomicBool>,
    finished_notify: Arc<Notify>,
}

enum PrebufferTakeOutcome {
    None,
    Completed {
        part_path: PathBuf,
        total_bytes: u64,
    },
    Resumable {
        part_path: PathBuf,
        meta_path: PathBuf,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
}

fn popup_sessions() -> &'static Mutex<HashMap<String, PopupWindowSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, PopupWindowSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn popup_prebuffers() -> &'static Mutex<HashMap<String, Arc<PrebufferRuntime>>> {
    static PREBUFFERS: OnceLock<Mutex<HashMap<String, Arc<PrebufferRuntime>>>> = OnceLock::new();
    PREBUFFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn prebuffer_semaphore() -> &'static Arc<tokio::sync::Semaphore> {
    static SEM: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    SEM.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_PREBUFFERS)))
}

fn next_generated_task_id() -> String {
    static TASK_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = TASK_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("task-{}-{}", now_ms, seq)
}

fn next_popup_id_and_stagger() -> (String, usize) {
    static POPUP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let seq = POPUP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let active_count = popup_sessions()
        .lock()
        .map(|m| m.len())
        .unwrap_or(seq as usize);
    (format!("dl-popup-{}", seq), active_count)
}

fn next_prebuffer_run_id() -> u64 {
    static RUN_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    RUN_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn sanitize_popup_id(raw: &str) -> String {
    let clean: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    if clean.is_empty() {
        "popup_invalid".to_string()
    } else {
        clean
    }
}

fn prebuffer_root_dir() -> PathBuf {
    let base = user_home_dir()
        .map(|h| h.join(".download-cepat"))
        .unwrap_or_else(|| std::env::temp_dir().join("download-cepat"));
    base.join("prebuffer")
}

fn prebuffer_part_path_for(popup_id: &str, run_id: u64) -> PathBuf {
    let safe = sanitize_popup_id(popup_id);
    prebuffer_root_dir().join(format!("{}_r{}.part", safe, run_id))
}

fn cleanup_prebuffer_files_at(part_path: &Path) -> usize {
    let mut removed = 0usize;
    let meta = core_engine::downloader::checkpoint_path_for(part_path);
    if part_path.is_file() && std::fs::remove_file(part_path).is_ok() {
        removed += 1;
    }
    if meta.is_file() && std::fs::remove_file(&meta).is_ok() {
        removed += 1;
    }
    let ytdl = PathBuf::from(format!("{}.ytdl", part_path.display()));
    if ytdl.is_file() && std::fs::remove_file(&ytdl).is_ok() {
        removed += 1;
    }
    removed
}

fn cleanup_prebuffer_by_id_prefix_at(root: &Path, popup_id: &str) -> usize {
    if !root.is_dir() {
        return 0;
    }
    let safe = sanitize_popup_id(popup_id);
    let prefix_run = format!("{}_", safe);
    let prefix_dot = format!("{}.", safe);
    let mut removed = 0usize;
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let p = entry.path();
            let Some(name) = entry.file_name().to_str().map(String::from) else {
                continue;
            };
            if name.starts_with(&prefix_run) || name.starts_with(&prefix_dot) {
                if p.is_file() && std::fs::remove_file(&p).is_ok() {
                    removed += 1;
                }
            }
        }
    }
    removed
}

fn cleanup_stale_prebuffer_dir_at(root: &Path) -> usize {
    if !root.is_dir() {
        return 0;
    }
    let mut removed = 0usize;
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && std::fs::remove_file(&p).is_ok() {
                removed += 1;
            } else if p.is_dir() && std::fs::remove_dir_all(&p).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

fn cleanup_stale_prebuffer_dir() -> usize {
    cleanup_stale_prebuffer_dir_at(&prebuffer_root_dir())
}

fn compute_popup_position(
    work_area_x: i32,
    work_area_y: i32,
    work_area_w: u32,
    work_area_h: u32,
    scale_factor: f64,
    win_w: f64,
    win_h: f64,
    stagger_index: usize,
) -> (f64, f64) {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let area_x = (work_area_x as f64) / scale;
    let area_y = (work_area_y as f64) / scale;
    let area_w = ((work_area_w as f64) / scale).max(win_w);
    let area_h = ((work_area_h as f64) / scale).max(win_h);

    let step = (stagger_index % 6) as f64 * 28.0;
    let base_x = area_x + ((area_w - win_w) * 0.5).max(0.0);
    let base_y = area_y + ((area_h - win_h) * 0.5).max(0.0);

    let max_x = area_x + (area_w - win_w).max(0.0);
    let max_y = area_y + (area_h - win_h).max(0.0);

    let x = (base_x + step).clamp(area_x, max_x);
    let y = (base_y + step).clamp(area_y, max_y);
    (x, y)
}

fn derive_initial_popup_meta(
    url: &str,
    provided_file_name: Option<&str>,
    mime_type: Option<&str>,
    initial_mode: &str,
    rt: &RuntimeDownloadSettings,
) -> (String, String, String) {
    let trimmed = url.trim();
    let explicit_name = provided_file_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(core_engine::inspector::sanitize_filename);

    if trimmed.is_empty() {
        let cat = if initial_mode == "torrent" {
            "torrents".to_string()
        } else {
            "general".to_string()
        };
        let dir = resolve_save_dir_for_category(rt, &cat).to_string();
        return (String::new(), cat, dir);
    }

    if core_engine::torrent::is_magnet_uri(trimmed) {
        let name = explicit_name.unwrap_or_else(|| {
            if let Ok(mag) = core_engine::torrent::parse_magnet_uri(trimmed) {
                mag.display_name
                    .unwrap_or_else(|| format!("torrent_{}", &mag.info_hash_hex[..12]))
            } else {
                "torrent_download".to_string()
            }
        });
        let dest = join_save_dir_and_file(resolve_save_dir_for_category(rt, "torrents"), &name);
        return (name, "torrents".to_string(), dest);
    }

    let is_torrent_mime = mime_type
        .map(|m| m.to_ascii_lowercase().contains("bittorrent"))
        .unwrap_or(false);
    if core_engine::torrent::is_torrent_reference(trimmed) || is_torrent_mime {
        let fallback_name = explicit_name.unwrap_or_else(|| {
            let from_url = trimmed
                .split(['?', '#'])
                .next()
                .unwrap_or(trimmed)
                .rsplit(['/', '\\'])
                .next()
                .map(url_decode)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "download.torrent".to_string());
            let clean = core_engine::inspector::sanitize_filename(&from_url);
            clean.strip_suffix(".torrent").unwrap_or(&clean).to_string()
        });
        let name = if fallback_name.is_empty() {
            "torrent_download".to_string()
        } else {
            fallback_name
        };
        let dest = join_save_dir_and_file(resolve_save_dir_for_category(rt, "torrents"), &name);
        return (name, "torrents".to_string(), dest);
    }

    if is_video_or_stream_url(trimmed) {
        let name = explicit_name.unwrap_or_else(|| "video_download.mp4".to_string());
        let cat = "video".to_string();
        let dest = join_save_dir_and_file(resolve_save_dir_for_category(rt, &cat), &name);
        return (name, cat, dest);
    }

    let inferred_name = explicit_name.unwrap_or_else(|| {
        let candidate = trimmed
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .rsplit(['/', '\\'])
            .next()
            .map(url_decode)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "download.bin".to_string());
        core_engine::inspector::sanitize_filename(&candidate)
    });
    let cat = core_engine::inspector::categorize_filename(&inferred_name);
    let dest = join_save_dir_and_file(resolve_save_dir_for_category(rt, &cat), &inferred_name);
    (inferred_name, cat, dest)
}

async fn create_popup_webview_window(
    app: &tauri::AppHandle,
    popup_id: &str,
    title: &str,
    width: f64,
    height: f64,
    min_width: f64,
    min_height: f64,
    stagger_idx: usize,
) -> Result<(), String> {
    let app_clone = app.clone();
    let label = popup_id.to_string();
    let win_title = title.to_string();

    tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        let _ = app_clone.set_activation_policy(tauri::ActivationPolicy::Regular);

        let url_str = format!("index.html?window=popup&popupId={}", label);
        let mut builder = tauri::WebviewWindowBuilder::new(
            &app_clone,
            label,
            tauri::WebviewUrl::App(PathBuf::from(url_str)),
        )
        .title(win_title)
        .inner_size(width, height)
        .min_inner_size(min_width, min_height)
        .resizable(true)
        .minimizable(true)
        .maximizable(true)
        .closable(true)
        .decorations(true)
        .focused(true)
        .visible(true);

        if let Ok(Some(monitor)) = app_clone.primary_monitor() {
            let wa = monitor.work_area();
            let scale = monitor.scale_factor();
            let (x, y) = compute_popup_position(
                wa.position.x,
                wa.position.y,
                wa.size.width,
                wa.size.height,
                scale,
                width,
                height,
                stagger_idx,
            );
            builder = builder.position(x, y);
        } else {
            builder = builder.center();
        }

        builder
            .build()
            .map(|_| ())
            .map_err(|e| format!("Failed creating popup window: {}", e))
    })
    .await
    .map_err(|e| format!("Failed joining popup window task: {}", e))?
}

async fn update_popup_window_for_mini_progress(
    app: &tauri::AppHandle,
    popup_id: &str,
    file_name: &str,
) {
    let app_clone = app.clone();
    let label = popup_id.to_string();
    let title = if file_name.trim().is_empty() {
        "Downloading - Download Cepat".to_string()
    } else {
        format!("{} - Download Cepat", file_name.trim())
    };
    let _ = tokio::task::spawn_blocking(move || {
        if let Some(win) = app_clone.get_webview_window(&label) {
            let _ = win.set_min_size(Some(tauri::LogicalSize::new(440.0, 300.0)));
            let _ = win.set_size(tauri::LogicalSize::new(520.0, 360.0));
            let _ = win.set_title(&title);
            let _ = win.unminimize();
            let _ = win.show();
        }
    })
    .await;
}

async fn close_popup_window_by_label(app: &tauri::AppHandle, popup_id: &str) {
    let app_clone = app.clone();
    let label = popup_id.to_string();
    let _ = tokio::task::spawn_blocking(move || {
        if let Some(win) = app_clone.get_webview_window(&label) {
            let _ = win.close();
        }
    })
    .await;
}

fn emit_prebuffer_update(
    app: &tauri::AppHandle,
    popup_id: &str,
    run_id: u64,
    mut update_fn: impl FnMut(&mut PopupWindowSession),
) -> bool {
    let payload_opt: Option<PopupPrebufferProgressPayload> = {
        let Ok(mut map) = popup_sessions().lock() else {
            return false;
        };
        let Some(session) = map.get_mut(popup_id) else {
            return false;
        };
        if session.mode != "confirm" || session.prebuffer.run_id != run_id {
            return false;
        }
        update_fn(session);
        Some(PopupPrebufferProgressPayload {
            popup_id: popup_id.to_string(),
            state: session.prebuffer.clone(),
        })
    };
    if let Some(payload) = payload_opt {
        let _ = app.emit("popup-prebuffer-progress", payload);
        true
    } else {
        false
    }
}

fn is_popup_prebuffer_run_current(popup_id: &str, run_id: u64) -> bool {
    popup_sessions()
        .lock()
        .ok()
        .and_then(|map| {
            map.get(popup_id)
                .map(|s| s.mode == "confirm" && s.prebuffer.run_id == run_id)
        })
        .unwrap_or(false)
}

async fn start_popup_pre_download_internal(
    app: &tauri::AppHandle,
    popup_id: &str,
    raw_url: &str,
    cookie: Option<String>,
    referer: Option<String>,
    user_agent: Option<String>,
    force: bool,
) -> Option<PopupPrebufferState> {
    let url = raw_url.trim().to_string();

    if !force && !url.is_empty() {
        if let Ok(map) = popup_sessions().lock() {
            if let Some(s) = map.get(popup_id) {
                if s.mode == "confirm"
                    && s.prebuffer.url == url
                    && matches!(
                        s.prebuffer.status.as_str(),
                        "inspecting" | "downloading" | "completed" | "skipped"
                    )
                {
                    return Some(s.prebuffer.clone());
                }
            }
        }
    }

    // Cancel any previous in-flight prebuffer runtime for this popup
    if let Ok(mut pre_map) = popup_prebuffers().lock() {
        if let Some(prev) = pre_map.remove(popup_id) {
            prev.controller.cancel();
            cleanup_prebuffer_files_at(&prev.part_path);
        }
    }

    let run_id = next_prebuffer_run_id();
    let part_path = prebuffer_part_path_for(popup_id, run_id);

    if url.is_empty() {
        let state = PopupPrebufferState {
            status: "idle".to_string(),
            run_id,
            url: String::new(),
            ..Default::default()
        };
        if let Ok(mut map) = popup_sessions().lock() {
            if let Some(session) = map.get_mut(popup_id) {
                if session.mode == "confirm" {
                    session.url = String::new();
                    session.cookie = cookie;
                    session.referer = referer;
                    session.user_agent = user_agent;
                    session.prebuffer = state.clone();
                }
            }
        }
        let _ = app.emit(
            "popup-prebuffer-progress",
            PopupPrebufferProgressPayload {
                popup_id: popup_id.to_string(),
                state: state.clone(),
            },
        );
        return Some(state);
    }

    let initial_state = PopupPrebufferState {
        status: "inspecting".to_string(),
        run_id,
        url: url.clone(),
        ..Default::default()
    };

    {
        let Ok(mut map) = popup_sessions().lock() else {
            return None;
        };
        let Some(session) = map.get_mut(popup_id) else {
            return None;
        };
        if session.mode != "confirm" {
            return None;
        }
        session.url = url.clone();
        if cookie.is_some() {
            session.cookie = cookie.clone();
        }
        if referer.is_some() {
            session.referer = referer.clone();
        }
        if user_agent.is_some() {
            session.user_agent = user_agent.clone();
        }
        session.prebuffer = initial_state.clone();
    }

    let _ = app.emit(
        "popup-prebuffer-progress",
        PopupPrebufferProgressPayload {
            popup_id: popup_id.to_string(),
            state: initial_state.clone(),
        },
    );

    let ctrl = core_engine::DownloadController::new();
    let finished_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let finished_notify = Arc::new(Notify::new());
    let runtime = Arc::new(PrebufferRuntime {
        run_id,
        url: url.clone(),
        part_path: part_path.clone(),
        controller: ctrl.clone(),
        finished: finished_flag.clone(),
        finished_notify: finished_notify.clone(),
    });

    if let Ok(mut pre_map) = popup_prebuffers().lock() {
        pre_map.insert(popup_id.to_string(), runtime.clone());
    }

    let app_clone = app.clone();
    let popup_id_own = popup_id.to_string();
    tauri::async_runtime::spawn(async move {
        let mark_finished = || {
            finished_flag.store(true, std::sync::atomic::Ordering::SeqCst);
            finished_notify.notify_waiters();
        };

        let rt_settings = get_runtime_settings();
        let proxy_url = rt_settings.proxy_url.clone();

        // Video platform / manifest streams or explicit torrent links: inspect metadata cleanly without pre-buffering
        if is_video_or_stream_url(&url) || core_engine::torrent::is_torrent_reference(&url) {
            let inspect_res = tokio::select! {
                res = inspect_url(
                    url.clone(),
                    cookie.clone(),
                    referer.clone(),
                    user_agent.clone(),
                    proxy_url.clone(),
                    None,
                    None,
                ) => Some(res),
                _ = ctrl.notified() => None,
            };

            if ctrl.is_stopped() || !is_popup_prebuffer_run_current(&popup_id_own, run_id) {
                cleanup_prebuffer_files_at(&part_path);
                mark_finished();
                return;
            }

            match inspect_res {
                Some(Ok(res)) => {
                    emit_prebuffer_update(&app_clone, &popup_id_own, run_id, |s| {
                        s.prebuffer.status = "skipped".to_string();
                        s.prebuffer.file_name = Some(res.file_name.clone());
                        s.prebuffer.total_bytes = res.file_size;
                        s.prebuffer.supports_range = res.supports_range;
                        s.prebuffer.suggested_category = Some(res.suggested_category.clone());
                        s.prebuffer.mime_type = res.mime_type.clone();
                        s.prebuffer.torrent_info = res.torrent_info.clone();
                        s.prebuffer.media_info = res.media_info.clone();
                        s.prebuffer.error = None;
                        if res.file_size.is_some() {
                            s.file_size = res.file_size;
                        }
                        if res.mime_type.is_some() {
                            s.mime_type = res.mime_type.clone();
                        }
                    });
                }
                Some(Err(e)) => {
                    emit_prebuffer_update(&app_clone, &popup_id_own, run_id, |s| {
                        s.prebuffer.status = "error".to_string();
                        s.prebuffer.error = Some(e.clone());
                    });
                }
                None => {}
            }
            mark_finished();
            return;
        }

        // Standard HTTP(S) / FTP inspection
        let inspector = match core_engine::inspector::UrlInspector::new() {
            Ok(i) => i,
            Err(e) => {
                emit_prebuffer_update(&app_clone, &popup_id_own, run_id, |s| {
                    s.prebuffer.status = "error".to_string();
                    s.prebuffer.error = Some(format!("Inspector init failed: {}", e));
                });
                mark_finished();
                return;
            }
        };

        let inspect_res = tokio::select! {
            res = inspector.inspect_with_options(
                &url,
                core_engine::inspector::InspectOptions {
                    cookie: cookie.as_deref(),
                    referer: referer.as_deref(),
                    user_agent: user_agent.as_deref(),
                    proxy_url: proxy_url.as_deref(),
                    auth_username: None,
                    auth_password: None,
                },
            ) => Some(res),
            _ = ctrl.notified() => None,
        };

        if ctrl.is_stopped() || !is_popup_prebuffer_run_current(&popup_id_own, run_id) {
            cleanup_prebuffer_files_at(&part_path);
            mark_finished();
            return;
        }

        let inspected = match inspect_res {
            Some(Ok(r)) => r,
            Some(Err(e)) => {
                emit_prebuffer_update(&app_clone, &popup_id_own, run_id, |s| {
                    s.prebuffer.status = "error".to_string();
                    s.prebuffer.error = Some(format!("{}", e));
                });
                mark_finished();
                return;
            }
            None => {
                mark_finished();
                return;
            }
        };

        // If the HTTP response turned out to be a .torrent metainfo file, skip prebuffering cleanly
        if inspected.torrent_info.is_some()
            || core_engine::torrent::is_torrent_reference(&inspected.final_url)
        {
            emit_prebuffer_update(&app_clone, &popup_id_own, run_id, |s| {
                s.prebuffer.status = "skipped".to_string();
                s.prebuffer.file_name = Some(inspected.file_name.clone());
                s.prebuffer.total_bytes = inspected.file_size;
                s.prebuffer.supports_range = true;
                s.prebuffer.suggested_category = Some("torrents".to_string());
                s.prebuffer.mime_type = inspected.mime_type.clone();
                s.prebuffer.torrent_info = inspected.torrent_info.clone();
                s.prebuffer.error = None;
                if inspected.file_size.is_some() {
                    s.file_size = inspected.file_size;
                }
                if inspected.mime_type.is_some() {
                    s.mime_type = inspected.mime_type.clone();
                }
            });
            mark_finished();
            return;
        }

        let file_size = inspected.file_size;
        let supports_range = inspected.supports_range;
        let dl_url = inspected.final_url.clone();

        if !emit_prebuffer_update(&app_clone, &popup_id_own, run_id, |s| {
            s.prebuffer.status = "downloading".to_string();
            s.prebuffer.file_name = Some(inspected.file_name.clone());
            s.prebuffer.total_bytes = file_size;
            s.prebuffer.supports_range = supports_range;
            s.prebuffer.suggested_category = Some(inspected.suggested_category.clone());
            s.prebuffer.mime_type = inspected.mime_type.clone();
            s.prebuffer.error = None;
            if file_size.is_some() {
                s.file_size = file_size;
            }
            if inspected.mime_type.is_some() {
                s.mime_type = inspected.mime_type.clone();
            }
        }) {
            mark_finished();
            return;
        }

        // Acquire bounded prebuffer concurrency slot (max 3 simultaneous prebuffers)
        let permit = tokio::select! {
            p = prebuffer_semaphore().clone().acquire_owned() => p.ok(),
            _ = ctrl.notified() => None,
        };

        let Some(_permit) = permit else {
            if ctrl.is_canceled() {
                cleanup_prebuffer_files_at(&part_path);
            }
            mark_finished();
            return;
        };

        if ctrl.is_stopped() || !is_popup_prebuffer_run_current(&popup_id_own, run_id) {
            if ctrl.is_canceled() {
                cleanup_prebuffer_files_at(&part_path);
            }
            mark_finished();
            return;
        }

        if let Some(parent) = part_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let downloader = match core_engine::downloader::Downloader::new() {
            Ok(d) => d,
            Err(e) => {
                emit_prebuffer_update(&app_clone, &popup_id_own, run_id, |s| {
                    s.prebuffer.status = "error".to_string();
                    s.prebuffer.error = Some(format!("Downloader init failed: {}", e));
                });
                mark_finished();
                return;
            }
        };

        let (tx, mut rx) =
            tokio::sync::broadcast::channel::<core_engine::model::ProgressTelemetry>(128);

        let app_monitor = app_clone.clone();
        let popup_monitor = popup_id_own.clone();
        let monitor_handle = tokio::spawn(async move {
            let mut last_downloaded = 0u64;
            let mut last_total = 0u64;
            while let Ok(telem) = rx.recv().await {
                last_downloaded = telem.downloaded_bytes;
                if let Some(t) = telem.total_bytes {
                    if t > 0 {
                        last_total = t;
                    }
                }
                let is_terminal = matches!(
                    telem.status,
                    core_engine::model::TaskStatus::Completed
                        | core_engine::model::TaskStatus::Error
                        | core_engine::model::TaskStatus::Paused
                        | core_engine::model::TaskStatus::Canceled
                );
                let status_str = match telem.status {
                    core_engine::model::TaskStatus::Paused => "paused",
                    core_engine::model::TaskStatus::Canceled => "paused",
                    core_engine::model::TaskStatus::Completed => "completed",
                    core_engine::model::TaskStatus::Error => "error",
                    _ => "downloading",
                };
                emit_prebuffer_update(&app_monitor, &popup_monitor, run_id, |s| {
                    s.prebuffer.status = status_str.to_string();
                    s.prebuffer.downloaded_bytes = telem.downloaded_bytes;
                    if let Some(tb) = telem.total_bytes {
                        if tb > 0 {
                            s.prebuffer.total_bytes = Some(tb);
                        }
                    }
                    s.prebuffer.speed_bytes_per_sec = telem.speed_bytes_per_sec;
                    s.prebuffer.progress_percent = telem.progress_percent as f64;
                    s.prebuffer.eta_seconds = telem.eta_seconds;
                });
                if is_terminal {
                    break;
                }
            }
            (last_downloaded, last_total)
        });

        let speed_limit_bps = (rt_settings.speed_limit_kbs > 0)
            .then_some(rt_settings.speed_limit_kbs * 1024);
        let pre_conns = rt_settings.default_connections.clamp(1, 32);

        let dl_res = downloader
            .download_with_options(
                &format!("prebuf-{}", run_id),
                &dl_url,
                &part_path,
                file_size,
                supports_range,
                pre_conns,
                Some(tx),
                Some(ctrl.clone()),
                core_engine::downloader::DownloadOptions {
                    cookie: cookie.as_deref(),
                    referer: referer.as_deref(),
                    user_agent: user_agent.as_deref(),
                    speed_limit_bytes_per_sec: speed_limit_bps,
                    proxy_url: proxy_url.as_deref(),
                    auth_username: None,
                    auth_password: None,
                    allow_url_refresh: true,
                },
            )
            .await;

        let (last_downloaded, last_total) = monitor_handle.await.unwrap_or((0, 0));

        if ctrl.is_canceled() || !is_popup_prebuffer_run_current(&popup_id_own, run_id) {
            cleanup_prebuffer_files_at(&part_path);
            mark_finished();
            return;
        }

        if ctrl.is_paused() {
            emit_prebuffer_update(&app_clone, &popup_id_own, run_id, |s| {
                s.prebuffer.status = "paused".to_string();
                s.prebuffer.speed_bytes_per_sec = 0;
            });
            mark_finished();
            return;
        }

        match dl_res {
            Ok(_) => {
                let disk_len = std::fs::metadata(&part_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                let final_len = if disk_len > 0 {
                    disk_len
                } else {
                    last_total.max(last_downloaded)
                };
                emit_prebuffer_update(&app_clone, &popup_id_own, run_id, |s| {
                    s.prebuffer.status = "completed".to_string();
                    s.prebuffer.downloaded_bytes = final_len;
                    s.prebuffer.total_bytes = Some(final_len);
                    s.prebuffer.speed_bytes_per_sec = 0;
                    s.prebuffer.progress_percent = 100.0;
                    s.prebuffer.eta_seconds = Some(0);
                    s.prebuffer.error = None;
                });
            }
            Err(e) => {
                cleanup_prebuffer_files_at(&part_path);
                emit_prebuffer_update(&app_clone, &popup_id_own, run_id, |s| {
                    s.prebuffer.status = "error".to_string();
                    s.prebuffer.speed_bytes_per_sec = 0;
                    s.prebuffer.error = Some(format!("{}", e));
                });
            }
        }

        mark_finished();
    });

    Some(initial_state)
}

async fn stop_and_take_prebuffer(popup_id: &str, requested_url: &str) -> PrebufferTakeOutcome {
    let runtime_opt = popup_prebuffers()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(popup_id));

    let Some(runtime) = runtime_opt else {
        cleanup_prebuffer_by_id_prefix_at(&prebuffer_root_dir(), popup_id);
        return PrebufferTakeOutcome::None;
    };

    if !runtime.finished.load(std::sync::atomic::Ordering::SeqCst) {
        runtime.controller.pause();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(2000);
        while !runtime.finished.load(std::sync::atomic::Ordering::SeqCst) {
            let notified = runtime.finished_notify.notified();
            tokio::pin!(notified);
            if runtime.finished.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                break;
            }
            if tokio::time::timeout(deadline - now, notified).await.is_err() {
                break;
            }
        }
    }

    let snapshot = popup_sessions()
        .lock()
        .ok()
        .and_then(|map| map.get(popup_id).map(|s| s.prebuffer.clone()));

    let Some(prebuf) = snapshot else {
        cleanup_prebuffer_files_at(&runtime.part_path);
        return PrebufferTakeOutcome::None;
    };

    if prebuf.run_id != runtime.run_id
        || runtime.url.trim() != requested_url.trim()
        || !runtime.finished.load(std::sync::atomic::Ordering::SeqCst)
    {
        cleanup_prebuffer_files_at(&runtime.part_path);
        return PrebufferTakeOutcome::None;
    }

    let part_path = runtime.part_path.clone();
    let meta_path = core_engine::downloader::checkpoint_path_for(&part_path);

    if prebuf.status == "completed" && part_path.is_file() {
        let total_bytes = std::fs::metadata(&part_path)
            .map(|m| m.len())
            .unwrap_or_else(|_| prebuf.total_bytes.unwrap_or(prebuf.downloaded_bytes));
        return PrebufferTakeOutcome::Completed {
            part_path,
            total_bytes,
        };
    }

    if prebuf.supports_range && part_path.is_file() && meta_path.is_file() {
        return PrebufferTakeOutcome::Resumable {
            part_path,
            meta_path,
            downloaded_bytes: prebuf.downloaded_bytes,
            total_bytes: prebuf.total_bytes,
        };
    }

    cleanup_prebuffer_files_at(&part_path);
    PrebufferTakeOutcome::None
}

async fn cleanup_popup_on_window_destroyed(popup_id: &str) {
    let session_opt = popup_sessions()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(popup_id));

    let keep_main_task = session_opt
        .as_ref()
        .map(|s| s.mode == "mini_progress")
        .unwrap_or(false);

    if let Ok(mut pre_map) = popup_prebuffers().lock() {
        if let Some(rt) = pre_map.remove(popup_id) {
            rt.controller.cancel();
            cleanup_prebuffer_files_at(&rt.part_path);
        }
    }

    if !keep_main_task {
        cleanup_prebuffer_by_id_prefix_at(&prebuffer_root_dir(), popup_id);
    }
}

async fn spawn_download_popup(
    app: &tauri::AppHandle,
    params: OpenDownloadPopupParams,
) -> Result<String, String> {
    let (popup_id, stagger_idx) = next_popup_id_and_stagger();
    let rt = get_runtime_settings();
    let source = params
        .source
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("manual")
        .to_string();
    let initial_mode = params
        .initial_mode
        .as_deref()
        .map(str::trim)
        .filter(|s| *s == "torrent" || *s == "url")
        .unwrap_or("url")
        .to_string();
    let url = params.url.as_deref().map(str::trim).unwrap_or("").to_string();
    let (file_name, category_id, destination_path) = derive_initial_popup_meta(
        &url,
        params.file_name.as_deref(),
        params.mime_type.as_deref(),
        &initial_mode,
        &rt,
    );

    let session = PopupWindowSession {
        popup_id: popup_id.clone(),
        mode: "confirm".to_string(),
        source,
        initial_mode: initial_mode.clone(),
        url: url.clone(),
        file_name: file_name.clone(),
        destination_path,
        category_id,
        num_connections: rt.default_connections.clamp(1, 32),
        format_id: None,
        resolution: None,
        cookie: params.cookie.clone(),
        referer: params.referer.clone(),
        user_agent: params.user_agent.clone(),
        file_size: params.file_size,
        mime_type: params.mime_type.clone(),
        proxy_url: rt.proxy_url.clone(),
        auth_username: None,
        auth_password: None,
        speed_limit_bytes_per_sec: (rt.speed_limit_kbs > 0).then_some(rt.speed_limit_kbs * 1024),
        task_id: None,
        prebuffer: PopupPrebufferState::default(),
    };

    if let Ok(mut map) = popup_sessions().lock() {
        map.insert(popup_id.clone(), session);
    }

    let win_title = if !file_name.is_empty() {
        format!("Download - {}", file_name)
    } else if initial_mode == "torrent" {
        "Add Torrent - Download Cepat".to_string()
    } else {
        "Add New Download - Download Cepat".to_string()
    };

    if let Err(e) = create_popup_webview_window(
        app,
        &popup_id,
        &win_title,
        590.0,
        540.0,
        480.0,
        430.0,
        stagger_idx,
    )
    .await
    {
        if let Ok(mut map) = popup_sessions().lock() {
            map.remove(&popup_id);
        }
        return Err(e);
    }

    if !url.is_empty() {
        let _ = start_popup_pre_download_internal(
            app,
            &popup_id,
            &url,
            params.cookie,
            params.referer,
            params.user_agent,
            true,
        )
        .await;
    }

    Ok(popup_id)
}

#[allow(clippy::too_many_arguments)]
async fn spawn_mini_progress_popup(
    app: &tauri::AppHandle,
    task_id: String,
    source: String,
    url: String,
    file_name: String,
    destination_path: String,
    category_id: String,
    num_connections: usize,
    format_id: Option<String>,
    resolution: Option<String>,
    cookie: Option<String>,
    referer: Option<String>,
    user_agent: Option<String>,
    proxy_url: Option<String>,
    speed_limit_bytes_per_sec: Option<u64>,
    is_queued: bool,
) -> Result<String, String> {
    let (popup_id, stagger_idx) = next_popup_id_and_stagger();
    let session = PopupWindowSession {
        popup_id: popup_id.clone(),
        mode: "mini_progress".to_string(),
        source,
        initial_mode: "url".to_string(),
        url,
        file_name: file_name.clone(),
        destination_path,
        category_id,
        num_connections: num_connections.clamp(1, 32),
        format_id,
        resolution,
        cookie,
        referer,
        user_agent,
        file_size: None,
        mime_type: None,
        proxy_url,
        auth_username: None,
        auth_password: None,
        speed_limit_bytes_per_sec,
        task_id: Some(task_id),
        prebuffer: PopupPrebufferState {
            status: if is_queued {
                "idle".to_string()
            } else {
                "skipped".to_string()
            },
            ..Default::default()
        },
    };

    if let Ok(mut map) = popup_sessions().lock() {
        map.insert(popup_id.clone(), session);
    }

    let win_title = format!("{} - Download Cepat", file_name);
    if let Err(e) = create_popup_webview_window(
        app,
        &popup_id,
        &win_title,
        520.0,
        360.0,
        440.0,
        300.0,
        stagger_idx,
    )
    .await
    {
        if let Ok(mut map) = popup_sessions().lock() {
            map.remove(&popup_id);
        }
        return Err(e);
    }

    Ok(popup_id)
}

#[tauri::command]
async fn open_download_popup(
    app: tauri::AppHandle,
    params: OpenDownloadPopupParams,
) -> Result<String, String> {
    spawn_download_popup(&app, params).await
}

#[tauri::command]
fn get_popup_session(popup_id: String) -> Option<PopupWindowSession> {
    popup_sessions()
        .lock()
        .ok()
        .and_then(|map| map.get(&popup_id).cloned())
}

#[tauri::command]
async fn start_popup_pre_download(
    app: tauri::AppHandle,
    params: StartPopupPreDownloadParams,
) -> Result<Option<PopupPrebufferState>, String> {
    Ok(start_popup_pre_download_internal(
        &app,
        &params.popup_id,
        &params.url,
        params.cookie,
        params.referer,
        params.user_agent,
        params.force.unwrap_or(false),
    )
    .await)
}

#[tauri::command]
async fn promote_popup_download(
    app: tauri::AppHandle,
    params: PopupPromoteParams,
) -> Result<PopupWindowSession, String> {
    let url = params.url.trim().to_string();
    if url.is_empty() {
        return Err("Download URL cannot be empty".to_string());
    }

    let raw_target = resolve_path(&params.destination_path);
    let target = resolve_unique_destination(&raw_target);
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let final_dest_str = target.display().to_string();

    let take_outcome = stop_and_take_prebuffer(&params.popup_id, &url).await;
    let task_id = next_generated_task_id();
    let rt = get_runtime_settings();
    let effective_conns = params.num_connections.clamp(1, 32);
    let eff_speed_kbs = params
        .speed_limit_kbs
        .filter(|&k| k > 0)
        .or_else(|| (rt.speed_limit_kbs > 0).then_some(rt.speed_limit_kbs));
    let eff_speed_bps = eff_speed_kbs.unwrap_or(0) * 1024;
    let eff_proxy = params
        .proxy_url
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| rt.proxy_url.clone());

    let (
        initial_status,
        initial_downloaded,
        initial_total,
        initial_progress,
        initial_range,
        start_resume,
        should_spawn_download,
    ) = match take_outcome {
        PrebufferTakeOutcome::Completed {
            part_path,
            total_bytes,
        } => {
            if let Err(e) = move_file_cross_device(&part_path, &target) {
                cleanup_prebuffer_files_at(&part_path);
                return Err(e);
            }
            cleanup_prebuffer_files_at(&part_path);
            let actual_len = path_content_size(&target).max(total_bytes);
            (
                "completed".to_string(),
                actual_len,
                Some(actual_len),
                100.0,
                true,
                false,
                false,
            )
        }
        PrebufferTakeOutcome::Resumable {
            part_path,
            meta_path,
            downloaded_bytes,
            total_bytes,
        } => {
            let target_meta = core_engine::downloader::checkpoint_path_for(&target);
            if move_file_cross_device(&part_path, &target).is_ok()
                && move_file_cross_device(&meta_path, &target_meta).is_ok()
            {
                let total_est = total_bytes.or(params.file_size);
                let pct = match total_est {
                    Some(t) if t > 0 => ((downloaded_bytes as f64 / t as f64) * 100.0).min(99.9),
                    _ => 0.0,
                };
                (
                    "downloading".to_string(),
                    downloaded_bytes,
                    total_est,
                    pct,
                    true,
                    true,
                    true,
                )
            } else {
                cleanup_download_artifacts(&target);
                cleanup_prebuffer_files_at(&part_path);
                (
                    "downloading".to_string(),
                    0,
                    params.file_size,
                    0.0,
                    false,
                    false,
                    true,
                )
            }
        }
        PrebufferTakeOutcome::None => (
            "downloading".to_string(),
            0,
            params.file_size,
            0.0,
            false,
            false,
            true,
        ),
    };

    let updated_session = {
        let Ok(mut map) = popup_sessions().lock() else {
            return Err("Popup session store lock failed".to_string());
        };
        let session = map
            .get_mut(&params.popup_id)
            .ok_or_else(|| "Popup session not found".to_string())?;
        session.mode = "mini_progress".to_string();
        session.url = url.clone();
        session.file_name = params.file_name.clone();
        session.destination_path = final_dest_str.clone();
        session.category_id = params.category_id.clone();
        session.num_connections = effective_conns;
        session.format_id = params.format_id.clone();
        session.resolution = params.resolution.clone();
        session.cookie = params.cookie.clone();
        session.referer = params.referer.clone();
        session.user_agent = params.user_agent.clone();
        session.proxy_url = eff_proxy.clone();
        session.auth_username = params.auth_username.clone();
        session.auth_password = params.auth_password.clone();
        session.speed_limit_bytes_per_sec = (eff_speed_bps > 0).then_some(eff_speed_bps);
        session.file_size = initial_total;
        session.mime_type = params.mime_type.clone().or_else(|| session.mime_type.clone());
        session.task_id = Some(task_id.clone());
        if !should_spawn_download {
            session.prebuffer.status = "completed".to_string();
            session.prebuffer.downloaded_bytes = initial_downloaded;
            session.prebuffer.total_bytes = initial_total;
            session.prebuffer.progress_percent = 100.0;
            session.prebuffer.speed_bytes_per_sec = 0;
        }
        session.clone()
    };

    update_popup_window_for_mini_progress(&app, &params.popup_id, &params.file_name).await;

    let _ = app.emit(
        "popup-download-started",
        PopupTaskEventPayload {
            id: task_id.clone(),
            url: url.clone(),
            file_name: params.file_name.clone(),
            destination_path: final_dest_str.clone(),
            category_id: params.category_id.clone(),
            file_size: initial_total,
            downloaded_bytes: initial_downloaded,
            progress_percent: initial_progress,
            status: initial_status.clone(),
            supports_range: initial_range,
            mime_type: updated_session.mime_type.clone(),
            num_connections: effective_conns,
            speed_limit_bytes_per_sec: eff_speed_bps,
            format_id: params.format_id.clone(),
            resolution: params.resolution.clone(),
            cookie: params.cookie.clone(),
            referer: params.referer.clone(),
            user_agent: params.user_agent.clone(),
            proxy_url: eff_proxy.clone(),
            auth_username: params.auth_username.clone(),
            auth_password: params.auth_password.clone(),
            source: updated_session.source.clone(),
        },
    );

    if should_spawn_download {
        start_real_download_with_controller(
            app,
            task_id,
            url,
            final_dest_str,
            effective_conns,
            params.format_id,
            params.cookie,
            params.referer,
            params.user_agent,
            eff_speed_kbs,
            Some(start_resume),
            eff_proxy,
            params.auth_username,
            params.auth_password,
            Some(true),
            None,
        )
        .await?;
    } else {
        emit_simple_telemetry(
            &app,
            &task_id,
            initial_downloaded,
            initial_total.unwrap_or(initial_downloaded),
            0,
            100.0,
            Some(0),
            "completed",
            &target,
            None,
        );
    }

    Ok(updated_session)
}

#[tauri::command]
async fn queue_popup_later(
    app: tauri::AppHandle,
    params: PopupPromoteParams,
) -> Result<(), String> {
    let url = params.url.trim().to_string();
    if url.is_empty() {
        return Err("Download URL cannot be empty".to_string());
    }

    let raw_target = resolve_path(&params.destination_path);
    let target = resolve_unique_destination(&raw_target);
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let final_dest_str = target.display().to_string();

    let take_outcome = stop_and_take_prebuffer(&params.popup_id, &url).await;
    let task_id = next_generated_task_id();
    let rt = get_runtime_settings();
    let effective_conns = params.num_connections.clamp(1, 32);
    let eff_speed_bps = params
        .speed_limit_kbs
        .filter(|&k| k > 0)
        .unwrap_or(rt.speed_limit_kbs)
        * 1024;
    let eff_proxy = params
        .proxy_url
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| rt.proxy_url.clone());

    let (status, downloaded_bytes, total_bytes, progress_percent, supports_range) =
        match take_outcome {
            PrebufferTakeOutcome::Completed {
                part_path,
                total_bytes,
            } => {
                if let Err(e) = move_file_cross_device(&part_path, &target) {
                    cleanup_prebuffer_files_at(&part_path);
                    return Err(e);
                }
                cleanup_prebuffer_files_at(&part_path);
                let actual_len = path_content_size(&target).max(total_bytes);
                ("completed".to_string(), actual_len, Some(actual_len), 100.0, true)
            }
            PrebufferTakeOutcome::Resumable {
                part_path,
                meta_path,
                downloaded_bytes,
                total_bytes,
            } => {
                let target_meta = core_engine::downloader::checkpoint_path_for(&target);
                if move_file_cross_device(&part_path, &target).is_ok()
                    && move_file_cross_device(&meta_path, &target_meta).is_ok()
                {
                    let total_est = total_bytes.or(params.file_size);
                    let pct = match total_est {
                        Some(t) if t > 0 => ((downloaded_bytes as f64 / t as f64) * 100.0).min(99.9),
                        _ => 0.0,
                    };
                    ("paused".to_string(), downloaded_bytes, total_est, pct, true)
                } else {
                    cleanup_download_artifacts(&target);
                    cleanup_prebuffer_files_at(&part_path);
                    ("paused".to_string(), 0, params.file_size, 0.0, false)
                }
            }
            PrebufferTakeOutcome::None => ("paused".to_string(), 0, params.file_size, 0.0, false),
        };

    let source = popup_sessions()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(&params.popup_id))
        .map(|s| s.source)
        .unwrap_or_else(|| "manual".to_string());

    let _ = app.emit(
        "popup-download-queued",
        PopupTaskEventPayload {
            id: task_id,
            url,
            file_name: params.file_name,
            destination_path: final_dest_str,
            category_id: params.category_id,
            file_size: total_bytes,
            downloaded_bytes,
            progress_percent,
            status,
            supports_range,
            mime_type: params.mime_type,
            num_connections: effective_conns,
            speed_limit_bytes_per_sec: eff_speed_bps,
            format_id: params.format_id,
            resolution: params.resolution,
            cookie: params.cookie,
            referer: params.referer,
            user_agent: params.user_agent,
            proxy_url: eff_proxy,
            auth_username: params.auth_username,
            auth_password: params.auth_password,
            source,
        },
    );

    close_popup_window_by_label(&app, &params.popup_id).await;
    Ok(())
}

#[tauri::command]
async fn cancel_popup_download(app: tauri::AppHandle, popup_id: String) -> Result<bool, String> {
    let session_opt = popup_sessions()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(&popup_id));

    if let Ok(mut pre_map) = popup_prebuffers().lock() {
        if let Some(rt) = pre_map.remove(&popup_id) {
            rt.controller.cancel();
            cleanup_prebuffer_files_at(&rt.part_path);
        }
    }
    cleanup_prebuffer_by_id_prefix_at(&prebuffer_root_dir(), &popup_id);

    if let Some(session) = session_opt {
        if let Some(task_id) = session.task_id {
            let _ = cancel_real_download(task_id.clone(), Some(session.destination_path.clone())).await;
            let target = resolve_path(&session.destination_path);
            emit_simple_telemetry(&app, &task_id, 0, 0, 0, 0.0, None, "canceled", &target, None);
        }
    }

    close_popup_window_by_label(&app, &popup_id).await;
    Ok(true)
}

#[tauri::command]
fn open_main_window(app: tauri::AppHandle) -> Result<(), String> {
    show_main_window(&app);
    Ok(())
}

#[tauri::command]
async fn minimize_popup_window(app: tauri::AppHandle, popup_id: String) -> Result<(), String> {
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || {
        if let Some(win) = app_clone.get_webview_window(&popup_id) {
            let _ = win.minimize();
        }
    })
    .await
    .map_err(|e| format!("Failed minimizing popup window: {}", e))
}

#[tauri::command]
async fn close_popup_window(app: tauri::AppHandle, popup_id: String) -> Result<(), String> {
    close_popup_window_by_label(&app, &popup_id).await;
    Ok(())
}

fn parse_bridge_add_links_payload(body: &str) -> Option<serde_json::Value> {
    let v = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let raw_links = v["links"].as_array()?;
    let mut cleaned: Vec<serde_json::Value> = Vec::new();
    let is_supported_link = |s: &str| {
        let lower = s.to_ascii_lowercase();
        lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("ftp://")
            || lower.starts_with("magnet:")
    };
    for item in raw_links {
        if let Some(u) = item.as_str() {
            let trimmed = u.trim();
            if is_supported_link(trimmed) {
                cleaned.push(serde_json::json!({
                    "url": trimmed,
                    "label": "",
                }));
            }
        } else if let Some(u) = item["url"].as_str() {
            let trimmed = u.trim();
            if is_supported_link(trimmed) {
                cleaned.push(serde_json::json!({
                    "url": trimmed,
                    "label": item["label"].as_str().unwrap_or(""),
                }));
            }
        }
    }
    if cleaned.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "links": cleaned,
        "pageUrl": v["pageUrl"].as_str().unwrap_or(""),
        "cookie": v["cookie"].as_str(),
        "userAgent": v["userAgent"].as_str(),
    }))
}

async fn start_browser_bridge_server(app: tauri::AppHandle) {
    let listener = loop {
        match tokio::net::TcpListener::bind("127.0.0.1:18291").await {
            Ok(l) => {
                log::info!("Browser bridge server listening on 127.0.0.1:18291");
                break l;
            }
            Err(e) => {
                log::warn!("Browser bridge server port 18291 busy ({}), retrying in 1.5s...", e);
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            }
        }
    };

    while let Ok((mut stream, _)) = listener.accept().await {
        let app_handle = app.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            if let Some(req) = read_full_http_request(&mut stream).await {
                if !req.is_empty() {
                    // 1. CORS Preflight
                    if req.starts_with("OPTIONS") {
                        let response = "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: 0\r\n\r\n";
                        let _ = stream.write_all(response.as_bytes()).await;
                        return;
                    }

                    // 1b. ROUTE: GET /status (Health check for browser extension)
                    if req.starts_with("GET /status") {
                        let current_lang = active_ui_language()
                            .lock()
                            .map(|g| g.clone())
                            .unwrap_or_else(|_| "en".to_string());
                        let resp_body = serde_json::json!({
                            "status": "ok",
                            "app": "Download Cepat",
                            "version": env!("CARGO_PKG_VERSION"),
                            "author": "Ujang Mustamar",
                            "language": current_lang,
                            "port": 18291
                        })
                        .to_string();
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\r\n{}",
                            resp_body.len(),
                            resp_body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                        return;
                    }

                    // 1c. ROUTE: POST /add_links (Download All Links from Browser Page)
                    if req.starts_with("POST /add_links") {
                        if let Some(body_idx) = req.find("\r\n\r\n") {
                            let body = &req[body_idx + 4..];
                            if let Some(payload) = parse_bridge_add_links_payload(body) {
                                let count = payload["links"].as_array().map(|a| a.len()).unwrap_or(0);
                                let _ = app_handle.emit("browser-add-links", payload);
                                show_main_window(&app_handle);
                                let resp_body = serde_json::json!({
                                    "status": "ok",
                                    "count": count,
                                    "message": "Links sent to Download Cepat"
                                })
                                .to_string();
                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\r\n{}",
                                    resp_body.len(),
                                    resp_body
                                );
                                let _ = stream.write_all(resp.as_bytes()).await;
                                return;
                            }
                        }
                    }

                    // 2. ROUTE: GET /inspect?url=... or POST /inspect
                    if req.starts_with("GET /inspect?url=") || req.starts_with("POST /inspect") {
                        let mut target_url = String::new();
                        let mut inspect_cookie: Option<String> = None;
                        let mut inspect_referer: Option<String> = None;
                        let mut inspect_ua: Option<String> = None;
                        if req.starts_with("GET /inspect?url=") {
                            let prefix = "GET /inspect?url=";
                            let rest = &req[prefix.len()..];
                            let end = rest.find(' ').unwrap_or(rest.len());
                            target_url = url_decode(&rest[..end]);
                        } else if let Some(body_idx) = req.find("\r\n\r\n") {
                            let body = &req[body_idx + 4..];
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
                                if let Some(u) = v["url"].as_str() {
                                    target_url = u.to_string();
                                }
                                inspect_cookie = v["cookie"].as_str().filter(|s| !s.trim().is_empty()).map(String::from);
                                inspect_referer = v["referer"].as_str().or_else(|| v["referrer"].as_str()).filter(|s| !s.trim().is_empty()).map(String::from);
                                inspect_ua = v["userAgent"].as_str().filter(|s| !s.trim().is_empty()).map(String::from);
                            }
                        }

                        let mut inspect_err_msg = "Video stream cannot be inspected.".to_string();
                        if !target_url.is_empty() {
                            let rt = get_runtime_settings();
                            match inspect_video_url_with_options(
                                &target_url,
                                inspect_cookie.as_deref(),
                                inspect_referer.as_deref(),
                                inspect_ua.as_deref(),
                                rt.proxy_url.as_deref(),
                                None,
                                None,
                            )
                            .await
                            {
                                Ok(res) => {
                                    let formats_json: Vec<serde_json::Value> = res.media_info.as_ref().map_or(vec![], |m| {
                                        m.formats.iter().map(|f| {
                                            serde_json::json!({
                                                "formatId": f.format_id,
                                                "resolution": f.resolution,
                                                "extension": f.extension,
                                                "fileSizeApprox": f.file_size_approx,
                                                "sizeLabel": f.file_size_approx.map(format_bytes_label).unwrap_or_else(|| "Dynamic size".to_string()),
                                                "isAudioOnly": f.is_audio_only
                                            })
                                        }).collect()
                                    });

                                    let title = res.file_name.trim_end_matches(".mp4").to_string();
                                    let resp_body = serde_json::json!({
                                        "status": "ok",
                                        "title": title,
                                        "durationSeconds": res.media_info.as_ref().and_then(|m| m.duration_seconds),
                                        "thumbnailUrl": res.media_info.as_ref().and_then(|m| m.thumbnail_url.clone()),
                                        "formats": formats_json
                                    }).to_string();

                                    let resp = format!(
                                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\r\n{}",
                                        resp_body.len(),
                                        resp_body
                                    );
                                    let _ = stream.write_all(resp.as_bytes()).await;
                                    return;
                                }
                                Err(e) => {
                                    inspect_err_msg = e;
                                }
                            }
                        }

                        let resp_body = serde_json::json!({
                            "status": "error",
                            "message": inspect_err_msg
                        }).to_string();
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\r\n{}",
                            resp_body.len(),
                            resp_body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                        return;
                    }

                    // 3. ROUTE: POST /start_download (Silent background start from Browser Dropdown + Mini Progress Popup)
                    if req.starts_with("POST /start_download") {
                        if let Some(body_idx) = req.find("\r\n\r\n") {
                            let body = &req[body_idx + 4..];
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
                                let url = v["url"].as_str().unwrap_or("").to_string();
                                let raw_title = v["title"].as_str().unwrap_or("Video").to_string();
                                let format_id = v["formatId"].as_str().unwrap_or("1080p_mp4").to_string();
                                let resolution = v["resolution"].as_str().unwrap_or("1080p (Full HD)").to_string();
                                let ext = v["extension"].as_str().unwrap_or("mp4").to_string();
                                let clean_ext: String = ext
                                    .chars()
                                    .filter(|c| c.is_ascii_alphanumeric())
                                    .collect::<String>()
                                    .to_ascii_lowercase();
                                let clean_ext = if clean_ext.is_empty() {
                                    "mp4".to_string()
                                } else {
                                    clean_ext
                                };
                                let is_audio = v["isAudioOnly"].as_bool().unwrap_or(false)
                                    || clean_ext == "mp3"
                                    || clean_ext == "m4a";
                                let rt = get_runtime_settings();
                                let conn_count = v["connections"]
                                    .as_u64()
                                    .or_else(|| v["numConnections"].as_u64())
                                    .map(|n| n as usize)
                                    .unwrap_or(rt.default_connections)
                                    .clamp(1, 32);
                                let cookie_opt = v["cookie"].as_str().filter(|s| !s.trim().is_empty()).map(String::from);
                                let referer_opt = v["referer"].as_str().or_else(|| v["referrer"].as_str()).filter(|s| !s.trim().is_empty()).map(String::from);
                                let ua_opt = v["userAgent"].as_str().filter(|s| !s.trim().is_empty()).map(String::from);

                                let clean_title = sanitize_media_title(&raw_title, "Video");
                                let task_id = next_generated_task_id();
                                let file_name = format!("{}.{}", clean_title, clean_ext);
                                let cat_id = if is_audio { "music" } else { "video" };
                                let dest_dir = if is_audio {
                                    &rt.music_save_dir
                                } else {
                                    &rt.video_save_dir
                                };
                                let dest_path = join_save_dir_and_file(dest_dir, &file_name);

                                let pre_ctrl = core_engine::DownloadController::new();
                                if rt.speed_limit_kbs > 0 {
                                    pre_ctrl.set_speed_limit(rt.speed_limit_kbs * 1024);
                                }
                                let is_queued = if let Ok(mut map) = active_downloads().lock() {
                                    if map.len() >= rt.max_tasks {
                                        true
                                    } else {
                                        map.insert(task_id.clone(), ActiveControl::Http(pre_ctrl.clone()));
                                        false
                                    }
                                } else {
                                    false
                                };
                                let speed_limit_bps = rt.speed_limit_kbs * 1024;

                                // Emit to main window so the task is listed cleanly in background
                                let _ = app_handle.emit("external-task-added", serde_json::json!({
                                    "id": task_id.clone(),
                                    "url": url.clone(),
                                    "fileName": file_name.clone(),
                                    "destinationPath": dest_path.clone(),
                                    "categoryId": cat_id,
                                    "formatId": format_id.clone(),
                                    "resolution": resolution.clone(),
                                    "numConnections": conn_count,
                                    "status": if is_queued { "queued" } else { "downloading" },
                                    "cookie": cookie_opt.clone(),
                                    "referer": referer_opt.clone(),
                                    "userAgent": ua_opt.clone(),
                                    "proxyUrl": rt.proxy_url.clone(),
                                    "speedLimitBytesPerSec": speed_limit_bps,
                                }));

                                // Spawn separate minimizable IDM-style Mini Progress popup without waking main window
                                let app_for_popup = app_handle.clone();
                                let tid_for_popup = task_id.clone();
                                let u_for_popup = url.clone();
                                let fn_for_popup = file_name.clone();
                                let dest_for_popup = dest_path.clone();
                                let cat_for_popup = cat_id.to_string();
                                let fmt_for_popup = Some(format_id.clone());
                                let res_for_popup = Some(resolution.clone());
                                let ck_for_popup = cookie_opt.clone();
                                let rf_for_popup = referer_opt.clone();
                                let ua_for_popup = ua_opt.clone();
                                let px_for_popup = rt.proxy_url.clone();
                                let sp_for_popup = (speed_limit_bps > 0).then_some(speed_limit_bps);
                                tauri::async_runtime::spawn(async move {
                                    if let Err(err) = spawn_mini_progress_popup(
                                        &app_for_popup,
                                        tid_for_popup,
                                        "browser".to_string(),
                                        u_for_popup,
                                        fn_for_popup,
                                        dest_for_popup,
                                        cat_for_popup,
                                        conn_count,
                                        fmt_for_popup,
                                        res_for_popup,
                                        ck_for_popup,
                                        rf_for_popup,
                                        ua_for_popup,
                                        px_for_popup,
                                        sp_for_popup,
                                        is_queued,
                                    )
                                    .await
                                    {
                                        log::warn!("Failed spawning mini progress popup: {}", err);
                                    }
                                });

                                // Spawn real background download immediately when an active slot was reserved
                                if !is_queued {
                                    let app_for_dl = app_handle.clone();
                                    let tid_for_dl = task_id.clone();
                                    let u_for_dl = url.clone();
                                    let dest_for_dl = dest_path.clone();
                                    let fmt_for_dl = Some(format_id.clone());
                                    let speed_kbs_opt = (rt.speed_limit_kbs > 0).then_some(rt.speed_limit_kbs);
                                    let proxy_for_dl = rt.proxy_url.clone();
                                    let ctrl_for_dl = pre_ctrl;
                                    tauri::async_runtime::spawn(async move {
                                        let _ = start_real_download_with_controller(
                                            app_for_dl,
                                            tid_for_dl,
                                            u_for_dl,
                                            dest_for_dl,
                                            conn_count,
                                            fmt_for_dl,
                                            cookie_opt,
                                            referer_opt,
                                            ua_opt,
                                            speed_kbs_opt,
                                            None,
                                            proxy_for_dl,
                                            None,
                                            None,
                                            Some(true),
                                            Some(ctrl_for_dl),
                                        )
                                        .await;
                                    });
                                }

                                let resp_body = serde_json::json!({
                                    "status": "ok",
                                    "message": "Download started in background",
                                    "taskId": task_id,
                                    "fileName": file_name,
                                    "destinationPath": dest_path,
                                    "numConnections": conn_count,
                                    "queued": is_queued
                                }).to_string();

                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\r\n{}",
                                    resp_body.len(),
                                    resp_body
                                );
                                let _ = stream.write_all(resp.as_bytes()).await;
                                return;
                            }
                        }
                    }

                    // 4. ROUTE: POST /add_download or GET /add_download?url=... (IDM-style Confirm Popup + Silent Pre-Download)
                    let mut found_url = String::new();
                    let mut found_filename = String::new();
                    let mut found_cookie: Option<String> = None;
                    let mut found_referer: Option<String> = None;
                    let mut found_user_agent: Option<String> = None;
                    let mut found_file_size: Option<u64> = None;
                    let mut found_mime_type: Option<String> = None;

                    if let Some(body_idx) = req.find("\r\n\r\n") {
                        let body = &req[body_idx + 4..];
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
                            if let Some(u) = v["url"].as_str() {
                                found_url = u.to_string();
                            }
                            if let Some(f) = v["fileName"].as_str() {
                                found_filename = f.to_string();
                            }
                            found_cookie = v["cookie"].as_str().filter(|s| !s.trim().is_empty()).map(String::from);
                            found_referer = v["referer"]
                                .as_str()
                                .or_else(|| v["referrer"].as_str())
                                .filter(|s| !s.trim().is_empty())
                                .map(String::from);
                            found_user_agent = v["userAgent"].as_str().filter(|s| !s.trim().is_empty()).map(String::from);
                            found_file_size = v["fileSize"].as_u64();
                            found_mime_type = v["mimeType"].as_str().filter(|s| !s.trim().is_empty()).map(String::from);
                        }
                    }

                    if found_url.is_empty() && req.starts_with("GET /add_download?url=") {
                        if let Some(query_start) = req.find("url=") {
                            let end = req[query_start + 4..].find(' ').unwrap_or(req.len() - (query_start + 4));
                            found_url = url_decode(&req[query_start + 4..query_start + 4 + end]);
                        }
                    }

                    if !found_url.is_empty() {
                        let popup_params = OpenDownloadPopupParams {
                            source: Some("browser".to_string()),
                            initial_mode: Some("url".to_string()),
                            url: Some(found_url.clone()),
                            file_name: (!found_filename.trim().is_empty()).then(|| found_filename.clone()),
                            cookie: found_cookie.clone(),
                            referer: found_referer.clone(),
                            user_agent: found_user_agent.clone(),
                            file_size: found_file_size,
                            mime_type: found_mime_type.clone(),
                        };
                        let app_for_popup = app_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(err) = spawn_download_popup(&app_for_popup, popup_params).await {
                                log::warn!("Failed spawning browser confirm popup ({}); falling back to main modal", err);
                                let _ = app_for_popup.emit("browser-add-download", serde_json::json!({
                                    "url": found_url,
                                    "fileName": found_filename,
                                    "cookie": found_cookie,
                                    "referer": found_referer,
                                    "userAgent": found_user_agent,
                                    "fileSize": found_file_size,
                                    "mimeType": found_mime_type
                                }));
                                show_main_window(&app_for_popup);
                            }
                        });

                        let body = "{\"status\":\"ok\",\"message\":\"Download received by Download Cepat\"}";
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                        return;
                    }

                    let resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: 15\r\n\r\n{\"status\":\"ok\"}";
                    let _ = stream.write_all(resp.as_bytes()).await;
                }
            }
        });
    }
}

fn show_main_window(app_handle: &tauri::AppHandle) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        #[cfg(target_os = "macos")]
        {
            let _ = handle.set_activation_policy(tauri::ActivationPolicy::Regular);
            let _ = handle.show();
        }
        if let Some(win) = handle.get_webview_window("main") {
            let _ = win.unminimize();
            let _ = win.show();
            let _ = win.set_focus();
        }
    });
}

fn hide_main_window_to_tray(window: &tauri::Window) {
    let _ = window.hide();
    #[cfg(target_os = "macos")]
    let _ = window
        .app_handle()
        .set_activation_policy(tauri::ActivationPolicy::Accessory);
}

fn should_hide_on_close(window_label: &str) -> bool {
    window_label == "main"
}

fn load_tray_icon_image() -> Option<tauri::image::Image<'static>> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(img) = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png")) {
            return Some(img);
        }
    }
    tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png")).ok()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            open_folder,
            open_file,
            check_file_exists,
            start_real_download,
            pause_real_download,
            cancel_real_download,
            stop_all_real_downloads,
            ensure_file_created,
            inspect_url,
            inspect_torrent_bytes,
            update_download_speed_limit,
            verify_file_checksum,
            check_disk_space,
            grab_site_links,
            rename_or_move_download,
            read_clipboard_text,
            read_text_file_preview,
            run_post_download_action,
            get_autostart_status,
            set_autostart,
            install_browser_extension_host,
            open_extension_folder,
            open_external_url,
            get_build_info,
            set_tray_language,
            sync_runtime_settings,
            check_for_app_update,
            download_and_install_app_update,
            open_download_popup,
            get_popup_session,
            start_popup_pre_download,
            promote_popup_download,
            queue_popup_later,
            cancel_popup_download,
            open_main_window,
            minimize_popup_window,
            close_popup_window
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let _ = cleanup_stale_prebuffer_dir();

            let initial_labels = tray_labels_for_lang("en");
            if let Ok((tray_menu, tray_state)) = (|| -> Result<(tauri::menu::Menu<tauri::Wry>, TrayMenuState), tauri::Error> {
                let show_item = tauri::menu::MenuItemBuilder::with_id("tray_show", initial_labels.show).build(app)?;
                let add_item = tauri::menu::MenuItemBuilder::with_id("tray_add", initial_labels.add).build(app)?;
                let pause_item = tauri::menu::MenuItemBuilder::with_id("tray_pause_all", initial_labels.pause_all).build(app)?;
                let sep = tauri::menu::PredefinedMenuItem::separator(app)?;
                let quit_item = tauri::menu::MenuItemBuilder::with_id("tray_quit", initial_labels.quit).build(app)?;
                let menu = tauri::menu::MenuBuilder::new(app)
                    .items(&[&show_item, &add_item, &pause_item, &sep, &quit_item])
                    .build()?;
                Ok((
                    menu,
                    TrayMenuState {
                        show_item,
                        add_item,
                        pause_item,
                        quit_item,
                    },
                ))
            })() {
                app.manage(tray_state);
                let mut builder = tauri::tray::TrayIconBuilder::with_id("dc-tray")
                    .tooltip(initial_labels.tooltip)
                    .menu(&tray_menu)
                    .show_menu_on_left_click(false)
                    .icon_as_template(cfg!(target_os = "macos"))
                    .on_menu_event(|app_handle, event| match event.id().as_ref() {
                        "tray_show" => {
                            show_main_window(app_handle);
                        }
                        "tray_add" => {
                            let handle_for_popup = app_handle.clone();
                            tauri::async_runtime::spawn(async move {
                                let params = OpenDownloadPopupParams {
                                    source: Some("tray".to_string()),
                                    initial_mode: Some("url".to_string()),
                                    url: None,
                                    file_name: None,
                                    cookie: None,
                                    referer: None,
                                    user_agent: None,
                                    file_size: None,
                                    mime_type: None,
                                };
                                if spawn_download_popup(&handle_for_popup, params).await.is_err() {
                                    show_main_window(&handle_for_popup);
                                    let _ = handle_for_popup.emit("tray-add-url", ());
                                }
                            });
                        }
                        "tray_pause_all" => {
                            let controls: Vec<ActiveControl> = active_downloads()
                                .lock()
                                .map(|m| m.values().cloned().collect())
                                .unwrap_or_default();
                            for ctrl in controls {
                                ctrl.pause();
                            }
                            let _ = app_handle.emit("tray-pause-all", ());
                        }
                        "tray_quit" => {
                            app_handle.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| match event {
                        tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        }
                        | tauri::tray::TrayIconEvent::DoubleClick {
                            button: tauri::tray::MouseButton::Left,
                            ..
                        } => {
                            show_main_window(tray.app_handle());
                        }
                        _ => {}
                    });

                if let Some(icon) = load_tray_icon_image().or_else(|| app.default_window_icon().cloned()) {
                    builder = builder.icon(icon);
                }

                app.manage(tray_menu);
                if let Err(err) = builder.build(app) {
                    log::warn!("Gagal membuat ikon taskbar Download Cepat: {}", err);
                }
            }

            let handle = app.handle().clone();
            std::thread::spawn(|| {
                let _ = sync_native_messaging_hosts();
            });
            tauri::async_runtime::spawn(async {
                let ytdlp_fut = async {
                    if core_engine::video_runtime::ytdlp_onedir_spec(
                        std::env::consts::OS,
                        std::env::consts::ARCH,
                    )
                    .is_some()
                        && get_ytdlp_onedir_bin().is_none()
                    {
                        if let Err(e) = upgrade_ytdlp_to_onedir_if_needed().await {
                            log::warn!("Upgrade yt-dlp onedir awal belum berhasil: {}", e);
                            if get_ytdlp_bin().is_none() {
                                if let Err(e2) = ensure_ytdlp_bin().await {
                                    log::warn!("Auto-provisioning awal yt-dlp belum berhasil: {}", e2);
                                }
                            }
                        }
                    } else if get_ytdlp_bin().is_none() {
                        if let Err(e) = ensure_ytdlp_bin().await {
                            log::warn!("Auto-provisioning awal yt-dlp belum berhasil: {}", e);
                        }
                    }
                };
                let js_fut = async {
                    if let Err(e) = ensure_js_runtime_bin().await {
                        log::warn!("Auto-provisioning awal runtime JS belum berhasil: {}", e);
                    }
                };
                let ffmpeg_fut = async {
                    if get_ffmpeg_bin().is_none() {
                        if let Err(e) = ensure_ffmpeg_bin().await {
                            log::warn!("Auto-provisioning awal FFmpeg belum berhasil: {}", e);
                        }
                    }
                };
                tokio::join!(ytdlp_fut, js_fut, ffmpeg_fut);
            });
            tauri::async_runtime::spawn(async move {
                start_browser_bridge_server(handle).await;
            });

            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                if should_hide_on_close(window.label()) {
                    api.prevent_close();
                    hide_main_window_to_tray(window);
                }
            }
            tauri::WindowEvent::Destroyed => {
                let label = window.label().to_string();
                if label.starts_with("dl-popup-") {
                    tauri::async_runtime::spawn(async move {
                        cleanup_popup_on_window_destroyed(&label).await;
                    });
                }
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { has_visible_windows, .. } = event {
                if !has_visible_windows {
                    show_main_window(app_handle);
                }
            }
            #[cfg(not(target_os = "macos"))]
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            let _ = &app_handle;
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_inspect_video_url_live() {
        let url = "https://www.youtube.com/watch?v=Jdj13fGMmmA";
        let res = inspect_video_url(url).await;
        assert!(res.is_ok(), "inspect_video_url should succeed for YouTube video: {:?}", res.err());
        let info = res.unwrap();
        assert!(!info.file_name.is_empty());
        assert!(info.media_info.is_some());
        let media = info.media_info.unwrap();
        assert!(!media.formats.is_empty());

        for fmt in &media.formats {
            println!("Verified genuine format: {} -> {} (approx: {:?} bytes)", fmt.format_id, fmt.resolution, fmt.file_size_approx);
            assert!(!fmt.resolution.is_empty());
            assert!(!fmt.format_id.is_empty());
        }

        let fmt_1080 = media.formats.iter().find(|f| f.format_id == "1080p_mp4");
        if let Some(f) = fmt_1080 {
            let size = f.file_size_approx.unwrap_or(0);
            assert!(size > 20_000_000 && size < 60_000_000, "1080p size should be around ~48MB, got {}", size);
        }

        let res_hdr = inspect_video_url_with_headers(
            "https://www.youtube.com/watch?v=WPul2u-pIog",
            Some("VISITOR_INFO1_LIVE=test; PREF=tz=Asia.Jakarta"),
            Some("https://www.youtube.com/watch?v=WPul2u-pIog"),
            Some("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"),
        )
        .await;
        assert!(res_hdr.is_ok(), "YouTube inspect with Chrome headers should succeed: {:?}", res_hdr.err());
        let formats_hdr = res_hdr.unwrap().media_info.unwrap().formats;
        assert!(
            formats_hdr.iter().any(|f| !f.is_audio_only),
            "YouTube inspect must return playable video resolutions, not audio only"
        );
    }

    #[test]
    fn test_youtube_and_runtime_helpers() {
        assert!(is_youtube_url("https://www.youtube.com/watch?v=WPul2u-pIog"));
        assert!(is_youtube_url("https://youtu.be/WPul2u-pIog"));
        assert!(!is_youtube_url("https://vimeo.com/123456"));

        let audio_only = serde_json::json!({
            "formats": [
                { "format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2" },
                { "format_id": "sb0", "ext": "mhtml", "vcodec": "none", "acodec": "none", "format_note": "storyboard" }
            ]
        });
        assert!(!json_has_playable_video_format(&audio_only));

        let with_video = serde_json::json!({
            "formats": [
                { "format_id": "137", "ext": "mp4", "vcodec": "avc1", "height": 1080 }
            ]
        });
        assert!(json_has_playable_video_format(&with_video));

        assert!(core_engine::video_runtime::ytdlp_onedir_spec("windows", "x86_64").is_some());
        assert!(core_engine::video_runtime::ytdlp_onedir_spec("linux", "x86_64").is_some());
        assert!(core_engine::video_runtime::ytdlp_onedir_spec("macos", "aarch64").is_none());
        assert!(core_engine::video_runtime::quickjs_download_url_for("windows", "x86_64").is_some());
        assert!(core_engine::video_runtime::quickjs_download_url_for("linux", "x86_64").is_some());
        assert!(!core_engine::video_runtime::youtube_fallback_extractor_args().contains("=all"));
        assert_eq!(
            core_engine::video_runtime::canonicalize_video_url("https://youtu.be/WPul2u-pIog?si=abc"),
            "youtube:v:WPul2u-pIog"
        );
    }

    #[test]
    fn test_cleanup_download_artifacts_removes_temp_files_only() {
        let test_dir = std::env::temp_dir().join(format!("dc_cleanup_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).expect("create temp dir");

        let archive_target = test_dir.join("W11Pro.7z");
        let audio_target = test_dir.join("Song.mp3");

        let junk_files = [
            "W11Pro.7z",
            "W11Pro.7z.dcmeta",
            "W11Pro.7z.part",
            "W11Pro.7z.part-Frag1",
            "W11Pro.7z.ytdl",
            "Song.mp3",
            "Song.mp3.part",
            "Song.temp_stream.mp4",
            "Song.temp_stream.mp4.part",
            "Song.temp_stream.mp4.ytdl",
            "Song.temp_stream.m4a",
            "Song.temp_stream.m4a.part",
            "Song.f140.m4a",
            "Song.f251.webm.part",
            "Song.mp3.vtmp.mp4",
            "Song.mp3.vtmp.mp4.dcmeta",
            "Song.mp3.atmp.m4a",
            "Song.mp3.atmp.m4a.dcmeta",
            "Song.mp3.mux.mp4",
        ];
        for name in &junk_files {
            std::fs::write(test_dir.join(name), b"partial").expect("write junk file");
        }

        let keep_files = [
            "W11Pro_FINAL.7z",
            "W11Pro.txt",
            "Song.mp4",
            "Song (1).mp3",
        ];
        for name in &keep_files {
            std::fs::write(test_dir.join(name), b"completed").expect("write keep file");
        }

        let removed_archive = cleanup_download_artifacts(&archive_target);
        let removed_audio = cleanup_download_artifacts(&audio_target);

        assert_eq!(removed_archive + removed_audio, junk_files.len());
        for name in &junk_files {
            assert!(!test_dir.join(name).exists(), "Expected {} to be deleted", name);
        }
        for name in &keep_files {
            assert!(test_dir.join(name).exists(), "Expected unrelated {} to be preserved", name);
        }

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_rename_or_move_download_and_dcmeta() {
        let test_dir = std::env::temp_dir().join(format!("dc_rename_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).expect("create temp dir");

        let old_file = test_dir.join("old_archive.zip");
        let new_file = test_dir.join("sub").join("renamed_archive.zip");
        let old_meta = core_engine::downloader::checkpoint_path_for(&old_file);
        let new_meta = core_engine::downloader::checkpoint_path_for(&new_file);

        std::fs::write(&old_file, b"hello-dc-archive").expect("write old file");
        std::fs::write(&old_meta, b"{\"total_bytes\":16}").expect("write old meta");

        let moved = rename_or_move_download_paths(&old_file, &new_file).expect("rename succeeds");
        assert_eq!(moved, new_file.display().to_string());
        assert!(!old_file.exists());
        assert!(!old_meta.exists());
        assert_eq!(std::fs::read(&new_file).unwrap(), b"hello-dc-archive");
        assert!(new_meta.exists());

        // Also verify multi-file torrent directory + .dctmeta move
        let old_torrent_dir = test_dir.join("Ubuntu-ISO");
        let new_torrent_dir = test_dir.join("moved").join("Ubuntu-ISO-Renamed");
        let old_tmeta = core_engine::torrent::torrent_checkpoint_path(&old_torrent_dir);
        let new_tmeta = core_engine::torrent::torrent_checkpoint_path(&new_torrent_dir);
        std::fs::create_dir_all(old_torrent_dir.join("nested")).expect("create torrent dir");
        std::fs::write(old_torrent_dir.join("nested").join("ubuntu.iso"), b"iso-data")
            .expect("write torrent payload");
        std::fs::write(&old_tmeta, b"{\"downloaded_pieces\":[]}").expect("write torrent meta");

        let moved_t =
            rename_or_move_download_paths(&old_torrent_dir, &new_torrent_dir).expect("move dir");
        assert_eq!(moved_t, new_torrent_dir.display().to_string());
        assert!(!old_torrent_dir.exists());
        assert!(!old_tmeta.exists());
        assert_eq!(
            std::fs::read(new_torrent_dir.join("nested").join("ubuntu.iso")).unwrap(),
            b"iso-data"
        );
        assert!(new_tmeta.exists());

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[tokio::test]
    async fn test_verify_file_checksum_and_disk_space_and_bridge_links() {
        let test_dir = std::env::temp_dir().join(format!("dc_checksum_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).expect("create temp dir");
        let target = test_dir.join("sample.bin");
        std::fs::write(&target, b"abc").expect("write sample");

        let sums = verify_file_checksum(target.display().to_string())
            .await
            .expect("verify checksum");
        assert_eq!(sums.size_bytes, 3);
        assert_eq!(sums.md5, "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(sums.sha1, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sums.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        let free = check_disk_space(test_dir.display().to_string()).expect("check disk space");
        assert!(free > 0);

        let parsed = parse_bridge_add_links_payload(
            r#"{"pageUrl":"https://example.com","links":["https://example.com/a.zip",{"url":"ftp://files.example.com/b.iso","label":"ISO"},"magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Sample","javascript:void(0)"]}"#,
        )
        .expect("parse links payload");
        let arr = parsed["links"].as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["url"].as_str().unwrap(), "https://example.com/a.zip");
        assert_eq!(arr[1]["url"].as_str().unwrap(), "ftp://files.example.com/b.iso");
        assert!(arr[2]["url"].as_str().unwrap().starts_with("magnet:?xt=urn:btih:"));

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_native_messaging_framing_and_manifests() {
        let payload = serde_json::json!({"action": "PING", "url": "https://example.com/video"});
        let mut buf = Vec::new();
        write_native_message(&mut buf, &payload).expect("write frame");

        let mut cursor = std::io::Cursor::new(&buf);
        let decoded = read_native_message(&mut cursor)
            .expect("read frame")
            .expect("non-eof payload");
        assert_eq!(decoded, payload);
        assert!(read_native_message(&mut cursor).expect("clean eof").is_none());

        assert!(is_native_messaging_invocation(&[
            "download-cepat-app".to_string(),
            "chrome-extension://gbeagmdkkhbopfcggmeffpbeblalfcpk/".to_string(),
        ]));
        assert!(!is_native_messaging_invocation(&[
            "/Applications/Download Cepat.app/Contents/MacOS/download-cepat-app".to_string(),
            "-psn_0_12345".to_string(),
        ]));

        let test_dir = std::env::temp_dir().join(format!("dc_prefs_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).expect("create temp dir");
        let prefs = test_dir.join("Secure Preferences");
        std::fs::write(
            &prefs,
            r#"{"extensions":{"settings":{"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa":{"manifest":{"name":"Download Cepat Test"},"path":"/tmp/Download Cepat/extension"},"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb":{"manifest":{"name":"Other Extension"},"path":"/tmp/other"}}}}"#,
        )
        .expect("write prefs");

        let mut discovered = std::collections::BTreeSet::new();
        collect_extension_ids_from_prefs_file(&prefs, &mut discovered);
        assert!(discovered.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert!(!discovered.contains("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));

        let chromium_json = build_chromium_manifest_json(
            "/Applications/Download Cepat.app/Contents/MacOS/download-cepat-app",
            &[
                OFFICIAL_CHROME_EXTENSION_ID.to_string(),
                "gbeagmdkkhbopfcggmeffpbeblalfcpk".to_string(),
            ],
        );
        assert!(!chromium_json.contains("chrome-extension://*/"));
        assert!(chromium_json.contains(&format!("chrome-extension://{}/", OFFICIAL_CHROME_EXTENSION_ID)));
        assert!(chromium_json.contains("chrome-extension://gbeagmdkkhbopfcggmeffpbeblalfcpk/"));

        let firefox_json = build_firefox_manifest_json("/Applications/Download Cepat.app/Contents/MacOS/download-cepat-app");
        assert!(firefox_json.contains("allowed_extensions"));
        assert!(!firefox_json.contains("allowed_origins"));

        let ext_sync_count = sync_external_extensions();
        assert!(ext_sync_count >= 1, "Should configure at least 1 browser external extension path");

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_cross_platform_helpers() {
        let home = user_home_dir().expect("home directory should resolve");
        assert_eq!(resolve_path("~"), home);
        assert_eq!(
            resolve_path("~/Downloads/Video/clip.mp4"),
            home.join("Downloads").join("Video").join("clip.mp4")
        );
        assert_eq!(
            resolve_path(r"~\Downloads\Video\clip.mp4"),
            home.join("Downloads").join("Video").join("clip.mp4")
        );

        assert_eq!(sanitize_media_title("CON", "Video"), "_CON");
        assert_eq!(sanitize_media_title("Video: Test? <1>...", "Video"), "Video_ Test_ _1_");
        assert_eq!(sanitize_media_title("   ", "Video"), "Video");
        assert!(sanitize_media_title(&"a".repeat(220), "Video").chars().count() <= 120);

        assert_eq!(
            path_to_file_uri(Path::new("/tmp/My Downloads/file 100%.zip")),
            "file:///tmp/My%20Downloads/file%20100%25.zip"
        );

        assert!(ytdlp_download_url_for("windows", "x86_64").ends_with("/yt-dlp.exe"));
        assert!(ytdlp_download_url_for("linux", "x86_64").ends_with("/yt-dlp_linux"));
        assert!(ytdlp_download_url_for("linux", "aarch64").ends_with("/yt-dlp_linux_aarch64"));
        assert!(ytdlp_download_url_for("macos", "aarch64").ends_with("/yt-dlp_macos"));

        let desktop = build_linux_autostart_desktop_entry("/opt/Download Cepat/App 100%$x.AppImage");
        assert!(desktop.contains("[Desktop Entry]"));
        assert!(desktop.contains("Exec=\"/opt/Download Cepat/App 100%%\\$x.AppImage\""));
        assert!(desktop.contains("X-GNOME-Autostart-enabled=true"));

        let base_dir = std::env::temp_dir().join(format!("dc_copy_ext_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base_dir);
        let src_dir = base_dir.join("src");
        let dst_dir = base_dir.join("dst");
        std::fs::create_dir_all(src_dir.join("icons")).expect("create icons dir");
        std::fs::write(src_dir.join("manifest.json"), b"{}").expect("write manifest");
        std::fs::write(src_dir.join("icons").join("icon-16.png"), b"png").expect("write icon");
        copy_dir_recursive(&src_dir, &dst_dir).expect("copy_dir_recursive should succeed");
        assert!(dst_dir.join("manifest.json").is_file());
        assert_eq!(
            std::fs::read(dst_dir.join("icons").join("icon-16.png")).unwrap(),
            b"png"
        );
        let _ = std::fs::remove_dir_all(&base_dir);
    }

    #[test]
    fn test_background_tray_helpers() {
        assert!(should_hide_on_close("main"));
        assert!(!should_hide_on_close("drop-target"));

        let template_img = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))
            .expect("embedded 44x44 monochrome macOS tray template PNG should decode cleanly");
        assert_eq!(template_img.width(), 44);
        assert_eq!(template_img.height(), 44);

        let color_img = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
            .expect("embedded 32x32 tray PNG should decode cleanly");
        assert_eq!(color_img.width(), 32);
        assert_eq!(color_img.height(), 32);

        let tray_img = load_tray_icon_image().expect("load_tray_icon_image should return valid image");
        #[cfg(target_os = "macos")]
        assert_eq!((tray_img.width(), tray_img.height()), (44, 44));
        #[cfg(not(target_os = "macos"))]
        assert_eq!((tray_img.width(), tray_img.height()), (32, 32));
    }

    #[test]
    fn test_official_build_info_and_tray_languages() {
        let info = build_info_payload(Some("Download Cepat".to_string()));
        assert_eq!(info.app_name, "Download Cepat");
        assert_eq!(info.author, "Ujang Mustamar");
        assert_eq!(info.publisher, "Ujang Mustamar");
        assert_eq!(info.copyright, "Copyright © 2026 Ujang Mustamar");
        assert_eq!(info.license, "MIT");
        assert_eq!(info.version, "1.0.5");

        let en = tray_labels_for_lang("en");
        assert_eq!(en.show, "Open Download Cepat");
        assert_eq!(en.quit, "Quit");

        let fallback = tray_labels_for_lang("xx-unknown");
        assert_eq!(fallback.show, en.show);
        assert_eq!(fallback.quit, en.quit);

        let all_langs = [
            "en", "id", "ms", "zh", "ja", "ko", "es", "fr", "de", "pt", "ru", "ar", "hi", "vi",
            "th", "tr",
        ];
        for code in all_langs {
            assert_eq!(normalize_tray_lang(code), code);
            let lbl = tray_labels_for_lang(code);
            assert!(!lbl.show.is_empty());
            assert!(!lbl.add.is_empty());
            assert!(!lbl.pause_all.is_empty());
            assert!(!lbl.quit.is_empty());
            assert!(!lbl.tooltip.is_empty());
        }

        assert_eq!(tray_labels_for_lang("ja").show, "Download Cepat を開く");
        assert_eq!(tray_labels_for_lang("ms").pause_all, "Jeda Semua Muat Turun");
        assert_eq!(tray_labels_for_lang("id").quit, "Keluar");
        assert_eq!(tray_labels_for_lang("ar").quit, "إنهاء");
    }

    #[test]
    fn test_auto_updater_platform_and_script_helpers() {
        assert_eq!(
            macos_app_bundle_for_exe(Path::new("/Applications/Download Cepat.app/Contents/MacOS/download-cepat-app")),
            Some(PathBuf::from("/Applications/Download Cepat.app"))
        );
        assert_eq!(
            macos_app_bundle_for_exe(Path::new("/usr/local/bin/download-cepat-app")),
            None
        );

        let nsis_cmd = build_windows_update_cmd(
            Path::new(r"C:\Temp\dc\Download.Cepat_1.0.4_x64-setup.exe"),
            Path::new(r"C:\Users\Alice\AppData\Local\Download Cepat\Download Cepat.exe"),
            false,
        )
        .expect("valid NSIS script");
        assert!(nsis_cmd.contains("start /wait \"\" \"C:\\Temp\\dc\\Download.Cepat_1.0.4_x64-setup.exe\" /S"));
        assert!(nsis_cmd.contains("start \"\" \"C:\\Users\\Alice\\AppData\\Local\\Download Cepat\\Download Cepat.exe\""));

        let msi_cmd = build_windows_update_cmd(
            Path::new(r"C:\Temp\dc\Download.Cepat_1.0.4_x64.msi"),
            Path::new(r"C:\Program Files\Download Cepat\Download Cepat.exe"),
            true,
        )
        .expect("valid MSI script");
        assert!(msi_cmd.contains("msiexec /i \"C:\\Temp\\dc\\Download.Cepat_1.0.4_x64.msi\" /passive /norestart"));

        // Injection / unsafe batch chars must be rejected
        assert!(build_windows_update_cmd(
            Path::new(r"C:\Temp\evil&calc.exe"),
            Path::new(r"C:\App\dc.exe"),
            false
        ).is_err());
    }

    #[test]
    fn test_runtime_settings_and_join_save_dir() {
        let def = RuntimeDownloadSettings::default();
        assert_eq!(def.default_connections, 8);
        assert_eq!(def.max_tasks, 2);
        assert_eq!(def.speed_limit_kbs, 0);
        assert!(def.proxy_url.is_none());
        assert_eq!(def.video_save_dir, "~/Downloads/Video");
        assert_eq!(def.music_save_dir, "~/Downloads/Music");
        assert_eq!(def.torrents_save_dir, "~/Downloads/Torrents");
        assert_eq!(resolve_save_dir_for_category(&def, "torrents"), "~/Downloads/Torrents");
        assert_eq!(resolve_save_dir_for_category(&def, "video"), "~/Downloads/Video");
        assert_eq!(resolve_save_dir_for_category(&def, "music"), "~/Downloads/Music");
        assert_eq!(resolve_save_dir_for_category(&def, "general"), "~/Downloads");

        let clamped = normalize_runtime_settings(RuntimeDownloadSettings {
            default_connections: 99,
            max_tasks: 0,
            speed_limit_kbs: 4096,
            proxy_url: Some("  socks5://127.0.0.1:1080  ".to_string()),
            video_save_dir: "   ".to_string(),
            music_save_dir: r"D:\Musik\".to_string(),
            torrents_save_dir: "   ".to_string(),
        });
        assert_eq!(clamped.default_connections, 32);
        assert_eq!(clamped.max_tasks, 1);
        assert_eq!(clamped.proxy_url.as_deref(), Some("socks5://127.0.0.1:1080"));
        assert_eq!(clamped.video_save_dir, "~/Downloads/Video");
        assert_eq!(clamped.music_save_dir, r"D:\Musik\");
        assert_eq!(clamped.torrents_save_dir, "~/Downloads/Torrents");

        let test_dir = std::env::temp_dir().join(format!("dc_rt_settings_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);
        let file = test_dir.join("runtime_settings.json");
        let saved_cfg = normalize_runtime_settings(RuntimeDownloadSettings {
            default_connections: 32,
            max_tasks: 5,
            speed_limit_kbs: 0,
            proxy_url: None,
            video_save_dir: r"E:\Videos\".to_string(),
            music_save_dir: "~/Downloads/Music".to_string(),
            torrents_save_dir: r"F:\Torrents\".to_string(),
        });
        save_runtime_settings_to_path(&file, &saved_cfg).expect("save runtime settings");
        let reloaded = load_runtime_settings_from_path(&file);
        assert_eq!(reloaded, saved_cfg);
        let _ = std::fs::remove_dir_all(&test_dir);

        assert_eq!(
            join_save_dir_and_file("~/Downloads/Video/", "clip.mp4"),
            "~/Downloads/Video/clip.mp4"
        );
        assert_eq!(
            join_save_dir_and_file(r"D:\Video\", "clip.mp4"),
            r"D:\Video\clip.mp4"
        );
        assert_eq!(join_save_dir_and_file("C:", "clip.mp4"), r"C:\clip.mp4");
        assert_eq!(join_save_dir_and_file("/", "clip.mp4"), "/clip.mp4");
    }

    #[test]
    fn test_build_ytdlp_format_selector_and_stream_headers() {
        assert_eq!(
            build_ytdlp_format_selector(Some("1080p_mp4"), "mp4", false, true),
            "bestvideo[height<=1080]+bestaudio/best[height<=1080]/best"
        );
        assert_eq!(
            build_ytdlp_format_selector_with_strict(Some("2160p_mp4"), "mp4", false, true, true),
            "bestvideo[height<=2160]+bestaudio"
        );
        assert_eq!(
            build_ytdlp_format_selector_with_strict(None, "mp4", false, true, true),
            "bestvideo+bestaudio"
        );
        assert_eq!(
            build_ytdlp_format_selector_with_strict(Some("313"), "mp4", false, true, true),
            "313+bestaudio"
        );
        assert_eq!(
            build_ytdlp_format_selector(Some("720p_mp4"), "mp4", false, false),
            "best[height<=720][ext=mp4]/best[height<=720]/best"
        );
        assert_eq!(
            build_ytdlp_format_selector(Some("audio_mp3"), "mp3", true, true),
            "bestaudio/18/best"
        );
        assert_eq!(
            build_ytdlp_format_selector(Some("audio_m4a"), "m4a", true, true),
            "bestaudio[ext=m4a]/bestaudio/18/best"
        );

        assert_eq!(parse_requested_height_opt(Some("2160p_mp4")), Some(2160));
        assert_eq!(parse_requested_height_opt(Some("1080p")), Some(1080));
        assert_eq!(parse_requested_height_opt(Some("audio_mp3")), None);

        let ff_dir = std::env::temp_dir().join(format!("dc_ff_check_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ff_dir);
        std::fs::create_dir_all(&ff_dir).expect("create ff check dir");
        let empty_ff = ff_dir.join("empty_ffmpeg");
        let valid_ff = ff_dir.join("system_ffmpeg");
        std::fs::write(&empty_ff, b"").expect("write empty");
        std::fs::write(&valid_ff, b"elf").expect("write non-empty");
        assert!(!is_valid_ffmpeg_candidate(&ff_dir.join("missing")));
        assert!(!is_valid_ffmpeg_candidate(&empty_ff));
        assert!(is_valid_ffmpeg_candidate(&valid_ff));
        let _ = std::fs::remove_dir_all(&ff_dir);

        let stream = core_engine::video_runtime::ResolvedHttpStream {
            url: "https://rr1.googlevideo.com/videoplayback".to_string(),
            ext: "mp4".to_string(),
            vcodec: "avc1".to_string(),
            acodec: "none".to_string(),
            filesize: Some(1024),
            height: Some(1080),
            fps: None,
            tbr_kbps: 0,
            abr_kbps: 0,
            user_agent: Some("StreamUA/1.0".to_string()),
            referer: None,
            cookie_header: None,
        };
        let (yt_cookie, yt_ref, yt_ua) = resolve_stream_request_headers(
            &stream,
            true,
            Some("SESSION=abc"),
            Some("https://www.youtube.com/watch?v=123"),
            Some("CallerUA/1.0"),
        );
        assert_eq!(yt_cookie, None);
        assert_eq!(yt_ref, Some("https://www.youtube.com/watch?v=123"));
        assert_eq!(yt_ua, Some("StreamUA/1.0"));

        let (gen_cookie, _, _) = resolve_stream_request_headers(
            &stream,
            false,
            Some("SESSION=abc"),
            None,
            None,
        );
        assert_eq!(gen_cookie, Some("SESSION=abc"));
    }

    #[test]
    fn test_popup_positioning_and_prebuffer_cleanup() {
        assert!(should_hide_on_close("main"));
        assert!(!should_hide_on_close("dl-popup-1"));
        assert!(!should_hide_on_close("dl-popup-42"));

        assert_eq!(sanitize_popup_id("dl-popup-12"), "dl-popup-12");
        assert_eq!(sanitize_popup_id("../evil/popup"), "evilpopup");
        assert_eq!(sanitize_popup_id("   "), "popup_invalid");

        // Primary 1920x1080 display at 1x scale
        let (x0, y0) = compute_popup_position(0, 0, 1920, 1080, 1.0, 590.0, 540.0, 0);
        let (x1, y1) = compute_popup_position(0, 0, 1920, 1080, 1.0, 590.0, 540.0, 1);
        assert_eq!((x0, y0), (665.0, 270.0));
        assert_eq!((x1, y1), (693.0, 298.0));

        // HiDPI 2x scale clamped strictly inside work area
        let (hx, hy) = compute_popup_position(0, 50, 2560, 1440, 2.0, 590.0, 540.0, 5);
        assert!(hx >= 0.0 && hx <= 1280.0 - 590.0);
        assert!(hy >= 25.0 && hy <= 25.0 + 720.0 - 540.0);

        // Zero scale safely defaults to 1.0
        let (zx, zy) = compute_popup_position(0, 0, 800, 600, 0.0, 590.0, 540.0, 20);
        assert!(zx >= 0.0 && zy >= 0.0);

        // Prebuffer cleanup helpers
        let pre_dir = std::env::temp_dir().join(format!("dc_prebuf_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pre_dir);
        std::fs::create_dir_all(&pre_dir).expect("create prebuf dir");

        let p1 = pre_dir.join("dl-popup-7_r1.part");
        let p1_meta = core_engine::downloader::checkpoint_path_for(&p1);
        let p1_r2 = pre_dir.join("dl-popup-7_r2.part");
        let p_other = pre_dir.join("dl-popup-8_r1.part");
        std::fs::write(&p1, b"part1").unwrap();
        std::fs::write(&p1_meta, b"{}").unwrap();
        std::fs::write(&p1_r2, b"part2").unwrap();
        std::fs::write(&p_other, b"other").unwrap();

        assert_eq!(cleanup_prebuffer_files_at(&p1), 2);
        assert!(!p1.exists());
        assert!(!p1_meta.exists());
        assert!(p1_r2.exists());
        assert!(p_other.exists());

        assert_eq!(cleanup_prebuffer_by_id_prefix_at(&pre_dir, "dl-popup-7"), 1);
        assert!(!p1_r2.exists());
        assert!(p_other.exists());

        assert_eq!(cleanup_stale_prebuffer_dir_at(&pre_dir), 1);
        assert!(!p_other.exists());

        let _ = std::fs::remove_dir_all(&pre_dir);
    }
}
