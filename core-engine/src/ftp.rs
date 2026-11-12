use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use url::Url;

use crate::downloader::{
    checkpoint_path_for, save_checkpoint, BandwidthLimiter, DownloadCheckpoint, DownloadController,
    DownloadOptions,
};
use crate::inspector::categorize_filename;
use crate::integrity::ensure_sufficient_disk_space;
use crate::model::{InspectResult, ProgressTelemetry, SegmentProgress, TaskStatus};

fn parse_ftp_url(raw_url: &str, opt_user: Option<&str>, opt_pass: Option<&str>) -> Result<(String, u16, String, String, String)> {
    let parsed = Url::parse(raw_url).context("Format URL FTP tidak valid")?;
    if !parsed.scheme().eq_ignore_ascii_case("ftp") {
        anyhow::bail!("Bukan URL FTP");
    }
    let host = parsed
        .host_str()
        .context("Host FTP tidak ditemukan")?
        .to_string();
    let port = parsed.port().unwrap_or(21);

    let url_user = if !parsed.username().is_empty() {
        Some(parsed.username().to_string())
    } else {
        None
    };
    let url_pass = parsed.password().map(|p| p.to_string());

    let user = opt_user
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or(url_user)
        .unwrap_or_else(|| "anonymous".to_string());
    let pass = opt_pass
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or(url_pass)
        .unwrap_or_else(|| "anonymous@downloadcepat.local".to_string());

    let remote_path = parsed.path().to_string();
    if remote_path.is_empty() || remote_path == "/" {
        anyhow::bail!("Path berkas FTP kosong");
    }

    Ok((host, port, user, pass, remote_path))
}

