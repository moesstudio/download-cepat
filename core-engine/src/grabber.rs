use anyhow::{Context, Result};
use std::collections::{HashSet, VecDeque};
use std::path::Path;
use url::Url;

use crate::inspector::{
    build_http_clients, categorize_filename, send_with_redirects, DEFAULT_USER_AGENT,
};
use crate::model::GrabbedAsset;

#[derive(Clone, Default)]
pub struct GrabOptions<'a> {
    pub max_depth: usize,
    pub same_host_only: bool,
    pub max_pages: usize,
    pub max_assets: usize,
    pub cookie: Option<&'a str>,
    pub referer: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    pub proxy_url: Option<&'a str>,
    pub auth_username: Option<&'a str>,
    pub auth_password: Option<&'a str>,
}

fn unescape_html_attr(raw: &str) -> String {
    raw.replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .trim()
        .to_string()
}

pub fn extract_raw_urls_from_html(html: &str) -> Vec<String> {
    let lower = html.to_lowercase();
    let mut found = Vec::new();
    let attrs = ["href=", "src=", "data-src=", "poster="];

    for attr in attrs {
        let mut search_from = 0usize;
        while let Some(rel_idx) = lower[search_from..].find(attr) {
            let val_start = search_from + rel_idx + attr.len();
            let rest = &html[val_start..];
            let trimmed_start = rest.len() - rest.trim_start().len();
            let slice = &rest[trimmed_start..];

            if let Some(first_ch) = slice.chars().next() {
                let raw_val = if first_ch == '"' || first_ch == '\'' {
                    let inner = &slice[1..];
                    inner.find(first_ch).map(|end| &inner[..end])
                } else {
                    let end = slice
                        .find(|c: char| c.is_whitespace() || c == '>' || c == '"')
                        .unwrap_or(slice.len());
                    Some(&slice[..end])
                };

                if let Some(val) = raw_val {
                    let unescaped = unescape_html_attr(val);
                    if !unescaped.is_empty()
                        && !unescaped.starts_with('#')
                        && !unescaped.to_lowercase().starts_with("javascript:")
                        && !unescaped.to_lowercase().starts_with("mailto:")
                        && !unescaped.to_lowercase().starts_with("data:")
                    {
                        found.push(unescaped);
                    }
                }
            }
            search_from = val_start;
        }
    }

    found
}

fn classify_asset(resolved_url: &Url) -> (String, String, String, String) {
    let path = resolved_url.path();
    let file_name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("index.html")
        .to_string();

    let ext = Path::new(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let is_image = matches!(
        ext.as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "bmp" | "ico" | "avif"
    );
    let is_web_page = ext.is_empty()
        || matches!(ext.as_str(), "html" | "htm" | "php" | "asp" | "aspx" | "jsp");

    let (category_id, resource_type) = if is_image {
        ("images".to_string(), "image".to_string())
    } else if ext == "m3u8" || ext == "mpd" {
        ("video".to_string(), "media".to_string())
    } else {
        let cat = categorize_filename(&file_name);
        if cat == "video" || cat == "music" {
            (cat, "media".to_string())
        } else if cat != "general" {
            (cat, "file".to_string())
        } else if is_web_page {
            ("general".to_string(), "page".to_string())
        } else {
            ("general".to_string(), "file".to_string())
        }
    };

    (file_name, ext, category_id, resource_type)
}

pub async fn grab_site_links(start_url: &str, opts: GrabOptions<'_>) -> Result<Vec<GrabbedAsset>> {
    let base = Url::parse(start_url).context("URL awal Site Grabber tidak valid")?;
    let base_host = base.host_str().unwrap_or_default().to_lowercase();
    let max_depth = opts.max_depth.clamp(1, 3);
    let max_pages = if opts.max_pages == 0 { 20 } else { opts.max_pages.min(40) };
    let max_assets = if opts.max_assets == 0 { 800 } else { opts.max_assets.min(1500) };

    let (client, _) = build_http_clients(opts.proxy_url)?;
    let mut visited_pages: HashSet<String> = HashSet::new();
    let mut seen_assets: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    let mut assets: Vec<GrabbedAsset> = Vec::new();

    queue.push_back((start_url.to_string(), 1));

    while let Some((page_url, depth)) = queue.pop_front() {
        if visited_pages.len() >= max_pages || assets.len() >= max_assets {
            break;
        }
        if !visited_pages.insert(page_url.clone()) {
            continue;
        }

        let resp = match send_with_redirects(
            &client,
            reqwest::Method::GET,
            &page_url,
            opts.cookie,
            opts.referer.or(Some(start_url)),
            opts.user_agent,
            DEFAULT_USER_AGENT,
            None,
            opts.auth_username,
            opts.auth_password,
        )
        .await
        {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };

        let final_page_url = resp.url().clone();
        let ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        if !ct.contains("text/html") && !ct.contains("application/xhtml") && !ct.is_empty() {
            continue;
        }

        let body = match resp.text().await {
            Ok(t) => t,
            Err(_) => continue,
        };

        for raw in extract_raw_urls_from_html(&body) {
            let Ok(mut resolved) = final_page_url.join(&raw) else {
                continue;
            };
            resolved.set_fragment(None);
            let scheme = resolved.scheme().to_lowercase();
            if scheme != "http" && scheme != "https" && scheme != "ftp" {
                continue;
            }

            if opts.same_host_only {
                let host = resolved.host_str().unwrap_or_default().to_lowercase();
                if host != base_host {
                    continue;
                }
            }

            let abs_url = resolved.to_string();
            let (file_name, extension, category_id, resource_type) = classify_asset(&resolved);

            if resource_type == "page" && depth < max_depth && visited_pages.len() + queue.len() < max_pages {
                if !visited_pages.contains(&abs_url) {
                    queue.push_back((abs_url.clone(), depth + 1));
                }
            }

            if resource_type != "page" && seen_assets.insert(abs_url.clone()) {
                assets.push(GrabbedAsset {
                    url: abs_url,
                    file_name,
                    extension,
                    category_id,
                    resource_type,
                    source_page: final_page_url.to_string(),
                });
                if assets.len() >= max_assets {
                    break;
                }
            }
        }
    }

    Ok(assets)
}
