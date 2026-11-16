use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use flate2::read::DeflateDecoder;
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
}
