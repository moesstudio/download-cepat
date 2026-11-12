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

    let mut temp_stream = target.to_path_buf();
    temp_stream.set_extension("temp_stream.mp4");

    let mut direct_candidates = vec![
        target.to_path_buf(),
        core_engine::downloader::checkpoint_path_for(target),
        PathBuf::from(format!("{}.part", target.display())),
        PathBuf::from(format!("{}.ytdl", target.display())),
    ];
    if temp_stream != target {
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
        let temp_name = temp_stream
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let stem = target
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let temp_stem = temp_stream
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
                let temp_frag = temp_stream != target
                    && !temp_name.is_empty()
                    && (name.starts_with(&format!("{}.part", temp_name))
                        || name.starts_with(&format!("{}.ytdl", temp_name)));
                let stream_split = (!stem.is_empty() && is_ytdlp_stream_fragment_file(&name, stem))
                    || (temp_stream != target
                        && !temp_stem.is_empty()
                        && is_ytdlp_stream_fragment_file(&name, temp_stem));

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

fn find_executable_in_dirs(bin_names: &[&str], extra_dirs: &[PathBuf]) -> Option<PathBuf> {
    let mut search_dirs: Vec<PathBuf> = extra_dirs.to_vec();
    if let Some(path_env) = std::env::var_os("PATH") {
        search_dirs.extend(std::env::split_paths(&path_env));
    }

    let suffix = std::env::consts::EXE_SUFFIX;
    for dir in search_dirs {
        for name in bin_names {
            let candidate = if !suffix.is_empty() && !name.ends_with(suffix) {
                dir.join(format!("{}{}", name, suffix))
            } else {
                dir.join(name)
            };
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn default_tool_dirs() -> Vec<PathBuf> {
    let mut dirs_list = Vec::new();
    if let Some(home) = dirs::home_dir().or_else(|| std::env::var_os("HOME").map(PathBuf::from)) {
        dirs_list.push(home.join(".download-cepat").join("bin"));
        dirs_list.push(home.join(".deno").join("bin"));
        dirs_list.push(home.join(".bun").join("bin"));
    }
    dirs_list.push(PathBuf::from("/opt/homebrew/bin"));
    dirs_list.push(PathBuf::from("/usr/local/bin"));
    dirs_list.push(PathBuf::from("/usr/bin"));
    dirs_list
}

fn detect_js_runtime_arg() -> Option<String> {
    let tool_dirs = default_tool_dirs();
    let runtimes: [(&str, &[&str]); 4] = [
        ("deno", &["deno"]),
        ("node", &["node"]),
        ("bun", &["bun"]),
        ("quickjs", &["qjs", "quickjs"]),
    ];
    for (runtime_name, bin_names) in runtimes {
        if let Some(path) = find_executable_in_dirs(bin_names, &tool_dirs) {
            return Some(format!("{}:{}", runtime_name, path.display()));
        }
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
    let home_dir = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default();
    let ytdlp_candidates = [
        format!("{}/.download-cepat/bin/yt-dlp", home_dir),
        format!("{}/.download-cepat/venv/bin/yt-dlp", home_dir),
        "/Users/mustamar/.download-cepat/bin/yt-dlp".to_string(),
        "/Users/mustamar/.download-cepat/venv/bin/yt-dlp".to_string(),
        "/Users/mustamar/.gemini/antigravity/brain/3cce09e5-7380-401d-9eee-538fa12dc83c/scratch/venv/bin/yt-dlp".to_string(),
        "/opt/homebrew/bin/yt-dlp".to_string(),
        "/usr/local/bin/yt-dlp".to_string(),
    ];
    ytdlp_candidates.into_iter().find(|p| Path::new(p).exists())
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

#[allow(dead_code)]
async fn inspect_video_url(url: &str) -> Option<InspectResult> {
    inspect_video_url_with_headers(url, None, None, None).await
}

async fn inspect_video_url_with_headers(
    url: &str,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
) -> Option<InspectResult> {
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

async fn run_ytdlp_dump_json(
    bin: &str,
    url: &str,
    force_all_clients: bool,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
    proxy_url: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
    timeout_secs: u64,
) -> Option<serde_json::Value> {
    let mut cmd = tokio::process::Command::new(bin);
    let has_js_rt = apply_ytdlp_runtime_env(&mut cmd);
    if force_all_clients || !has_js_rt {
        cmd.arg("--extractor-args").arg("youtube:player_client=all");
    }
    cmd.arg("--dump-single-json");
    cmd.arg("--no-playlist");
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
    .ok()?
    .ok()?;

    if !output.status.success() || output.stdout.is_empty() {
        let err_snippet = String::from_utf8_lossy(&output.stderr);
        if !err_snippet.trim().is_empty() {
            log::warn!(
                "yt-dlp inspect failed for {}: {}",
                url,
                err_snippet.lines().last().unwrap_or("")
            );
        }
        return None;
    }

    serde_json::from_slice::<serde_json::Value>(&output.stdout).ok()
}

async fn inspect_video_url_with_options(
    url: &str,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
    proxy_url: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
) -> Option<InspectResult> {
    let bin = get_ytdlp_bin()?;
    let is_yt = is_youtube_url(url);

    // Primary fast extraction (uses detected JS runtime e.g. node/deno without forcing slow player_client=all)
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
        12,
    )
    .await;

    let json_val = match primary {
        Some(v) if !is_yt || json_has_playable_video_format(&v) => v,
        fallback_candidate => {
            // Fallback for YouTube session/cookie bot-gate or SABR client mismatch:
            // retry cleanly without browser Cookie header and/or with all clients enabled.
            let retry_all = !is_yt || fallback_candidate.is_some();
            let retry = run_ytdlp_dump_json(
                &bin,
                url,
                retry_all,
                if is_yt { None } else { cookie },
                referer,
                if is_yt { None } else { user_agent },
                proxy_url,
                auth_username,
                auth_password,
                11,
            )
            .await;

            match (retry, fallback_candidate) {
                (Some(r), Some(c)) => {
                    if json_has_playable_video_format(&r) || !json_has_playable_video_format(&c) {
                        r
                    } else {
                        c
                    }
                }
                (Some(r), None) => r,
                (None, Some(c)) => c,
                (None, None) => return None,
            }
        }
    };

    let raw_title = json_val["title"].as_str().unwrap_or("video");
    let clean_title: String = raw_title
        .chars()
        .filter(|c| !matches!(*c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let clean_title = if clean_title.trim().is_empty() {
        "video".to_string()
    } else {
        clean_title.trim().to_string()
    };

    let duration_seconds = json_val["duration"].as_u64().unwrap_or(0);
    let thumbnail_url = json_val["thumbnail"].as_str().map(|s| s.to_string());

    let formats_array = json_val["formats"].as_array()?;

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

    Some(InspectResult {
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
    if p.starts_with("~/") || p == "~" {
        if let Some(home) = dirs::home_dir().or_else(|| std::env::var_os("HOME").map(PathBuf::from)) {
            if p == "~" {
                return home;
            }
            return home.join(&p[2..]);
        }
    }
    PathBuf::from(p)
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

#[tauri::command]
fn open_folder(path: String) -> Result<String, String> {
    let mut target = resolve_path(&path);

    // If target has a file extension or is a file, reveal in Finder/Explorer or get parent directory
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
                let _ = std::process::Command::new("explorer")
                    .arg(format!("/select,\"{}\"", target.display()))
                    .spawn();
                return Ok(format!("Membuka berkas di Explorer: {}", target.display()));
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
    let target = resolve_path(&path);

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
            let home_dir = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default();
            let ytdlp_bin = get_ytdlp_bin();

            if let Some(bin) = ytdlp_bin {
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

                let is_audio = target.extension().map_or(false, |ext| {
                    let e = ext.to_string_lossy().to_lowercase();
                    e == "mp3" || e == "m4a" || e == "flac" || e == "wav"
                });

                let tmp_video = if is_audio {
                    let mut t = target.clone();
                    t.set_extension("temp_stream.mp4");
                    t
                } else {
                    target.clone()
                };

                let mut cmd = tokio::process::Command::new(&bin);
                let has_js_rt = apply_ytdlp_runtime_env(&mut cmd);
                if !has_js_rt {
                    cmd.arg("--extractor-args").arg("youtube:player_client=all");
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

                // Locate ffmpeg for multi-stream video + audio merging
                if let Some(ffmpeg_path) = find_executable_in_dirs(&["ffmpeg"], &default_tool_dirs()) {
                    if let Some(ffmpeg_dir) = ffmpeg_path.parent() {
                        cmd.arg("--ffmpeg-location").arg(ffmpeg_dir);
                    }
                }

                let fmt_selector = if let Some(fid) = format_id.as_deref() {
                    if fid == "audio_mp3" {
                        "bestaudio/18/best".to_string()
                    } else if fid == "audio_m4a" {
                        "bestaudio[ext=m4a]/bestaudio/18/best".to_string()
                    } else if let Some(h_str) = fid.strip_suffix("p_mp4") {
                        if let Ok(h) = h_str.parse::<u32>() {
                            format!("bestvideo[height<={h}]+bestaudio/best[height<={h}]/best")
                        } else {
                            "bestvideo+bestaudio/best".to_string()
                        }
                    } else if let Some(h_str) = fid.strip_suffix('p') {
                        if let Ok(h) = h_str.parse::<u32>() {
                            format!("bestvideo[height<={h}]+bestaudio/best[height<={h}]/best")
                        } else {
                            "bestvideo+bestaudio/best".to_string()
                        }
                    } else {
                        if is_audio {
                            "bestaudio/18/best".to_string()
                        } else {
                            "bestvideo+bestaudio/best".to_string()
                        }
                    }
                } else if is_audio {
                    "bestaudio/18/best".to_string()
                } else {
                    "bestvideo+bestaudio/best".to_string()
                };

                cmd.arg("-f").arg(&fmt_selector);
                if !is_audio {
                    cmd.arg("--merge-output-format").arg("mp4");
                }
                cmd.arg("-o").arg(&tmp_video);
                cmd.arg(&url);
                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::null()); // DO NOT PIPE STDERR TO AVOID DEADLOCK

                if let Ok(mut child) = cmd.spawn() {
                    let stdout = child.stdout.take();
                    let app_for_lines = app.clone();
                    let tid_for_lines = task_id.clone();
                    let target_for_lines = target.clone();

                    // Read progress lines asynchronously from yt-dlp stdout
                    let line_task = tokio::spawn(async move {
                        if let Some(out) = stdout {
                            use tokio::io::AsyncBufReadExt;
                            let mut reader = tokio::io::BufReader::new(out).lines();
                            while let Ok(Some(line)) = reader.next_line().await {
                                if let Some((pct, dl_bytes, total_bytes, speed_bps, eta)) = parse_ytdlp_progress_line(&line) {
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

                    let status = tokio::select! {
                        res = child.wait() => Some(res),
                        _ = stop_notify.notified() => {
                            let _ = child.kill().await;
                            let _ = child.wait().await;
                            None
                        }
                    };
                    let _ = line_task.await;
                    unregister_active_download(&task_id);

                    if canceled_flag.load(std::sync::atomic::Ordering::SeqCst) {
                        remove_task_target(&task_id);
                        cleanup_download_artifacts(&target);
                        emit_simple_telemetry(&app, &task_id, 0, 0, 0, 0.0, None, "canceled", &target, None);
                        return;
                    }

                    if paused_flag.load(std::sync::atomic::Ordering::SeqCst) {
                        emit_simple_telemetry(&app, &task_id, 0, 0, 0, 0.0, None, "paused", &target, None);
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

                                let converter_script = format!("{}/.download-cepat/bin/converter.py", home_dir);
                                let python_bin = format!("{}/.download-cepat/venv/bin/python3", home_dir);

                                let mut converted = false;
                                if std::path::Path::new(&converter_script).exists() && std::path::Path::new(&python_bin).exists() {
                                    let conv_res = tokio::process::Command::new(&python_bin)
                                        .arg(&converter_script)
                                        .arg(&tmp_video)
                                        .arg(&target)
                                        .status()
                                        .await;
                                    if let Ok(c_status) = conv_res {
                                        if c_status.success() && target.exists() {
                                            converted = true;
                                        }
                                    }
                                }

                                if !converted {
                                    #[cfg(target_os = "macos")]
                                    {
                                        let mut m4a_path = target.clone();
                                        m4a_path.set_extension("m4a");
                                        let _ = tokio::process::Command::new("afconvert")
                                            .arg("-f").arg("m4af")
                                            .arg("-d").arg("aac")
                                            .arg(&tmp_video)
                                            .arg(&m4a_path)
                                            .status().await;

                                        if target != m4a_path && !target.exists() && m4a_path.exists() {
                                            let _ = std::fs::copy(&m4a_path, &target);
                                        }
                                    }
                                }

                                let _ = std::fs::remove_file(&tmp_video);
                            }

                            remove_task_target(&task_id);
                            let file_len = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
                            emit_simple_telemetry(&app, &task_id, file_len, file_len, 0, 100.0, Some(0), "completed", &target, None);
                            return;
                        }
                    }
                }
            }

            unregister_active_download(&task_id);

            // Emitted if video stream extraction failed
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
                Some("Gagal memproses stream video/audio. Pastikan link dapat diakses.".to_string()),
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
        if let Some(res) = inspect_video_url_with_options(
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
            return Ok(res);
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
    #[allow(unreachable_code)]
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
            Ok("sleep".to_string())
        }
        "quit" => {
            app.exit(0);
            Ok("quit".to_string())
        }
        other => Err(format!("Aksi pasca-unduhan tidak dikenal: {}", other)),
    }
}

#[tauri::command]
fn get_autostart_status() -> bool {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let plist = home.join("Library/LaunchAgents/com.moesstudio.downloadcepat.plist");
            return plist.exists();
        }
    }
    false
}

#[tauri::command]
fn set_autostart(enable: bool) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
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
    }
    Ok(false)
}

const KNOWN_CHROMIUM_EXTENSION_IDS: &[&str] = &[
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

    let browser_roots = [
        home.join("Library/Application Support/Google/Chrome"),
        home.join("Library/Application Support/Microsoft Edge"),
        home.join("Library/Application Support/BraveSoftware/Brave-Browser"),
        home.join("Library/Application Support/Chromium"),
        home.join("Library/Application Support/Vivaldi"),
    ];

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
    let mac_installed = std::path::Path::new("/Applications/Download Cepat.app/Contents/MacOS/download-cepat-app");
    if mac_installed.exists() {
        return mac_installed.display().to_string();
    }
    if let Ok(exe) = std::env::current_exe() {
        if exe.exists() {
            return exe.display().to_string();
        }
    }
    mac_installed.display().to_string()
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

/// Writes valid Native Messaging host manifests for Chrome, Edge, Brave, and Firefox.
pub fn sync_native_messaging_hosts() -> Result<usize, String> {
    let home = dirs::home_dir().ok_or("Gagal mendeteksi direktori home")?;
    let host_path = resolve_native_host_binary_path();
    let ext_ids = collect_installed_extension_ids(&home);
    let chromium_manifest = build_chromium_manifest_json(&host_path, &ext_ids);
    let firefox_manifest = build_firefox_manifest_json(&host_path);

    let chromium_dirs = [
        home.join("Library/Application Support/Google/Chrome/NativeMessagingHosts"),
        home.join("Library/Application Support/Microsoft Edge/NativeMessagingHosts"),
        home.join("Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts"),
    ];

    let mut installed_count = 0;
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
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(25)));
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
    Ok(format!(
        "Konektor native berhasil dipasang ke {} browser (Chrome, Edge, Brave, Firefox).",
        installed_count
    ))
}

#[tauri::command]
fn open_extension_folder() -> Result<String, String> {
    let candidates = [
        PathBuf::from("/Users/mustamar/Documents/APLIKASI/Download Cepat/extension"),
        resolve_path("~/Documents/APLIKASI/Download Cepat/extension"),
    ];

    for path in &candidates {
        if path.exists() {
            let _ = open::that(path);
            return Ok(format!("Folder ekstensi dibuka di Finder: {}", path.display()));
        }
    }

    Err("Folder ekstensi tidak ditemukan.".to_string())
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

                        if !target_url.is_empty() {
                            if let Some(res) = inspect_video_url_with_headers(
                                &target_url,
                                inspect_cookie.as_deref(),
                                inspect_referer.as_deref(),
                                inspect_ua.as_deref(),
                            ).await {
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
                        }

                        let resp_body = serde_json::json!({
                            "status": "error",
                            "message": "Stream video tidak dapat dianalisis."
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
                                let is_audio = v["isAudioOnly"].as_bool().unwrap_or(false) || ext == "mp3" || ext == "m4a";
                                let conn_count = v["connections"].as_u64().or_else(|| v["numConnections"].as_u64()).unwrap_or(8) as usize;
                                let cookie_opt = v["cookie"].as_str().filter(|s| !s.trim().is_empty()).map(String::from);
                                let referer_opt = v["referer"].as_str().or_else(|| v["referrer"].as_str()).filter(|s| !s.trim().is_empty()).map(String::from);
                                let ua_opt = v["userAgent"].as_str().filter(|s| !s.trim().is_empty()).map(String::from);

                                let clean_title: String = raw_title.chars().map(|c| match c {
                                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                                    other => other,
                                }).collect();
                                let clean_title = if clean_title.trim().is_empty() { "Video".to_string() } else { clean_title.trim().to_string() };

                                let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                                let task_id = format!("task-{}", now_ms);
                                let file_name = format!("{}.{}", clean_title, ext);
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
            get_build_info,
            set_tray_language
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
            #[cfg(target_os = "windows")]
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let _ = (app_handle, event);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_inspect_video_url_live() {
        let url = "https://www.youtube.com/watch?v=Jdj13fGMmmA";
        let res = inspect_video_url(url).await;
        assert!(res.is_some(), "inspect_video_url should return a result for YouTube video");
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
        assert!(res_hdr.is_some(), "YouTube inspect with Chrome headers should succeed");
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
            &["gbeagmdkkhbopfcggmeffpbeblalfcpk".to_string()],
        );
        assert!(!chromium_json.contains("chrome-extension://*/"));
        assert!(chromium_json.contains("chrome-extension://gbeagmdkkhbopfcggmeffpbeblalfcpk/"));

        let firefox_json = build_firefox_manifest_json("/Applications/Download Cepat.app/Contents/MacOS/download-cepat-app");
        assert!(firefox_json.contains("allowed_extensions"));

        let _ = std::fs::remove_dir_all(&test_dir);
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
        assert_eq!(info.version, "1.0.0");

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
}
