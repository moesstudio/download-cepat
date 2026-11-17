use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use flate2::read::{DeflateDecoder, GzDecoder};
use sha2::{Digest, Sha256};
use tokio::sync::{watch, Mutex};

use crate::model::InspectResult;

pub const DEFAULT_INSPECT_CACHE_TTL_SECS: u64 = 90;
pub const DEFAULT_INSPECT_CACHE_MAX_ENTRIES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YtdlpOnedirSpec {
    pub url: &'static str,
    pub bin_names: &'static [&'static str],
}

/// Returns unpacked PyInstaller `--onedir` yt-dlp ZIP archive metadata for platforms where
/// unpacking eliminates per-invocation `_MEI*` temp extraction and AV scanning bottlenecks.
pub fn ytdlp_onedir_spec(os: &str, arch: &str) -> Option<YtdlpOnedirSpec> {
    match (os, arch) {
        ("windows", "x86_64") => Some(YtdlpOnedirSpec {
            url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_win.zip",
            bin_names: &["yt-dlp"],
        }),
        ("windows", "aarch64") => Some(YtdlpOnedirSpec {
            url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_win_arm64.zip",
            bin_names: &["yt-dlp_arm64", "yt-dlp"],
        }),
        ("windows", "x86") => Some(YtdlpOnedirSpec {
            url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_win_x86.zip",
            bin_names: &["yt-dlp_x86", "yt-dlp"],
        }),
        ("linux", "x86_64") => Some(YtdlpOnedirSpec {
            url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux.zip",
            bin_names: &["yt-dlp_linux", "yt-dlp"],
        }),
        ("linux", "aarch64") => Some(YtdlpOnedirSpec {
            url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64.zip",
            bin_names: &["yt-dlp_linux_aarch64", "yt-dlp_linux", "yt-dlp"],
        }),
        ("linux", "arm") => Some(YtdlpOnedirSpec {
            url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_armv7l.zip",
            bin_names: &["yt-dlp_linux_armv7l", "yt-dlp_linux", "yt-dlp"],
        }),
        _ => None,
    }
}

/// Standalone QuickJS-NG (`qjs`) release binaries (~1.2–2.5 MB, no archive extraction needed).
/// Enables fast YouTube EJS solving via `--js-runtimes quickjs:<path>` without downloading
/// Node/Deno or falling back to slow `youtube:player_client=all`.
pub fn quickjs_download_url_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") | ("windows", "aarch64") => {
            Some("https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-windows-x86_64.exe")
        }
        ("windows", "x86") => {
            Some("https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-windows-x86.exe")
        }
        ("linux", "x86_64") => {
            Some("https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-linux-x86_64")
        }
        ("linux", "aarch64") => {
            Some("https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-linux-aarch64")
        }
        ("linux", "arm") => {
            Some("https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-linux-armv7")
        }
        ("linux", "x86") => {
            Some("https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-linux-x86")
        }
        ("macos", "aarch64") => {
            Some("https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-darwin-arm64")
        }
        ("macos", "x86_64") => {
            Some("https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-darwin-x86_64")
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FfmpegAssetSpec {
    pub url: &'static str,
    pub min_compressed_bytes: u64,
    pub min_extracted_bytes: u64,
}

/// Static FFmpeg 6.1.1 gzip-compressed release binaries from `eugeneware/ffmpeg-static`.
/// Supplies split-stream HD/4K/8K muxing (`vp9`/`av01`/`avc1` + `m4a`/`opus`) and audio
/// transcoding when system FFmpeg is absent (e.g., fresh Windows or Linux installs).
pub fn ffmpeg_download_spec(os: &str, arch: &str) -> Option<FfmpegAssetSpec> {
    match (os, arch) {
        ("windows", "x86_64") | ("windows", "aarch64") => Some(FfmpegAssetSpec {
            url: "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-win32-x64.gz",
            min_compressed_bytes: 5_000_000,
            min_extracted_bytes: 10_000_000,
        }),
        ("macos", "aarch64") => Some(FfmpegAssetSpec {
            url: "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-darwin-arm64.gz",
            min_compressed_bytes: 5_000_000,
            min_extracted_bytes: 10_000_000,
        }),
        ("macos", "x86_64") => Some(FfmpegAssetSpec {
            url: "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-darwin-x64.gz",
            min_compressed_bytes: 5_000_000,
            min_extracted_bytes: 10_000_000,
        }),
        ("linux", "x86_64") => Some(FfmpegAssetSpec {
            url: "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-linux-x64.gz",
            min_compressed_bytes: 5_000_000,
            min_extracted_bytes: 10_000_000,
        }),
        ("linux", "aarch64") => Some(FfmpegAssetSpec {
            url: "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-linux-arm64.gz",
            min_compressed_bytes: 5_000_000,
            min_extracted_bytes: 10_000_000,
        }),
        ("linux", "arm") => Some(FfmpegAssetSpec {
            url: "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-linux-arm.gz",
            min_compressed_bytes: 5_000_000,
            min_extracted_bytes: 10_000_000,
        }),
        ("linux", "x86") => Some(FfmpegAssetSpec {
            url: "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-linux-ia32.gz",
            min_compressed_bytes: 5_000_000,
            min_extracted_bytes: 10_000_000,
        }),
        _ => None,
    }
}

/// Decompresses a `.gz` single-file release asset into `dest_path`, verifying gzip CRC/size trailer.
pub fn extract_gzip_file(gz_path: &Path, dest_path: &Path) -> Result<u64, String> {
    if let Some(parent) = dest_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "Gagal membuat folder tujuan ekstraksi ({}): {}",
                    parent.display(),
                    e
                )
            })?;
        }
    }

    let in_file = std::fs::File::open(gz_path)
        .map_err(|e| format!("Gagal membuka arsip GZIP {}: {}", gz_path.display(), e))?;
    let mut decoder = GzDecoder::new(std::io::BufReader::new(in_file));
    let out_file = std::fs::File::create(dest_path).map_err(|e| {
        format!(
            "Gagal membuat berkas hasil ekstraksi {}: {}",
            dest_path.display(),
            e
        )
    })?;
    let mut writer = BufWriter::new(out_file);

    let mut written: u64 = 0;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = match decoder.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                drop(writer);
                let _ = std::fs::remove_file(dest_path);
                return Err(format!(
                    "Gagal mendekompresi arsip GZIP {}: {}",
                    gz_path.display(),
                    e
                ));
            }
        };
        if let Err(e) = writer.write_all(&buf[..n]) {
            drop(writer);
            let _ = std::fs::remove_file(dest_path);
            return Err(format!(
                "Gagal menulis berkas {}: {}",
                dest_path.display(),
                e
            ));
        }
        written = written.saturating_add(n as u64);
    }

    if let Err(e) = writer.flush() {
        drop(writer);
        let _ = std::fs::remove_file(dest_path);
        return Err(format!("Gagal flush berkas {}: {}", dest_path.display(), e));
    }

    Ok(written)
}

/// Parses `node --version` output (e.g. `"v22.14.0\n"`) into major version integer.
pub fn parse_node_major_version(raw: &str) -> Option<u32> {
    let clean = raw.trim().trim_start_matches(['v', 'V']);
    let major_str = clean.split('.').next()?.trim();
    major_str.parse::<u32>().ok()
}

/// Fast fallback YouTube extractor args used only when JS solving failed or is unavailable.
/// Replaces the 15-25s `youtube:player_client=all` sweep with 3 high-yield clients.
pub fn youtube_fallback_extractor_args() -> &'static str {
    "youtube:player_client=android_vr,web_safari,default"
}

fn is_youtube_host(host: &str) -> bool {
    let h = host.trim().to_ascii_lowercase();
    h == "youtube.com"
        || h.ends_with(".youtube.com")
        || h == "youtu.be"
        || h == "youtube-nocookie.com"
        || h.ends_with(".youtube-nocookie.com")
}

fn extract_youtube_video_id(parsed: &url::Url) -> Option<String> {
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !is_youtube_host(&host) {
        return None;
    }

    if host == "youtu.be" {
        let id = parsed
            .path_segments()?
            .next()
            .unwrap_or("")
            .trim();
        if !id.is_empty() && id.len() >= 6 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Some(id.to_string());
        }
        return None;
    }

    let path = parsed.path();
    if path == "/watch" || path == "/watch/" {
        for (k, v) in parsed.query_pairs() {
            if k == "v" {
                let id = v.trim();
                if !id.is_empty() && id.len() >= 6 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                    return Some(id.to_string());
                }
            }
        }
    }

    let segs: Vec<&str> = parsed.path_segments()?.filter(|s| !s.is_empty()).collect();
    if segs.len() >= 2 && matches!(segs[0], "shorts" | "live" | "embed" | "v") {
        let id = segs[1].trim();
        if !id.is_empty() && id.len() >= 6 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Some(id.to_string());
        }
    }

    None
}

fn is_tracking_query_param(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.starts_with("utm_")
        || matches!(
            k.as_str(),
            "si" | "pp" | "fbclid" | "gclid" | "igshid" | "feature" | "ab_channel"
        )
}

