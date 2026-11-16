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
    LANG.get_or_init(|| Mutex::new("id".to_string()))
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
        engine: "Download Cepat Core Engine (Rust Multi-Chunk HTTP/FTP)".to_string(),
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

fn tray_labels_for_lang(lang: &str) -> TrayLabels {
    match lang.trim().to_ascii_lowercase().as_str() {
        "en" => TrayLabels {
            show: "Open Download Cepat",
            add: "Add New URL...",
            pause_all: "Pause All Downloads",
            quit: "Quit",
            tooltip: "Download Cepat — Click to Open",
        },
        "ms" => TrayLabels {
            show: "Buka Download Cepat",
            add: "Tambah URL Baharu...",
            pause_all: "Jeda Semua Muat Turun",
            quit: "Keluar",
            tooltip: "Download Cepat — Klik untuk Membuka",
        },
        "ja" => TrayLabels {
            show: "Download Cepat を開く",
            add: "新しい URL を追加...",
            pause_all: "すべて一時停止",
            quit: "終了",
            tooltip: "Download Cepat — クリックして開く",
        },
        _ => TrayLabels {
            show: "Buka Download Cepat",
            add: "Tambah URL Baru...",
            pause_all: "Jeda Semua Unduhan",
            quit: "Keluar",
            tooltip: "Download Cepat — Klik untuk Membuka",
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
    let normalized = match lang.trim().to_ascii_lowercase().as_str() {
        "en" => "en",
        "ms" => "ms",
        "ja" => "ja",
        _ => "id",
    };
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

fn cleanup_download_artifacts(target: &Path) -> usize {
    let mut removed_count = 0usize;
    let mut remove_if_exists = |path: &Path| {
        if path.is_file() && std::fs::remove_file(path).is_ok() {
            removed_count += 1;
        }
    };

    let temp_streams: Vec<PathBuf> = ["temp_stream.mp4", "temp_stream.m4a"]
        .into_iter()
        .map(|ext| target.with_extension(ext))
        .filter(|p| p != target)
        .collect();

    let mut direct_candidates = vec![
        target.to_path_buf(),
        core_engine::downloader::checkpoint_path_for(target),
        PathBuf::from(format!("{}.part", target.display())),
        PathBuf::from(format!("{}.ytdl", target.display())),
    ];
    for temp_stream in &temp_streams {
        direct_candidates.push(temp_stream.clone());
        direct_candidates.push(PathBuf::from(format!("{}.part", temp_stream.display())));
        direct_candidates.push(PathBuf::from(format!("{}.ytdl", temp_stream.display())));
    }
    for candidate in &direct_candidates {
        remove_if_exists(candidate);
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

                if target_frag || temp_frag || stream_split {
                    remove_if_exists(&path);
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
    if cleaned.chars().count() > 160 {
        cleaned = cleaned.chars().take(160).collect::<String>();
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

    // If target has a file extension or is a file, reveal in Finder/Explorer/File Manager or get parent directory
    if target.is_file() || target.extension().is_some() {
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

fn resolve_unique_destination(target: &Path) -> PathBuf {
    if !target.exists() || core_engine::downloader::checkpoint_path_for(target).exists() {
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
    let raw_target = resolve_path(&destination_path);
    let target = if resume.unwrap_or(false) {
        raw_target
    } else {
        resolve_unique_destination(&raw_target)
    };
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    tauri::async_runtime::spawn(async move {
        register_task_target(&task_id, &target);

        let is_video_or_stream = is_video_or_stream_url(&url)
            || format_id.as_deref().map_or(false, |f| !f.trim().is_empty());

        if is_video_or_stream {
            let ytdlp_res = ensure_ytdlp_ready_for_video().await;
            let mut ytdlp_error_detail: Option<String> = ytdlp_res.as_ref().err().cloned();

            if let Ok(bin) = ytdlp_res {
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

                // Emit initial start progress immediately
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

                let target_ext = target
                    .extension()
                    .map(|e| e.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                let is_audio = matches!(target_ext.as_str(), "mp3" | "m4a" | "flac" | "wav");

                // Locate ffmpeg before selecting streams so progressive fallback works if ffmpeg is absent
                let ffmpeg_path = find_executable_in_dirs(&["ffmpeg"], &default_tool_dirs());
                let has_ffmpeg = ffmpeg_path.is_some();

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
                if num_connections > 1 {
                    cmd.arg("-N").arg(num_connections.clamp(1, 32).to_string());
                }
                if let Some(kbs) = speed_limit_kbs.filter(|&k| k > 0) {
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
                    if let Some(ffmpeg_dir) = ff_path.parent() {
                        cmd.arg("--ffmpeg-location").arg(ffmpeg_dir);
                    }
                }

                let parse_height_selector = |h: u32| -> String {
                    if has_ffmpeg {
                        format!("bestvideo[height<={h}]+bestaudio/best[height<={h}]/best")
                    } else {
                        format!("best[height<={h}][ext=mp4]/best[height<={h}]/best")
                    }
                };
                let default_video_selector = if has_ffmpeg {
                    "bestvideo+bestaudio/best".to_string()
                } else {
                    "best[ext=mp4]/best".to_string()
                };
                let default_m4a_selector = if has_ffmpeg {
                    "bestaudio[ext=m4a]/bestaudio/18/best".to_string()
                } else {
                    "bestaudio[ext=m4a]/best[ext=m4a]/bestaudio/18/best".to_string()
                };

                let fmt_selector = if let Some(fid) = format_id.as_deref() {
                    if fid == "audio_mp3" {
                        "bestaudio/18/best".to_string()
                    } else if fid == "audio_m4a" {
                        default_m4a_selector.clone()
                    } else if let Some(h_str) = fid.strip_suffix("p_mp4") {
                        if let Ok(h) = h_str.parse::<u32>() {
                            parse_height_selector(h)
                        } else {
                            default_video_selector.clone()
                        }
                    } else if let Some(h_str) = fid.strip_suffix('p') {
                        if let Ok(h) = h_str.parse::<u32>() {
                            parse_height_selector(h)
                        } else {
                            default_video_selector.clone()
                        }
                    } else if is_audio {
                        if target_ext == "m4a" {
                            default_m4a_selector.clone()
                        } else {
                            "bestaudio/18/best".to_string()
                        }
                    } else {
                        default_video_selector.clone()
                    }
                } else if is_audio {
                    if target_ext == "m4a" {
                        default_m4a_selector
                    } else {
                        "bestaudio/18/best".to_string()
                    }
                } else {
                    default_video_selector
                };

                cmd.arg("-f").arg(&fmt_selector);
                if !is_audio && has_ffmpeg {
                    cmd.arg("--merge-output-format").arg("mp4");
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
                                    // Telemetry for audio conversion phase
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

                                    let mut converted = false;
                                    if let Some(home) = user_home_dir() {
                                        let converter_script =
                                            home.join(".download-cepat").join("bin").join("converter.py");
                                        let venv_dir = home.join(".download-cepat").join("venv");
                                        let python_candidates = [
                                            venv_dir.join("bin").join("python3"),
                                            venv_dir.join("Scripts").join("python.exe"),
                                        ];
                                        let python_bin = python_candidates
                                            .into_iter()
                                            .find(|p| p.is_file())
                                            .or_else(|| {
                                                find_executable_in_dirs(
                                                    &["python3", "python"],
                                                    &default_tool_dirs(),
                                                )
                                            });

                                        if converter_script.is_file() {
                                            if let Some(py) = python_bin {
                                                let mut conv_cmd = tokio::process::Command::new(&py);
                                                hide_window_tokio(&mut conv_cmd);
                                                conv_cmd
                                                    .arg(&converter_script)
                                                    .arg(&tmp_video)
                                                    .arg(&target)
                                                    .stdout(std::process::Stdio::null())
                                                    .stderr(std::process::Stdio::null());
                                                if let Ok(c_status) = conv_cmd.status().await {
                                                    if c_status.success() {
                                                        converted = std::fs::metadata(&target)
                                                            .map(|m| m.is_file() && m.len() > 0)
                                                            .unwrap_or(false);
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if !converted {
                                        if let Some(ref ff_path) = ffmpeg_path {
                                            let mut ff_cmd = tokio::process::Command::new(ff_path);
                                            hide_window_tokio(&mut ff_cmd);
                                            ff_cmd.arg("-y").arg("-i").arg(&tmp_video).arg("-vn");
                                            match target_ext.as_str() {
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
                                                .arg(&target)
                                                .stdout(std::process::Stdio::null())
                                                .stderr(std::process::Stdio::null());
                                            if let Ok(ff_status) = ff_cmd.status().await {
                                                if ff_status.success() {
                                                    converted = std::fs::metadata(&target)
                                                        .map(|m| m.is_file() && m.len() > 0)
                                                        .unwrap_or(false);
                                                }
                                            }
                                        }
                                    }

                                    if !converted {
                                        #[cfg(target_os = "macos")]
                                        {
                                            let mut m4a_path = target.clone();
                                            m4a_path.set_extension("m4a");
                                            let _ = tokio::process::Command::new("afconvert")
                                                .arg("-f")
                                                .arg("m4af")
                                                .arg("-d")
                                                .arg("aac")
                                                .arg(&tmp_video)
                                                .arg(&m4a_path)
                                                .stdout(std::process::Stdio::null())
                                                .stderr(std::process::Stdio::null())
                                                .status()
                                                .await;

                                            if target != m4a_path && !target.exists() && m4a_path.exists() {
                                                let _ = std::fs::copy(&m4a_path, &target);
                                            }
                                            if std::fs::metadata(&target)
                                                .map(|m| m.is_file() && m.len() > 0)
                                                .unwrap_or(false)
                                            {
                                                converted = true;
                                            }
                                        }
                                    }

                                    if !converted && target_ext == "m4a" && tmp_video.is_file() {
                                        let is_isobmff = std::fs::File::open(&tmp_video)
                                            .and_then(|mut f| {
                                                use std::io::Read;
                                                let mut hdr = [0u8; 12];
                                                f.read_exact(&mut hdr).map(|_| &hdr[4..8] == b"ftyp")
                                            })
                                            .unwrap_or(false);
                                        if is_isobmff
                                            && move_file_cross_device(&tmp_video, &target).is_ok()
                                        {
                                            converted = std::fs::metadata(&target)
                                                .map(|m| m.is_file() && m.len() > 0)
                                                .unwrap_or(false);
                                        }
                                    }

                                    let _ = std::fs::remove_file(&tmp_video);

                                    if !converted {
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
                                            Some(format!(
                                                "Gagal mengonversi audio ke .{}. Pasang FFmpeg di sistem agar konversi audio (MP3/M4A) dapat berjalan.",
                                                target_ext
                                            )),
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

        // Standard HTTP / HTTPS multi-connection download
        let controller = core_engine::DownloadController::new();
        register_active_download(&task_id, ActiveControl::Http(controller.clone()));

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

        // Inspect URL to get real file size, Range support, and reject HTML hijack pages
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
                &url,
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

        // Emit initial progress event
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

        let (tx, mut rx) = tokio::sync::broadcast::channel::<core_engine::model::ProgressTelemetry>(128);

        // Forward telemetry to Tauri frontend (including real ETA and per-thread segments!)
        let app_clone = app.clone();
        let target_str = target.display().to_string();
        let monitor_handle = tokio::spawn(async move {
            while let Ok(telem) = rx.recv().await {
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
                    },
                );
                if is_terminal {
                    break;
                }
            }
        });

        let speed_limit_bps = speed_limit_kbs.filter(|&k| k > 0).map(|k| k * 1024);
        let download_result = downloader
            .download_with_options(
                &task_id,
                &dl_url,
                &target,
                file_size,
                supports_range,
                num_connections,
                Some(tx),
                Some(controller.clone()),
                core_engine::downloader::DownloadOptions {
                    cookie: cookie.as_deref(),
                    referer: referer.as_deref(),
                    user_agent: user_agent.as_deref(),
                    speed_limit_bytes_per_sec: speed_limit_bps,
                    proxy_url: proxy_url.as_deref(),
                    auth_username: auth_username.as_deref(),
                    auth_password: auth_password.as_deref(),
                    allow_url_refresh: allow_url_refresh.unwrap_or(false),
                },
            )
            .await;

        let _ = monitor_handle.await;
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
                let file_len = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
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
    let Some(home) = user_home_dir() else { return 0 };
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

#[cfg(any(target_os = "linux", test))]
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

fn parse_bridge_add_links_payload(body: &str) -> Option<serde_json::Value> {
    let v = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let raw_links = v["links"].as_array()?;
    let mut cleaned: Vec<serde_json::Value> = Vec::new();
    for item in raw_links {
        if let Some(u) = item.as_str() {
            let trimmed = u.trim();
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.starts_with("ftp://") {
                cleaned.push(serde_json::json!({
                    "url": trimmed,
                    "label": "",
                }));
            }
        } else if let Some(u) = item["url"].as_str() {
            let trimmed = u.trim();
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.starts_with("ftp://") {
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
                            .unwrap_or_else(|_| "id".to_string());
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
                                    "message": "Daftar link berhasil dikirim ke Download Cepat"
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

                        let mut inspect_err_msg = "Stream video tidak dapat dianalisis.".to_string();
                        if !target_url.is_empty() {
                            match inspect_video_url_with_headers(
                                &target_url,
                                inspect_cookie.as_deref(),
                                inspect_referer.as_deref(),
                                inspect_ua.as_deref(),
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
                                                "sizeLabel": f.file_size_approx.map(format_bytes_label).unwrap_or_else(|| "Ukuran dinamis".to_string()),
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

                    // 3. ROUTE: POST /start_download (Silent background start from Browser Dropdown)
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
                                let conn_count = v["connections"].as_u64().or_else(|| v["numConnections"].as_u64()).unwrap_or(8) as usize;
                                let cookie_opt = v["cookie"].as_str().filter(|s| !s.trim().is_empty()).map(String::from);
                                let referer_opt = v["referer"].as_str().or_else(|| v["referrer"].as_str()).filter(|s| !s.trim().is_empty()).map(String::from);
                                let ua_opt = v["userAgent"].as_str().filter(|s| !s.trim().is_empty()).map(String::from);

                                let clean_title = sanitize_media_title(&raw_title, "Video");

                                let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                                let task_id = format!("task-{}", now_ms);
                                let file_name = format!("{}.{}", clean_title, clean_ext);
                                let cat_id = if is_audio { "music" } else { "video" };
                                let dest_path = if is_audio {
                                    format!("~/Downloads/Music/{}", file_name)
                                } else {
                                    format!("~/Downloads/Video/{}", file_name)
                                };

                                // Emit to frontend so the task is displayed in the active list
                                let _ = app_handle.emit("external-task-added", serde_json::json!({
                                    "id": task_id.clone(),
                                    "url": url.clone(),
                                    "fileName": file_name.clone(),
                                    "destinationPath": dest_path.clone(),
                                    "categoryId": cat_id,
                                    "formatId": format_id.clone(),
                                    "resolution": resolution.clone(),
                                    "numConnections": conn_count,
                                    "status": "downloading"
                                }));

                                // Spawn real background download!
                                let app_for_dl = app_handle.clone();
                                let tid_for_dl = task_id.clone();
                                let u_for_dl = url.clone();
                                let dest_for_dl = dest_path.clone();
                                let fmt_for_dl = Some(format_id.clone());
                                tauri::async_runtime::spawn(async move {
                                    let _ = start_real_download(
                                        app_for_dl,
                                        tid_for_dl,
                                        u_for_dl,
                                        dest_for_dl,
                                        conn_count,
                                        fmt_for_dl,
                                        cookie_opt,
                                        referer_opt,
                                        ua_opt,
                                        None,
                                        None,
                                        None,
                                        None,
                                        None,
                                        None,
                                    )
                                    .await;
                                });

                                let resp_body = serde_json::json!({
                                    "status": "ok",
                                    "message": "Unduhan berhasil dimulai di latar belakang",
                                    "taskId": task_id,
                                    "fileName": file_name,
                                    "destinationPath": dest_path
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

                    // 4. ROUTE: POST /add_download or GET /add_download?url=... (Interactive modal popup)
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
                        let _ = app_handle.emit("browser-add-download", serde_json::json!({
                            "url": found_url,
                            "fileName": found_filename,
                            "cookie": found_cookie,
                            "referer": found_referer,
                            "userAgent": found_user_agent,
                            "fileSize": found_file_size,
                            "mimeType": found_mime_type
                        }));

                        show_main_window(&app_handle);

                        let body = "{\"status\":\"ok\",\"message\":\"Unduhan berhasil diterima oleh Download Cepat\"}";
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
            check_for_app_update,
            download_and_install_app_update
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let initial_labels = tray_labels_for_lang("id");
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
                            show_main_window(app_handle);
                            let _ = app_handle.emit("tray-add-url", ());
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
                tokio::join!(ytdlp_fut, js_fut);
            });
            tauri::async_runtime::spawn(async move {
                start_browser_bridge_server(handle).await;
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if should_hide_on_close(window.label()) {
                    api.prevent_close();
                    hide_main_window_to_tray(window);
                }
            }
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
            r#"{"pageUrl":"https://example.com","links":["https://example.com/a.zip",{"url":"ftp://files.example.com/b.iso","label":"ISO"},"javascript:void(0)"]}"#,
        )
        .expect("parse links payload");
        let arr = parsed["links"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["url"].as_str().unwrap(), "https://example.com/a.zip");
        assert_eq!(arr[1]["url"].as_str().unwrap(), "ftp://files.example.com/b.iso");

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
        assert!(sanitize_media_title(&"a".repeat(220), "Video").chars().count() <= 160);

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
        assert_eq!(info.version, "1.0.2");

        let en = tray_labels_for_lang("en");
        assert_eq!(en.show, "Open Download Cepat");
        assert_eq!(en.quit, "Quit");

        let ja = tray_labels_for_lang("ja");
        assert_eq!(ja.show, "Download Cepat を開く");

        let ms = tray_labels_for_lang("ms");
        assert_eq!(ms.pause_all, "Jeda Semua Muat Turun");

        let id = tray_labels_for_lang("id");
        assert_eq!(id.quit, "Keluar");
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
            Path::new(r"C:\Temp\dc\Download.Cepat_1.0.2_x64-setup.exe"),
            Path::new(r"C:\Users\Alice\AppData\Local\Download Cepat\Download Cepat.exe"),
            false,
        )
        .expect("valid NSIS script");
        assert!(nsis_cmd.contains("start /wait \"\" \"C:\\Temp\\dc\\Download.Cepat_1.0.2_x64-setup.exe\" /S"));
        assert!(nsis_cmd.contains("start \"\" \"C:\\Users\\Alice\\AppData\\Local\\Download Cepat\\Download Cepat.exe\""));

        let msi_cmd = build_windows_update_cmd(
            Path::new(r"C:\Temp\dc\Download.Cepat_1.0.2_x64.msi"),
            Path::new(r"C:\Program Files\Download Cepat\Download Cepat.exe"),
            true,
        )
        .expect("valid MSI script");
        assert!(msi_cmd.contains("msiexec /i \"C:\\Temp\\dc\\Download.Cepat_1.0.2_x64.msi\" /passive /norestart"));

        // Injection / unsafe batch chars must be rejected
        assert!(build_windows_update_cmd(
            Path::new(r"C:\Temp\evil&calc.exe"),
            Path::new(r"C:\App\dc.exe"),
            false
        ).is_err());
    }
}
