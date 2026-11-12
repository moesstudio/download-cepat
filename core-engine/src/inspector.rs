use anyhow::{Context, Result};
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE,
    COOKIE, ETAG, LAST_MODIFIED, RANGE, REFERER, USER_AGENT,
};
use std::path::Path;
use url::Url;

use crate::model::InspectResult;

pub(crate) const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
pub(crate) const FALLBACK_USER_AGENT: &str = "curl/8.7.1";

pub(crate) fn root_domain(host: &str) -> String {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 2 {
        format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]).to_lowercase()
    } else {
        host.to_lowercase()
    }
}

pub(crate) fn apply_request_headers(
    mut req: reqwest::RequestBuilder,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
    default_ua: &str,
) -> reqwest::RequestBuilder {
    let ua = user_agent
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default_ua);
    if let Ok(val) = HeaderValue::from_str(ua) {
        req = req.header(USER_AGENT, val);
    }
    if let Some(c) = cookie.map(str::trim).filter(|s| !s.is_empty()) {
        if let Ok(val) = HeaderValue::from_str(c) {
            req = req.header(COOKIE, val);
        }
    }
    if let Some(r) = referer.map(str::trim).filter(|s| !s.is_empty()) {
        if let Ok(val) = HeaderValue::from_str(r) {
            req = req.header(REFERER, val);
        }
    }
    req
}

pub(crate) async fn send_with_redirects(
    client: &reqwest::Client,
    initial_method: reqwest::Method,
    initial_url: &str,
    cookie: Option<&str>,
    referer: Option<&str>,
    user_agent: Option<&str>,
    default_ua: &str,
    range_header: Option<&str>,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
) -> std::result::Result<reqwest::Response, reqwest::Error> {
    let origin_parsed = Url::parse(initial_url).ok();
    let origin_root = origin_parsed
        .as_ref()
        .and_then(|u| u.host_str())
        .map(root_domain);

    let mut current_url = initial_url.to_string();
    let mut method = initial_method;
    let mut current_referer = referer.map(|s| s.to_string());

    for _hop in 0..10 {
        let current_parsed = Url::parse(&current_url).ok();
        let current_root = current_parsed
            .as_ref()
            .and_then(|u| u.host_str())
            .map(root_domain);

        let allow_sensitive = match (&origin_root, &current_root) {
            (Some(a), Some(b)) => a == b,
            _ => current_url == initial_url,
        };

        let mut req = apply_request_headers(
            client.request(method.clone(), &current_url),
            if allow_sensitive { cookie } else { None },
            current_referer.as_deref(),
            user_agent,
            default_ua,
        );

        if allow_sensitive {
            if let Some(user) = auth_username.map(str::trim).filter(|s| !s.is_empty()) {
                req = req.basic_auth(user, auth_password.filter(|p| !p.is_empty()));
            }
        }

        if let Some(r) = range_header {
            if method == reqwest::Method::GET {
                req = req.header(RANGE, r);
            }
        }

        let res = req.send().await?;
        let status = res.status();
        if status.is_redirection() {
            if let Some(loc) = res.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()) {
                let next_url = if let Some(ref base) = current_parsed {
                    base.join(loc).map(|u| u.to_string()).unwrap_or_else(|_| loc.to_string())
                } else {
                    loc.to_string()
                };

                if status == reqwest::StatusCode::SEE_OTHER {
                    method = reqwest::Method::GET;
                }
                if current_referer.is_none() {
                    current_referer = Some(current_url.clone());
                }
                current_url = next_url;
                continue;
            }
        }
        return Ok(res);
    }

    // One last request if loop completed at max hops
    let req = apply_request_headers(
        client.request(method, &current_url),
        None,
        current_referer.as_deref(),
        user_agent,
        default_ua,
    );
    req.send().await
}

