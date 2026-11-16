use std::cmp::Ordering;
use std::path::Path;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

pub const GITHUB_RELEASE_API_URL: &str =
    "https://api.github.com/repos/moesstudio/download-cepat/releases/latest";
pub const GITHUB_RELEASES_PAGE_URL: &str =
    "https://github.com/moesstudio/download-cepat/releases";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitHubReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitHubReleasePayload {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub draft: Option<bool>,
    #[serde(default)]
    pub prerelease: Option<bool>,
    #[serde(default)]
    pub assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateInstallKind {
    WindowsNsis,
    WindowsMsi,
    MacosAppTarball,
    MacosDmg,
    LinuxAppImage,
    LinuxDeb,
    LinuxRpm,
    Manual,
}

impl UpdateInstallKind {
    pub fn as_str(self) -> &'static str {
        match self {
            UpdateInstallKind::WindowsNsis => "windows_nsis",
            UpdateInstallKind::WindowsMsi => "windows_msi",
            UpdateInstallKind::MacosAppTarball => "macos_app_tarball",
            UpdateInstallKind::MacosDmg => "macos_dmg",
            UpdateInstallKind::LinuxAppImage => "linux_appimage",
            UpdateInstallKind::LinuxDeb => "linux_deb",
            UpdateInstallKind::LinuxRpm => "linux_rpm",
            UpdateInstallKind::Manual => "manual",
        }
    }

    pub fn in_place_supported(self) -> bool {
        matches!(
            self,
            UpdateInstallKind::WindowsNsis
                | UpdateInstallKind::WindowsMsi
                | UpdateInstallKind::MacosAppTarball
                | UpdateInstallKind::LinuxAppImage
                | UpdateInstallKind::LinuxDeb
                | UpdateInstallKind::LinuxRpm
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_url: String,
    pub release_notes: String,
    pub published_at: Option<String>,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub asset_size: Option<u64>,
    pub asset_sha256: Option<String>,
    pub install_kind: String,
    pub in_place_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformEnvironment {
    pub os: String,
    pub arch: String,
    pub is_appimage: bool,
    pub has_dpkg: bool,
    pub has_rpm: bool,
    pub inside_macos_bundle: bool,
}

impl PlatformEnvironment {
    pub fn detect_current(inside_macos_bundle: bool) -> Self {
        #[cfg(target_os = "linux")]
        let is_appimage = std::env::var_os("APPIMAGE")
            .map(std::path::PathBuf::from)
            .map_or(false, |p| p.is_file());
        #[cfg(not(target_os = "linux"))]
        let is_appimage = false;

        #[cfg(target_os = "linux")]
        let (has_dpkg, has_rpm) = {
            let dpkg = Path::new("/usr/bin/dpkg").exists()
                || Path::new("/bin/dpkg").exists()
                || Path::new("/etc/debian_version").exists();
            let rpm = Path::new("/usr/bin/rpm").exists()
                || Path::new("/bin/rpm").exists()
                || Path::new("/etc/redhat-release").exists()
                || Path::new("/etc/fedora-release").exists();
            (dpkg, rpm)
        };
        #[cfg(not(target_os = "linux"))]
        let (has_dpkg, has_rpm) = (false, false);

        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            is_appimage,
            has_dpkg,
            has_rpm,
            inside_macos_bundle,
        }
    }
}

pub fn normalize_version_string(raw: &str) -> String {
    raw.trim()
        .trim_start_matches(['v', 'V'])
        .split('+')
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

pub fn parse_semver_parts(raw: &str) -> Option<(u64, u64, u64, Option<String>)> {
    let clean = normalize_version_string(raw);
    if clean.is_empty() {
        return None;
    }
    let (core, pre) = match clean.split_once('-') {
        Some((c, p)) => (c, Some(p.to_string()).filter(|s| !s.is_empty())),
        None => (clean.as_str(), None),
    };
    let mut nums = core.split('.');
    let major = nums.next()?.parse::<u64>().ok()?;
    let minor = nums.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = nums.next().unwrap_or("0").parse::<u64>().ok()?;
    if nums.next().is_some() {
        return None;
    }
    Some((major, minor, patch, pre))
}

pub fn compare_versions(current: &str, latest: &str) -> Ordering {
    let (Some(a), Some(b)) = (parse_semver_parts(current), parse_semver_parts(latest)) else {
        return normalize_version_string(current).cmp(&normalize_version_string(latest));
    };
    match (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)) {
        Ordering::Equal => match (&a.3, &b.3) {
            (None, None) => Ordering::Equal,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(pa), Some(pb)) => pa.cmp(pb),
        },
        other => other,
    }
}

pub fn is_trusted_release_url(raw_url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(raw_url.trim()) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    let Some(host) = parsed.host_str().map(|h| h.to_ascii_lowercase()) else {
        return false;
    };
    host == "github.com"
        || host == "api.github.com"
        || host.ends_with(".githubusercontent.com")
}

pub fn extract_sha256_digest(digest: Option<&str>) -> Option<String> {
    let raw = digest?.trim();
    let hex_part = raw
        .strip_prefix("sha256:")
        .or_else(|| raw.strip_prefix("SHA256:"))
        .unwrap_or(raw)
        .trim();
    if hex_part.len() == 64 && hex_part.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(hex_part.to_ascii_lowercase())
    } else {
        None
    }
}