/// Canonicalizes video page URLs so identical targets inspected across hover pre-warm,
/// modal click, or HTTP -> Native Messaging fallback hit the exact same cache/singleflight slot.
pub fn canonicalize_video_url(raw_url: &str) -> String {
    let trimmed = raw_url.trim();
    let Ok(mut parsed) = url::Url::parse(trimmed) else {
        return trimmed.to_string();
    };

    if let Some(yt_id) = extract_youtube_video_id(&parsed) {
        return format!("youtube:v:{}", yt_id);
    }

    parsed.set_fragment(None);
    let kept_pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| !is_tracking_query_param(k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept_pairs.is_empty() {
        parsed.set_query(None);
    } else {
        let mut ser = url::form_urlencoded::Serializer::new(String::new());
        let mut sorted = kept_pairs;
        sorted.sort();
        for (k, v) in sorted {
            ser.append_pair(&k, &v);
        }
        let query_str = ser.finish();
        parsed.set_query(Some(&query_str));
    }
    parsed.to_string()
}

/// Computes a collision-free SHA-256 fingerprint over request headers/proxy/auth so
/// cached results never leak across authenticated vs anonymous inspect contexts.
pub fn inspect_options_fingerprint(
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
    proxy_url: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    let fields: [(&str, Option<&str>); 6] = [
        ("ck", cookie),
        ("rf", referer),
        ("ua", user_agent),
        ("px", proxy_url),
        ("au", auth_username),
        ("ap", auth_password),
    ];
    for (tag, val) in fields {
        let cleaned = val.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");
        hasher.update(tag.as_bytes());
        hasher.update(b":");
        hasher.update((cleaned.len() as u64).to_le_bytes());
        hasher.update(cleaned.as_bytes());
    }
    hex::encode(hasher.finalize())
}

pub fn rewrite_inspect_result_url(mut res: InspectResult, caller_url: &str) -> InspectResult {
    let prev_url = res.url.clone();
    res.url = caller_url.to_string();
    if res.final_url == prev_url || res.final_url.is_empty() {
        res.final_url = caller_url.to_string();
    }
    if let Some(ref mut media) = res.media_info {
        for fmt in &mut media.formats {
            if fmt.download_url == prev_url || fmt.download_url.is_empty() {
                fmt.download_url = caller_url.to_string();
            }
        }
    }
    res
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InspectCacheKey {
    canonical_url: String,
    options_fingerprint: String,
}

impl InspectCacheKey {
    pub fn new(raw_url: &str, options_fingerprint: &str) -> Self {
        Self {
            canonical_url: canonicalize_video_url(raw_url),
            options_fingerprint: options_fingerprint.to_string(),
        }
    }
}

#[derive(Clone)]
struct CachedInspectEntry {
    result: InspectResult,
    inserted_at: Instant,
}

struct InspectCacheState {
    entries: HashMap<InspectCacheKey, CachedInspectEntry>,
    order: VecDeque<InspectCacheKey>,
    ttl: Duration,
    max_entries: usize,
}

impl InspectCacheState {
    fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            ttl,
            max_entries: max_entries.max(1),
        }
    }

    fn purge_expired(&mut self, now: Instant) {
        let ttl = self.ttl;
        self.entries.retain(|_, v| now.saturating_duration_since(v.inserted_at) <= ttl);
        let entries = &self.entries;
        self.order.retain(|k| entries.contains_key(k));
    }

    fn get(&mut self, key: &InspectCacheKey) -> Option<InspectResult> {
        let now = Instant::now();
        self.purge_expired(now);
        self.entries.get(key).map(|e| e.result.clone())
    }

    fn insert(&mut self, key: InspectCacheKey, result: InspectResult) {
        let now = Instant::now();
        self.purge_expired(now);
        if self.entries.contains_key(&key) {
            self.order.retain(|k| k != &key);
        }
        while self.entries.len() >= self.max_entries {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
        self.order.push_back(key.clone());
        self.entries.insert(
            key,
            CachedInspectEntry {
                result,
                inserted_at: now,
            },
        );
    }
}

struct InFlightSlot {
    rx: watch::Receiver<Option<Result<InspectResult, String>>>,
}

struct InFlightCleanupGuard {
    key: Option<InspectCacheKey>,
    in_flight: Arc<Mutex<HashMap<InspectCacheKey, Arc<InFlightSlot>>>>,
    tx: watch::Sender<Option<Result<InspectResult, String>>>,
    finished: bool,
}

impl Drop for InFlightCleanupGuard {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.tx.send(Some(Err("Inspeksi video dibatalkan.".to_string())));
        }
        if let Some(key) = self.key.take() {
            let in_flight = self.in_flight.clone();
            let removed_sync = match in_flight.try_lock() {
                Ok(mut guard) => {
                    guard.remove(&key);
                    true
                }
                Err(_) => false,
            };
            if !removed_sync {
                tokio::spawn(async move {
                    let mut guard = in_flight.lock().await;
                    guard.remove(&key);
                });
            }
        }
    }
}

/// Coordinates video metadata inspection with a short TTL cache and singleflight
/// deduplication across simultaneous callers (e.g., hover pre-warm, click, HTTP & native fallback).
pub struct VideoInspectCoordinator {
    cache: Arc<Mutex<InspectCacheState>>,
    in_flight: Arc<Mutex<HashMap<InspectCacheKey, Arc<InFlightSlot>>>>,
}

impl VideoInspectCoordinator {
    pub fn new() -> Self {
        Self::with_ttl_and_capacity(
            Duration::from_secs(DEFAULT_INSPECT_CACHE_TTL_SECS),
            DEFAULT_INSPECT_CACHE_MAX_ENTRIES,
        )
    }