pub fn build_http_clients(proxy_url: Option<&str>) -> Result<(reqwest::Client, reqwest::Client)> {
    let mut default_headers = HeaderMap::new();
    default_headers.insert("Accept", HeaderValue::from_static("*/*"));
    default_headers.insert("Accept-Language", HeaderValue::from_static("en-US,en;q=0.9,id;q=0.8"));

    let mut primary_builder = reqwest::Client::builder()
        .default_headers(default_headers)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(12));

    let mut fallback_headers = HeaderMap::new();
    fallback_headers.insert("Accept", HeaderValue::from_static("*/*"));

    let mut fallback_builder = reqwest::Client::builder()
        .default_headers(fallback_headers)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(12));

    if let Some(p_raw) = proxy_url.map(str::trim).filter(|s| !s.is_empty()) {
        let proxy = reqwest::Proxy::all(p_raw)
            .with_context(|| format!("Format Proxy URL tidak valid: {}", p_raw))?;
        primary_builder = primary_builder.proxy(proxy.clone());
        fallback_builder = fallback_builder.proxy(proxy);
    }

    Ok((primary_builder.build()?, fallback_builder.build()?))
}

fn path_has_html_extension(raw_or_path: &str) -> bool {
    let clean = raw_or_path
        .split('?')
        .next()
        .unwrap_or(raw_or_path)
        .split('#')
        .next()
        .unwrap_or(raw_or_path)
        .to_lowercase();
    clean.ends_with(".html") || clean.ends_with(".htm") || clean.ends_with(".xhtml")
}

pub fn is_unexpected_html_response(
    raw_url: &str,
    final_url: &str,
    headers: &HeaderMap,
    expected_dest_path: Option<&Path>,
) -> bool {
    let ct = match headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
        Some(v) => v.to_lowercase(),
        None => return false,
    };

    if !ct.contains("text/html") && !ct.contains("application/xhtml+xml") {
        return false;
    }

    // Allow intentional .html/.htm downloads
    if let Some(dest) = expected_dest_path {
        if let Some(ext) = dest.extension().and_then(|e| e.to_str()) {
            let ext_l = ext.to_lowercase();
            if ext_l == "html" || ext_l == "htm" || ext_l == "xhtml" {
                return false;
            }
        }
    }

    if let Some(cd) = headers.get(CONTENT_DISPOSITION).and_then(|v| v.to_str().ok()) {
        let cd_name = extract_filename_from_content_disposition(cd).to_lowercase();
        if cd.to_lowercase().contains("attachment") && path_has_html_extension(&cd_name) {
            return false;
        }
    }

    if path_has_html_extension(raw_url) && path_has_html_extension(final_url) {
        return false;
    }

    true
}

#[derive(Clone, Default)]
pub struct InspectOptions<'a> {
    pub cookie: Option<&'a str>,
    pub referer: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    pub proxy_url: Option<&'a str>,
    pub auth_username: Option<&'a str>,
    pub auth_password: Option<&'a str>,
}

pub struct UrlInspector {
    client: reqwest::Client,
    fallback_client: reqwest::Client,
}

impl UrlInspector {
    pub fn new() -> Result<Self> {
        Self::new_with_proxy(None)
    }

    pub fn new_with_proxy(proxy_url: Option<&str>) -> Result<Self> {
        let (client, fallback_client) = build_http_clients(proxy_url)?;
        Ok(Self { client, fallback_client })
    }

    pub async fn inspect(&self, raw_url: &str) -> Result<InspectResult> {
        self.inspect_with_options(raw_url, InspectOptions::default()).await
    }

