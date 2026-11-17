use anyhow::{Context, Result};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use sha1::{Digest as Sha1Digest, Sha1};
use std::collections::{HashSet, VecDeque};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{broadcast, Mutex, Notify, Semaphore};
use url::Url;

use crate::downloader::{BandwidthLimiter, DownloadController, DownloadOptions};
use crate::inspector::sanitize_filename;
use crate::integrity::ensure_sufficient_disk_space;
use crate::model::{
    InspectResult, ProgressTelemetry, SegmentProgress, TaskStatus, TorrentFileEntry, TorrentInfo,
};

const BLOCK_SIZE: usize = 16 * 1024;
const MAX_PIPELINED_BLOCKS: usize = 8;
const MAX_CONCURRENT_PEERS: usize = 16;
const MAX_TORRENT_META_BYTES: usize = 16 * 1024 * 1024;
const BT_PROTOCOL_STR: &[u8; 19] = b"BitTorrent protocol";

const DEFAULT_PUBLIC_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "http://tracker.openbittorrent.com:80/announce",
];

// ============================================================================
// Bencode Parser & Encoder
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BVal {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<BVal>),
    Dict(Vec<(Vec<u8>, BVal)>),
}

impl BVal {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            BVal::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            BVal::Bytes(b) => Some(b.as_slice()),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<String> {
        self.as_bytes()
            .map(|b| String::from_utf8_lossy(b).to_string())
    }

    pub fn as_list(&self) -> Option<&[BVal]> {
        match self {
            BVal::List(l) => Some(l.as_slice()),
            _ => None,
        }
    }

    pub fn dict_get(&self, key: &[u8]) -> Option<&BVal> {
        match self {
            BVal::Dict(entries) => entries.iter().find(|(k, _)| k.as_slice() == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

pub fn parse_bencode_with_info_span(data: &[u8]) -> Result<(BVal, Option<(usize, usize)>, usize)> {
    let mut pos = 0usize;
    let mut info_span = None;
    let val = parse_bval_inner(data, &mut pos, true, &mut info_span, 0)?;
    Ok((val, info_span, pos))
}

pub fn parse_bencode(data: &[u8]) -> Result<BVal> {
    let (val, _, _) = parse_bencode_with_info_span(data)?;
    Ok(val)
}

fn parse_bval_inner(
    data: &[u8],
    pos: &mut usize,
    capture_info: bool,
    info_span: &mut Option<(usize, usize)>,
    depth: usize,
) -> Result<BVal> {
    if depth > 64 {
        anyhow::bail!("Bencode nesting depth exceeded");
    }
    if *pos >= data.len() {
        anyhow::bail!("Unexpected end of bencode input");
    }
    match data[*pos] {
        b'i' => {
            *pos += 1;
            let start = *pos;
            while *pos < data.len() && data[*pos] != b'e' {
                *pos += 1;
            }
            if *pos >= data.len() {
                anyhow::bail!("Unterminated bencode integer");
            }
            let s = std::str::from_utf8(&data[start..*pos]).context("Invalid UTF-8 in bencode int")?;
            let num: i64 = s.parse().with_context(|| format!("Invalid bencode integer '{}'", s))?;
            *pos += 1;
            Ok(BVal::Int(num))
        }
        b'l' => {
            *pos += 1;
            let mut items = Vec::new();
            while *pos < data.len() && data[*pos] != b'e' {
                items.push(parse_bval_inner(data, pos, false, info_span, depth + 1)?);
            }
            if *pos >= data.len() {
                anyhow::bail!("Unterminated bencode list");
            }
            *pos += 1;
            Ok(BVal::List(items))
        }
        b'd' => {
            *pos += 1;
            let mut entries = Vec::new();
            while *pos < data.len() && data[*pos] != b'e' {
                let key = parse_bencode_bytes(data, pos)?;
                let val_start = *pos;
                let is_info = capture_info && depth == 0 && key.as_slice() == b"info";
                let val = parse_bval_inner(data, pos, false, info_span, depth + 1)?;
                let val_end = *pos;
                if is_info {
                    *info_span = Some((val_start, val_end));
                }
                entries.push((key, val));
            }
            if *pos >= data.len() {
                anyhow::bail!("Unterminated bencode dictionary");
            }
            *pos += 1;
            Ok(BVal::Dict(entries))
        }
        b'0'..=b'9' => {
            let bytes = parse_bencode_bytes(data, pos)?;
            Ok(BVal::Bytes(bytes))
        }
        other => anyhow::bail!("Invalid bencode token byte: 0x{:02x}", other),
    }
}

fn parse_bencode_bytes(data: &[u8], pos: &mut usize) -> Result<Vec<u8>> {
    let start = *pos;
    while *pos < data.len() && data[*pos] != b':' {
        if !data[*pos].is_ascii_digit() {
            anyhow::bail!("Invalid bencode string length digit");
        }
        *pos += 1;
    }
    if *pos >= data.len() || *pos == start {
        anyhow::bail!("Invalid bencode byte string header");
    }
    let len_str = std::str::from_utf8(&data[start..*pos])?;
    let len: usize = len_str.parse().context("Bencode length overflow")?;
    *pos += 1; // skip ':'
    let end = pos.checked_add(len).context("Bencode string bounds overflow")?;
    if end > data.len() {
        anyhow::bail!("Truncated bencode byte string");
    }
    let out = data[*pos..end].to_vec();
    *pos = end;
    Ok(out)
}

pub fn encode_bencode(val: &BVal) -> Vec<u8> {
    let mut out = Vec::new();
    encode_bval_into(val, &mut out);
    out
}

fn encode_bval_into(val: &BVal, out: &mut Vec<u8>) {
    match val {
        BVal::Int(n) => {
            out.push(b'i');
            out.extend_from_slice(n.to_string().as_bytes());
            out.push(b'e');
        }
        BVal::Bytes(b) => {
            out.extend_from_slice(b.len().to_string().as_bytes());
            out.push(b':');
            out.extend_from_slice(b);
        }
        BVal::List(items) => {
            out.push(b'l');
            for item in items {
                encode_bval_into(item, out);
            }
            out.push(b'e');
        }
        BVal::Dict(entries) => {
            out.push(b'd');
            let mut sorted = entries.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in sorted {
                out.extend_from_slice(k.len().to_string().as_bytes());
                out.push(b':');
                out.extend_from_slice(&k);
                encode_bval_into(&v, out);
            }
            out.push(b'e');
        }
    }
}

// ============================================================================
// Torrent & Magnet Data Structures
// ============================================================================

#[derive(Debug, Clone)]
pub struct MagnetLink {
    pub info_hash: [u8; 20],
    pub info_hash_hex: String,
    pub display_name: Option<String>,
    pub trackers: Vec<String>,
    pub exact_length: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct TorrentFileLayout {
    pub rel_components: Vec<String>,
    pub rel_path_display: String,
    pub length: u64,
    pub global_start: u64,
    pub global_end: u64,
}

#[derive(Debug, Clone)]
pub struct ParsedTorrent {
    pub info_hash: [u8; 20],
    pub info_hash_hex: String,
    pub raw_info: Vec<u8>,
    pub name: String,
    pub piece_length: u64,
    pub piece_hashes: Vec<[u8; 20]>,
    pub files: Vec<TorrentFileLayout>,
    pub total_size: u64,
    pub trackers: Vec<String>,
    pub is_multi_file: bool,
}

impl ParsedTorrent {
    pub fn to_torrent_info(&self, is_magnet: bool) -> TorrentInfo {
        TorrentInfo {
            info_hash: self.info_hash_hex.clone(),
            name: self.name.clone(),
            total_size: self.total_size,
            piece_length: self.piece_length,
            piece_count: self.piece_hashes.len(),
            trackers: self.trackers.clone(),
            files: self
                .files
                .iter()
                .map(|f| TorrentFileEntry {
                    path: f.rel_path_display.clone(),
                    length: f.length,
                })
                .collect(),
            is_multi_file: self.is_multi_file,
            is_magnet,
        }
    }
}

pub fn decode_base32_rfc4648(input: &str) -> Result<Vec<u8>> {
    let trimmed = input.trim().trim_end_matches('=');
    let mut buffer: u32 = 0;
    let mut bits_left: u8 = 0;
    let mut out = Vec::with_capacity((trimmed.len() * 5) / 8);

    for ch in trimmed.bytes() {
        let val = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a',
            b'2'..=b'7' => 26 + (ch - b'2'),
            _ => anyhow::bail!("Invalid Base32 character '{}'", ch as char),
        };
        buffer = (buffer << 5) | (val as u32);
        bits_left += 5;
        if bits_left >= 8 {
            bits_left -= 8;
            out.push(((buffer >> bits_left) & 0xff) as u8);
        }
    }
    Ok(out)
}

pub fn is_magnet_uri(raw: &str) -> bool {
    raw.trim().to_ascii_lowercase().starts_with("magnet:")
}

pub fn is_torrent_reference(raw: &str) -> bool {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.starts_with("magnet:") || lower.starts_with("dc-torrent://") {
        return true;
    }
    let without_frag = lower.split('#').next().unwrap_or(&lower);
    let without_query = without_frag.split('?').next().unwrap_or(without_frag);
    without_query.ends_with(".torrent")
}

pub fn torrent_cache_dir() -> PathBuf {
    let base = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .map(|h| h.join(".download-cepat"))
        .unwrap_or_else(|| std::env::temp_dir().join("download-cepat"));
    base.join("torrents")
}

pub fn cache_torrent_bytes(bytes: &[u8]) -> Result<String> {
    let parsed = parse_torrent_bytes(bytes)?;
    let dir = torrent_cache_dir();
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(format!("{}.torrent", parsed.info_hash_hex));
    std::fs::write(&target, bytes)?;
    Ok(format!("dc-torrent://{}", parsed.info_hash_hex))
}

pub fn load_cached_torrent_bytes(dc_ref: &str) -> Result<Vec<u8>> {
    let hash = dc_ref
        .trim()
        .strip_prefix("dc-torrent://")
        .or_else(|| dc_ref.trim().strip_prefix("dc-torrent:"))
        .context("Invalid dc-torrent reference")?
        .trim_matches('/')
        .to_lowercase();
    if hash.len() != 40 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("Invalid cached torrent info_hash");
    }
    let path = torrent_cache_dir().join(format!("{}.torrent", hash));
    std::fs::read(&path).with_context(|| format!("Cached torrent file not found at {}", path.display()))
}

fn percent_decode_str(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex_str) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex_str, 16) {
                    out.push(val);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn parse_magnet_uri(uri: &str) -> Result<MagnetLink> {
    let trimmed = uri.trim();
    if !is_magnet_uri(trimmed) {
        anyhow::bail!("Not a valid magnet URI");
    }
    let query = trimmed
        .split_once('?')
        .map(|(_, q)| q)
        .unwrap_or_else(|| trimmed.strip_prefix("magnet:").unwrap_or(""));

    let mut info_hash: Option<[u8; 20]> = None;
    let mut display_name: Option<String> = None;
    let mut trackers: Vec<String> = Vec::new();
    let mut exact_length: Option<u64> = None;

    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let key_lower = k.to_ascii_lowercase();
        let val_decoded = percent_decode_str(v);

        match key_lower.as_str() {
            "xt" => {
                let val_lower = val_decoded.to_ascii_lowercase();
                if let Some(hash_part) = val_lower.strip_prefix("urn:btih:") {
                    if hash_part.len() == 40 {
                        let decoded = hex::decode(hash_part)
                            .context("Invalid 40-char hex info_hash in magnet URI")?;
                        if decoded.len() == 20 {
                            let mut arr = [0u8; 20];
                            arr.copy_from_slice(&decoded);
                            info_hash = Some(arr);
                        }
                    } else if hash_part.len() == 32 {
                        let decoded = decode_base32_rfc4648(hash_part)
                            .context("Invalid 32-char base32 info_hash in magnet URI")?;
                        if decoded.len() == 20 {
                            let mut arr = [0u8; 20];
                            arr.copy_from_slice(&decoded);
                            info_hash = Some(arr);
                        }
                    }
                }
            }
            "dn" => {
                let clean = sanitize_filename(&val_decoded);
                if !clean.is_empty() && clean != "download.bin" {
                    display_name = Some(clean);
                }
            }
            "tr" => {
                let tr = val_decoded.trim().to_string();
                if !tr.is_empty() && !trackers.contains(&tr) {
                    trackers.push(tr);
                }
            }
            "xl" => {
                if let Ok(len) = val_decoded.trim().parse::<u64>() {
                    if len > 0 {
                        exact_length = Some(len);
                    }
                }
            }
            _ => {}
        }
    }

    let hash = info_hash.context("Magnet URI does not contain a valid urn:btih info_hash")?;
    let info_hash_hex = hex::encode(hash);

    Ok(MagnetLink {
        info_hash: hash,
        info_hash_hex,
        display_name,
        trackers,
        exact_length,
    })
}

pub fn parse_torrent_bytes(bytes: &[u8]) -> Result<ParsedTorrent> {
    let (root, info_span, _) = parse_bencode_with_info_span(bytes)?;
    let (s, e) = info_span.context("Missing 'info' dictionary in .torrent metainfo")?;
    let raw_info = bytes[s..e].to_vec();
    let info_dict = root.dict_get(b"info").context("Missing 'info' key")?;

    let mut trackers: Vec<String> = Vec::new();
    if let Some(announce) = root.dict_get(b"announce").and_then(|v| v.as_str()) {
        let tr = announce.trim().to_string();
        if !tr.is_empty() {
            trackers.push(tr);
        }
    }
    if let Some(announce_list) = root.dict_get(b"announce-list").and_then(|v| v.as_list()) {
        for tier in announce_list {
            if let Some(urls) = tier.as_list() {
                for u in urls {
                    if let Some(s) = u.as_str() {
                        let tr = s.trim().to_string();
                        if !tr.is_empty() && !trackers.contains(&tr) {
                            trackers.push(tr);
                        }
                    }
                }
            }
        }
    }

    parse_torrent_from_info_dict(info_dict, &raw_info, trackers, None)
}

pub fn parse_torrent_from_raw_info(
    raw_info: &[u8],
    expected_info_hash: Option<[u8; 20]>,
    trackers: Vec<String>,
) -> Result<ParsedTorrent> {
    let info_dict = parse_bencode(raw_info)?;
    parse_torrent_from_info_dict(&info_dict, raw_info, trackers, expected_info_hash)
}

fn parse_torrent_from_info_dict(
    info_dict: &BVal,
    raw_info: &[u8],
    trackers: Vec<String>,
    expected_info_hash: Option<[u8; 20]>,
) -> Result<ParsedTorrent> {
    let mut hasher = Sha1::new();
    hasher.update(raw_info);
    let digest = hasher.finalize();
    let mut info_hash = [0u8; 20];
    info_hash.copy_from_slice(&digest);

    if let Some(exp) = expected_info_hash {
        if exp != info_hash {
            anyhow::bail!(
                "Torrent metadata SHA-1 mismatch: expected {}, got {}",
                hex::encode(exp),
                hex::encode(info_hash)
            );
        }
    }

    let info_hash_hex = hex::encode(info_hash);
    let raw_name = info_dict
        .dict_get(b"name")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| format!("torrent_{}", &info_hash_hex[..12]));
    let name = sanitize_filename(&raw_name);

    let piece_length_i = info_dict
        .dict_get(b"piece length")
        .and_then(|v| v.as_int())
        .context("Missing 'piece length' in torrent info")?;
    if piece_length_i <= 0 {
        anyhow::bail!("Invalid non-positive piece length");
    }
    let piece_length = piece_length_i as u64;

    let pieces_bytes = info_dict
        .dict_get(b"pieces")
        .and_then(|v| v.as_bytes())
        .context("Missing 'pieces' hashes in torrent info")?;
    if pieces_bytes.len() % 20 != 0 || pieces_bytes.is_empty() {
        anyhow::bail!("Invalid 'pieces' byte array length in torrent info");
    }
    let mut piece_hashes = Vec::with_capacity(pieces_bytes.len() / 20);
    for chunk in pieces_bytes.chunks_exact(20) {
        let mut h = [0u8; 20];
        h.copy_from_slice(chunk);
        piece_hashes.push(h);
    }

    let mut files = Vec::new();
    let mut offset: u64 = 0;
    let is_multi_file: bool;

    if let Some(file_list) = info_dict.dict_get(b"files").and_then(|v| v.as_list()) {
        is_multi_file = true;
        for f in file_list {
            let len_i = f
                .dict_get(b"length")
                .and_then(|v| v.as_int())
                .context("Torrent file entry missing length")?;
            if len_i < 0 {
                anyhow::bail!("Torrent file entry has negative length");
            }
            let len = len_i as u64;
            let path_items = f
                .dict_get(b"path")
                .and_then(|v| v.as_list())
                .context("Torrent file entry missing path")?;
            let mut comps = Vec::new();
            for item in path_items {
                if let Some(seg_raw) = item.as_str() {
                    let trimmed = seg_raw.trim();
                    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
                        continue;
                    }
                    let clean_seg = sanitize_filename(trimmed);
                    if !clean_seg.is_empty() && clean_seg != "." && clean_seg != ".." {
                        comps.push(clean_seg);
                    }
                }
            }
            if comps.is_empty() {
                comps.push(format!("file_{}.bin", files.len() + 1));
            }
            let rel_path_display = comps.join("/");
            files.push(TorrentFileLayout {
                rel_components: comps,
                rel_path_display,
                length: len,
                global_start: offset,
                global_end: offset + len,
            });
            offset += len;
        }
    } else {
        is_multi_file = false;
        let len_i = info_dict
            .dict_get(b"length")
            .and_then(|v| v.as_int())
            .context("Single-file torrent missing 'length'")?;
        if len_i < 0 {
            anyhow::bail!("Single-file torrent has negative length");
        }
        let len = len_i as u64;
        files.push(TorrentFileLayout {
            rel_components: vec![name.clone()],
            rel_path_display: name.clone(),
            length: len,
            global_start: 0,
            global_end: len,
        });
        offset = len;
    }