fn asset_matches_arch(lower_name: &str, arch: &str, allow_universal: bool) -> bool {
    if allow_universal && lower_name.contains("universal") {
        return true;
    }
    match arch {
        "aarch64" => {
            lower_name.contains("aarch64")
                || lower_name.contains("arm64")
        }
        "x86_64" => {
            (lower_name.contains("x86_64")
                || lower_name.contains("amd64")
                || lower_name.contains("x64")
                || lower_name.contains("intel"))
                && !lower_name.contains("arm64")
                && !lower_name.contains("aarch64")
        }
        other => lower_name.contains(&other.to_ascii_lowercase()),
    }
}

fn find_matching_asset<'a>(
    assets: &'a [GitHubReleaseAsset],
    suffix: &str,
    arch: &str,
    allow_universal: bool,
) -> Option<&'a GitHubReleaseAsset> {
    let suffix_lower = suffix.to_ascii_lowercase();
    let valid: Vec<&GitHubReleaseAsset> = assets
        .iter()
        .filter(|a| {
            let lower = a.name.to_ascii_lowercase();
            lower.ends_with(&suffix_lower) && is_trusted_release_url(&a.browser_download_url)
        })
        .collect();

    if let Some(exact) = valid
        .iter()
        .copied()
        .find(|a| asset_matches_arch(&a.name.to_ascii_lowercase(), arch, false))
    {
        return Some(exact);
    }

    if allow_universal {
        if let Some(univ) = valid
            .iter()
            .copied()
            .find(|a| a.name.to_ascii_lowercase().contains("universal"))
        {
            return Some(univ);
        }
    }

    None
}

pub fn select_asset_for_platform(
    assets: &[GitHubReleaseAsset],
    env: &PlatformEnvironment,
) -> Option<(GitHubReleaseAsset, UpdateInstallKind)> {
    match env.os.as_str() {
        "windows" => {
            if let Some(exe) = find_matching_asset(assets, "-setup.exe", &env.arch, false) {
                return Some((exe.clone(), UpdateInstallKind::WindowsNsis));
            }
            if let Some(msi) = find_matching_asset(assets, ".msi", &env.arch, false) {
                return Some((msi.clone(), UpdateInstallKind::WindowsMsi));
            }
            None
        }
        "macos" => {
            if env.inside_macos_bundle {
                if let Some(tarball) = find_matching_asset(assets, ".app.tar.gz", &env.arch, true) {
                    return Some((tarball.clone(), UpdateInstallKind::MacosAppTarball));
                }
            }
            if let Some(dmg) = find_matching_asset(assets, ".dmg", &env.arch, true) {
                return Some((dmg.clone(), UpdateInstallKind::MacosDmg));
            }
            if let Some(tarball) = find_matching_asset(assets, ".app.tar.gz", &env.arch, true) {
                return Some((tarball.clone(), UpdateInstallKind::MacosAppTarball));
            }
            None
        }
        "linux" => {
            if env.is_appimage {
                if let Some(appimg) = find_matching_asset(assets, ".appimage", &env.arch, false) {
                    return Some((appimg.clone(), UpdateInstallKind::LinuxAppImage));
                }
            }
            if env.has_dpkg {
                if let Some(deb) = find_matching_asset(assets, ".deb", &env.arch, false) {
                    return Some((deb.clone(), UpdateInstallKind::LinuxDeb));
                }
            }
            if env.has_rpm {
                if let Some(rpm) = find_matching_asset(assets, ".rpm", &env.arch, false) {
                    return Some((rpm.clone(), UpdateInstallKind::LinuxRpm));
                }
            }
            if let Some(appimg) = find_matching_asset(assets, ".appimage", &env.arch, false) {
                return Some((appimg.clone(), UpdateInstallKind::LinuxAppImage));
            }
            if let Some(deb) = find_matching_asset(assets, ".deb", &env.arch, false) {
                return Some((deb.clone(), UpdateInstallKind::LinuxDeb));
            }
            if let Some(rpm) = find_matching_asset(assets, ".rpm", &env.arch, false) {
                return Some((rpm.clone(), UpdateInstallKind::LinuxRpm));
            }
            None
        }
        _ => None,
    }
}