    pub async fn inspect_with_headers(
        &self,
        raw_url: &str,
        cookie: Option<&str>,
        referer: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<InspectResult> {
        self.inspect_with_options(
            raw_url,
            InspectOptions {
                cookie,
                referer,
                user_agent,
                ..Default::default()
            },
        )
        .await
    }

    pub async fn inspect_with_options(
        &self,
        raw_url: &str,
        opts: InspectOptions<'_>,
    ) -> Result<InspectResult> {
        let parsed_url = Url::parse(raw_url).context("Format URL tidak valid")?;
        if parsed_url.scheme().eq_ignore_ascii_case("ftp") {
            return crate::ftp::inspect_ftp(raw_url, opts.auth_username, opts.auth_password).await;
        }

        let cookie = opts.cookie;
        let referer = opts.referer;
        let user_agent = opts.user_agent;
        let auth_u = opts.auth_username;
        let auth_p = opts.auth_password;

        let (dyn_primary, dyn_fallback);
        let (active_client, active_fallback) = if opts.proxy_url.map_or(false, |s| !s.trim().is_empty()) {
            let pair = build_http_clients(opts.proxy_url)?;
            dyn_primary = pair.0;
            dyn_fallback = pair.1;
            (&dyn_primary, &dyn_fallback)
        } else {
            (&self.client, &self.fallback_client)
        };

        let has_browser_identity = cookie.map_or(false, |s| !s.trim().is_empty())
            || user_agent.map_or(false, |s| !s.trim().is_empty());

        // 1. First probe using HEAD request
        let head_res = send_with_redirects(
            active_client,
            reqwest::Method::HEAD,
            raw_url,
            cookie,
            referer,
            user_agent,
            DEFAULT_USER_AGENT,
            None,
            auth_u,
            auth_p,
        )
        .await;

        let need_get_probe = match &head_res {
            Ok(res) if res.status().is_success() => {
                let f_url = res.url().as_str();
                is_unexpected_html_response(raw_url, f_url, res.headers(), None)
                    || res.headers().get(CONTENT_LENGTH).is_none()
            }
            _ => true,
        };

        let response = if !need_get_probe {
            head_res.unwrap()
        } else {
            let get_res = send_with_redirects(
                active_client,
                reqwest::Method::GET,
                raw_url,
                cookie,
                referer,
                user_agent,
                DEFAULT_USER_AGENT,
                Some("bytes=0-0"),
                auth_u,
                auth_p,
            )
            .await;

            match get_res {
                Ok(res) if res.status().is_success() => res,
                Ok(res) if !has_browser_identity
                    && (res.status() == reqwest::StatusCode::FORBIDDEN
                        || res.status() == reqwest::StatusCode::UNAUTHORIZED) =>
                {
                    send_with_redirects(
                        active_fallback,
                        reqwest::Method::GET,
                        raw_url,
                        cookie,
                        referer,
                        None,
                        FALLBACK_USER_AGENT,
                        Some("bytes=0-0"),
                        auth_u,
                        auth_p,
                    )
                    .await
                    .context("Gagal menghubungi server untuk inspeksi URL")?
                }
                Ok(res) => res,
                Err(e) => {
                    if !has_browser_identity {
                        send_with_redirects(
                            active_fallback,
                            reqwest::Method::GET,
                            raw_url,
                            cookie,
                            referer,
                            None,
                            FALLBACK_USER_AGENT,
                            Some("bytes=0-0"),
                            auth_u,
                            auth_p,
                        )
                        .await
                        .with_context(|| format!("Gagal menghubungi server untuk inspeksi URL: {}", e))?
                    } else {
                        return Err(anyhow::anyhow!("Gagal menghubungi server untuk inspeksi URL: {}", e));
                    }
                }
            }
        };

        let final_url = response.url().to_string();
        let status = response.status();
        let headers = response.headers();

        if !status.is_success() {
            anyhow::bail!(
                "Server menolak permintaan atau berkas tidak tersedia (HTTP {}). Pastikan tautan masih aktif dan sesi browser valid.",
                status
            );
        }

        if is_unexpected_html_response(raw_url, &final_url, headers, None) {
            anyhow::bail!(
                "Server mengembalikan halaman web HTML bukan file asli dari '{}'. Tautan kemungkinan kedaluwarsa atau membutuhkan Cookie & Referer aktif dari browser (mulai unduhan langsung dari Chrome/Firefox dengan ekstensi aktif).",
                final_url
            );
        }

        // 2. Extract Content-Length & Range support
        let mut file_size: Option<u64> = None;
        let mut supports_range = false;

        if let Some(cr) = headers.get("content-range") {
            // E.g. "bytes 0-0/10485760"
            if let Ok(cr_str) = cr.to_str() {
                supports_range = true;
                if let Some(slash_idx) = cr_str.rfind('/') {
                    if let Ok(total) = cr_str[slash_idx + 1..].trim().parse::<u64>() {
                        file_size = Some(total);
                    }
                }
            }
        }

        if file_size.is_none() {
            if let Some(cl) = headers.get(CONTENT_LENGTH) {
                if let Ok(cl_str) = cl.to_str() {
                    if let Ok(bytes) = cl_str.trim().parse::<u64>() {
                        file_size = Some(bytes);
                    }
                }
            }
        }

        if !supports_range {
            if let Some(ar) = headers.get(ACCEPT_RANGES) {
                if let Ok(ar_str) = ar.to_str() {
                    if ar_str.to_lowercase().contains("bytes") {
                        supports_range = true;
                    }
                }
            }
        }

        // 3. Extract Filename from Content-Disposition or URL path
        let mut file_name = String::new();
        if let Some(cd) = headers.get(CONTENT_DISPOSITION) {
            if let Ok(cd_str) = cd.to_str() {
                file_name = extract_filename_from_content_disposition(cd_str);
            }
        }

        if file_name.is_empty() {
            if let Ok(parsed_final) = Url::parse(&final_url) {
                // Check query string for filename
                for (k, v) in parsed_final.query_pairs() {
                    let k_lower = k.to_lowercase();
                    if k_lower == "file" || k_lower == "filename" || k_lower == "name" {
                        if !v.is_empty() {
                            file_name = v.to_string();
                            break;
                        }
                    }
                }

                if file_name.is_empty() {
                    if let Some(segments) = parsed_final.path_segments() {
                        if let Some(last) = segments.last() {
                            let decoded = urlencoding_decode(last);
                            if !decoded.is_empty() && !decoded.ends_with('/') {
                                file_name = decoded;
                            }
                        }
                    }
                }
            }
        }

        // Fallback to raw_url filename if final_url lacked an extension
        if (file_name.is_empty() || !file_name.contains('.')) && raw_url != final_url {
            if let Ok(parsed_raw) = Url::parse(raw_url) {
                if let Some(segments) = parsed_raw.path_segments() {
                    if let Some(last) = segments.last() {
                        let decoded = urlencoding_decode(last);
                        if decoded.contains('.') && !decoded.ends_with('/') {
                            file_name = decoded;
                        }
                    }
                }
            }
        }

        // 4. Extract ETag & Last-Modified & Content-Type
        let etag = headers.get(ETAG).and_then(|v| v.to_str().ok()).map(|s| s.trim_matches('"').to_string());
        let last_modified = headers.get(LAST_MODIFIED).and_then(|v| v.to_str().ok()).map(|s| s.to_string());
        let mime_type = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(|s| s.to_string());

        // If filename does not contain an extension, infer one from MIME type
        if !file_name.contains('.') {
            if let Some(ref m) = mime_type {
                let m_lower = m.to_lowercase();
                let inferred_ext = if m_lower.contains("pdf") {
                    ".pdf"
                } else if m_lower.contains("7z") {
                    ".7z"
                } else if m_lower.contains("rar") {
                    ".rar"
                } else if m_lower.contains("zip") {
                    ".zip"
                } else if m_lower.contains("iso9660") {
                    ".iso"
                } else if m_lower.contains("x-tar") || m_lower.contains("tar") {
                    ".tar"
                } else if m_lower.contains("gzip") {
                    ".tar.gz"
                } else if m_lower.contains("video/mp4") {
                    ".mp4"
                } else if m_lower.contains("matroska") {
                    ".mkv"
                } else if m_lower.contains("video/webm") {
                    ".webm"
                } else if m_lower.contains("audio/mpeg") || m_lower.contains("audio/mp3") {
                    ".mp3"
                } else if m_lower.contains("dmg") || m_lower.contains("diskimage") {
                    ".dmg"
                } else {
                    ""
                };
                if !inferred_ext.is_empty() {
                    file_name.push_str(inferred_ext);
                }
            }
        }

        if file_name.is_empty() {
            file_name = "download.bin".to_string();
        }

        // Clean filename (remove illegal characters)
        file_name = sanitize_filename(&file_name);

        let media_info = detect_video_stream(raw_url, &final_url, &file_name, mime_type.as_deref(), file_size);
        let mut suggested_category = categorize_filename(&file_name);
        if let Some(ref m) = media_info {
            suggested_category = "video".to_string();
            if file_name == "watch" || file_name == "download.bin" || !file_name.contains('.') {
                file_name = format!("{}.mp4", m.title);
            }
        }

        Ok(InspectResult {
            url: raw_url.to_string(),
            final_url,
            file_name,
            file_size,
            supports_range,
            mime_type,
            etag,
            last_modified,
            suggested_category,
            media_info,
        })
    }
}

pub fn detect_video_stream(
    raw_url: &str,
    final_url: &str,
    file_name: &str,
    mime_type: Option<&str>,
    file_size: Option<u64>,
) -> Option<crate::model::MediaInfo> {
    use crate::model::{MediaInfo, VideoFormat};

    let url_lower = raw_url.to_lowercase();
    let final_lower = final_url.to_lowercase();
    let _is_video_platform = url_lower.contains("youtube.com")
        || url_lower.contains("youtu.be")
        || url_lower.contains("vimeo.com")
        || url_lower.contains("tiktok.com")
        || url_lower.contains("instagram.com")
        || url_lower.contains("x.com")
        || url_lower.contains("twitter.com")
        || url_lower.contains("dailymotion.com")
        || url_lower.contains("twitch.tv");

    let is_video_file = final_lower.ends_with(".mp4")
        || final_lower.ends_with(".mkv")
        || final_lower.ends_with(".webm")
        || final_lower.ends_with(".m3u8")
        || final_lower.ends_with(".mpd")
        || mime_type.map_or(false, |m| m.starts_with("video/"));

    if !is_video_file {
        return None;
    }

    let base_name = file_name
        .trim_end_matches(".mp4")
        .trim_end_matches(".mkv")
        .trim_end_matches(".webm")
        .trim_end_matches(".bin");

    let clean_title = base_name.to_string();

    Some(MediaInfo {
        title: clean_title,
        thumbnail_url: None,
        duration_seconds: None,
        formats: vec![
            VideoFormat {
                format_id: "original".to_string(),
                resolution: "Original Stream".to_string(),
                extension: "mp4".to_string(),
                file_size_approx: file_size,
                video_codec: Some("H.264 / AVC".to_string()),
                audio_codec: Some("AAC".to_string()),
                download_url: final_url.to_string(),
                is_audio_only: false,
            },
        ],
    })
}

fn extract_filename_from_content_disposition(cd: &str) -> String {
    for part in cd.split(';') {
        let trimmed = part.trim();
        if let Some(val) = trimmed.strip_prefix("filename*=") {
            // E.g. filename*=UTF-8''my%20file.zip
            if let Some(quote_idx) = val.find("''") {
                let raw_encoded = &val[quote_idx + 2..];
                return urlencoding_decode(raw_encoded.trim_matches('"'));
            }
        } else if let Some(val) = trimmed.strip_prefix("filename=") {
            let name = val.trim().trim_matches('"').trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    String::new()
}

fn urlencoding_decode(s: &str) -> String {
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
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(result).unwrap_or_else(|_| s.to_string())
}

fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| match c {
        '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
        other => other,
    }).collect();
    if cleaned.is_empty() { "download.bin".to_string() } else { cleaned }
}

pub fn categorize_filename(name: &str) -> String {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "iso" | "tgz" => "compressed".to_string(),
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "epub" | "csv" => "documents".to_string(),
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" | "opus" => "music".to_string(),
        "exe" | "msi" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" | "apk" => "programs".to_string(),
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "ts" | "m4v" | "3gp" => "video".to_string(),
        _ => "general".to_string(),
    }
}