    Ok(ParsedTorrent {
        info_hash,
        info_hash_hex,
        raw_info: raw_info.to_vec(),
        name,
        piece_length,
        piece_hashes,
        files,
        total_size: offset,
        trackers,
        is_multi_file,
    })
}

pub fn inspect_magnet(uri: &str) -> Result<InspectResult> {
    let mag = parse_magnet_uri(uri)?;
    let fallback_name = mag
        .display_name
        .clone()
        .unwrap_or_else(|| format!("torrent_{}", &mag.info_hash_hex[..12]));
    let file_name = sanitize_filename(&fallback_name);

    let torrent_info = TorrentInfo {
        info_hash: mag.info_hash_hex.clone(),
        name: file_name.clone(),
        total_size: mag.exact_length.unwrap_or(0),
        piece_length: 0,
        piece_count: 0,
        trackers: mag.trackers,
        files: Vec::new(),
        is_multi_file: false,
        is_magnet: true,
    };

    Ok(InspectResult {
        url: uri.to_string(),
        final_url: uri.to_string(),
        file_name,
        file_size: mag.exact_length,
        supports_range: true,
        mime_type: Some("application/x-bittorrent".to_string()),
        etag: Some(mag.info_hash_hex),
        last_modified: None,
        suggested_category: "torrents".to_string(),
        media_info: None,
        torrent_info: Some(torrent_info),
    })
}

pub fn inspect_torrent_bytes(source_url: &str, bytes: &[u8]) -> Result<InspectResult> {
    let parsed = parse_torrent_bytes(bytes)?;
    let torrent_info = parsed.to_torrent_info(false);
    Ok(InspectResult {
        url: source_url.to_string(),
        final_url: source_url.to_string(),
        file_name: parsed.name.clone(),
        file_size: Some(parsed.total_size),
        supports_range: true,
        mime_type: Some("application/x-bittorrent".to_string()),
        etag: Some(parsed.info_hash_hex),
        last_modified: None,
        suggested_category: "torrents".to_string(),
        media_info: None,
        torrent_info: Some(torrent_info),
    })
}

pub fn inspect_cached_torrent(dc_ref: &str) -> Result<InspectResult> {
    let bytes = load_cached_torrent_bytes(dc_ref)?;
    inspect_torrent_bytes(dc_ref, &bytes)
}

pub fn inspect_local_torrent_file(path: &Path) -> Result<(String, InspectResult)> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Failed to read torrent file {}", path.display()))?;
    if bytes.len() > MAX_TORRENT_META_BYTES {
        anyhow::bail!("Torrent metainfo exceeds 16 MiB safety limit");
    }
    let dc_ref = cache_torrent_bytes(&bytes)?;
    let res = inspect_torrent_bytes(&dc_ref, &bytes)?;
    Ok((dc_ref, res))
}

// ============================================================================
// Tracker Announce (HTTP/HTTPS & UDP BEP 15)
// ============================================================================

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TrackerAnnounceResult {
    peers: Vec<SocketAddr>,
    seeders: usize,
    leechers: usize,
}