async fn read_ftp_reply(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> Result<(u16, String)> {
    let mut first_line = String::new();
    let n = tokio::time::timeout(Duration::from_secs(15), reader.read_line(&mut first_line))
        .await
        .context("Timeout membaca respons server FTP")??;
    if n == 0 {
        anyhow::bail!("Server FTP menutup koneksi lebih awal");
    }

    let trimmed = first_line.trim_end_matches(['\r', '\n']);
    if trimmed.len() < 3 {
        anyhow::bail!("Respons FTP tidak valid: {}", trimmed);
    }
    let code: u16 = trimmed[..3]
        .parse()
        .with_context(|| format!("Kode status FTP tidak valid: {}", trimmed))?;

    let mut full_msg = trimmed.to_string();
    if trimmed.as_bytes().get(3) == Some(&b'-') {
        let end_prefix = format!("{:03} ", code);
        loop {
            let mut next_line = String::new();
            let rn = tokio::time::timeout(Duration::from_secs(15), reader.read_line(&mut next_line))
                .await
                .context("Timeout membaca respons multi-baris FTP")??;
            if rn == 0 {
                break;
            }
            let nt = next_line.trim_end_matches(['\r', '\n']);
            full_msg.push('\n');
            full_msg.push_str(nt);
            if nt.starts_with(&end_prefix) {
                break;
            }
        }
    }

    Ok((code, full_msg))
}

async fn send_ftp_cmd(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    cmd: &str,
) -> Result<(u16, String)> {
    let line = format!("{}\r\n", cmd);
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await?;
    read_ftp_reply(reader).await
}

async fn connect_and_login(
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
) -> Result<(BufReader<tokio::net::tcp::OwnedReadHalf>, tokio::net::tcp::OwnedWriteHalf)> {
    let stream = tokio::time::timeout(Duration::from_secs(12), TcpStream::connect((host, port)))
        .await
        .context("Timeout menghubungkan ke server FTP")?
        .with_context(|| format!("Gagal terhubung ke FTP {}:{}", host, port))?;

    let (rx, mut tx) = stream.into_split();
    let mut reader = BufReader::new(rx);

    let (greet_code, greet_msg) = read_ftp_reply(&mut reader).await?;
    if greet_code >= 400 {
        anyhow::bail!("FTP menolak koneksi: {}", greet_msg);
    }

    let (user_code, user_msg) = send_ftp_cmd(&mut reader, &mut tx, &format!("USER {}", user)).await?;
    if user_code == 331 || user_code == 332 {
        let (pass_code, pass_msg) =
            send_ftp_cmd(&mut reader, &mut tx, &format!("PASS {}", pass)).await?;
        if pass_code >= 400 {
            anyhow::bail!("Autentikasi FTP gagal: {}", pass_msg);
        }
    } else if user_code >= 400 {
        anyhow::bail!("Username FTP ditolak: {}", user_msg);
    }

    let _ = send_ftp_cmd(&mut reader, &mut tx, "TYPE I").await?;
    Ok((reader, tx))
}

fn parse_epsv_port(msg: &str) -> Option<u16> {
    let start = msg.find('(')?;
    let end = msg[start + 1..].find(')')? + start + 1;
    let inner = &msg[start + 1..end];
    let delim = inner.chars().next()?;
    let port_str = inner.split(delim).filter(|p| !p.is_empty()).last()?;
    port_str.parse::<u16>().ok()
}

fn parse_pasv_port(msg: &str) -> Option<u16> {
    let start = msg.find('(')?;
    let end = msg[start + 1..].find(')')? + start + 1;
    let nums: Vec<u16> = msg[start + 1..end]
        .split(',')
        .filter_map(|s| s.trim().parse::<u16>().ok())
        .collect();
    if nums.len() == 6 {
        Some(nums[4] * 256 + nums[5])
    } else {
        None
    }
}

async fn open_passive_data_stream(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    host: &str,
) -> Result<TcpStream> {
    let (epsv_code, epsv_msg) = send_ftp_cmd(reader, writer, "EPSV").await?;
    let data_port = if epsv_code == 229 {
        parse_epsv_port(&epsv_msg).context("Gagal parsing port EPSV FTP")?
    } else {
        let (pasv_code, pasv_msg) = send_ftp_cmd(reader, writer, "PASV").await?;
        if pasv_code != 227 {
            anyhow::bail!("FTP PASV gagal: {}", pasv_msg);
        }
        parse_pasv_port(&pasv_msg).context("Gagal parsing port PASV FTP")?
    };

    let data_stream = tokio::time::timeout(Duration::from_secs(12), TcpStream::connect((host, data_port)))
        .await
        .context("Timeout membuka koneksi data FTP")?
        .context("Gagal membuka koneksi data FTP")?;
    Ok(data_stream)
}

pub async fn inspect_ftp(
    raw_url: &str,
    auth_username: Option<&str>,
    auth_password: Option<&str>,
) -> Result<InspectResult> {
    let (host, port, user, pass, remote_path) = parse_ftp_url(raw_url, auth_username, auth_password)?;
    let (mut reader, mut writer) = connect_and_login(&host, port, &user, &pass).await?;

    let (size_code, size_msg) = send_ftp_cmd(&mut reader, &mut writer, &format!("SIZE {}", remote_path)).await?;
    let file_size = if size_code == 213 {
        size_msg
            .split_whitespace()
            .last()
            .and_then(|s| s.parse::<u64>().ok())
    } else {
        None
    };

    let (rest_code, _) = send_ftp_cmd(&mut reader, &mut writer, "REST 0").await?;
    let supports_range = rest_code == 350;

    let _ = send_ftp_cmd(&mut reader, &mut writer, "QUIT").await;

    let file_name = Path::new(&remote_path)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("ftp_download.bin")
        .to_string();

    let suggested_category = categorize_filename(&file_name);

    Ok(InspectResult {
        url: raw_url.to_string(),
        final_url: raw_url.to_string(),
        file_name,
        file_size,
        supports_range,
        mime_type: None,
        etag: None,
        last_modified: None,
        suggested_category,
        media_info: None,
    })
}

pub async fn download_ftp(
    task_id: &str,
    url: &str,
    destination_path: &Path,
    expected_size: Option<u64>,
    supports_range: bool,
    telemetry_tx: Option<broadcast::Sender<ProgressTelemetry>>,
    controller: DownloadController,
    options: &DownloadOptions<'_>,
) -> Result<()> {
    let (host, port, user, pass, remote_path) =
        parse_ftp_url(url, options.auth_username, options.auth_password)?;

    if let Some(parent) = destination_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let (mut reader, mut writer) = connect_and_login(&host, port, &user, &pass).await?;

    let total_size = match expected_size {
        Some(s) if s > 0 => s,
        _ => {
            let (code, msg) = send_ftp_cmd(&mut reader, &mut writer, &format!("SIZE {}", remote_path)).await?;
            if code == 213 {
                msg.split_whitespace().last().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0)
            } else {
                0
            }
        }
    };

    let meta_path = checkpoint_path_for(destination_path);
    let mut resume_offset = 0u64;

    if supports_range && total_size > 0 && destination_path.exists() && meta_path.exists() {
        if let Ok(raw_meta) = std::fs::read_to_string(&meta_path) {
            if let Ok(cp) = serde_json::from_str::<DownloadCheckpoint>(&raw_meta) {
                let url_ok = cp.url == url || options.allow_url_refresh;
                if url_ok && cp.total_size == total_size {
                    if let Some(seg) = cp.segments.first() {
                        let actual_len = std::fs::metadata(destination_path).map(|m| m.len()).unwrap_or(0);
                        if actual_len >= seg.downloaded_bytes {
                            resume_offset = seg.downloaded_bytes.min(total_size);
                        }
                    }
                }
            }
        }
    }

    if resume_offset == 0 && total_size > 0 {
        ensure_sufficient_disk_space(destination_path, total_size)?;
    }

    let mut file = if resume_offset > 0 {
        let mut f = OpenOptions::new().write(true).open(destination_path)?;
        f.seek(SeekFrom::Start(resume_offset))?;
        f
    } else {
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(destination_path)?
    };

    let mut data_stream = open_passive_data_stream(&mut reader, &mut writer, &host).await?;
    if resume_offset > 0 {
        let (rc, rm) = send_ftp_cmd(&mut reader, &mut writer, &format!("REST {}", resume_offset)).await?;
        if rc != 350 {
            anyhow::bail!("Server FTP menolak resume REST {}: {}", resume_offset, rm);
        }
    }

    let (retr_code, retr_msg) =
        send_ftp_cmd(&mut reader, &mut writer, &format!("RETR {}", remote_path)).await?;
    if retr_code >= 400 {
        anyhow::bail!("Gagal memulai RETR FTP: {}", retr_msg);
    }

    let limiter = BandwidthLimiter::new(
        controller
            .speed_limit()
            .or(options.speed_limit_bytes_per_sec)
            .unwrap_or(0),
    );
    let dummy_aborted = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut downloaded = resume_offset;
    let mut buf = vec![0u8; 65_536];
    let mut last_emit = Instant::now();
    let mut samples: VecDeque<(Instant, u64)> = VecDeque::new();
    samples.push_back((Instant::now(), downloaded));
    let mut smoothed_bps: f64 = 0.0;

    let emit_progress = |status: TaskStatus, dl: u64, spd: u64| {
        if let Some(ref sender) = telemetry_tx {
            let pct = if total_size > 0 {
                ((dl as f64 / total_size as f64) * 100.0).clamp(0.0, 99.9) as f32
            } else {
                0.0
            };
            let eta = if spd > 0 && total_size > dl {
                Some(((total_size - dl) as f64 / spd as f64).ceil() as u64)
            } else {
                None
            };
            let seg = SegmentProgress {
                segment_index: 0,
                start_byte: 0,
                end_byte: total_size.saturating_sub(1),
                current_byte: dl,
                downloaded_bytes: dl,
                is_completed: status == TaskStatus::Completed,
            };
            let _ = sender.send(ProgressTelemetry {
                task_id: task_id.to_string(),
                total_bytes: if total_size > 0 { Some(total_size) } else { None },
                downloaded_bytes: dl,
                speed_bytes_per_sec: spd,
                progress_percent: if status == TaskStatus::Completed { 100.0 } else { pct },
                eta_seconds: if status == TaskStatus::Completed { Some(0) } else { eta },
                status,
                segments: vec![seg],
            });
        }
    };

    emit_progress(TaskStatus::Downloading, downloaded, 0);

    loop {
        if controller.is_stopped() {
            break;
        }
        if let Some(ref lim) = limiter {
            lim.set_limit(controller.speed_limit().unwrap_or(0));
        }

        let n = tokio::select! {
            res = data_stream.read(&mut buf) => res?,
            _ = controller.notified() => break,
        };
        if n == 0 {
            break;
        }

        if let Some(ref lim) = limiter {
            lim.throttle(n as u64, &controller, &dummy_aborted).await;
            if controller.is_stopped() {
                break;
            }
        }

        file.write_all(&buf[..n])?;
        downloaded += n as u64;

        let now = Instant::now();
        samples.push_back((now, downloaded));
        while samples.len() > 2 && now.duration_since(samples[0].0) > Duration::from_secs(2) {
            samples.pop_front();
        }
        if now.duration_since(last_emit) >= Duration::from_millis(250) {
            let (oldest_t, oldest_b) = samples.front().copied().unwrap_or((now, downloaded));
            let dt = now.duration_since(oldest_t).as_secs_f64();
            let raw_bps = if dt >= 0.15 {
                downloaded.saturating_sub(oldest_b) as f64 / dt
            } else {
                0.0
            };
            smoothed_bps = if smoothed_bps <= 0.0 {
                raw_bps
            } else {
                0.6 * raw_bps + 0.4 * smoothed_bps
            };
            emit_progress(TaskStatus::Downloading, downloaded, smoothed_bps.round() as u64);
            last_emit = now;
        }
    }

    file.flush()?;
    drop(data_stream);

    if controller.is_canceled() {
        let _ = std::fs::remove_file(&meta_path);
        let _ = std::fs::remove_file(destination_path);
        emit_progress(TaskStatus::Canceled, 0, 0);
        return Ok(());
    }

    if controller.is_paused() {
        if supports_range && total_size > 0 {
            save_checkpoint(
                &meta_path,
                url,
                total_size,
                &[(0, 0, total_size.saturating_sub(1))],
                &[downloaded],
            );
        }
        emit_progress(TaskStatus::Paused, downloaded, 0);
        return Ok(());
    }

    let _ = std::fs::remove_file(&meta_path);
    let _ = send_ftp_cmd(&mut reader, &mut writer, "QUIT").await;
    emit_progress(TaskStatus::Completed, downloaded, 0);
    Ok(())
}
