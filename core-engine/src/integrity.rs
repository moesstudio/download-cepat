use anyhow::{Context, Result};
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

use crate::model::FileChecksums;

/// Compute MD5, SHA-1, and SHA-256 digests in a single streaming pass (constant RAM).
pub fn compute_file_checksums(path: &Path) -> Result<FileChecksums> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Gagal membuka berkas untuk verifikasi checksum: {}", path.display()))?;
    let size_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);

    let mut md5_hasher = Md5::new();
    let mut sha1_hasher = Sha1::new();
    let mut sha256_hasher = Sha256::new();
    let mut buf = [0u8; 65_536];

    loop {
        let n = file.read(&mut buf).context("Gagal membaca berkas saat menghitung checksum")?;
        if n == 0 {
            break;
        }
        let slice = &buf[..n];
        md5_hasher.update(slice);
        sha1_hasher.update(slice);
        sha256_hasher.update(slice);
    }

    Ok(FileChecksums {
        md5: hex::encode(md5_hasher.finalize()),
        sha1: hex::encode(sha1_hasher.finalize()),
        sha256: hex::encode(sha256_hasher.finalize()),
        size_bytes,
    })
}

/// Returns free disk space (bytes) available for non-privileged users on the target filesystem.
#[cfg(unix)]
pub fn available_disk_space(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let mut probe = path.to_path_buf();
    while !probe.exists() {
        if !probe.pop() {
            probe = std::path::PathBuf::from("/");
            break;
        }
    }

    let c_path = CString::new(probe.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 {
        return None;
    }

    let frsize = if stat.f_frsize > 0 {
        stat.f_frsize as u64
    } else {
        stat.f_bsize as u64
    };
    Some((stat.f_bavail as u64).saturating_mul(frsize))
}

#[cfg(not(unix))]
pub fn available_disk_space(_path: &Path) -> Option<u64> {
    None
}

fn format_gb_or_mb(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.2} MB", bytes as f64 / 1_048_576.0)
    }
}

/// Verify enough disk capacity exists before pre-allocating large downloads.
pub fn ensure_sufficient_disk_space(dest: &Path, required_bytes: u64) -> Result<()> {
    if required_bytes == 0 {
        return Ok(());
    }
    if let Some(avail) = available_disk_space(dest) {
        if avail < required_bytes {
            anyhow::bail!(
                "Ruang disk tidak cukup: tersisa {}, dibutuhkan {}.",
                format_gb_or_mb(avail),
                format_gb_or_mb(required_bytes)
            );
        }
    }
    Ok(())
}