pub fn evaluate_release(
    current_version: &str,
    release: &GitHubReleasePayload,
    env: &PlatformEnvironment,
) -> AppUpdateInfo {
    let current_clean = normalize_version_string(current_version);
    let latest_clean = normalize_version_string(&release.tag_name);
    let is_draft = release.draft.unwrap_or(false);
    let newer = !is_draft && compare_versions(&current_clean, &latest_clean) == Ordering::Less;

    let selected = if newer {
        select_asset_for_platform(&release.assets, env)
    } else {
        None
    };

    let (asset_name, asset_url, asset_size, asset_sha256, kind) = match selected {
        Some((asset, kind)) => {
            let sha = extract_sha256_digest(asset.digest.as_deref());
            (
                Some(asset.name),
                Some(asset.browser_download_url),
                Some(asset.size),
                sha,
                kind,
            )
        }
        None => (None, None, None, None, UpdateInstallKind::Manual),
    };

    let release_url = if is_trusted_release_url(&release.html_url) {
        release.html_url.clone()
    } else {
        GITHUB_RELEASES_PAGE_URL.to_string()
    };

    AppUpdateInfo {
        current_version: current_clean,
        latest_version: latest_clean,
        update_available: newer,
        release_url,
        release_notes: release.body.clone().unwrap_or_default(),
        published_at: release.published_at.clone(),
        asset_name,
        asset_url,
        asset_size,
        asset_sha256,
        install_kind: kind.as_str().to_string(),
        in_place_supported: kind.in_place_supported(),
    }
}

pub async fn fetch_latest_github_release(current_version: &str) -> Result<GitHubReleasePayload, String> {
    fetch_release_from_url(GITHUB_RELEASE_API_URL, current_version, true).await
}

pub async fn fetch_release_from_url(
    endpoint_url: &str,
    current_version: &str,
    enforce_github_origin: bool,
) -> Result<GitHubReleasePayload, String> {
    if enforce_github_origin && !is_trusted_release_url(endpoint_url) {
        return Err("Endpoint pembaruan tidak berasal dari domain GitHub resmi.".to_string());
    }
    let ua = format!("Download-Cepat/{}", normalize_version_string(current_version));
    let client = reqwest::Client::builder()
        .user_agent(ua)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("Gagal menginisialisasi klien HTTPS pembaruan: {}", e))?;

    let resp = client
        .get(endpoint_url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Gagal menghubungi server pembaruan GitHub: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Server pembaruan merespons status HTTP {}.",
            resp.status().as_u16()
        ));
    }

    resp.json::<GitHubReleasePayload>()
        .await
        .map_err(|e| format!("Format metadata rilis GitHub tidak valid: {}", e))
}

pub async fn check_github_release(
    current_version: &str,
    env: &PlatformEnvironment,
) -> Result<AppUpdateInfo, String> {
    let payload = fetch_latest_github_release(current_version).await?;
    Ok(evaluate_release(current_version, &payload, env))
}

pub async fn download_verified_asset<F>(
    download_url: &str,
    dest_path: &Path,
    expected_sha256: &str,
    user_agent_version: &str,
    enforce_github_origin: bool,
    mut on_progress: F,
) -> Result<u64, String>
where
    F: FnMut(u64, Option<u64>),
{
    if enforce_github_origin && !is_trusted_release_url(download_url) {
        return Err("URL paket pembaruan bukan berasal dari GitHub resmi.".to_string());
    }
    let clean_expected = extract_sha256_digest(Some(expected_sha256))
        .ok_or_else(|| "Checksum SHA-256 paket pembaruan tidak tersedia atau tidak valid.".to_string())?;

    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Gagal membuat folder sementara pembaruan: {}", e))?;
    }

    let tmp_path = dest_path.with_extension("download.tmp");
    let _ = std::fs::remove_file(&tmp_path);

    let ua = format!("Download-Cepat/{}", normalize_version_string(user_agent_version));
    let client = reqwest::Client::builder()
        .user_agent(ua)
        .connect_timeout(std::time::Duration::from_secs(20))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| format!("Gagal menginisialisasi pengunduh pembaruan: {}", e))?;

    let resp = client
        .get(download_url)
        .send()
        .await
        .map_err(|e| format!("Gagal mengunduh paket pembaruan: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Unduhan pembaruan gagal dengan status HTTP {}.",
            resp.status().as_u16()
        ));
    }

    let final_url = resp.url().as_str().to_string();
    if enforce_github_origin && !is_trusted_release_url(&final_url) {
        return Err("Redirect unduhan mengarah ke host di luar GitHub resmi.".to_string());
    }

    let total_bytes = resp.content_length();
    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Gagal membuat berkas sementara pembaruan: {}", e))?;

    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("Koneksi terputus saat mengunduh pembaruan: {}", e)
        })?;
        file.write_all(&chunk).await.map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("Gagal menulis berkas pembaruan ke disk: {}", e)
        })?;
        hasher.update(&chunk);
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        on_progress(downloaded, total_bytes);
    }

    file.flush().await.map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("Gagal menyelesaikan penulisan berkas pembaruan: {}", e)
    })?;
    drop(file);

    let actual_sha = hex::encode(hasher.finalize());
    if actual_sha != clean_expected {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!(
            "Verifikasi keamanan SHA-256 pembaruan gagal (diharapkan {}, diterima {}).",
            &clean_expected[..12],
            &actual_sha[..12]
        ));
    }

    if dest_path.exists() {
        let _ = std::fs::remove_file(dest_path);
    }
    std::fs::rename(&tmp_path, dest_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("Gagal menyimpan paket pembaruan ke {}: {}", dest_path.display(), e)
    })?;

    Ok(downloaded)
}