    pub fn with_ttl_and_capacity(ttl: Duration, max_entries: usize) -> Self {
        Self {
            cache: Arc::new(Mutex::new(InspectCacheState::new(ttl, max_entries))),
            in_flight: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn clear(&self) {
        self.cache.lock().await.entries.clear();
        self.cache.lock().await.order.clear();
    }

    pub async fn cached_len(&self) -> usize {
        let mut cache = self.cache.lock().await;
        cache.purge_expired(Instant::now());
        cache.entries.len()
    }

    pub async fn get_or_inspect<F, Fut>(
        &self,
        raw_url: &str,
        options_fingerprint: &str,
        fetcher: F,
    ) -> Result<InspectResult, String>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<InspectResult, String>> + Send + 'static,
    {
        let key = InspectCacheKey::new(raw_url, options_fingerprint);

        if let Some(hit) = self.cache.lock().await.get(&key) {
            return Ok(rewrite_inspect_result_url(hit, raw_url));
        }

        let mut rx = {
            let mut map = self.in_flight.lock().await;

            // Double-check cache after lock ordering in case another worker just finished.
            if let Some(hit) = self.cache.lock().await.get(&key) {
                return Ok(rewrite_inspect_result_url(hit, raw_url));
            }

            if let Some(existing) = map.get(&key) {
                existing.rx.clone()
            } else {
                let (tx, rx) = watch::channel(None);
                let slot = Arc::new(InFlightSlot { rx: rx.clone() });
                map.insert(key.clone(), slot);

                let cache = self.cache.clone();
                let in_flight = self.in_flight.clone();
                let cleanup_key = key.clone();

                tokio::spawn(async move {
                    let mut guard = InFlightCleanupGuard {
                        key: Some(cleanup_key.clone()),
                        in_flight: in_flight.clone(),
                        tx: tx.clone(),
                        finished: false,
                    };

                    let result = fetcher().await;
                    if let Ok(ref res) = result {
                        let mut cache_guard = cache.lock().await;
                        cache_guard.insert(cleanup_key.clone(), res.clone());
                    }

                    {
                        let mut map_guard = in_flight.lock().await;
                        map_guard.remove(&cleanup_key);
                    }
                    guard.key = None;
                    guard.finished = true;
                    let _ = tx.send(Some(result));
                });

                rx
            }
        };

        loop {
            if let Some(res) = rx.borrow().clone() {
                return res.map(|r| rewrite_inspect_result_url(r, raw_url));
            }
            if rx.changed().await.is_err() {
                return Err("Gagal menerima hasil inspeksi video.".to_string());
            }
        }
    }
}

impl Default for VideoInspectCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

fn read_u16_le(slice: &[u8], offset: usize) -> Result<u16, String> {
    let bytes: [u8; 2] = slice
        .get(offset..offset + 2)
        .ok_or_else(|| "Arsip ZIP terpotong (u16).".to_string())?
        .try_into()
        .map_err(|_| "Arsip ZIP tidak valid (u16).".to_string())?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32_le(slice: &[u8], offset: usize) -> Result<u32, String> {
    let bytes: [u8; 4] = slice
        .get(offset..offset + 4)
        .ok_or_else(|| "Arsip ZIP terpotong (u32).".to_string())?
        .try_into()
        .map_err(|_| "Arsip ZIP tidak valid (u32).".to_string())?;
    Ok(u32::from_le_bytes(bytes))
}

fn sanitize_zip_entry_path(raw_name: &str) -> Result<PathBuf, String> {
    let normalized = raw_name.replace('\\', "/");
    let mut clean = PathBuf::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return Err(format!("Jalur entri ZIP tidak aman ditolak: {}", raw_name));
        }
        let p = Path::new(part);
        match p.components().next() {
            Some(Component::Normal(_)) => clean.push(part),
            _ => return Err(format!("Komponen jalur ZIP tidak valid: {}", raw_name)),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err("Entri jalur ZIP kosong.".to_string());
    }
    Ok(clean)
}

/// Pure-Rust ZIP extractor supporting Stored (0) and Deflate (8) entries with CRC32
/// verification, ZIP-slip rejection, and Unix file permission restoration.
pub fn extract_zip_archive(zip_path: &Path, dest_dir: &Path) -> Result<usize, String> {
    let mut file = std::fs::File::open(zip_path)
        .map_err(|e| format!("Gagal membuka arsip ZIP {}: {}", zip_path.display(), e))?;
    let total_len = file
        .metadata()
        .map_err(|e| format!("Gagal membaca ukuran arsip ZIP: {}", e))?
        .len();
    if total_len < 22 {
        return Err("Arsip ZIP tidak lengkap (< 22 byte).".to_string());
    }

    // Search backwards within max comment window (64 KB + 22 bytes) for PK\x05\x06 EOCD.
    let tail_len = total_len.min(65_557) as usize;
    let tail_start = total_len - tail_len as u64;
    file.seek(SeekFrom::Start(tail_start))
        .map_err(|e| format!("Gagal membaca footer ZIP: {}", e))?;
    let mut tail_buf = vec![0u8; tail_len];
    file.read_exact(&mut tail_buf)
        .map_err(|e| format!("Gagal membaca EOCD ZIP: {}", e))?;

    let mut eocd_idx: Option<usize> = None;
    for i in (0..=tail_len.saturating_sub(22)).rev() {
        if tail_buf[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
            let comment_len = u16::from_le_bytes([tail_buf[i + 20], tail_buf[i + 21]]) as usize;
            if i + 22 + comment_len == tail_len {
                eocd_idx = Some(i);
                break;
            }
        }
    }
    let eocd_pos = eocd_idx.ok_or_else(|| "Signature End-Of-Central-Directory ZIP tidak ditemukan.".to_string())?;
    let eocd = &tail_buf[eocd_pos..];

    let total_entries = read_u16_le(eocd, 10)? as usize;
    let cd_size = read_u32_le(eocd, 12)? as usize;
    let cd_offset = read_u32_le(eocd, 16)? as u64;
    if total_entries == u16::MAX as usize || cd_size as u32 == u32::MAX || cd_offset as u32 == u32::MAX {
        return Err("Arsip ZIP64 tidak didukung oleh ekstraktor internal.".to_string());
    }
    if cd_offset + cd_size as u64 > total_len {
        return Err("Central Directory ZIP rusak atau terpotong.".to_string());
    }

    file.seek(SeekFrom::Start(cd_offset))
        .map_err(|e| format!("Gagal menuju Central Directory ZIP: {}", e))?;
    let mut cd_buf = vec![0u8; cd_size];
    file.read_exact(&mut cd_buf)
        .map_err(|e| format!("Gagal membaca Central Directory ZIP: {}", e))?;

    std::fs::create_dir_all(dest_dir)
        .map_err(|e| format!("Gagal membuat folder tujuan ZIP ({}): {}", dest_dir.display(), e))?;

    let mut extracted_files = 0usize;
    let mut offset = 0usize;
    for _ in 0..total_entries {
        if offset + 46 > cd_buf.len() {
            return Err("Header Central Directory ZIP terpotong.".to_string());
        }
        let hdr = &cd_buf[offset..];
        if read_u32_le(hdr, 0)? != 0x0201_4b50 {
            return Err("Signature entri Central Directory ZIP tidak valid.".to_string());
        }

        let version_made_by = read_u16_le(hdr, 4)?;
        let flags = read_u16_le(hdr, 8)?;
        let method = read_u16_le(hdr, 10)?;
        let expected_crc = read_u32_le(hdr, 16)?;
        let comp_size = read_u32_le(hdr, 20)? as u64;
        let uncomp_size = read_u32_le(hdr, 24)? as u64;
        let name_len = read_u16_le(hdr, 28)? as usize;
        let extra_len = read_u16_le(hdr, 30)? as usize;
        let comment_len = read_u16_le(hdr, 32)? as usize;
        let external_attr = read_u32_le(hdr, 38)?;
        let local_header_offset = read_u32_le(hdr, 42)? as u64;

        let name_start = offset + 46;
        let name_end = name_start + name_len;
        let next_offset = name_end + extra_len + comment_len;
        if next_offset > cd_buf.len() {
            return Err("Nama entri ZIP melebihi batas Central Directory.".to_string());
        }
        if flags & 0x0001 != 0 {
            return Err("Arsip ZIP terenkripsi tidak didukung.".to_string());
        }

        let raw_name = String::from_utf8_lossy(&cd_buf[name_start..name_end]).into_owned();
        offset = next_offset;

        if raw_name.ends_with('/') || raw_name.ends_with('\\') {
            if let Ok(rel_dir) = sanitize_zip_entry_path(&raw_name) {
                let target_dir = dest_dir.join(rel_dir);
                std::fs::create_dir_all(&target_dir).map_err(|e| {
                    format!("Gagal membuat subfolder ZIP ({}): {}", target_dir.display(), e)
                })?;
            }
            continue;
        }

        let rel_path = sanitize_zip_entry_path(&raw_name)?;
        let target_file = dest_dir.join(&rel_path);
        if let Some(parent) = target_file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!("Gagal membuat folder induk ZIP ({}): {}", parent.display(), e)
            })?;
        }

        if local_header_offset + 30 > total_len {
            return Err(format!("Local file header di luar batas untuk {}.", raw_name));
        }
        file.seek(SeekFrom::Start(local_header_offset))
            .map_err(|e| format!("Gagal seek local header {}: {}", raw_name, e))?;
        let mut lfh = [0u8; 30];
        file.read_exact(&mut lfh)
            .map_err(|e| format!("Gagal membaca local header {}: {}", raw_name, e))?;
        if read_u32_le(&lfh, 0)? != 0x0403_4b50 {
            return Err(format!("Signature local header ZIP tidak valid pada {}.", raw_name));
        }
        let lfh_name_len = read_u16_le(&lfh, 26)? as u64;
        let lfh_extra_len = read_u16_le(&lfh, 28)? as u64;
        let data_start = local_header_offset + 30 + lfh_name_len + lfh_extra_len;
        if data_start + comp_size > total_len {
            return Err(format!("Data terkompresi ZIP terpotong untuk {}.", raw_name));
        }
        file.seek(SeekFrom::Start(data_start))
            .map_err(|e| format!("Gagal menuju data entri ZIP {}: {}", raw_name, e))?;

        let mut reader_limited = (&mut file).take(comp_size);
        let mut out_file = BufWriter::new(
            std::fs::File::create(&target_file)
                .map_err(|e| format!("Gagal menulis berkas {}: {}", target_file.display(), e))?,
        );

        let mut hasher = crc32fast::Hasher::new();
        let mut written_bytes: u64 = 0;
        let mut buf = [0u8; 32 * 1024];

        match method {
            0 => loop {
                let n = reader_limited
                    .read(&mut buf)
                    .map_err(|e| format!("Gagal membaca entri stored {}: {}", raw_name, e))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
                out_file
                    .write_all(&buf[..n])
                    .map_err(|e| format!("Gagal menulis {}: {}", target_file.display(), e))?;
                written_bytes += n as u64;
            },
            8 => {
                let mut decoder = DeflateDecoder::new(reader_limited);
                loop {
                    let n = decoder
                        .read(&mut buf)
                        .map_err(|e| format!("Gagal mendekompresi {}: {}", raw_name, e))?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                    out_file
                        .write_all(&buf[..n])
                        .map_err(|e| format!("Gagal menulis {}: {}", target_file.display(), e))?;
                    written_bytes += n as u64;
                }
            }
            other => {
                return Err(format!(
                    "Metode kompresi ZIP {} tidak didukung pada {}.",
                    other, raw_name
                ));
            }
        }

        out_file
            .flush()
            .map_err(|e| format!("Gagal flush berkas {}: {}", target_file.display(), e))?;

        if written_bytes != uncomp_size {
            return Err(format!(
                "Ukuran hasil ekstraksi tidak cocok untuk {} (dapat {} byte, harap {} byte).",
                raw_name, written_bytes, uncomp_size
            ));
        }
        if hasher.finalize() != expected_crc {
            return Err(format!("Checksum CRC32 gagal untuk entri ZIP {}.", raw_name));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let creator = version_made_by >> 8;
            let unix_mode = (external_attr >> 16) & 0o777;
            if creator == 3 && unix_mode != 0 {
                let _ = std::fs::set_permissions(
                    &target_file,
                    std::fs::Permissions::from_mode(unix_mode),
                );
            }
        }
        #[cfg(not(unix))]
        let _ = (version_made_by, external_attr);