fn generate_peer_id() -> [u8; 20] {
    let mut id = [0u8; 20];
    id[..8].copy_from_slice(b"-DC0100-");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(123456789);
    let mut seed = (nanos as u64) ^ (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    const ALPHANUM: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    for slot in &mut id[8..] {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *slot = ALPHANUM[(seed as usize) % ALPHANUM.len()];
    }
    id
}

fn raw_percent_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for &b in bytes {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn parse_compact_peers_v4(data: &[u8]) -> Vec<SocketAddr> {
    let mut peers = Vec::with_capacity(data.len() / 6);
    for chunk in data.chunks_exact(6) {
        let ip = Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]);
        let port = u16::from_be_bytes([chunk[4], chunk[5]]);
        if port > 0 && !ip.is_unspecified() {
            peers.push(SocketAddr::V4(SocketAddrV4::new(ip, port)));
        }
    }
    peers
}

fn parse_compact_peers_v6(data: &[u8]) -> Vec<SocketAddr> {
    let mut peers = Vec::with_capacity(data.len() / 18);
    for chunk in data.chunks_exact(18) {
        let mut octets = [0u8; 16];
        octets.copy_from_slice(&chunk[..16]);
        let ip = Ipv6Addr::from(octets);
        let port = u16::from_be_bytes([chunk[16], chunk[17]]);
        if port > 0 && !ip.is_unspecified() {
            peers.push(SocketAddr::V6(SocketAddrV6::new(ip, port, 0, 0)));
        }
    }
    peers
}

async fn announce_http_tracker(
    client: &reqwest::Client,
    tracker_url: &str,
    info_hash: &[u8; 20],
    peer_id: &[u8; 20],
    downloaded: u64,
    left: u64,
    event: &str,
) -> Result<TrackerAnnounceResult> {
    let sep = if tracker_url.contains('?') { '&' } else { '?' };
    let url = format!(
        "{}{}info_hash={}&peer_id={}&port=6881&uploaded=0&downloaded={}&left={}&compact=1&numwant=50&event={}",
        tracker_url,
        sep,
        raw_percent_encode(info_hash),
        raw_percent_encode(peer_id),
        downloaded,
        left,
        event
    );

    let resp = tokio::time::timeout(
        Duration::from_secs(6),
        client.get(&url).header("User-Agent", "DownloadCepat/1.0").send(),
    )
    .await
    .context("HTTP tracker timeout")??;

    if !resp.status().is_success() {
        anyhow::bail!("HTTP tracker returned status {}", resp.status());
    }

    let body = resp.bytes().await?;
    let val = parse_bencode(&body)?;
    if let Some(reason) = val.dict_get(b"failure reason").and_then(|v| v.as_str()) {
        anyhow::bail!("Tracker failure: {}", reason);
    }

    let mut peers = Vec::new();
    if let Some(peers_val) = val.dict_get(b"peers") {
        if let Some(compact) = peers_val.as_bytes() {
            peers.extend(parse_compact_peers_v4(compact));
        } else if let Some(list) = peers_val.as_list() {
            for item in list {
                if let (Some(ip_s), Some(port_i)) = (
                    item.dict_get(b"ip").and_then(|v| v.as_str()),
                    item.dict_get(b"port").and_then(|v| v.as_int()),
                ) {
                    if let (Ok(ip), Ok(port)) = (ip_s.parse::<std::net::IpAddr>(), u16::try_from(port_i)) {
                        if port > 0 {
                            peers.push(SocketAddr::new(ip, port));
                        }
                    }
                }
            }
        }
    }

    if let Some(compact6) = val.dict_get(b"peers6").and_then(|v| v.as_bytes()) {
        peers.extend(parse_compact_peers_v6(compact6));
    }

    let seeders = val
        .dict_get(b"complete")
        .and_then(|v| v.as_int())
        .unwrap_or(0)
        .max(0) as usize;
    let leechers = val
        .dict_get(b"incomplete")
        .and_then(|v| v.as_int())
        .unwrap_or(0)
        .max(0) as usize;

    Ok(TrackerAnnounceResult {
        peers,
        seeders,
        leechers,
    })
}

async fn announce_udp_tracker(
    tracker_url: &str,
    info_hash: &[u8; 20],
    peer_id: &[u8; 20],
    downloaded: u64,
    left: u64,
    event_code: u32,
) -> Result<TrackerAnnounceResult> {
    let parsed = Url::parse(tracker_url).context("Invalid UDP tracker URL")?;
    let host = parsed.host_str().context("UDP tracker missing host")?;
    let port = parsed.port().unwrap_or(80);
    let target_addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port)).await?.collect();
    let remote_addr = target_addrs
        .into_iter()
        .find(|a| a.is_ipv4())
        .context("No resolved address for UDP tracker")?;

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(remote_addr).await?;

    let tx_id: u32 = ((std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(42))
        ^ std::process::id())
    .wrapping_add(0x5A17);

    // 1. Connect packet
    let mut connect_req = Vec::with_capacity(16);
    connect_req.extend_from_slice(&0x0000_0417_2710_1980u64.to_be_bytes());
    connect_req.extend_from_slice(&0u32.to_be_bytes()); // action = connect
    connect_req.extend_from_slice(&tx_id.to_be_bytes());
    socket.send(&connect_req).await?;

    let mut resp_buf = [0u8; 2048];
    let n = tokio::time::timeout(Duration::from_secs(4), socket.recv(&mut resp_buf))
        .await
        .context("UDP tracker connect timeout")??;
    if n < 16 {
        anyhow::bail!("Truncated UDP connect response");
    }
    let action = u32::from_be_bytes([resp_buf[0], resp_buf[1], resp_buf[2], resp_buf[3]]);
    let resp_tx = u32::from_be_bytes([resp_buf[4], resp_buf[5], resp_buf[6], resp_buf[7]]);
    if action != 0 || resp_tx != tx_id {
        anyhow::bail!("UDP tracker invalid connect handshake");
    }
    let mut conn_bytes = [0u8; 8];
    conn_bytes.copy_from_slice(&resp_buf[8..16]);

    // 2. Announce packet
    let tx_id2 = tx_id.wrapping_add(1);
    let mut ann_req = Vec::with_capacity(98);
    ann_req.extend_from_slice(&conn_bytes);
    ann_req.extend_from_slice(&1u32.to_be_bytes()); // action = announce
    ann_req.extend_from_slice(&tx_id2.to_be_bytes());
    ann_req.extend_from_slice(info_hash);
    ann_req.extend_from_slice(peer_id);
    ann_req.extend_from_slice(&downloaded.to_be_bytes());
    ann_req.extend_from_slice(&left.to_be_bytes());
    ann_req.extend_from_slice(&0u64.to_be_bytes()); // uploaded
    ann_req.extend_from_slice(&event_code.to_be_bytes());
    ann_req.extend_from_slice(&0u32.to_be_bytes()); // ip default
    ann_req.extend_from_slice(&tx_id.wrapping_mul(3).to_be_bytes()); // key
    ann_req.extend_from_slice(&50i32.to_be_bytes()); // num_want
    ann_req.extend_from_slice(&6881u16.to_be_bytes()); // port
    socket.send(&ann_req).await?;

    let n2 = tokio::time::timeout(Duration::from_secs(4), socket.recv(&mut resp_buf))
        .await
        .context("UDP tracker announce timeout")??;
    if n2 < 20 {
        anyhow::bail!("Truncated UDP announce response");
    }
    let act2 = u32::from_be_bytes([resp_buf[0], resp_buf[1], resp_buf[2], resp_buf[3]]);
    let rtx2 = u32::from_be_bytes([resp_buf[4], resp_buf[5], resp_buf[6], resp_buf[7]]);
    if act2 != 1 || rtx2 != tx_id2 {
        anyhow::bail!("UDP tracker invalid announce response");
    }
    let leechers = u32::from_be_bytes([resp_buf[12], resp_buf[13], resp_buf[14], resp_buf[15]]) as usize;
    let seeders = u32::from_be_bytes([resp_buf[16], resp_buf[17], resp_buf[18], resp_buf[19]]) as usize;
    let peers = parse_compact_peers_v4(&resp_buf[20..n2]);

    Ok(TrackerAnnounceResult {
        peers,
        seeders,
        leechers,
    })
}

async fn gather_tracker_peers(
    client: &reqwest::Client,
    trackers: &[String],
    info_hash: &[u8; 20],
    peer_id: &[u8; 20],
    downloaded: u64,
    left: u64,
) -> (Vec<SocketAddr>, usize) {
    let mut futs = Vec::new();
    for tr in trackers {
        let lower = tr.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            let c = client.clone();
            let tr_s = tr.clone();
            let ih = *info_hash;
            let pid = *peer_id;
            futs.push(tokio::spawn(async move {
                announce_http_tracker(&c, &tr_s, &ih, &pid, downloaded, left, "started").await
            }));
        } else if lower.starts_with("udp://") {
            let tr_s = tr.clone();
            let ih = *info_hash;
            let pid = *peer_id;
            futs.push(tokio::spawn(async move {
                announce_udp_tracker(&tr_s, &ih, &pid, downloaded, left, 2).await
            }));
        }
    }

    let results = join_all(futs).await;
    let mut seen = HashSet::new();
    let mut all_peers = Vec::new();
    let mut total_seeds = 0usize;
    for res in results {
        if let Ok(Ok(ann)) = res {
            total_seeds = total_seeds.saturating_add(ann.seeders);
            for p in ann.peers {
                if seen.insert(p) {
                    all_peers.push(p);
                }
            }
        }
    }
    (all_peers, total_seeds)
}

// ============================================================================
// Torrent Checkpointing & Disk Writer
// ============================================================================

#[derive(Serialize, Deserialize, Debug, Clone)]
struct TorrentCheckpoint {
    info_hash_hex: String,
    raw_info: Vec<u8>,
    verified: Vec<bool>,
    downloaded_bytes: u64,
}

pub fn torrent_checkpoint_path(destination_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.dctmeta", destination_path.display()))
}

fn save_torrent_checkpoint(
    meta_path: &Path,
    parsed: &ParsedTorrent,
    verified: &[bool],
    downloaded_bytes: u64,
) {
    let cp = TorrentCheckpoint {
        info_hash_hex: parsed.info_hash_hex.clone(),
        raw_info: parsed.raw_info.clone(),
        verified: verified.to_vec(),
        downloaded_bytes,
    };
    if let Ok(json) = serde_json::to_vec(&cp) {
        let tmp = PathBuf::from(format!("{}.tmp", meta_path.display()));
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, meta_path);
        }
    }
}

fn load_torrent_checkpoint(meta_path: &Path) -> Option<TorrentCheckpoint> {
    let data = std::fs::read(meta_path).ok()?;
    serde_json::from_slice(&data).ok()
}

fn resolve_file_disk_path(destination_path: &Path, parsed: &ParsedTorrent, file: &TorrentFileLayout) -> PathBuf {
    if !parsed.is_multi_file {
        return destination_path.to_path_buf();
    }
    let mut out = destination_path.to_path_buf();
    for comp in &file.rel_components {
        out.push(comp);
    }
    out
}

fn write_piece_to_disk(
    destination_path: &Path,
    parsed: &ParsedTorrent,
    piece_index: usize,
    data: &[u8],
) -> Result<()> {
    let piece_start = (piece_index as u64) * parsed.piece_length;
    let piece_end = piece_start + (data.len() as u64);

    for file in &parsed.files {
        if file.global_end <= piece_start || file.global_start >= piece_end {
            continue;
        }
        let ov_start = piece_start.max(file.global_start);
        let ov_end = piece_end.min(file.global_end);
        if ov_end <= ov_start {
            continue;
        }
        let src_start = (ov_start - piece_start) as usize;
        let src_end = (ov_end - piece_start) as usize;
        let file_offset = ov_start - file.global_start;

        let target = resolve_file_disk_path(destination_path, parsed, file);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .open(&target)
            .with_context(|| format!("Failed opening torrent output file {}", target.display()))?;
        f.seek(SeekFrom::Start(file_offset))?;
        f.write_all(&data[src_start..src_end])?;
    }
    Ok(())
}

// ============================================================================
// Peer Wire Protocol & Swarm Download Engine
// ============================================================================

#[allow(dead_code)]
struct SwarmPieces {
    verified: Vec<bool>,
    in_flight: Vec<bool>,
    piece_count: usize,
    downloaded_bytes: u64,
    total_size: u64,
    piece_length: u64,
    trackers: Vec<String>,
}

impl SwarmPieces {
    fn from_parsed(parsed: &ParsedTorrent, existing_verified: Option<&[bool]>) -> Self {
        let count = parsed.piece_hashes.len();
        let mut verified = vec![false; count];
        let mut dl: u64 = 0;
        if let Some(prev) = existing_verified {
            if prev.len() == count {
                for (i, &v) in prev.iter().enumerate() {
                    verified[i] = v;
                    if v {
                        dl += piece_byte_size(i, parsed.total_size, parsed.piece_length, count);
                    }
                }
            }
        }
        Self {
            verified,
            in_flight: vec![false; count],
            piece_count: count,
            downloaded_bytes: dl,
            total_size: parsed.total_size,
            piece_length: parsed.piece_length,
            trackers: parsed.trackers.clone(),
        }
    }

    fn all_verified(&self) -> bool {
        self.piece_count > 0 && self.verified.iter().all(|&v| v)
    }

    fn next_piece_for_peer(&mut self, bitfield: &[bool]) -> Option<usize> {
        for idx in 0..self.piece_count {
            if !self.verified[idx] && !self.in_flight[idx] {
                if bitfield.get(idx).copied().unwrap_or(false) {
                    self.in_flight[idx] = true;
                    return Some(idx);
                }
            }
        }
        None
    }
}

fn piece_byte_size(index: usize, total_size: u64, piece_length: u64, piece_count: usize) -> u64 {
    if piece_count == 0 || total_size == 0 {
        return 0;
    }
    if index + 1 < piece_count {
        piece_length
    } else {
        let rem = total_size % piece_length;
        if rem == 0 {
            piece_length
        } else {
            rem
        }
    }
}

struct SharedSwarm {
    info_hash: [u8; 20],
    peer_id: [u8; 20],
    parsed: Mutex<Option<ParsedTorrent>>,
    pieces: Mutex<Option<SwarmPieces>>,
    metadata_state: Mutex<Option<MagnetMetadataAssembler>>,
    metadata_notify: Notify,
    completion_notify: Notify,
    disk_lock: std::sync::Mutex<()>,
    destination_path: PathBuf,
    meta_path: PathBuf,
    is_finished: AtomicBool,
    live_bytes: AtomicU64,
    total_size_atomic: AtomicU64,
    active_peers: AtomicUsize,
    known_seeds: AtomicUsize,
    trackers: Vec<String>,
}

struct MagnetMetadataAssembler {
    metadata_size: usize,
    total_pieces: usize,
    received: Vec<Option<Vec<u8>>>,
    requested: Vec<bool>,
}

impl MagnetMetadataAssembler {
    fn new(size: usize) -> Self {
        let total = (size + BLOCK_SIZE - 1) / BLOCK_SIZE;
        Self {
            metadata_size: size,
            total_pieces: total,
            received: vec![None; total],
            requested: vec![false; total],
        }
    }

    fn next_missing_piece(&mut self) -> Option<usize> {
        for i in 0..self.total_pieces {
            if self.received[i].is_none() && !self.requested[i] {
                self.requested[i] = true;
                return Some(i);
            }
        }
        for i in 0..self.total_pieces {
            if self.received[i].is_none() {
                return Some(i);
            }
        }
        None
    }

    fn store_piece(&mut self, index: usize, bytes: Vec<u8>) -> Option<Vec<u8>> {
        if index < self.total_pieces && self.received[index].is_none() {
            self.received[index] = Some(bytes);
        }
        if self.received.iter().all(|p| p.is_some()) {
            let mut full = Vec::with_capacity(self.metadata_size);
            for part in self.received.iter().flatten() {
                full.extend_from_slice(part);
            }
            if full.len() > self.metadata_size {
                full.truncate(self.metadata_size);
            }
            return Some(full);
        }
        None
    }
}

fn build_handshake(info_hash: &[u8; 20], peer_id: &[u8; 20]) -> [u8; 68] {
    let mut pkt = [0u8; 68];
    pkt[0] = 19;
    pkt[1..20].copy_from_slice(BT_PROTOCOL_STR);
    // BEP 10 Extension Protocol bit (reserved[5] bit 4 -> 0x10)
    pkt[25] = 0x10;
    pkt[28..48].copy_from_slice(info_hash);
    pkt[48..68].copy_from_slice(peer_id);
    pkt
}

async fn send_peer_msg(stream: &mut TcpStream, msg_id: Option<u8>, payload: &[u8]) -> Result<()> {
    let total_len: u32 = match msg_id {
        Some(_) => (1 + payload.len()) as u32,
        None => 0,
    };
    stream.write_all(&total_len.to_be_bytes()).await?;
    if let Some(id) = msg_id {
        stream.write_all(&[id]).await?;
        if !payload.is_empty() {
            stream.write_all(payload).await?;
        }
    }
    Ok(())
}

async fn send_extended_handshake(stream: &mut TcpStream) -> Result<()> {
    let dict = BVal::Dict(vec![(
        b"m".to_vec(),
        BVal::Dict(vec![(b"ut_metadata".to_vec(), BVal::Int(1))]),
    )]);
    let encoded = encode_bencode(&dict);
    let mut payload = Vec::with_capacity(1 + encoded.len());
    payload.push(0); // extended handshake id = 0
    payload.extend_from_slice(&encoded);
    send_peer_msg(stream, Some(20), &payload).await
}

async fn send_ut_metadata_request(stream: &mut TcpStream, peer_ext_id: u8, piece: usize) -> Result<()> {
    let dict = BVal::Dict(vec![
        (b"msg_type".to_vec(), BVal::Int(0)),
        (b"piece".to_vec(), BVal::Int(piece as i64)),
    ]);
    let encoded = encode_bencode(&dict);
    let mut payload = Vec::with_capacity(1 + encoded.len());
    payload.push(peer_ext_id);
    payload.extend_from_slice(&encoded);
    send_peer_msg(stream, Some(20), &payload).await
}

async fn send_block_request(
    stream: &mut TcpStream,
    piece_idx: u32,
    begin: u32,
    length: u32,
) -> Result<()> {
    let mut buf = [0u8; 12];
    buf[0..4].copy_from_slice(&piece_idx.to_be_bytes());
    buf[4..8].copy_from_slice(&begin.to_be_bytes());
    buf[8..12].copy_from_slice(&length.to_be_bytes());
    send_peer_msg(stream, Some(6), &buf).await
}

struct ActivePieceWork {
    piece_index: usize,
    piece_size: usize,
    buffer: Vec<u8>,
    received_blocks: Vec<bool>,
    num_blocks: usize,
    next_to_request: usize,
    in_flight_blocks: usize,
}

impl ActivePieceWork {
    fn new(piece_index: usize, piece_size: usize) -> Self {
        let num_blocks = (piece_size + BLOCK_SIZE - 1) / BLOCK_SIZE;
        Self {
            piece_index,
            piece_size,
            buffer: vec![0u8; piece_size],
            received_blocks: vec![false; num_blocks],
            num_blocks,
            next_to_request: 0,
            in_flight_blocks: 0,
        }
    }

    fn is_complete(&self) -> bool {
        self.received_blocks.iter().all(|&b| b)
    }
}

async fn read_framed_message(stream: &mut TcpStream, controller: &DownloadController) -> Result<Option<(u8, Vec<u8>)>> {
    let mut len_buf = [0u8; 4];
    tokio::select! {
        _ = controller.notified() => return Ok(None),
        res = tokio::time::timeout(Duration::from_secs(20), stream.read_exact(&mut len_buf)) => {
            res.context("Peer frame header read timeout")??;
        }
    }
    let msg_len = u32::from_be_bytes(len_buf) as usize;
    if msg_len == 0 {
        // Keep-alive
        return Ok(Some((255, Vec::new())));
    }
    if msg_len > 2 * 1024 * 1024 {
        anyhow::bail!("Peer sent oversized frame: {}", msg_len);
    }
    let mut payload = vec![0u8; msg_len];
    tokio::select! {
        _ = controller.notified() => return Ok(None),
        res = tokio::time::timeout(Duration::from_secs(20), stream.read_exact(&mut payload)) => {
            res.context("Peer payload read timeout")??;
        }
    }
    let id = payload[0];
    payload.drain(0..1);
    Ok(Some((id, payload)))
}