        extracted_files += 1;
    }

    Ok(extracted_files)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHttpStream {
    pub url: String,
    pub ext: String,
    pub vcodec: String,
    pub acodec: String,
    pub filesize: Option<u64>,
    pub height: Option<u64>,
    pub fps: Option<u64>,
    pub tbr_kbps: u64,
    pub abr_kbps: u64,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub cookie_header: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoStreamPlan {
    /// Progressive video+audio stream or single audio stream downloadable directly via HTTP Range.
    SingleStream(ResolvedHttpStream),
    /// Separate high-resolution video track and audio track to download at full concurrency and mux.
    SplitVideoAudio {
        video: ResolvedHttpStream,
        audio: ResolvedHttpStream,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompositePhaseProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub progress_percent: f64,
    pub eta_seconds: Option<u64>,
}

fn is_manifest_stream_url(raw_url: &str) -> bool {
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

fn header_lookup_ci(
    obj: Option<&serde_json::Map<String, serde_json::Value>>,
    target_key: &str,
) -> Option<String> {
    let map = obj?;
    for (k, v) in map {
        if k.eq_ignore_ascii_case(target_key) {
            if let Some(s) = v.as_str() {
                let clean = s.replace(['\r', '\n', '\0'], " ").trim().to_string();
                if !clean.is_empty() {
                    return Some(clean);
                }
            }
        }
    }
    None
}

fn parse_direct_http_stream(
    entry: &serde_json::Value,
    fallback_headers: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<ResolvedHttpStream> {
    let url = entry["url"].as_str()?.trim();
    let url_lower = url.to_ascii_lowercase();
    if !(url_lower.starts_with("http://") || url_lower.starts_with("https://")) {
        return None;
    }
    if is_manifest_stream_url(url) {
        return None;
    }

    let ext = entry["ext"].as_str().unwrap_or("").trim().to_ascii_lowercase();
    let note = entry["format_note"].as_str().unwrap_or("").to_ascii_lowercase();
    let protocol = entry["protocol"].as_str().unwrap_or("").trim().to_ascii_lowercase();

    if ext == "mhtml" || note.contains("storyboard") {
        return None;
    }
    // Only allow direct HTTP/HTTPS byte-streams (skip HLS m3u8 / DASH mpd / RTMP / etc.)
    if !matches!(protocol.as_str(), "https" | "http") {
        return None;
    }

    let vcodec = entry["vcodec"].as_str().unwrap_or("none").trim().to_string();
    let acodec = entry["acodec"].as_str().unwrap_or("none").trim().to_string();
    if vcodec == "none" && acodec == "none" {
        return None;
    }

    let entry_headers = entry["http_headers"].as_object();
    let user_agent = header_lookup_ci(entry_headers, "User-Agent")
        .or_else(|| header_lookup_ci(fallback_headers, "User-Agent"));
    let referer = header_lookup_ci(entry_headers, "Referer")
        .or_else(|| header_lookup_ci(fallback_headers, "Referer"));
    let cookie_header = header_lookup_ci(entry_headers, "Cookie")
        .or_else(|| header_lookup_ci(fallback_headers, "Cookie"));

    let filesize = entry["filesize"]
        .as_u64()
        .or_else(|| entry["filesize_approx"].as_u64())
        .filter(|&n| n > 0);
    let height = entry["height"].as_u64().filter(|&h| h > 0);
    let fps = entry["fps"].as_f64().map(|f| f.round() as u64).filter(|&f| f > 0);
    let tbr_kbps = entry["tbr"]
        .as_f64()
        .map(|v| (v * 100.0).round() as u64)
        .unwrap_or(0);
    let abr_kbps = entry["abr"]
        .as_f64()
        .map(|v| (v * 100.0).round() as u64)
        .unwrap_or(0);

    Some(ResolvedHttpStream {
        url: url.to_string(),
        ext,
        vcodec,
        acodec,
        filesize,
        height,
        fps,
        tbr_kbps,
        abr_kbps,
        user_agent,
        referer,
        cookie_header,
    })
}

fn parse_requested_height(format_id: Option<&str>) -> Option<u64> {
    let fid = format_id?.trim();
    if let Some(h) = fid.strip_suffix("p_mp4") {
        return h.parse::<u64>().ok().filter(|&v| v > 0);
    }
    if let Some(h) = fid.strip_suffix('p') {
        return h.parse::<u64>().ok().filter(|&v| v > 0);
    }
    None
}

fn select_best_audio_stream(
    streams: &[ResolvedHttpStream],
    prefer_m4a: bool,
) -> Option<ResolvedHttpStream> {
    streams
        .iter()
        .filter(|s| s.acodec != "none")
        .max_by_key(|s| {
            let pure_audio = u8::from(s.vcodec == "none");
            let m4a_match = u8::from(
                prefer_m4a
                    && (s.ext == "m4a"
                        || s.acodec.to_ascii_lowercase().starts_with("mp4a")),
            );
            let known_size = u8::from(s.filesize.is_some());
            (
                pure_audio,
                m4a_match,
                s.abr_kbps,
                s.tbr_kbps,
                known_size,
                s.filesize.unwrap_or(0),
            )
        })
        .cloned()
}

fn video_container_compatibility_score(s: &ResolvedHttpStream, target_ext: &str) -> u8 {
    let vlow = s.vcodec.to_ascii_lowercase();
    if target_ext.eq_ignore_ascii_case("mp4") {
        if s.ext == "mp4" && vlow.starts_with("avc") {
            5
        } else if s.ext == "mp4" {
            4
        } else if s.ext == "webm" && !vlow.starts_with("vp8") {
            3
        } else if s.ext == "webm" {
            1
        } else {
            2
        }
    } else if s.ext.eq_ignore_ascii_case(target_ext) {
        5
    } else {
        2
    }
}

fn is_playable_video_entry(f: &serde_json::Value) -> bool {
    let ext = f["ext"].as_str().unwrap_or("").trim();
    let protocol = f["protocol"].as_str().unwrap_or("").trim();
    let note = f["format_note"].as_str().unwrap_or("");
    let vcodec = f["vcodec"].as_str().unwrap_or("none").trim();
    !ext.eq_ignore_ascii_case("mhtml")
        && !protocol.to_ascii_lowercase().contains("mhtml")
        && !note.to_ascii_lowercase().contains("storyboard")
        && vcodec != "none"
        && f["height"].as_u64().unwrap_or(0) > 0
}

fn max_playable_video_height_under_cap(
    formats_arr: &[serde_json::Value],
    cap: Option<u64>,
) -> Option<u64> {
    let all_heights: Vec<u64> = formats_arr
        .iter()
        .filter(|f| is_playable_video_entry(f))
        .filter_map(|f| f["height"].as_u64().filter(|&h| h > 0))
        .collect();
    if all_heights.is_empty() {
        return None;
    }
    match cap {
        Some(c) => all_heights.iter().copied().filter(|&h| h <= c).max(),
        None => all_heights.into_iter().max(),
    }
}

/// Returns `true` when the requested video resolution/format exists in `json` only as a split
/// video-only stream paired with separate audio (requiring FFmpeg muxing), so callers must not
/// silently downgrade a high-resolution request (e.g. 4K/1080p) to 360p progressive if FFmpeg fails.
pub fn requested_height_requires_split(
    json: &serde_json::Value,
    format_id: Option<&str>,
) -> bool {
    let clean_fid = format_id.map(str::trim).filter(|s| !s.is_empty());
    if matches!(clean_fid, Some("audio_mp3") | Some("audio_m4a")) {
        return false;
    }

    if let Some(formats_arr) = json["formats"].as_array().filter(|a| !a.is_empty()) {
        let has_audio_only = formats_arr.iter().any(|f| {
            let ext = f["ext"].as_str().unwrap_or("").trim();
            let protocol = f["protocol"].as_str().unwrap_or("").trim();
            !ext.eq_ignore_ascii_case("mhtml")
                && !protocol.to_ascii_lowercase().contains("mhtml")
                && f["vcodec"].as_str().unwrap_or("none").trim() == "none"
                && f["acodec"].as_str().unwrap_or("none").trim() != "none"
        });
        if !has_audio_only {
            return false;
        }

        let requested_max_h = parse_requested_height(clean_fid);
        if let (Some(fid), None) = (clean_fid, requested_max_h) {
            if let Some(raw_entry) = formats_arr
                .iter()
                .find(|f| f["format_id"].as_str() == Some(fid))
            {
                let vcodec = raw_entry["vcodec"].as_str().unwrap_or("none").trim();
                let acodec = raw_entry["acodec"].as_str().unwrap_or("none").trim();
                return vcodec != "none" && acodec == "none";
            }
        }

        if let Some(target_h) = max_playable_video_height_under_cap(formats_arr, requested_max_h) {
            let has_progressive_at_target = formats_arr.iter().any(|f| {
                is_playable_video_entry(f)
                    && f["height"].as_u64() == Some(target_h)
                    && f["acodec"].as_str().unwrap_or("none").trim() != "none"
            });
            return !has_progressive_at_target;
        }
        return false;
    }

    if let Some(req_arr) = json["requested_formats"].as_array() {
        if req_arr.len() >= 2 {
            let has_v = req_arr
                .iter()
                .any(|f| f["vcodec"].as_str().unwrap_or("none").trim() != "none");
            let has_a = req_arr.iter().any(|f| {
                f["vcodec"].as_str().unwrap_or("none").trim() == "none"
                    && f["acodec"].as_str().unwrap_or("none").trim() != "none"
            });
            return has_v && has_a;
        }
    }

    false
}

/// Inspects a `yt-dlp --dump-single-json` document and resolves direct HTTP(S) progressive or
/// split video+audio track URLs so `core_engine::Downloader` can fetch them with up to 32
/// concurrent HTTP `Range` connections instead of falling back to single-socket `yt-dlp`.
/// Returns `None` for HLS/DASH manifests or when split tracks require missing FFmpeg.
pub fn extract_direct_stream_plan(
    json: &serde_json::Value,
    format_id: Option<&str>,
    target_ext: &str,
    want_audio_only: bool,
    allow_split: bool,
) -> Option<VideoStreamPlan> {
    let root_headers = json["http_headers"].as_object();
    let clean_target_ext = target_ext.trim().to_ascii_lowercase();
    let effective_target_ext = if clean_target_ext.is_empty() {
        "mp4"
    } else {
        clean_target_ext.as_str()
    };
    let requested_max_h = parse_requested_height(format_id);
    let audio_mode = want_audio_only
        || matches!(format_id, Some("audio_mp3") | Some("audio_m4a"));
    let expected_max_h = json["formats"]
        .as_array()
        .filter(|a| !a.is_empty() && !audio_mode)
        .and_then(|arr| max_playable_video_height_under_cap(arr, requested_max_h));

    // 1. Select cleanly from full `formats` array first so cached inspect metadata
    // honors the user's exact format/resolution choice (rather than yt-dlp's default requested_formats).
    if let Some(formats_arr) = json["formats"].as_array().filter(|a| !a.is_empty()) {
        let direct_streams: Vec<ResolvedHttpStream> = formats_arr
            .iter()
            .filter_map(|f| parse_direct_http_stream(f, root_headers))
            .collect();

        if !direct_streams.is_empty() {
            let prefer_m4a = effective_target_ext == "mp4"
                || effective_target_ext == "m4a"
                || format_id == Some("audio_m4a");

            if audio_mode {
                if let Some(best_audio) = select_best_audio_stream(
                    &direct_streams,
                    format_id == Some("audio_m4a") || effective_target_ext == "m4a",
                ) {
                    return Some(VideoStreamPlan::SingleStream(best_audio));
                }
            } else {
                // Exact custom format ID lookup if caller passed raw yt-dlp format id (e.g. "22", "137")
                if let Some(fid) = format_id.map(str::trim).filter(|s| !s.is_empty()) {
                    if requested_max_h.is_none() {
                        if let Some(raw_entry) = formats_arr
                            .iter()
                            .find(|f| f["format_id"].as_str() == Some(fid))
                        {
                            let Some(raw_match) = parse_direct_http_stream(raw_entry, root_headers)
                            else {
                                return None;
                            };
                            if raw_match.acodec != "none" && raw_match.vcodec != "none" {
                                return Some(VideoStreamPlan::SingleStream(raw_match));
                            } else if raw_match.vcodec != "none" {
                                if allow_split {
                                    if let Some(audio) =
                                        select_best_audio_stream(&direct_streams, prefer_m4a)
                                    {
                                        return Some(VideoStreamPlan::SplitVideoAudio {
                                            video: raw_match,
                                            audio,
                                        });
                                    }
                                }
                                return None;
                            } else if raw_match.acodec != "none" {
                                return Some(VideoStreamPlan::SingleStream(raw_match));
                            }
                            return None;
                        }
                    }
                }

                let best_audio = select_best_audio_stream(&direct_streams, prefer_m4a);

                // Candidate video streams must either contain audio or be pairable via FFmpeg muxing.
                let eligible_videos: Vec<&ResolvedHttpStream> = direct_streams
                    .iter()
                    .filter(|s| {
                        s.vcodec != "none"
                            && (s.acodec != "none" || (allow_split && best_audio.is_some()))
                    })
                    .collect();

                if !eligible_videos.is_empty() {
                    let under_cap: Vec<&ResolvedHttpStream> = match requested_max_h {
                        Some(cap) => {
                            let filtered: Vec<&ResolvedHttpStream> = eligible_videos
                                .iter()
                                .copied()
                                .filter(|s| s.height.map_or(false, |h| h <= cap))
                                .collect();
                            if filtered.is_empty() {
                                eligible_videos
                            } else {
                                filtered
                            }
                        }
                        None => eligible_videos,
                    };

                    if let Some(chosen_video) = under_cap
                        .into_iter()
                        .max_by_key(|s| {
                            let h = s.height.unwrap_or(0);
                            let compat = video_container_compatibility_score(s, effective_target_ext);
                            let progressive_bonus =
                                if !allow_split && s.acodec != "none" { 1u8 } else { 0u8 };
                            let fps = s.fps.unwrap_or(0);
                            let size_or_tbr = s.filesize.unwrap_or(s.tbr_kbps);
                            (h, progressive_bonus, compat, fps, size_or_tbr)
                        })
                        .cloned()
                    {
                        if let Some(min_h) = expected_max_h {
                            if chosen_video.height.unwrap_or(0) < min_h {
                                return None;
                            }
                        }
                        if chosen_video.acodec != "none" {
                            return Some(VideoStreamPlan::SingleStream(chosen_video));
                        }
                        if allow_split {
                            if let Some(audio) = best_audio {
                                return Some(VideoStreamPlan::SplitVideoAudio {
                                    video: chosen_video,
                                    audio,
                                });
                            }
                        }
                    }
                }
            }
        }
        if !audio_mode && expected_max_h.is_some() {
            return None;
        }
    }

    // 2. Fallback to pre-resolved `requested_formats` (when `formats` had no direct streams).
    if let Some(req_arr) = json["requested_formats"].as_array().filter(|a| !a.is_empty()) {
        let mut resolved = Vec::with_capacity(req_arr.len());
        let mut all_direct = true;
        for item in req_arr {
            if let Some(s) = parse_direct_http_stream(item, root_headers) {
                resolved.push(s);
            } else {
                all_direct = false;
                break;
            }
        }
        if all_direct && !resolved.is_empty() {
            if audio_mode {
                let prefer_m4a = effective_target_ext == "m4a"
                    || format_id == Some("audio_m4a");
                if let Some(best_a) = select_best_audio_stream(&resolved, prefer_m4a)
                    .or_else(|| resolved.into_iter().last())
                {
                    return Some(VideoStreamPlan::SingleStream(best_a));
                }
            } else if resolved.len() == 1 {
                let only = resolved.into_iter().next()?;
                let cap_ok =
                    requested_max_h.map_or(true, |cap| only.height.map_or(true, |h| h <= cap));
                let height_ok = expected_max_h.map_or(true, |min_h| only.height.unwrap_or(0) >= min_h);
                if cap_ok && height_ok {
                    return Some(VideoStreamPlan::SingleStream(only));
                }
            } else if allow_split && resolved.len() >= 2 {
                if let (Some(video), Some(audio)) = (
                    resolved.iter().find(|s| s.vcodec != "none").cloned(),
                    resolved
                        .iter()
                        .find(|s| s.vcodec == "none" && s.acodec != "none")
                        .or_else(|| resolved.iter().rfind(|s| s.acodec != "none"))
                        .cloned(),
                ) {
                    let cap_ok = requested_max_h
                        .map_or(true, |cap| video.height.map_or(true, |h| h <= cap));
                    let height_ok =
                        expected_max_h.map_or(true, |min_h| video.height.unwrap_or(0) >= min_h);
                    if cap_ok && height_ok {
                        return Some(VideoStreamPlan::SplitVideoAudio { video, audio });
                    }
                }
            }
        }
    }

    // 3. Single-stream top-level URL fallback (when extractor returns direct stream object)
    if let Some(single) = parse_direct_http_stream(json, root_headers) {
        if audio_mode {
            if single.acodec != "none" {
                return Some(VideoStreamPlan::SingleStream(single));
            }
        } else {
            let cap_ok =
                requested_max_h.map_or(true, |cap| single.height.map_or(true, |h| h <= cap));
            let height_ok =
                expected_max_h.map_or(true, |min_h| single.height.unwrap_or(0) >= min_h);
            if cap_ok && height_ok {
                return Some(VideoStreamPlan::SingleStream(single));
            }
        }
    }

    None
}

/// Computes unified monotonic progress telemetry across sequential video + audio HTTP phases
/// before FFmpeg container muxing.
pub fn compute_split_phase_telemetry(
    prior_bytes: u64,
    phase_downloaded: u64,
    combined_total_bytes: u64,
    speed_bytes_per_sec: u64,
    max_pct_before_mux: f64,
) -> CompositePhaseProgress {
    let downloaded_bytes = prior_bytes.saturating_add(phase_downloaded);
    let has_known_total = combined_total_bytes > 0;
    let total_bytes = if has_known_total {
        combined_total_bytes.max(downloaded_bytes)
    } else {
        downloaded_bytes
    };
    let cap = max_pct_before_mux.clamp(1.0, 99.5);

    let progress_percent = if has_known_total {
        ((downloaded_bytes as f64 / total_bytes as f64) * cap).clamp(1.0, cap)
    } else if downloaded_bytes > 0 {
        let mb = downloaded_bytes as f64 / (1024.0 * 1024.0);
        ((1.0 - (-mb / 8.0).exp()) * (cap - 5.0) + 5.0).clamp(2.0, cap)
    } else {
        1.0
    };

    let eta_seconds =
        if has_known_total && speed_bytes_per_sec > 0 && combined_total_bytes > downloaded_bytes {
            Some(
                ((combined_total_bytes - downloaded_bytes) as f64 / speed_bytes_per_sec as f64)
                    .ceil() as u64,
            )
        } else if has_known_total && downloaded_bytes >= combined_total_bytes {
            Some(1)
        } else {
            None
        };

    CompositePhaseProgress {
        downloaded_bytes,
        total_bytes,
        progress_percent,
        eta_seconds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn build_synthetic_zip(entries: &[(&str, &[u8], bool, u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();

        for (name, data, deflate, unix_mode) in entries {
            let local_offset = out.len() as u32;
            let mut crc = crc32fast::Hasher::new();
            crc.update(data);
            let crc_val = crc.finalize();

            let (method, payload) = if *deflate {
                let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
                enc.write_all(data).unwrap();
                (8u16, enc.finish().unwrap())
            } else {
                (0u16, data.to_vec())
            };

            let name_bytes = name.as_bytes();
            // Local file header
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc_val.to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name_bytes);
            out.extend_from_slice(&payload);

            // Central directory header
            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&((3u16 << 8) | 20u16).to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc_val.to_le_bytes());
            central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&(unix_mode << 16).to_le_bytes());
            central.extend_from_slice(&local_offset.to_le_bytes());
            central.extend_from_slice(name_bytes);
        }

        let cd_offset = out.len() as u32;
        let cd_size = central.len() as u32;
        out.extend_from_slice(&central);

        // End of central directory
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());

        out
    }

    fn make_mock_inspect(url: &str, title: &str) -> InspectResult {
        InspectResult {
            url: url.to_string(),
            final_url: url.to_string(),
            file_name: format!("{}.mp4", title),
            file_size: Some(10_000_000),
            supports_range: true,
            mime_type: Some("video/mp4".to_string()),
            etag: None,
            last_modified: None,
            suggested_category: "video".to_string(),
            media_info: Some(crate::model::MediaInfo {
                title: title.to_string(),
                thumbnail_url: None,
                duration_seconds: Some(120),
                formats: vec![crate::model::VideoFormat {
                    format_id: "1080p_mp4".to_string(),
                    resolution: "1080p (Full HD)".to_string(),
                    extension: "mp4".to_string(),
                    file_size_approx: Some(10_000_000),
                    video_codec: Some("avc1".to_string()),
                    audio_codec: Some("AAC".to_string()),
                    download_url: url.to_string(),
                    is_audio_only: false,
                }],
            }),
            torrent_info: None,
        }
    }

    #[test]
    fn test_platform_asset_specs_and_node_version_parser() {
        let win_x64 = ytdlp_onedir_spec("windows", "x86_64").unwrap();
        assert!(win_x64.url.ends_with("/yt-dlp_win.zip"));
        assert_eq!(win_x64.bin_names, &["yt-dlp"]);

        let win_arm = ytdlp_onedir_spec("windows", "aarch64").unwrap();
        assert!(win_arm.url.ends_with("/yt-dlp_win_arm64.zip"));
        assert!(win_arm.bin_names.contains(&"yt-dlp_arm64"));

        let lin_x64 = ytdlp_onedir_spec("linux", "x86_64").unwrap();
        assert!(lin_x64.url.ends_with("/yt-dlp_linux.zip"));
        assert!(lin_x64.bin_names.contains(&"yt-dlp_linux"));

        let lin_arm = ytdlp_onedir_spec("linux", "aarch64").unwrap();
        assert!(lin_arm.url.ends_with("/yt-dlp_linux_aarch64.zip"));
        assert!(lin_arm.bin_names.contains(&"yt-dlp_linux_aarch64"));

        assert_eq!(
            quickjs_download_url_for("windows", "x86_64").unwrap(),
            "https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-windows-x86_64.exe"
        );
        assert_eq!(
            quickjs_download_url_for("linux", "x86_64").unwrap(),
            "https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-linux-x86_64"
        );
        assert_eq!(
            quickjs_download_url_for("macos", "aarch64").unwrap(),
            "https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-darwin-arm64"
        );

        assert_eq!(parse_node_major_version("v22.14.0\n"), Some(22));
        assert_eq!(parse_node_major_version("v18.20.4\r\n"), Some(18));
        assert_eq!(parse_node_major_version("24.1.0-rc.1"), Some(24));
        assert_eq!(parse_node_major_version("invalid"), None);
        assert!(!youtube_fallback_extractor_args().contains("=all"));
    }

    #[test]
    fn test_canonicalize_video_url_and_options_fingerprint() {
        let a = canonicalize_video_url("https://www.youtube.com/watch?v=Jdj13fGMmmA&list=RD123&index=2");
        let b = canonicalize_video_url("https://youtu.be/Jdj13fGMmmA?si=trackingToken");
        let c = canonicalize_video_url("https://www.youtube.com/shorts/Jdj13fGMmmA?feature=share");
        assert_eq!(a, "youtube:v:Jdj13fGMmmA");
        assert_eq!(b, "youtube:v:Jdj13fGMmmA");
        assert_eq!(c, "youtube:v:Jdj13fGMmmA");

        let vimeo1 = canonicalize_video_url("https://vimeo.com/12345?utm_source=x&foo=1#t=5s");
        let vimeo2 = canonicalize_video_url("https://vimeo.com/12345?foo=1");
        assert_eq!(vimeo1, vimeo2);

        let fp_anon = inspect_options_fingerprint(None, None, None, None, None, None);
        let fp_cookie = inspect_options_fingerprint(Some("SID=abc"), None, None, None, None, None);
        assert_ne!(fp_anon, fp_cookie);
    }

    #[tokio::test]
    async fn test_video_inspect_coordinator_singleflight_and_ttl_cache() {
        let coord = VideoInspectCoordinator::with_ttl_and_capacity(Duration::from_secs(60), 4);
        let calls = Arc::new(AtomicUsize::new(0));

        // 1. Concurrent callers for the same YouTube video & headers must share one fetch
        let url1 = "https://www.youtube.com/watch?v=Jdj13fGMmmA";
        let url2 = "https://youtu.be/Jdj13fGMmmA?si=shared";
        let fp = inspect_options_fingerprint(Some("SID=1"), Some(url1), Some("Chrome"), None, None, None);

        let c1 = calls.clone();
        let fut1 = coord.get_or_inspect(url1, &fp, move || async move {
            c1.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(60)).await;
            Ok(make_mock_inspect("https://www.youtube.com/watch?v=Jdj13fGMmmA", "ClipA"))
        });

        let c2 = calls.clone();
        let fut2 = coord.get_or_inspect(url2, &fp, move || async move {
            c2.fetch_add(1, Ordering::SeqCst);
            Ok(make_mock_inspect("https://youtu.be/Jdj13fGMmmA?si=shared", "ClipA"))
        });

        let (r1, r2) = tokio::join!(fut1, fut2);
        let res1 = r1.unwrap();
        let res2 = r2.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "Singleflight must invoke fetcher once");
        assert_eq!(res1.url, url1);
        assert_eq!(res2.url, url2);
        assert_eq!(res2.media_info.unwrap().formats[0].download_url, url2);

        // 2. Subsequent hit uses TTL cache (zero fetcher calls)
        let c3 = calls.clone();
        let res3 = coord
            .get_or_inspect(url1, &fp, move || async move {
                c3.fetch_add(1, Ordering::SeqCst);
                Ok(make_mock_inspect(url1, "ShouldNotRun"))
            })
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(res3.file_name, "ClipA.mp4");

        // 3. Different cookie fingerprint must NOT share cached authenticated entry
        let fp_other = inspect_options_fingerprint(None, None, None, None, None, None);
        let c4 = calls.clone();
        let _ = coord
            .get_or_inspect(url1, &fp_other, move || async move {
                c4.fetch_add(1, Ordering::SeqCst);
                Ok(make_mock_inspect(url1, "ClipAnon"))
            })
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        // 4. Errors must NEVER be cached
        let err_fp = inspect_options_fingerprint(None, None, None, None, None, None);
        let err_url = "https://vimeo.com/999999";
        let ce1 = calls.clone();
        let err_res = coord
            .get_or_inspect(err_url, &err_fp, move || async move {
                ce1.fetch_add(1, Ordering::SeqCst);
                Err("transient network error".to_string())
            })
            .await;
        assert!(err_res.is_err());

        let ce2 = calls.clone();
        let ok_after_err = coord
            .get_or_inspect(err_url, &err_fp, move || async move {
                ce2.fetch_add(1, Ordering::SeqCst);
                Ok(make_mock_inspect(err_url, "Recovered"))
            })
            .await;
        assert!(ok_after_err.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn test_extract_zip_archive_validates_crc_and_rejects_zip_slip() {
        let base = std::env::temp_dir().join(format!("dc_zip_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let payload_bin = b"#!/bin/sh\necho yt-dlp\n";
        let payload_py = b"print('ejs helper')\n".repeat(40);
        let valid_zip_bytes = build_synthetic_zip(&[
            ("yt-dlp_linux", payload_bin, false, 0o755),
            ("_internal/yt_dlp_ejs/main.py", &payload_py, true, 0o644),
        ]);
        let valid_zip = base.join("valid.zip");
        std::fs::write(&valid_zip, &valid_zip_bytes).unwrap();

        let out_dir = base.join("out");
        let count = extract_zip_archive(&valid_zip, &out_dir).expect("valid zip should extract");
        assert_eq!(count, 2);
        assert_eq!(std::fs::read(out_dir.join("yt-dlp_linux")).unwrap(), payload_bin);
        assert_eq!(
            std::fs::read(out_dir.join("_internal").join("yt_dlp_ejs").join("main.py")).unwrap(),
            payload_py
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(out_dir.join("yt-dlp_linux"))
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0, "Executable permission bit must be restored");
        }

        // ZIP-slip must be rejected
        let evil_zip_bytes = build_synthetic_zip(&[("../evil.txt", b"pwned", false, 0o644)]);
        let evil_zip = base.join("evil.zip");
        std::fs::write(&evil_zip, &evil_zip_bytes).unwrap();
        let slip_res = extract_zip_archive(&evil_zip, &base.join("out_evil"));
        assert!(slip_res.is_err());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_extract_direct_stream_plan_selections_and_manifest_filtering() {
        // 1. Pre-resolved requested_formats with per-format HTTP headers
        let req_json = serde_json::json!({
            "http_headers": {
                "User-Agent": "RootUA/1.0",
                "Referer": "https://www.youtube.com/watch?v=WkKtsgji7yo"
            },
            "requested_formats": [
                {
                    "format_id": "137",
                    "url": "https://rr1---sn.googlevideo.com/videoplayback?itag=137",
                    "ext": "mp4",
                    "protocol": "https",
                    "vcodec": "avc1.640028",
                    "acodec": "none",
                    "height": 1080,
                    "fps": 60,
                    "filesize": 900_000_000u64,
                    "http_headers": {
                        "Cookie": "GPS=1"
                    }
                },
                {
                    "format_id": "140",
                    "url": "https://rr1---sn.googlevideo.com/videoplayback?itag=140",
                    "ext": "m4a",
                    "protocol": "https",
                    "vcodec": "none",
                    "acodec": "mp4a.40.2",
                    "abr": 128.0,
                    "filesize": 20_000_000u64
                }
            ]
        });

        let split_plan = extract_direct_stream_plan(&req_json, Some("1080p_mp4"), "mp4", false, true)
            .expect("split direct HTTP plan should resolve");
        match split_plan {
            VideoStreamPlan::SplitVideoAudio { video, audio } => {
                assert_eq!(video.filesize, Some(900_000_000));
                assert_eq!(video.user_agent.as_deref(), Some("RootUA/1.0"));
                assert_eq!(video.referer.as_deref(), Some("https://www.youtube.com/watch?v=WkKtsgji7yo"));
                assert_eq!(video.cookie_header.as_deref(), Some("GPS=1"));
                assert_eq!(audio.ext, "m4a");
                assert_eq!(audio.filesize, Some(20_000_000));
            }
            other => panic!("expected SplitVideoAudio, got {:?}", other),
        }

        // 2. Full formats array selection: storyboard & m3u8 filtered, 1080p AVC + M4A chosen
        let formats_json = serde_json::json!({
            "http_headers": { "User-Agent": "TestUA" },
            "formats": [
                {
                    "format_id": "sb0",
                    "url": "https://i.ytimg.com/sb/storyboard.mhtml",
                    "ext": "mhtml",
                    "format_note": "storyboard",
                    "protocol": "https",
                    "vcodec": "none",
                    "acodec": "none"
                },
                {
                    "format_id": "hls1",
                    "url": "https://manifest.googlevideo.com/api/manifest/hls_playlist/index.m3u8?foo=1",
                    "ext": "mp4",
                    "protocol": "m3u8_native",
                    "vcodec": "avc1",
                    "acodec": "mp4a.40.2",
                    "height": 1080
                },
                {
                    "format_id": "18",
                    "url": "https://rr2.googlevideo.com/videoplayback?itag=18",
                    "ext": "mp4",
                    "protocol": "https",
                    "vcodec": "avc1.42001E",
                    "acodec": "mp4a.40.2",
                    "height": 360,
                    "filesize": 120_000_000u64
                },
                {
                    "format_id": "137",
                    "url": "https://rr2.googlevideo.com/videoplayback?itag=137",
                    "ext": "mp4",
                    "protocol": "https",
                    "vcodec": "avc1.640028",
                    "acodec": "none",
                    "height": 1080,
                    "fps": 60,
                    "filesize": 900_000_000u64
                },
                {
                    "format_id": "248",
                    "url": "https://rr2.googlevideo.com/videoplayback?itag=248",
                    "ext": "webm",
                    "protocol": "https",
                    "vcodec": "vp9",
                    "acodec": "none",
                    "height": 1080,
                    "fps": 60,
                    "filesize": 800_000_000u64
                },
                {
                    "format_id": "313",
                    "url": "https://rr2.googlevideo.com/videoplayback?itag=313",
                    "ext": "webm",
                    "protocol": "https",
                    "vcodec": "vp9",
                    "acodec": "none",
                    "height": 2160,
                    "fps": 60,
                    "filesize": 2_400_000_000u64
                },
                {
                    "format_id": "140",
                    "url": "https://rr2.googlevideo.com/videoplayback?itag=140",
                    "ext": "m4a",
                    "protocol": "https",
                    "vcodec": "none",
                    "acodec": "mp4a.40.2",
                    "abr": 128.0,
                    "filesize": 20_000_000u64
                },
                {
                    "format_id": "251",
                    "url": "https://rr2.googlevideo.com/videoplayback?itag=251",
                    "ext": "webm",
                    "protocol": "https",
                    "vcodec": "none",
                    "acodec": "opus",
                    "abr": 160.0,
                    "filesize": 25_000_000u64
                }
            ]
        });

        let plan_1080 = extract_direct_stream_plan(&formats_json, Some("1080p_mp4"), "mp4", false, true)
            .expect("1080p_mp4 should resolve");
        match plan_1080 {
            VideoStreamPlan::SplitVideoAudio { video, audio } => {
                assert_eq!(video.height, Some(1080));
                assert!(video.vcodec.starts_with("avc1"), "Expected AVC MP4 for 1080p_mp4");
                assert_eq!(audio.ext, "m4a", "Expected M4A companion for MP4 mux");
            }
            other => panic!("expected SplitVideoAudio, got {:?}", other),
        }

        let plan_4k = extract_direct_stream_plan(&formats_json, Some("2160p"), "webm", false, true)
            .expect("2160p should resolve");
        match plan_4k {
            VideoStreamPlan::SplitVideoAudio { video, audio } => {
                assert_eq!(video.height, Some(2160));
                assert_eq!(audio.ext, "webm", "Expected best opus for webm");
            }
            other => panic!("expected SplitVideoAudio, got {:?}", other),
        }

        // Without FFmpeg split allowed, high-res requests (1080p/2160p) must return None instead
        // of silently downgrading to progressive 360p; explicit 360p progressive still works.
        assert!(
            extract_direct_stream_plan(&formats_json, Some("1080p_mp4"), "mp4", false, false)
                .is_none(),
            "1080p_mp4 must not silently downgrade to 360p when FFmpeg is absent"
        );
        assert!(
            extract_direct_stream_plan(&formats_json, Some("2160p_mp4"), "mp4", false, false)
                .is_none(),
            "2160p_mp4 must not silently downgrade to 360p when FFmpeg is absent"
        );
        assert!(
            extract_direct_stream_plan(&formats_json, Some("137"), "mp4", false, false).is_none(),
            "Explicit split format_id must not downgrade to 360p when FFmpeg is absent"
        );
        assert!(
            extract_direct_stream_plan(&formats_json, Some("hls1"), "mp4", false, true).is_none(),
            "Explicit HLS format_id must return None so yt-dlp handles it"
        );

        let plan_prog =
            extract_direct_stream_plan(&formats_json, Some("360p_mp4"), "mp4", false, false)
                .expect("360p_mp4 should resolve progressive SingleStream even without FFmpeg");
        match plan_prog {
            VideoStreamPlan::SingleStream(s) => assert_eq!(s.height, Some(360)),
            other => panic!("expected SingleStream, got {:?}", other),
        }

        // Audio-only selection
        let plan_mp3 = extract_direct_stream_plan(&formats_json, Some("audio_mp3"), "mp3", true, true)
            .expect("audio_mp3 should resolve highest bitrate pure audio");
        match plan_mp3 {
            VideoStreamPlan::SingleStream(s) => assert_eq!(s.acodec, "opus"),
            other => panic!("expected SingleStream, got {:?}", other),
        }

        let plan_m4a = extract_direct_stream_plan(&formats_json, Some("audio_m4a"), "m4a", true, true)
            .expect("audio_m4a should prefer m4a");
        match plan_m4a {
            VideoStreamPlan::SingleStream(s) => assert_eq!(s.ext, "m4a"),
            other => panic!("expected SingleStream, got {:?}", other),
        }

        // When cached inspect JSON includes yt-dlp's default requested_formats (e.g. 2160p),
        // requested 360p or audio_mp3 must still honor caller selection from `formats`.
        let mut mixed_json = formats_json.clone();
        mixed_json["requested_formats"] = serde_json::json!([
            {
                "format_id": "313",
                "url": "https://rr2.googlevideo.com/videoplayback?itag=313",
                "ext": "webm",
                "protocol": "https",
                "vcodec": "vp9",
                "acodec": "none",
                "height": 2160
            },
            {
                "format_id": "251",
                "url": "https://rr2.googlevideo.com/videoplayback?itag=251",
                "ext": "webm",
                "protocol": "https",
                "vcodec": "none",
                "acodec": "opus"
            }
        ]);
        match extract_direct_stream_plan(&mixed_json, Some("360p_mp4"), "mp4", false, true).unwrap() {
            VideoStreamPlan::SingleStream(s) => assert_eq!(s.height, Some(360)),
            other => panic!("expected 360p progressive SingleStream, got {:?}", other),
        }
        match extract_direct_stream_plan(&mixed_json, Some("audio_mp3"), "mp3", true, true).unwrap() {
            VideoStreamPlan::SingleStream(s) => assert_eq!(s.vcodec, "none"),
            other => panic!("expected audio-only SingleStream, got {:?}", other),
        }

        // Pure HLS manifest document must return None
        let hls_only = serde_json::json!({
            "formats": [{
                "format_id": "hls",
                "url": "https://cdn.example.com/live/master.m3u8",
                "ext": "mp4",
                "protocol": "m3u8_native",
                "vcodec": "avc1",
                "acodec": "mp4a.40.2"
            }]
        });
        assert!(extract_direct_stream_plan(&hls_only, None, "mp4", false, true).is_none());

        // Mixed 2160p HLS + 360p HTTPS must NOT silently downgrade 2160p -> 360p
        let mixed_hls_high = serde_json::json!({
            "formats": [
                {
                    "format_id": "18",
                    "url": "https://rr2.googlevideo.com/videoplayback?itag=18",
                    "ext": "mp4",
                    "protocol": "https",
                    "vcodec": "avc1.42001E",
                    "acodec": "mp4a.40.2",
                    "height": 360
                },
                {
                    "format_id": "hls_4k",
                    "url": "https://manifest.googlevideo.com/api/manifest/hls_playlist/index.m3u8",
                    "ext": "mp4",
                    "protocol": "m3u8_native",
                    "vcodec": "vp9",
                    "acodec": "none",
                    "height": 2160
                },
                {
                    "format_id": "140",
                    "url": "https://rr2.googlevideo.com/videoplayback?itag=140",
                    "ext": "m4a",
                    "protocol": "https",
                    "vcodec": "none",
                    "acodec": "mp4a.40.2"
                }
            ]
        });
        assert!(
            extract_direct_stream_plan(&mixed_hls_high, Some("2160p_mp4"), "mp4", false, true)
                .is_none(),
            "Mixed 2160p HLS + 360p progressive must delegate to yt-dlp for 2160p"
        );
    }

    #[test]
    fn test_ffmpeg_specs_gzip_extraction_and_split_detection() {
        use flate2::write::GzEncoder;

        for (os, arch, expected_suffix) in [
            ("windows", "x86_64", "ffmpeg-win32-x64.gz"),
            ("windows", "aarch64", "ffmpeg-win32-x64.gz"),
            ("macos", "aarch64", "ffmpeg-darwin-arm64.gz"),
            ("macos", "x86_64", "ffmpeg-darwin-x64.gz"),
            ("linux", "x86_64", "ffmpeg-linux-x64.gz"),
            ("linux", "aarch64", "ffmpeg-linux-arm64.gz"),
            ("linux", "arm", "ffmpeg-linux-arm.gz"),
            ("linux", "x86", "ffmpeg-linux-ia32.gz"),
        ] {
            let spec = ffmpeg_download_spec(os, arch).expect("ffmpeg spec must exist");
            assert!(
                spec.url.ends_with(expected_suffix),
                "unexpected spec URL for {}/{}: {}",
                os,
                arch,
                spec.url
            );
            assert!(spec.min_compressed_bytes >= 1_000_000);
            assert!(spec.min_extracted_bytes >= 5_000_000);
        }

        // GZIP extraction roundtrip & corrupt rejection
        let base = std::env::temp_dir().join(format!("dc_gz_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let payload = b"synthetic-ffmpeg-binary-payload-for-test-".repeat(256);
        let mut gz_bytes = Vec::new();
        {
            let mut enc = GzEncoder::new(&mut gz_bytes, Compression::default());
            enc.write_all(&payload).unwrap();
            enc.finish().unwrap();
        }
        let gz_path = base.join("ffmpeg.gz");
        let out_path = base.join("bin").join("ffmpeg");
        std::fs::write(&gz_path, &gz_bytes).unwrap();

        let extracted = extract_gzip_file(&gz_path, &out_path).expect("valid gzip should extract");
        assert_eq!(extracted, payload.len() as u64);
        assert_eq!(std::fs::read(&out_path).unwrap(), payload);

        let bad_gz_path = base.join("corrupt.gz");
        let bad_out_path = base.join("bin").join("bad_ffmpeg");
        std::fs::write(&bad_gz_path, &gz_bytes[..gz_bytes.len() / 2]).unwrap();
        assert!(extract_gzip_file(&bad_gz_path, &bad_out_path).is_err());
        assert!(
            !bad_out_path.exists(),
            "failed gzip extraction must clean up partial output"
        );
        let _ = std::fs::remove_dir_all(&base);

        // Split requirement detection across YouTube 4K + 360p + audio + VP8/VP9 scoring
        let yt_4k = serde_json::json!({
            "formats": [
                {
                    "format_id": "18",
                    "url": "https://rr2.googlevideo.com/videoplayback?itag=18",
                    "ext": "mp4",
                    "protocol": "https",
                    "vcodec": "avc1.42001E",
                    "acodec": "mp4a.40.2",
                    "height": 360,
                    "filesize": 120_000_000u64
                },
                {
                    "format_id": "271",
                    "url": "https://rr2.googlevideo.com/videoplayback?itag=271",
                    "ext": "webm",
                    "protocol": "https",
                    "vcodec": "vp8",
                    "acodec": "none",
                    "height": 2160,
                    "filesize": 2_900_000_000u64
                },
                {
                    "format_id": "313",
                    "url": "https://rr2.googlevideo.com/videoplayback?itag=313",
                    "ext": "webm",
                    "protocol": "https",
                    "vcodec": "vp9",
                    "acodec": "none",
                    "height": 2160,
                    "filesize": 2_400_000_000u64
                },
                {
                    "format_id": "140",
                    "url": "https://rr2.googlevideo.com/videoplayback?itag=140",
                    "ext": "m4a",
                    "protocol": "https",
                    "vcodec": "none",
                    "acodec": "mp4a.40.2",
                    "abr": 128.0,
                    "filesize": 20_000_000u64
                }
            ]
        });

        assert!(requested_height_requires_split(&yt_4k, Some("2160p_mp4")));
        assert!(requested_height_requires_split(&yt_4k, Some("313")));
        assert!(!requested_height_requires_split(&yt_4k, Some("360p_mp4")));
        assert!(!requested_height_requires_split(&yt_4k, Some("audio_mp3")));
        assert!(!requested_height_requires_split(&yt_4k, Some("audio_m4a")));

        // VP9 WebM must rank ahead of VP8 WebM when targeting MP4 container
        match extract_direct_stream_plan(&yt_4k, Some("2160p_mp4"), "mp4", false, true).unwrap() {
            VideoStreamPlan::SplitVideoAudio { video, audio } => {
                assert_eq!(video.vcodec, "vp9");
                assert_eq!(audio.ext, "m4a");
            }
            other => panic!("expected SplitVideoAudio, got {:?}", other),
        }

        // Generic video with no separate audio track does NOT require FFmpeg split
        let generic_muxed = serde_json::json!({
            "formats": [{
                "format_id": "hd",
                "url": "https://cdn.example.com/1080p.mp4",
                "ext": "mp4",
                "protocol": "https",
                "vcodec": "avc1",
                "acodec": "none",
                "height": 1080
            }]
        });
        assert!(!requested_height_requires_split(&generic_muxed, Some("1080p_mp4")));
    }

    #[test]
    fn test_compute_split_phase_telemetry() {
        let p1 = compute_split_phase_telemetry(0, 500, 1000, 100, 96.0);
        assert_eq!(p1.downloaded_bytes, 500);
        assert_eq!(p1.total_bytes, 1000);
        assert!((p1.progress_percent - 48.0).abs() < 0.01);
        assert_eq!(p1.eta_seconds, Some(5));

        let p2 = compute_split_phase_telemetry(800, 200, 1000, 200, 96.0);
        assert_eq!(p2.downloaded_bytes, 1000);
        assert!((p2.progress_percent - 96.0).abs() < 0.01);
        assert_eq!(p2.eta_seconds, Some(1));

        let p_unknown = compute_split_phase_telemetry(0, 2 * 1024 * 1024, 0, 0, 96.0);
        assert!(p_unknown.progress_percent > 5.0 && p_unknown.progress_percent <= 96.0);
        assert_eq!(p_unknown.eta_seconds, None);
    }
}