async fn run_peer_session(
    addr: SocketAddr,
    swarm: Arc<SharedSwarm>,
    controller: DownloadController,
    limiter: Option<Arc<BandwidthLimiter>>,
) -> Result<()> {
    if controller.is_stopped() || swarm.is_finished.load(Ordering::Relaxed) {
        return Ok(());
    }

    let mut stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr))
        .await
        .context("Peer TCP connect timeout")??;

    let handshake = build_handshake(&swarm.info_hash, &swarm.peer_id);
    stream.write_all(&handshake).await?;

    let mut peer_hs = [0u8; 68];
    tokio::time::timeout(Duration::from_secs(6), stream.read_exact(&mut peer_hs))
        .await
        .context("Peer handshake read timeout")??;

    if peer_hs[0] != 19 || &peer_hs[1..20] != BT_PROTOCOL_STR || peer_hs[28..48] != swarm.info_hash {
        anyhow::bail!("Peer handshake info_hash mismatch");
    }

    swarm.active_peers.fetch_add(1, Ordering::Relaxed);
    struct PeerGuard {
        swarm: Arc<SharedSwarm>,
        held_piece: Option<usize>,
    }
    impl Drop for PeerGuard {
        fn drop(&mut self) {
            self.swarm.active_peers.fetch_sub(1, Ordering::Relaxed);
            if let Some(piece_idx) = self.held_piece.take() {
                if let Ok(mut guard) = self.swarm.pieces.try_lock() {
                    if let Some(ref mut sp) = *guard {
                        if piece_idx < sp.in_flight.len() {
                            sp.in_flight[piece_idx] = false;
                        }
                    }
                }
            }
        }
    }
    let mut guard = PeerGuard {
        swarm: swarm.clone(),
        held_piece: None,
    };

    let peer_supports_ext = (peer_hs[25] & 0x10) != 0;
    if peer_supports_ext {
        let _ = send_extended_handshake(&mut stream).await;
    }

    // Send interested right away so peer unchokes us promptly
    send_peer_msg(&mut stream, Some(2), &[]).await?;

    let mut peer_bitfield: Vec<bool> = Vec::new();
    let mut choked_by_peer = true;
    let mut peer_ut_metadata_id: Option<u8> = None;
    let mut active_piece: Option<ActivePieceWork> = None;
    let mut metadata_requested = false;
    let aborted_dummy = AtomicBool::new(false);

    loop {
        if controller.is_stopped() || swarm.is_finished.load(Ordering::Relaxed) {
            break;
        }

        // Request magnet metadata if we don't have ParsedTorrent yet and peer supports ut_metadata
        if swarm.parsed.lock().await.is_none() && !metadata_requested {
            if let Some(ext_id) = peer_ut_metadata_id {
                let mut meta_guard = swarm.metadata_state.lock().await;
                if let Some(ref mut asm) = *meta_guard {
                    if let Some(m_idx) = asm.next_missing_piece() {
                        metadata_requested = true;
                        drop(meta_guard);
                        let _ = send_ut_metadata_request(&mut stream, ext_id, m_idx).await;
                    }
                }
            }
        }

        // If unchoked & we have ParsedTorrent, acquire piece work & pipeline block requests
        if !choked_by_peer {
            if active_piece.is_none() {
                let parsed_opt = swarm.parsed.lock().await.clone();
                if let Some(parsed) = parsed_opt {
                    if peer_bitfield.len() < parsed.piece_hashes.len() {
                        peer_bitfield.resize(parsed.piece_hashes.len(), false);
                    }
                    let mut pieces_guard = swarm.pieces.lock().await;
                    if let Some(ref mut sp) = *pieces_guard {
                        if sp.all_verified() {
                            swarm.is_finished.store(true, Ordering::SeqCst);
                            swarm.completion_notify.notify_waiters();
                            break;
                        }
                        if let Some(piece_idx) = sp.next_piece_for_peer(&peer_bitfield) {
                            guard.held_piece = Some(piece_idx);
                            let p_size = piece_byte_size(
                                piece_idx,
                                parsed.total_size,
                                parsed.piece_length,
                                parsed.piece_hashes.len(),
                            ) as usize;
                            active_piece = Some(ActivePieceWork::new(piece_idx, p_size));
                        }
                    }
                }
            }

            if let Some(ref mut work) = active_piece {
                while work.next_to_request < work.num_blocks
                    && work.in_flight_blocks < MAX_PIPELINED_BLOCKS
                {
                    let b_idx = work.next_to_request;
                    let begin = b_idx * BLOCK_SIZE;
                    let req_len = (work.piece_size - begin).min(BLOCK_SIZE);
                    send_block_request(
                        &mut stream,
                        work.piece_index as u32,
                        begin as u32,
                        req_len as u32,
                    )
                    .await?;
                    work.next_to_request += 1;
                    work.in_flight_blocks += 1;
                }
            }
        }

        let Some((msg_id, payload)) = read_framed_message(&mut stream, &controller).await? else {
            break;
        };

        match msg_id {
            255 => continue, // keep-alive
            0 => {
                choked_by_peer = true;
            }
            1 => {
                choked_by_peer = false;
            }
            4 => {
                // Have
                if payload.len() >= 4 {
                    let idx = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
                    if idx >= peer_bitfield.len() && idx < 2_000_000 {
                        peer_bitfield.resize(idx + 1, false);
                    }
                    if idx < peer_bitfield.len() {
                        peer_bitfield[idx] = true;
                    }
                }
            }
            5 => {
                // Bitfield
                let mut bf = Vec::with_capacity(payload.len() * 8);
                for &byte in &payload {
                    for bit in (0..8).rev() {
                        bf.push(((byte >> bit) & 1) == 1);
                    }
                }
                peer_bitfield = bf;
            }
            7 => {
                // Piece block
                if payload.len() >= 8 {
                    let p_idx = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
                    let begin = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]) as usize;
                    let block_data = &payload[8..];

                    if let Some(ref lim) = limiter {
                        lim.set_limit(controller.speed_limit().unwrap_or(0));
                        lim.throttle(block_data.len() as u64, &controller, &aborted_dummy)
                            .await;
                    }

                    let mut completed_piece: Option<(usize, Vec<u8>)> = None;
                    if let Some(ref mut work) = active_piece {
                        if work.piece_index == p_idx && begin + block_data.len() <= work.piece_size {
                            work.buffer[begin..begin + block_data.len()].copy_from_slice(block_data);
                            let block_num = begin / BLOCK_SIZE;
                            if block_num < work.received_blocks.len() && !work.received_blocks[block_num] {
                                work.received_blocks[block_num] = true;
                                work.in_flight_blocks = work.in_flight_blocks.saturating_sub(1);
                                swarm.live_bytes.fetch_add(block_data.len() as u64, Ordering::Relaxed);
                            }
                            if work.is_complete() {
                                completed_piece = Some((work.piece_index, std::mem::take(&mut work.buffer)));
                            }
                        }
                    }

                    if let Some((idx, buf)) = completed_piece {
                        active_piece = None;
                        guard.held_piece = None;
                        let parsed_opt = swarm.parsed.lock().await.clone();
                        if let Some(parsed) = parsed_opt {
                            let mut sha = Sha1::new();
                            sha.update(&buf);
                            let digest = sha.finalize();
                            let valid = digest.as_slice() == parsed.piece_hashes[idx];
                            if valid {
                                {
                                    let _io = swarm.disk_lock.lock().unwrap();
                                    write_piece_to_disk(&swarm.destination_path, &parsed, idx, &buf)?;
                                }
                                let mut pieces_guard = swarm.pieces.lock().await;
                                if let Some(ref mut sp) = *pieces_guard {
                                    sp.in_flight[idx] = false;
                                    if !sp.verified[idx] {
                                        sp.verified[idx] = true;
                                        sp.downloaded_bytes += buf.len() as u64;
                                    }
                                    swarm.live_bytes.store(sp.downloaded_bytes, Ordering::Relaxed);
                                    if idx % 8 == 0 || sp.all_verified() {
                                        save_torrent_checkpoint(
                                            &swarm.meta_path,
                                            &parsed,
                                            &sp.verified,
                                            sp.downloaded_bytes,
                                        );
                                    }
                                    if sp.all_verified() {
                                        swarm.is_finished.store(true, Ordering::SeqCst);
                                        swarm.completion_notify.notify_waiters();
                                        break;
                                    }
                                }
                            } else {
                                // SHA-1 failed; release piece to retry from swarm
                                let mut pieces_guard = swarm.pieces.lock().await;
                                if let Some(ref mut sp) = *pieces_guard {
                                    sp.in_flight[idx] = false;
                                }
                            }
                        }
                    }
                }
            }
            20 => {
                // Extended message (BEP 10 / BEP 9)
                if !payload.is_empty() {
                    let ext_id = payload[0];
                    let ext_body = &payload[1..];
                    if ext_id == 0 {
                        if let Ok(dict) = parse_bencode(ext_body) {
                            if let Some(m) = dict.dict_get(b"m") {
                                if let Some(ut_id) = m.dict_get(b"ut_metadata").and_then(|v| v.as_int()) {
                                    if ut_id > 0 && ut_id <= 255 {
                                        peer_ut_metadata_id = Some(ut_id as u8);
                                    }
                                }
                            }
                            if let Some(msize) = dict.dict_get(b"metadata_size").and_then(|v| v.as_int()) {
                                if msize > 0 && (msize as usize) <= MAX_TORRENT_META_BYTES {
                                    let mut meta_guard = swarm.metadata_state.lock().await;
                                    if meta_guard.is_none() && swarm.parsed.lock().await.is_none() {
                                        *meta_guard = Some(MagnetMetadataAssembler::new(msize as usize));
                                    }
                                }
                            }
                        }
                    } else if ext_id == 1 {
                        // Our local id for ut_metadata is 1
                        if let Ok((hdr, _, end_pos)) = parse_bencode_with_info_span(ext_body) {
                            let msg_type = hdr.dict_get(b"msg_type").and_then(|v| v.as_int()).unwrap_or(-1);
                            let piece_idx = hdr.dict_get(b"piece").and_then(|v| v.as_int()).unwrap_or(-1);
                            if msg_type == 1 && piece_idx >= 0 && end_pos < ext_body.len() {
                                metadata_requested = false;
                                let piece_bytes = ext_body[end_pos..].to_vec();
                                let assembled_opt = {
                                    let mut meta_guard = swarm.metadata_state.lock().await;
                                    if let Some(ref mut asm) = *meta_guard {
                                        asm.store_piece(piece_idx as usize, piece_bytes)
                                    } else {
                                        None
                                    }
                                };
                                if let Some(raw_info) = assembled_opt {
                                    if let Ok(parsed) = parse_torrent_from_raw_info(
                                        &raw_info,
                                        Some(swarm.info_hash),
                                        swarm.trackers.clone(),
                                    ) {
                                        ensure_sufficient_disk_space(
                                            swarm.destination_path.parent().unwrap_or(&swarm.destination_path),
                                            parsed.total_size,
                                        )?;
                                        if parsed.is_multi_file {
                                            let _ = std::fs::create_dir_all(&swarm.destination_path);
                                        }
                                        swarm.total_size_atomic.store(parsed.total_size, Ordering::Relaxed);
                                        let mut parsed_guard = swarm.parsed.lock().await;
                                        if parsed_guard.is_none() {
                                            let sp = SwarmPieces::from_parsed(&parsed, None);
                                            *parsed_guard = Some(parsed);
                                            *swarm.pieces.lock().await = Some(sp);
                                            swarm.metadata_notify.notify_waiters();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Release active unverified piece on graceful pause/exit
    if let Some(work) = active_piece.take() {
        let mut pieces_guard = swarm.pieces.lock().await;
        if let Some(ref mut sp) = *pieces_guard {
            if work.piece_index < sp.in_flight.len() {
                sp.in_flight[work.piece_index] = false;
            }
        }
    }
    guard.held_piece = None;
    Ok(())
}

fn build_torrent_segments(
    piece_count: usize,
    total_size: u64,
    verified: &[bool],
) -> Vec<SegmentProgress> {
    if total_size == 0 || piece_count == 0 {
        return Vec::new();
    }
    let buckets = piece_count.min(16).max(1);
    let mut out = Vec::with_capacity(buckets);
    for b in 0..buckets {
        let p_start = (b * piece_count) / buckets;
        let p_end = (((b + 1) * piece_count) / buckets).max(p_start + 1).min(piece_count);
        let done_count = verified[p_start..p_end].iter().filter(|&&v| v).count();
        let seg_count = p_end - p_start;
        let byte_start = (b as u64 * total_size) / buckets as u64;
        let byte_end = (((b + 1) as u64 * total_size) / buckets as u64)
            .saturating_sub(1)
            .max(byte_start);
        let span = byte_end - byte_start + 1;
        let dl = if done_count == seg_count {
            span
        } else {
            (span * done_count as u64) / seg_count as u64
        };
        out.push(SegmentProgress {
            segment_index: b,
            start_byte: byte_start,
            end_byte: byte_end,
            current_byte: byte_start + dl,
            downloaded_bytes: dl,
            is_completed: done_count == seg_count,
        });
    }
    out
}

pub async fn download_torrent(
    task_id: &str,
    source_url: &str,
    destination_path: &Path,
    telemetry_tx: Option<broadcast::Sender<ProgressTelemetry>>,
    controller: DownloadController,
    options: &DownloadOptions<'_>,
) -> Result<()> {
    let client = crate::inspector::build_http_clients(options.proxy_url)?.0;

    // 1. Resolve initial magnet or .torrent metainfo (including resume checkpoint)
    let meta_path = torrent_checkpoint_path(destination_path);
    let mut initial_parsed: Option<ParsedTorrent> = None;
    let mut resumed_verified: Option<Vec<bool>> = None;
    let info_hash: [u8; 20];
    let mut trackers: Vec<String>;

    if is_magnet_uri(source_url) {
        let mag = parse_magnet_uri(source_url)?;
        info_hash = mag.info_hash;
        trackers = mag.trackers;
        if let Some(cp) = load_torrent_checkpoint(&meta_path) {
            if cp.info_hash_hex == mag.info_hash_hex && !cp.raw_info.is_empty() {
                if let Ok(p) = parse_torrent_from_raw_info(&cp.raw_info, Some(info_hash), trackers.clone()) {
                    resumed_verified = Some(cp.verified);
                    initial_parsed = Some(p);
                }
            }
        }
    } else if source_url.to_ascii_lowercase().starts_with("dc-torrent://") {
        let bytes = load_cached_torrent_bytes(source_url)?;
        let parsed = parse_torrent_bytes(&bytes)?;
        info_hash = parsed.info_hash;
        trackers = parsed.trackers.clone();
        if let Some(cp) = load_torrent_checkpoint(&meta_path) {
            if cp.info_hash_hex == parsed.info_hash_hex {
                resumed_verified = Some(cp.verified);
            }
        }
        initial_parsed = Some(parsed);
    } else if source_url.to_ascii_lowercase().starts_with("http://")
        || source_url.to_ascii_lowercase().starts_with("https://")
    {
        let resp = crate::inspector::send_with_redirects(
            &client,
            reqwest::Method::GET,
            source_url,
            options.cookie,
            options.referer,
            options.user_agent,
            crate::inspector::DEFAULT_USER_AGENT,
            None,
            options.auth_username,
            options.auth_password,
        )
        .await?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed downloading .torrent metainfo (HTTP {})", resp.status());
        }
        let body = resp.bytes().await?;
        if body.len() > MAX_TORRENT_META_BYTES {
            anyhow::bail!("Remote .torrent exceeds 16 MiB size cap");
        }
        let parsed = parse_torrent_bytes(&body)?;
        info_hash = parsed.info_hash;
        trackers = parsed.trackers.clone();
        if let Some(cp) = load_torrent_checkpoint(&meta_path) {
            if cp.info_hash_hex == parsed.info_hash_hex {
                resumed_verified = Some(cp.verified);
            }
        }
        initial_parsed = Some(parsed);
    } else {
        let local_path = if let Some(stripped) = source_url.strip_prefix("file://") {
            PathBuf::from(stripped)
        } else {
            PathBuf::from(source_url)
        };
        let bytes = std::fs::read(&local_path)
            .with_context(|| format!("Unable to open torrent file {}", local_path.display()))?;
        let parsed = parse_torrent_bytes(&bytes)?;
        info_hash = parsed.info_hash;
        trackers = parsed.trackers.clone();
        if let Some(cp) = load_torrent_checkpoint(&meta_path) {
            if cp.info_hash_hex == parsed.info_hash_hex {
                resumed_verified = Some(cp.verified);
            }
        }
        initial_parsed = Some(parsed);
    }

    if trackers.is_empty() {
        for &d in DEFAULT_PUBLIC_TRACKERS {
            trackers.push(d.to_string());
        }
    }

    let initial_pieces = initial_parsed
        .as_ref()
        .map(|p| SwarmPieces::from_parsed(p, resumed_verified.as_deref()));

    let initial_downloaded = initial_pieces.as_ref().map(|s| s.downloaded_bytes).unwrap_or(0);
    let initial_total = initial_parsed.as_ref().map(|p| p.total_size).unwrap_or(0);

    if let Some(ref p) = initial_parsed {
        let space_root = destination_path.parent().unwrap_or(destination_path);
        ensure_sufficient_disk_space(space_root, p.total_size.saturating_sub(initial_downloaded))?;
        if p.is_multi_file {
            std::fs::create_dir_all(destination_path)?;
        } else if let Some(parent) = destination_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
    } else if let Some(parent) = destination_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let swarm = Arc::new(SharedSwarm {
        info_hash,
        peer_id: generate_peer_id(),
        parsed: Mutex::new(initial_parsed),
        pieces: Mutex::new(initial_pieces),
        metadata_state: Mutex::new(None),
        metadata_notify: Notify::new(),
        completion_notify: Notify::new(),
        disk_lock: std::sync::Mutex::new(()),
        destination_path: destination_path.to_path_buf(),
        meta_path: meta_path.clone(),
        is_finished: AtomicBool::new(false),
        live_bytes: AtomicU64::new(initial_downloaded),
        total_size_atomic: AtomicU64::new(initial_total),
        active_peers: AtomicUsize::new(0),
        known_seeds: AtomicUsize::new(0),
        trackers: trackers.clone(),
    });

    // Fast path if all pieces were already verified from checkpoint
    if let Some(ref sp) = *swarm.pieces.lock().await {
        if sp.all_verified() {
            let _ = std::fs::remove_file(&meta_path);
            if let Some(ref sender) = telemetry_tx {
                let segs = build_torrent_segments(sp.piece_count, sp.total_size, &sp.verified);
                let _ = sender.send(ProgressTelemetry {
                    task_id: task_id.to_string(),
                    total_bytes: Some(sp.total_size),
                    downloaded_bytes: sp.total_size,
                    speed_bytes_per_sec: 0,
                    progress_percent: 100.0,
                    eta_seconds: Some(0),
                    status: TaskStatus::Completed,
                    segments: segs,
                    peers: Some(0),
                    seeds: Some(0),
                });
            }
            return Ok(());
        }
    }

    let speed_limit = controller
        .speed_limit()
        .or(options.speed_limit_bytes_per_sec)
        .unwrap_or(0);
    let limiter = BandwidthLimiter::new(speed_limit);

    // 2. Telemetry monitor task
    let monitor_swarm = swarm.clone();
    let monitor_tx = telemetry_tx.clone();
    let monitor_task_id = task_id.to_string();
    let monitor_done = Arc::new(AtomicBool::new(false));
    let monitor_done_flag = monitor_done.clone();

    let monitor_handle = tokio::spawn(async move {
        let mut last_bytes = monitor_swarm.live_bytes.load(Ordering::Relaxed);
        let mut last_tick = Instant::now();
        let mut speed_window: VecDeque<(Instant, u64)> = VecDeque::new();

        while !monitor_done_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if monitor_done_flag.load(Ordering::Relaxed) {
                break;
            }
            let now = Instant::now();
            let current = monitor_swarm.live_bytes.load(Ordering::Relaxed);
            let total = monitor_swarm.total_size_atomic.load(Ordering::Relaxed);
            let delta = current.saturating_sub(last_bytes);
            last_bytes = current;

            speed_window.push_back((now, delta));
            while let Some(&(t, _)) = speed_window.front() {
                if now.duration_since(t) > Duration::from_secs(3) {
                    speed_window.pop_front();
                } else {
                    break;
                }
            }
            let window_bytes: u64 = speed_window.iter().map(|(_, b)| *b).sum();
            let window_secs = now
                .duration_since(speed_window.front().map(|(t, _)| *t).unwrap_or(last_tick))
                .as_secs_f64()
                .max(0.25);
            let speed_bps = (window_bytes as f64 / window_secs) as u64;
            last_tick = now;

            let (pct, segs) = {
                let pieces_guard = monitor_swarm.pieces.lock().await;
                if let Some(ref sp) = *pieces_guard {
                    let p = if sp.total_size > 0 {
                        ((current.min(sp.total_size) as f64 / sp.total_size as f64) * 100.0) as f32
                    } else {
                        0.0
                    };
                    (
                        p.clamp(0.0, 99.9),
                        build_torrent_segments(sp.piece_count, sp.total_size, &sp.verified),
                    )
                } else {
                    (0.0, Vec::new())
                }
            };

            let eta = if total > current && speed_bps > 0 {
                Some((total - current) / speed_bps)
            } else {
                None
            };

            if let Some(ref sender) = monitor_tx {
                let _ = sender.send(ProgressTelemetry {
                    task_id: monitor_task_id.clone(),
                    total_bytes: if total > 0 { Some(total) } else { None },
                    downloaded_bytes: current,
                    speed_bytes_per_sec: speed_bps,
                    progress_percent: pct,
                    eta_seconds: eta,
                    status: TaskStatus::Downloading,
                    segments: segs,
                    peers: Some(monitor_swarm.active_peers.load(Ordering::Relaxed)),
                    seeds: Some(monitor_swarm.known_seeds.load(Ordering::Relaxed)),
                });
            }
        }
    });

    // 3. Swarm Tracker + Peer Orchestration Loop
    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_PEERS));
    let mut active_handles = Vec::new();
    let mut tried_peers: HashSet<SocketAddr> = HashSet::new();
    let mut round = 0usize;

    while !controller.is_stopped() && !swarm.is_finished.load(Ordering::Relaxed) {
        let dl_now = swarm.live_bytes.load(Ordering::Relaxed);
        let tot_now = swarm.total_size_atomic.load(Ordering::Relaxed);
        let left_now = tot_now.saturating_sub(dl_now);

        let (peers, seeds) = gather_tracker_peers(
            &client,
            &swarm.trackers,
            &swarm.info_hash,
            &swarm.peer_id,
            dl_now,
            left_now,
        )
        .await;

        if seeds > 0 {
            swarm.known_seeds.store(seeds, Ordering::Relaxed);
        }

        if peers.is_empty() && swarm.active_peers.load(Ordering::Relaxed) == 0 && round == 0 {
            monitor_done.store(true, Ordering::Relaxed);
            let _ = monitor_handle.await;
            anyhow::bail!("No peers returned from torrent trackers");
        }

        for peer_addr in peers {
            if !tried_peers.insert(peer_addr) && round < 2 {
                continue;
            }
            let permit = sem.clone().acquire_owned().await?;
            let sw = swarm.clone();
            let ctrl = controller.clone();
            let lim = limiter.clone();
            let handle = tokio::spawn(async move {
                let _permit = permit;
                let _ = run_peer_session(peer_addr, sw, ctrl, lim).await;
            });
            active_handles.push(handle);
        }

        tokio::select! {
            _ = controller.notified() => break,
            _ = swarm.completion_notify.notified() => break,
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
        }

        // If all spawned peers exited without finishing and we exhausted retries, break
        active_handles.retain(|h| !h.is_finished());
        round += 1;
        if active_handles.is_empty() && round >= 4 {
            break;
        }
    }

    for h in active_handles {
        let _ = tokio::time::timeout(Duration::from_secs(2), h).await;
    }

    monitor_done.store(true, Ordering::Relaxed);
    let _ = monitor_handle.await;

    // 4. Final state handling (Canceled / Paused / Completed / Incomplete Error)
    if controller.is_canceled() {
        let _ = std::fs::remove_file(&meta_path);
        let parsed_opt = swarm.parsed.lock().await.clone();
        if let Some(parsed) = parsed_opt {
            if parsed.is_multi_file {
                let _ = std::fs::remove_dir_all(destination_path);
            } else {
                let _ = std::fs::remove_file(destination_path);
            }
        } else {
            let _ = std::fs::remove_file(destination_path);
        }
        if let Some(ref sender) = telemetry_tx {
            let total = swarm.total_size_atomic.load(Ordering::Relaxed);
            let _ = sender.send(ProgressTelemetry {
                task_id: task_id.to_string(),
                total_bytes: if total > 0 { Some(total) } else { None },
                downloaded_bytes: 0,
                speed_bytes_per_sec: 0,
                progress_percent: 0.0,
                eta_seconds: None,
                status: TaskStatus::Canceled,
                segments: vec![],
                peers: Some(0),
                seeds: Some(0),
            });
        }
        return Ok(());
    }

    if controller.is_paused() {
        let parsed_opt = swarm.parsed.lock().await.clone();
        let pieces_guard = swarm.pieces.lock().await;
        if let (Some(parsed), Some(sp)) = (parsed_opt, pieces_guard.as_ref()) {
            save_torrent_checkpoint(&meta_path, &parsed, &sp.verified, sp.downloaded_bytes);
            if let Some(ref sender) = telemetry_tx {
                let pct = if sp.total_size > 0 {
                    ((sp.downloaded_bytes as f64 / sp.total_size as f64) * 100.0) as f32
                } else {
                    0.0
                };
                let _ = sender.send(ProgressTelemetry {
                    task_id: task_id.to_string(),
                    total_bytes: Some(sp.total_size),
                    downloaded_bytes: sp.downloaded_bytes,
                    speed_bytes_per_sec: 0,
                    progress_percent: pct.clamp(0.0, 99.9),
                    eta_seconds: None,
                    status: TaskStatus::Paused,
                    segments: build_torrent_segments(sp.piece_count, sp.total_size, &sp.verified),
                    peers: Some(0),
                    seeds: Some(swarm.known_seeds.load(Ordering::Relaxed)),
                });
            }
        }
        return Ok(());
    }

    let (all_ok, total_sz, dl_sz, segs) = {
        let guard = swarm.pieces.lock().await;
        if let Some(ref sp) = *guard {
            (
                sp.all_verified(),
                sp.total_size,
                sp.downloaded_bytes,
                build_torrent_segments(sp.piece_count, sp.total_size, &sp.verified),
            )
        } else {
            (false, 0, 0, Vec::new())
        }
    };

    if !all_ok {
        let parsed_opt = swarm.parsed.lock().await.clone();
        if let (Some(parsed), Some(sp)) = (parsed_opt, swarm.pieces.lock().await.as_ref()) {
            save_torrent_checkpoint(&meta_path, &parsed, &sp.verified, sp.downloaded_bytes);
        }
        anyhow::bail!(
            "Torrent download stopped before completing all pieces ({} / {} bytes verified)",
            dl_sz,
            total_sz
        );
    }

    let _ = std::fs::remove_file(&meta_path);

    if let Some(ref sender) = telemetry_tx {
        let _ = sender.send(ProgressTelemetry {
            task_id: task_id.to_string(),
            total_bytes: Some(total_sz),
            downloaded_bytes: total_sz,
            speed_bytes_per_sec: 0,
            progress_percent: 100.0,
            eta_seconds: Some(0),
            status: TaskStatus::Completed,
            segments: segs,
            peers: Some(0),
            seeds: Some(swarm.known_seeds.load(Ordering::Relaxed)),
        });
    }

    Ok(())
}

// ============================================================================
// Unit & Loopback Integration Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    fn build_sample_torrent_bytes(
        name: &str,
        piece_len: usize,
        payload: &[u8],
        tracker_url: &str,
        multi_file_splits: Option<&[(&str, usize)]>,
    ) -> Vec<u8> {
        let mut pieces_hashes = Vec::new();
        for chunk in payload.chunks(piece_len) {
            let mut sha = Sha1::new();
            sha.update(chunk);
            pieces_hashes.extend_from_slice(&sha.finalize());
        }

        let mut info_entries = vec![
            (b"name".to_vec(), BVal::Bytes(name.as_bytes().to_vec())),
            (b"piece length".to_vec(), BVal::Int(piece_len as i64)),
            (b"pieces".to_vec(), BVal::Bytes(pieces_hashes)),
        ];

        if let Some(files) = multi_file_splits {
            let list = files
                .iter()
                .map(|(rel, len)| {
                    let comps: Vec<BVal> = rel
                        .split('/')
                        .map(|c| BVal::Bytes(c.as_bytes().to_vec()))
                        .collect();
                    BVal::Dict(vec![
                        (b"length".to_vec(), BVal::Int(*len as i64)),
                        (b"path".to_vec(), BVal::List(comps)),
                    ])
                })
                .collect();
            info_entries.push((b"files".to_vec(), BVal::List(list)));
        } else {
            info_entries.push((b"length".to_vec(), BVal::Int(payload.len() as i64)));
        }

        let root = BVal::Dict(vec![
            (b"announce".to_vec(), BVal::Bytes(tracker_url.as_bytes().to_vec())),
            (b"info".to_vec(), BVal::Dict(info_entries)),
        ]);
        encode_bencode(&root)
    }

    async fn spawn_mock_peer(
        payload: Vec<u8>,
        piece_len: usize,
        raw_info: Vec<u8>,
        expected_hash: [u8; 20],
        advertise_metadata: bool,
    ) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let num_pieces = (payload.len() + piece_len - 1) / piece_len;

        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let p_data = payload.clone();
                let r_info = raw_info.clone();
                tokio::spawn(async move {
                    let mut hs = [0u8; 68];
                    if sock.read_exact(&mut hs).await.is_err() {
                        return;
                    }
                    let mut reply = [0u8; 68];
                    reply[0] = 19;
                    reply[1..20].copy_from_slice(BT_PROTOCOL_STR);
                    if advertise_metadata {
                        reply[25] = 0x10;
                    }
                    reply[28..48].copy_from_slice(&expected_hash);
                    reply[48..68].copy_from_slice(b"-MOCK01-123456789012");
                    if sock.write_all(&reply).await.is_err() {
                        return;
                    }

                    // Send bitfield with all pieces
                    let bf_bytes_len = (num_pieces + 7) / 8;
                    let mut bf = vec![0u8; bf_bytes_len];
                    for idx in 0..num_pieces {
                        bf[idx / 8] |= 1 << (7 - (idx % 8));
                    }
                    let _ = send_peer_msg(&mut sock, Some(5), &bf).await;

                    if advertise_metadata {
                        let ext_dict = BVal::Dict(vec![
                            (
                                b"m".to_vec(),
                                BVal::Dict(vec![(b"ut_metadata".to_vec(), BVal::Int(2))]),
                            ),
                            (b"metadata_size".to_vec(), BVal::Int(r_info.len() as i64)),
                        ]);
                        let enc = encode_bencode(&ext_dict);
                        let mut pl = Vec::with_capacity(1 + enc.len());
                        pl.push(0);
                        pl.extend_from_slice(&enc);
                        let _ = send_peer_msg(&mut sock, Some(20), &pl).await;
                    }

                    // Send unchoke
                    let _ = send_peer_msg(&mut sock, Some(1), &[]).await;

                    let ctrl = DownloadController::new();
                    let mut client_ut_meta_id: u8 = 1;
                    while let Ok(Some((id, body))) = read_framed_message(&mut sock, &ctrl).await {
                        match id {
                            6 if body.len() >= 12 => {
                                let p_idx = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
                                let begin = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
                                let length = u32::from_be_bytes([body[8], body[9], body[10], body[11]]) as usize;
                                let offset = p_idx * piece_len + begin;
                                if offset + length <= p_data.len() {
                                    let mut pl = Vec::with_capacity(8 + length);
                                    pl.extend_from_slice(&(p_idx as u32).to_be_bytes());
                                    pl.extend_from_slice(&(begin as u32).to_be_bytes());
                                    pl.extend_from_slice(&p_data[offset..offset + length]);
                                    let _ = send_peer_msg(&mut sock, Some(7), &pl).await;
                                }
                            }
                            20 if !body.is_empty() => {
                                let ext_id = body[0];
                                let rest = &body[1..];
                                if ext_id == 0 {
                                    if let Ok(d) = parse_bencode(rest) {
                                        if let Some(val) = d
                                            .dict_get(b"m")
                                            .and_then(|m| m.dict_get(b"ut_metadata"))
                                            .and_then(|v| v.as_int())
                                        {
                                            client_ut_meta_id = val as u8;
                                        }
                                    }
                                } else if ext_id == 2 {
                                    if let Ok(req) = parse_bencode(rest) {
                                        let piece_idx = req.dict_get(b"piece").and_then(|v| v.as_int()).unwrap_or(0) as usize;
                                        let start = piece_idx * BLOCK_SIZE;
                                        let end = (start + BLOCK_SIZE).min(r_info.len());
                                        if start < r_info.len() {
                                            let hdr = encode_bencode(&BVal::Dict(vec![
                                                (b"msg_type".to_vec(), BVal::Int(1)),
                                                (b"piece".to_vec(), BVal::Int(piece_idx as i64)),
                                                (b"total_size".to_vec(), BVal::Int(r_info.len() as i64)),
                                            ]));
                                            let mut pl = Vec::with_capacity(1 + hdr.len() + (end - start));
                                            pl.push(client_ut_meta_id);
                                            pl.extend_from_slice(&hdr);
                                            pl.extend_from_slice(&r_info[start..end]);
                                            let _ = send_peer_msg(&mut sock, Some(20), &pl).await;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                });
            }
        });

        addr
    }

    async fn spawn_mock_http_tracker(peer_addr: SocketAddr) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let ip_octets = match peer_addr {
            SocketAddr::V4(v4) => v4.ip().octets(),
            SocketAddr::V6(_) => [127, 0, 0, 1],
        };
        let port_be = peer_addr.port().to_be_bytes();
        let mut compact = Vec::with_capacity(6);
        compact.extend_from_slice(&ip_octets);
        compact.extend_from_slice(&port_be);

        tokio::spawn(async move {
            let body = encode_bencode(&BVal::Dict(vec![
                (b"complete".to_vec(), BVal::Int(3)),
                (b"incomplete".to_vec(), BVal::Int(1)),
                (b"interval".to_vec(), BVal::Int(1800)),
                (b"peers".to_vec(), BVal::Bytes(compact)),
            ]));
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let resp_body = body.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let _ = sock.read(&mut buf).await;
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        resp_body.len()
                    );
                    let _ = sock.write_all(hdr.as_bytes()).await;
                    let _ = sock.write_all(&resp_body).await;
                });
            }
        });
        format!("http://{}/announce", addr)
    }

    #[test]
    fn test_bencode_and_base32_magnet_parsing() {
        let raw_hash: [u8; 20] = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23,
            0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67,
        ];
        // Hex magnet
        let hex_mag = format!(
            "magnet:?xt=urn:btih:{}&dn=Ubuntu+24.04.iso&tr=udp%3A%2F%2Ftracker.example.com%3A6969",
            hex::encode(raw_hash)
        );
        let parsed_hex = parse_magnet_uri(&hex_mag).expect("hex magnet parses");
        assert_eq!(parsed_hex.info_hash, raw_hash);
        assert_eq!(parsed_hex.display_name.as_deref(), Some("Ubuntu 24.04.iso"));
        assert_eq!(parsed_hex.trackers, vec!["udp://tracker.example.com:6969"]);

        // Base32 magnet (RFC 4648 encoding of raw_hash)
        let b32_mag = "magnet:?xt=urn:btih:AERUKZ4JVPG66AJDIVTYTK6N54ASGRLH&dn=Archive.zip";
        let parsed_b32 = parse_magnet_uri(b32_mag).expect("base32 magnet parses");
        assert_eq!(parsed_b32.info_hash, raw_hash);
        assert_eq!(parsed_b32.display_name.as_deref(), Some("Archive.zip"));
    }

    #[test]
    fn test_multifile_path_sanitization_and_inspect() {
        let payload = vec![b'A'; 40_000];
        let mut torrent_bytes = build_sample_torrent_bytes(
            "BundleFolder",
            16_384,
            &payload,
            "http://127.0.0.1:9999/announce",
            Some(&[("../../evil/secret.txt", 15_000), ("sub/readme.md", 25_000)]),
        );
        // Quick sanity parse
        let parsed = parse_torrent_bytes(&torrent_bytes).expect("parse multi-file torrent");
        assert!(parsed.is_multi_file);
        assert_eq!(parsed.files.len(), 2);
        assert!(!parsed.files[0].rel_components.contains(&"..".to_string()));
        assert_eq!(parsed.files[0].rel_components, vec!["evil", "secret.txt"]);
        assert_eq!(parsed.total_size, 40_000);

        let inspected = inspect_torrent_bytes("https://example.com/bundle.torrent", &torrent_bytes)
            .expect("inspect bytes");
        assert_eq!(inspected.suggested_category, "torrents");
        assert!(inspected.torrent_info.is_some());
        let ti = inspected.torrent_info.unwrap();
        assert_eq!(ti.files.len(), 2);
        assert!(ti.is_multi_file);
        torrent_bytes.clear();
    }

    #[tokio::test]
    async fn test_loopback_torrent_download_single_file() {
        let mut payload = Vec::with_capacity(45_000);
        for i in 0..45_000u32 {
            payload.push(((i * 31) % 251) as u8);
        }
        let dummy_torrent =
            build_sample_torrent_bytes("sample.bin", 16_384, &payload, "http://127.0.0.1:1/announce", None);
        let parsed_dummy = parse_torrent_bytes(&dummy_torrent).unwrap();

        let peer_addr = spawn_mock_peer(
            payload.clone(),
            16_384,
            parsed_dummy.raw_info.clone(),
            parsed_dummy.info_hash,
            false,
        )
        .await;
        let tracker_url = spawn_mock_http_tracker(peer_addr).await;

        let real_torrent =
            build_sample_torrent_bytes("sample.bin", 16_384, &payload, &tracker_url, None);
        let dc_ref = cache_torrent_bytes(&real_torrent).expect("cache torrent");

        let tmp_dir = std::env::temp_dir().join(format!("dc_t_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let dest = tmp_dir.join("sample.bin");

        let ctrl = DownloadController::new();
        let (tx, mut rx) = broadcast::channel(64);
        download_torrent(
            "t-single",
            &dc_ref,
            &dest,
            Some(tx),
            ctrl,
            &DownloadOptions::default(),
        )
        .await
        .expect("torrent download completes");

        let disk_bytes = std::fs::read(&dest).expect("downloaded file exists");
        assert_eq!(disk_bytes, payload);

        let mut saw_completed = false;
        while let Ok(t) = rx.try_recv() {
            if t.status == TaskStatus::Completed {
                saw_completed = true;
            }
        }
        assert!(saw_completed);
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[tokio::test]
    async fn test_loopback_magnet_ut_metadata_and_udp_tracker() {
        let mut payload = Vec::with_capacity(38_000);
        for i in 0..38_000u32 {
            payload.push(((i ^ 0x5A) & 0xFF) as u8);
        }
        let dummy_torrent =
            build_sample_torrent_bytes("MagnetRelease", 16_384, &payload, "udp://127.0.0.1:1/announce", Some(&[("part1.dat", 20_000), ("nested/part2.dat", 18_000)]));
        let parsed = parse_torrent_bytes(&dummy_torrent).unwrap();

        let peer_addr = spawn_mock_peer(
            payload.clone(),
            16_384,
            parsed.raw_info.clone(),
            parsed.info_hash,
            true,
        )
        .await;

        // Spawn mock BEP 15 UDP tracker
        let udp_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let udp_addr = udp_sock.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 256];
            let mut active_conn_id = 0x1122_3344_5566_7788u64;
            while let Ok((n, client_addr)) = udp_sock.recv_from(&mut buf).await {
                if n >= 16 {
                    let act = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
                    let tx = &buf[12..16];
                    if act == 0 {
                        active_conn_id += 1;
                        let mut reply = Vec::with_capacity(16);
                        reply.extend_from_slice(&0u32.to_be_bytes());
                        reply.extend_from_slice(tx);
                        reply.extend_from_slice(&active_conn_id.to_be_bytes());
                        let _ = udp_sock.send_to(&reply, client_addr).await;
                    } else if act == 1 {
                        let mut reply = Vec::with_capacity(26);
                        reply.extend_from_slice(&1u32.to_be_bytes());
                        reply.extend_from_slice(tx);
                        reply.extend_from_slice(&1800u32.to_be_bytes()); // interval
                        reply.extend_from_slice(&1u32.to_be_bytes()); // leechers
                        reply.extend_from_slice(&4u32.to_be_bytes()); // seeders
                        if let SocketAddr::V4(v4) = peer_addr {
                            reply.extend_from_slice(&v4.ip().octets());
                            reply.extend_from_slice(&v4.port().to_be_bytes());
                        }
                        let _ = udp_sock.send_to(&reply, client_addr).await;
                    }
                }
            }
        });

        let tr_encoded = format!("udp%3A%2F%2F{}%2Fannounce", udp_addr);
        let magnet_uri = format!(
            "magnet:?xt=urn:btih:{}&dn=MagnetRelease&tr={}",
            parsed.info_hash_hex, tr_encoded
        );

        let inspected = inspect_magnet(&magnet_uri).expect("inspect magnet");
        assert_eq!(inspected.suggested_category, "torrents");
        assert!(inspected.torrent_info.as_ref().unwrap().is_magnet);

        let tmp_dir = std::env::temp_dir().join(format!("dc_mag_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let dest_dir = tmp_dir.join("MagnetRelease");

        download_torrent(
            "t-magnet",
            &magnet_uri,
            &dest_dir,
            None,
            DownloadController::new(),
            &DownloadOptions::default(),
        )
        .await
        .expect("magnet ut_metadata + udp download completes");

        let p1 = std::fs::read(dest_dir.join("part1.dat")).unwrap();
        let p2 = std::fs::read(dest_dir.join("nested").join("part2.dat")).unwrap();
        assert_eq!(p1, &payload[..20_000]);
        assert_eq!(p2, &payload[20_000..]);
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
