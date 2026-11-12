use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex, Notify};

use crate::inspector::{
    build_http_clients, is_unexpected_html_response, send_with_redirects, DEFAULT_USER_AGENT,
    FALLBACK_USER_AGENT,
};
use crate::integrity::ensure_sufficient_disk_space;
use crate::model::{ProgressTelemetry, SegmentProgress, TaskStatus};

#[derive(Clone, Default)]
pub struct DownloadOptions<'a> {
    pub cookie: Option<&'a str>,
    pub referer: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    pub speed_limit_bytes_per_sec: Option<u64>,
    pub proxy_url: Option<&'a str>,
    pub auth_username: Option<&'a str>,
    pub auth_password: Option<&'a str>,
    pub allow_url_refresh: bool,
}

#[derive(Clone)]
pub struct DownloadController {
    is_paused: Arc<AtomicBool>,
    is_canceled: Arc<AtomicBool>,
    stop_notify: Arc<Notify>,
    speed_limit_bps: Arc<AtomicU64>,
    has_speed_override: Arc<AtomicBool>,
}

impl DownloadController {
    pub fn new() -> Self {
        Self {
            is_paused: Arc::new(AtomicBool::new(false)),
            is_canceled: Arc::new(AtomicBool::new(false)),
            stop_notify: Arc::new(Notify::new()),
            speed_limit_bps: Arc::new(AtomicU64::new(0)),
            has_speed_override: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn pause(&self) {
        self.is_paused.store(true, Ordering::SeqCst);
        self.stop_notify.notify_waiters();
    }

    pub fn cancel(&self) {
        self.is_paused.store(false, Ordering::SeqCst);
        self.is_canceled.store(true, Ordering::SeqCst);
        self.stop_notify.notify_waiters();
    }

    pub fn set_speed_limit(&self, bytes_per_sec: u64) {
        self.speed_limit_bps.store(bytes_per_sec, Ordering::SeqCst);
        self.has_speed_override.store(true, Ordering::SeqCst);
    }

    pub fn speed_limit(&self) -> Option<u64> {
        if self.has_speed_override.load(Ordering::Relaxed) {
            Some(self.speed_limit_bps.load(Ordering::Relaxed))
        } else {
            None
        }
    }

    pub fn notified(&self) -> tokio::sync::futures::Notified<'_> {
        self.stop_notify.notified()
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::SeqCst)
    }

    pub fn is_canceled(&self) -> bool {
        self.is_canceled.load(Ordering::SeqCst)
    }

    pub fn is_stopped(&self) -> bool {
        self.is_paused() || self.is_canceled()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct SegmentCheckpoint {
    pub index: usize,
    pub start_byte: u64,
    pub end_byte: u64,
    pub downloaded_bytes: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct DownloadCheckpoint {
    pub url: String,
    pub total_size: u64,
    pub segments: Vec<SegmentCheckpoint>,
}

pub fn checkpoint_path_for(destination_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.dcmeta", destination_path.display()))
}

pub(crate) struct BandwidthLimiter {
    limit_per_sec: AtomicU64,
    state: Mutex<(Instant, f64)>,
}

impl BandwidthLimiter {
    pub(crate) fn new(limit_bytes_per_sec: u64) -> Option<Arc<Self>> {
        Some(Arc::new(Self {
            limit_per_sec: AtomicU64::new(limit_bytes_per_sec),
            state: Mutex::new((Instant::now(), limit_bytes_per_sec as f64)),
        }))
    }

    pub(crate) fn set_limit(&self, limit_bytes_per_sec: u64) {
        self.limit_per_sec.store(limit_bytes_per_sec, Ordering::Relaxed);
    }

    pub(crate) async fn throttle(&self, bytes: u64, controller: &DownloadController, aborted: &AtomicBool) {
        if bytes == 0 {
            return;
        }
        let current_limit = self.limit_per_sec.load(Ordering::Relaxed);
        if current_limit == 0 {
            return;
        }
        let limit = current_limit as f64;
        let wait_total = {
            let mut guard = self.state.lock().await;
            let now = Instant::now();
            let elapsed = now.duration_since(guard.0).as_secs_f64();
            guard.0 = now;
            guard.1 = (guard.1 + elapsed * limit).min(limit);
            guard.1 -= bytes as f64;
            if guard.1 < 0.0 {
                let deficit_secs = (-guard.1) / limit;
                Some(Duration::from_secs_f64(deficit_secs.min(2.0)))
            } else {
                None
            }
        };

        if let Some(mut remaining) = wait_total {
            while remaining > Duration::ZERO {
                if controller.is_stopped() || aborted.load(Ordering::Relaxed) {
                    return;
                }
                let live_limit = self.limit_per_sec.load(Ordering::Relaxed);
                if live_limit == 0 {
                    return;
                }
                let slice = remaining.min(Duration::from_millis(120));
                tokio::time::sleep(slice).await;
                remaining = remaining.saturating_sub(slice);
            }
        }
    }
}

pub struct Downloader {
    client: reqwest::Client,
    fallback_client: reqwest::Client,
}

impl Downloader {
    pub fn new() -> Result<Self> {
        let (client, fallback_client) = build_http_clients(None)?;
        Ok(Self {
            client,
            fallback_client,
        })
    }

    pub async fn download(
        &self,
        task_id: &str,
        url: &str,
        destination_path: &Path,
        file_size: Option<u64>,
        supports_range: bool,
        num_connections: usize,
        telemetry_tx: Option<broadcast::Sender<ProgressTelemetry>>,
        controller: Option<DownloadController>,
    ) -> Result<()> {
        self.download_with_options(
            task_id,
            url,
            destination_path,
            file_size,
            supports_range,
            num_connections,
            telemetry_tx,
            controller,
            DownloadOptions::default(),
        )
        .await
    }

    pub async fn download_with_headers(
        &self,
        task_id: &str,
        url: &str,
        destination_path: &Path,
        file_size: Option<u64>,
        supports_range: bool,
        num_connections: usize,
        telemetry_tx: Option<broadcast::Sender<ProgressTelemetry>>,
        controller: Option<DownloadController>,
        cookie: Option<&str>,
        referer: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<()> {
        self.download_with_options(
            task_id,
            url,
            destination_path,
            file_size,
            supports_range,
            num_connections,
            telemetry_tx,
            controller,
            DownloadOptions {
                cookie,
                referer,
                user_agent,
                ..Default::default()
            },
        )
        .await
    }

    pub async fn download_with_options(
        &self,
        task_id: &str,
        url: &str,
        destination_path: &Path,
        file_size: Option<u64>,
        supports_range: bool,
        num_connections: usize,
        telemetry_tx: Option<broadcast::Sender<ProgressTelemetry>>,
        controller: Option<DownloadController>,
        options: DownloadOptions<'_>,
    ) -> Result<()> {
        let controller = controller.unwrap_or_else(DownloadController::new);
        if url.trim().to_ascii_lowercase().starts_with("ftp://") {
            return crate::ftp::download_ftp(
                task_id,
                url,
                destination_path,
                file_size,
                supports_range,
                telemetry_tx,
                controller,
                &options,
            )
            .await;
        }
        let initial_conns = if !supports_range || file_size.is_none() || file_size == Some(0) {
            1
        } else {
            num_connections.clamp(1, 32)
        };

        // Connection step-down plan when rate-limited / server drops parallel sockets:
        // e.g. 32 -> 4 -> 1 (single stream without Range)
        let mut plan: Vec<(usize, bool)> = vec![(initial_conns, supports_range)];
        if supports_range && initial_conns > 4 {
            plan.push((4, true));
        }
        if supports_range && initial_conns > 1 {
            plan.push((1, false));
        }

        let mut last_err: Option<anyhow::Error> = None;

        for (step_idx, &(attempt_conns, attempt_range)) in plan.iter().enumerate() {
            if controller.is_stopped() {
                return Ok(());
            }

            // When stepping down to non-range single connection, remove stale multi-part checkpoint
            if !attempt_range && step_idx > 0 {
                let meta_path = checkpoint_path_for(destination_path);
                let _ = std::fs::remove_file(&meta_path);
            }

            match self
                .run_download_attempt(
                    task_id,
                    url,
                    destination_path,
                    file_size,
                    attempt_range,
                    attempt_conns,
                    telemetry_tx.clone(),
                    controller.clone(),
                    &options,
                )
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) => {
                    let msg = format!("{}", e);
                    let is_permanent = msg.contains("halaman web HTML")
                        || msg.contains("404 Not Found")
                        || msg.contains("410 Gone")
                        || msg.contains("401 Unauthorized");

                    last_err = Some(e);

                    if is_permanent || step_idx + 1 >= plan.len() || controller.is_stopped() {
                        break;
                    }

                    // Brief cooldown before retrying with fewer concurrent connections
                    tokio::time::sleep(Duration::from_millis(600)).await;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Unduhan gagal.")))
    }

    async fn run_download_attempt(
        &self,
        task_id: &str,
        url: &str,
        destination_path: &Path,
        file_size: Option<u64>,
        supports_range: bool,
        mut num_connections: usize,
        telemetry_tx: Option<broadcast::Sender<ProgressTelemetry>>,
        controller: DownloadController,
        options: &DownloadOptions<'_>,
    ) -> Result<()> {
        if let Some(parent) = destination_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if !supports_range || file_size.is_none() || file_size == Some(0) {
            num_connections = 1;
        } else {
            num_connections = num_connections.clamp(1, 32);
        }

        let initial_size = file_size.unwrap_or(0);
        let total_size_atomic = Arc::new(AtomicU64::new(initial_size));
        let meta_path = checkpoint_path_for(destination_path);

        // Try loading existing checkpoint to support true Pause/Resume & Refresh URL without losing progress
        let mut resumed_from_checkpoint = false;
        let mut segments: Vec<(usize, u64, u64)> = Vec::new();
        let mut initial_segment_progress: Vec<u64> = Vec::new();

        if supports_range && initial_size > 0 && destination_path.exists() && meta_path.exists() {
            if let Ok(raw_meta) = std::fs::read_to_string(&meta_path) {
                if let Ok(cp) = serde_json::from_str::<DownloadCheckpoint>(&raw_meta) {
                    let url_matches = cp.url == url || options.allow_url_refresh;
                    if url_matches && cp.total_size == initial_size && !cp.segments.is_empty() {
                        let actual_disk_len = std::fs::metadata(destination_path).map(|m| m.len()).unwrap_or(0);
                        if actual_disk_len == initial_size {
                            resumed_from_checkpoint = true;
                            for seg in &cp.segments {
                                let seg_len = seg.end_byte.saturating_sub(seg.start_byte) + 1;
                                segments.push((seg.index, seg.start_byte, seg.end_byte));
                                initial_segment_progress.push(seg.downloaded_bytes.min(seg_len));
                            }
                        }
                    }
                }
            }
        }

        if !resumed_from_checkpoint {
            if initial_size > 0 {
                ensure_sufficient_disk_space(destination_path, initial_size)?;
                let initial_file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(destination_path)
                    .context("Gagal membuat/mengalokasikan file tujuan")?;
                initial_file.set_len(initial_size)?;
                drop(initial_file);
            } else {
                let initial_file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(destination_path)
                    .context("Gagal membuat file tujuan")?;
                drop(initial_file);
            }

            if num_connections > 1 && initial_size > 0 {
                let chunk_size = initial_size / (num_connections as u64);
                for i in 0..num_connections {
                    let start = (i as u64) * chunk_size;
                    let end = if i == num_connections - 1 {
                        initial_size - 1
                    } else {
                        start + chunk_size - 1
                    };
                    segments.push((i, start, end));
                    initial_segment_progress.push(0);
                }
            } else {
                segments.push((0, 0, initial_size.saturating_sub(1)));
                initial_segment_progress.push(0);
            }

            if supports_range && initial_size > 0 {
                save_checkpoint(&meta_path, url, initial_size, &segments, &initial_segment_progress);
            }
        } else if options.allow_url_refresh && supports_range && initial_size > 0 {
            save_checkpoint(&meta_path, url, initial_size, &segments, &initial_segment_progress);
        }

        let initial_sum: u64 = initial_segment_progress.iter().sum();
        let coordinator = Arc::new(SegmentCoordinator::from_initial(
            &segments,
            &initial_segment_progress,
        ));

        let total_downloaded = Arc::new(AtomicU64::new(initial_sum));
        let is_completed = Arc::new(AtomicBool::new(false));
        let is_aborted = Arc::new(AtomicBool::new(false));
        let wrote_any_chunk = Arc::new(AtomicBool::new(initial_sum > 0));
        let initial_limit = controller
            .speed_limit()
            .or(options.speed_limit_bytes_per_sec)
            .unwrap_or(0);
        let limiter = BandwidthLimiter::new(initial_limit);

        let session_start = Instant::now();

        // Emit immediate initial segment snapshot so UI sees all parallel threads at 0% (or resumed offsets)
        if let Some(ref sender) = telemetry_tx {
            let init_segs = build_segment_progresses(&segments, &initial_segment_progress);
            let init_pct = if initial_size > 0 {
                ((initial_sum as f64 / initial_size as f64) * 100.0) as f32
            } else {
                0.0
            };
            let _ = sender.send(ProgressTelemetry {
                task_id: task_id.to_string(),
                total_bytes: if initial_size > 0 { Some(initial_size) } else { None },
                downloaded_bytes: initial_sum,
                speed_bytes_per_sec: 0,
                progress_percent: init_pct.clamp(0.0, 100.0),
                eta_seconds: None,
                status: TaskStatus::Downloading,
                segments: init_segs,
            });
        }

        // Telemetry & checkpoint monitor loop (also syncs live speed cap changes!)
        let monitor_handle = {
            let tx = telemetry_tx.clone();
            let total_dl = total_downloaded.clone();
            let coord_ref = coordinator.clone();
            let completed = is_completed.clone();
            let aborted = is_aborted.clone();
            let ctrl = controller.clone();
            let lim_ref = limiter.clone();
            let task_id_owned = task_id.to_string();
            let total_size_ref = total_size_atomic.clone();
            let meta_path_owned = meta_path.clone();
            let url_owned = url.to_string();

            tokio::spawn(async move {
                let start_now = Instant::now();
                let mut samples: VecDeque<(Instant, u64)> = VecDeque::with_capacity(12);
                samples.push_back((start_now, total_dl.load(Ordering::Relaxed)));
                let mut smoothed_speed_bps: f64 = 0.0;

                while !completed.load(Ordering::Relaxed)
                    && !aborted.load(Ordering::Relaxed)
                    && !ctrl.is_stopped()
                {
                    if let (Some(lim), Some(override_bps)) = (&lim_ref, ctrl.speed_limit()) {
                        lim.set_limit(override_bps);
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    if completed.load(Ordering::Relaxed)
                        || aborted.load(Ordering::Relaxed)
                        || ctrl.is_stopped()
                    {
                        break;
                    }
                    if let (Some(lim), Some(override_bps)) = (&lim_ref, ctrl.speed_limit()) {
                        lim.set_limit(override_bps);
                    }

                    let current_bytes = total_dl.load(Ordering::Relaxed);
                    let now = Instant::now();
                    samples.push_back((now, current_bytes));
                    while samples.len() > 2
                        && now.duration_since(samples[1].0) >= Duration::from_secs(2)
                    {
                        samples.pop_front();
                    }

                    if let Some(&(base_time, base_bytes)) = samples.front() {
                        let window_secs = now.duration_since(base_time).as_secs_f64();
                        let window_bytes = current_bytes.saturating_sub(base_bytes);
                        let window_speed = if window_secs > 0.05 {
                            window_bytes as f64 / window_secs
                        } else {
                            0.0
                        };

                        smoothed_speed_bps = if smoothed_speed_bps <= 0.0 {
                            window_speed
                        } else {
                            smoothed_speed_bps * 0.65 + window_speed * 0.35
                        };

                        let session_elapsed = session_start.elapsed().as_secs_f64();
                        let session_bytes = current_bytes.saturating_sub(initial_sum);
                        let avg_speed = if session_elapsed >= 0.4 && session_bytes > 0 {
                            (session_bytes as f64 / session_elapsed) as u64
                        } else {
                            0
                        };

                        let speed_bytes_per_sec = smoothed_speed_bps.round() as u64;
                        let eta_speed = speed_bytes_per_sec.max(avg_speed);

                        let current_total = total_size_ref.load(Ordering::Relaxed);
                        let percent = if current_total > 0 {
                            ((current_bytes as f64 / current_total as f64) * 100.0) as f32
                        } else if current_bytes > 0 {
                            let mb = current_bytes as f64 / (1024.0 * 1024.0);
                            let p = (1.0 - (-mb / 4.0).exp()) * 85.0 + 5.0;
                            p.clamp(5.0, 95.0) as f32
                        } else {
                            1.0
                        };

                        let eta = if eta_speed > 0 && current_total > current_bytes {
                            Some(((current_total - current_bytes) as f64 / eta_speed as f64).ceil() as u64)
                        } else if current_total > 0 && current_bytes >= current_total {
                            Some(0)
                        } else {
                            None
                        };

                        let (seg_defs, seg_values) = coord_ref.snapshot();
                        if supports_range && current_total > 0 {
                            save_checkpoint(&meta_path_owned, &url_owned, current_total, &seg_defs, &seg_values);
                        }

                        let segment_progresses = build_segment_progresses(&seg_defs, &seg_values);

                        let telemetry = ProgressTelemetry {
                            task_id: task_id_owned.clone(),
                            total_bytes: if current_total > 0 { Some(current_total) } else { None },
                            downloaded_bytes: current_bytes,
                            speed_bytes_per_sec,
                            progress_percent: percent.clamp(0.0, 100.0),
                            eta_seconds: eta,
                            status: TaskStatus::Downloading,
                            segments: segment_progresses,
                        };

                        if let Some(ref sender) = tx {
                            let _ = sender.send(telemetry);
                        }
                    }
                }
            })
        };

        let (active_client, active_fallback) =
            if options.proxy_url.map_or(false, |s| !s.trim().is_empty()) {
                build_http_clients(options.proxy_url)?
            } else {
                (self.client.clone(), self.fallback_client.clone())
            };

        let worker_ctx = Arc::new(SegmentWorkerContext {
            client: active_client,
            fallback_client: active_fallback,
            url: url.to_string(),
            destination_path: destination_path.to_path_buf(),
            supports_range,
            initial_size,
            total_downloaded: total_downloaded.clone(),
            total_size: total_size_atomic.clone(),
            controller: controller.clone(),
            is_aborted: is_aborted.clone(),
            wrote_any_chunk: wrote_any_chunk.clone(),
            limiter: limiter.clone(),
            cookie: options.cookie.map(|s| s.to_string()),
            referer: options.referer.map(|s| s.to_string()),
            user_agent: options.user_agent.map(|s| s.to_string()),
            auth_username: options.auth_username.map(|s| s.to_string()),
            auth_password: options.auth_password.map(|s| s.to_string()),
        });

        let allow_stealing = supports_range && initial_size > 0 && num_connections > 1;
        let min_split_bytes = (initial_size / (num_connections.max(1) as u64 * 4)).clamp(32_768, 1_048_576);
        let worker_count = num_connections.max(1);
        let mut handles = Vec::with_capacity(worker_count);

        for _ in 0..worker_count {
            let coord = coordinator.clone();
            let ctx = worker_ctx.clone();
            let handle = tokio::spawn(async move {
                while !ctx.controller.is_stopped() && !ctx.is_aborted.load(Ordering::Relaxed) {
                    let next_slot = match coord.claim_next() {
                        Some(s) => Some(s),
                        None if allow_stealing => coord.try_steal_segment(min_split_bytes),
                        None => None,
                    };
                    let Some(slot) = next_slot else {
                        break;
                    };
                    download_segment_slot(slot, &ctx).await?;
                }
                Ok::<(), anyhow::Error>(())
            });
            handles.push(handle);
        }

        let mut first_error: Option<anyhow::Error> = None;
        for handle in handles {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    is_aborted.store(true, Ordering::SeqCst);
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                }
                Err(e) => {
                    is_aborted.store(true, Ordering::SeqCst);
                    if first_error.is_none() {
                        first_error = Some(anyhow::anyhow!("Worker unduhan gagal: {}", e));
                    }
                }
            }
        }

        is_completed.store(true, Ordering::Relaxed);
        let _ = monitor_handle.await;

        let (final_segments, final_seg_values) = coordinator.snapshot();
        let current_downloaded: u64 = final_seg_values.iter().sum();

        if controller.is_canceled() {
            let _ = std::fs::remove_file(&meta_path);
            let _ = std::fs::remove_file(destination_path);
            if let Some(ref sender) = telemetry_tx {
                let _ = sender.send(ProgressTelemetry {
                    task_id: task_id.to_string(),
                    total_bytes: if initial_size > 0 { Some(initial_size) } else { None },
                    downloaded_bytes: 0,
                    speed_bytes_per_sec: 0,
                    progress_percent: 0.0,
                    eta_seconds: None,
                    status: TaskStatus::Canceled,
                    segments: vec![],
                });
            }
            return Ok(());
        }

        if controller.is_paused() {
            if supports_range && initial_size > 0 {
                save_checkpoint(&meta_path, url, initial_size, &final_segments, &final_seg_values);
            }
            if let Some(ref sender) = telemetry_tx {
                let percent = if initial_size > 0 {
                    ((current_downloaded as f64 / initial_size as f64) * 100.0) as f32
                } else {
                    0.0
                };
                let _ = sender.send(ProgressTelemetry {
                    task_id: task_id.to_string(),
                    total_bytes: if initial_size > 0 { Some(initial_size) } else { None },
                    downloaded_bytes: current_downloaded,
                    speed_bytes_per_sec: 0,
                    progress_percent: percent.clamp(0.0, 99.9),
                    eta_seconds: None,
                    status: TaskStatus::Paused,
                    segments: build_segment_progresses(&final_segments, &final_seg_values),
                });
            }
            return Ok(());
        }

        if let Some(err) = first_error {
            if supports_range && initial_size > 0 && wrote_any_chunk.load(Ordering::Relaxed) {
                save_checkpoint(&meta_path, url, initial_size, &final_segments, &final_seg_values);
            } else {
                let _ = std::fs::remove_file(destination_path);
                let _ = std::fs::remove_file(&meta_path);
            }
            return Err(err);
        }

        // Remove checkpoint metadata on full clean completion
        let _ = std::fs::remove_file(&meta_path);

        let actual_size = std::fs::metadata(destination_path)
            .map(|m| m.len())
            .unwrap_or(current_downloaded);

        if let Some(ref sender) = telemetry_tx {
            let completed_segs: Vec<SegmentProgress> = final_segments
                .iter()
                .map(|&(s_idx, start, end)| {
                    let seg_len = if actual_size > 0 && end >= start {
                        end - start + 1
                    } else {
                        actual_size
                    };
                    SegmentProgress {
                        segment_index: s_idx,
                        start_byte: start,
                        end_byte: end,
                        current_byte: start + seg_len,
                        downloaded_bytes: seg_len,
                        is_completed: true,
                    }
                })
                .collect();
            let telemetry = ProgressTelemetry {
                task_id: task_id.to_string(),
                total_bytes: Some(actual_size),
                downloaded_bytes: actual_size,
                speed_bytes_per_sec: 0,
                progress_percent: 100.0,
                eta_seconds: Some(0),
                status: TaskStatus::Completed,
                segments: completed_segs,
            };
            let _ = sender.send(telemetry);
        }

        Ok(())
    }
}

struct SegmentSlot {
    index: usize,
    start_byte: u64,
    end_byte: AtomicU64,
    downloaded_bytes: AtomicU64,
    write_lock: std::sync::Mutex<()>,
}

struct SegmentCoordinator {
    slots: std::sync::Mutex<Vec<Arc<SegmentSlot>>>,
    next_unclaimed: std::sync::atomic::AtomicUsize,
}

impl SegmentCoordinator {
    fn from_initial(segments: &[(usize, u64, u64)], initial_progress: &[u64]) -> Self {
        let slots = segments
            .iter()
            .enumerate()
            .map(|(idx, &(seg_idx, start, end))| {
                let done = initial_progress.get(idx).copied().unwrap_or(0);
                Arc::new(SegmentSlot {
                    index: seg_idx,
                    start_byte: start,
                    end_byte: AtomicU64::new(end),
                    downloaded_bytes: AtomicU64::new(done),
                    write_lock: std::sync::Mutex::new(()),
                })
            })
            .collect();
        Self {
            slots: std::sync::Mutex::new(slots),
            next_unclaimed: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn claim_next(&self) -> Option<Arc<SegmentSlot>> {
        let guard = self.slots.lock().ok()?;
        let idx = self.next_unclaimed.fetch_add(1, Ordering::Relaxed);
        guard.get(idx).cloned()
    }

    /// Steal half of the remaining un-downloaded byte range from the slowest active segment.
    fn try_steal_segment(&self, min_split_bytes: u64) -> Option<Arc<SegmentSlot>> {
        let snapshot = self.slots.lock().ok()?.clone();
        let mut best_slot: Option<(Arc<SegmentSlot>, u64)> = None;

        for slot in &snapshot {
            let _wguard = slot.write_lock.lock().ok()?;
            let done = slot.downloaded_bytes.load(Ordering::Relaxed);
            let cur_end = slot.end_byte.load(Ordering::Acquire);
            let cur_pos = slot.start_byte.saturating_add(done);
            if cur_end >= cur_pos {
                let remaining = (cur_end - cur_pos).saturating_add(1);
                if remaining >= min_split_bytes {
                    let is_better = best_slot
                        .as_ref()
                        .map_or(true, |(_, best_rem)| remaining > *best_rem);
                    if is_better {
                        best_slot = Some((slot.clone(), remaining));
                    }
                }
            }
        }

        let (donor, _) = best_slot?;
        let new_slot = {
            let _wguard = donor.write_lock.lock().ok()?;
            let done = donor.downloaded_bytes.load(Ordering::Relaxed);
            let cur_end = donor.end_byte.load(Ordering::Acquire);
            let cur_pos = donor.start_byte.saturating_add(done);
            if cur_end < cur_pos {
                return None;
            }
            let remaining = (cur_end - cur_pos).saturating_add(1);
            if remaining < min_split_bytes {
                return None;
            }
            let split_start = cur_pos + (remaining / 2);
            if split_start <= cur_pos || split_start > cur_end {
                return None;
            }
            donor.end_byte.store(split_start - 1, Ordering::Release);

            let mut guard = self.slots.lock().ok()?;
            let new_idx = guard.len();
            let created = Arc::new(SegmentSlot {
                index: new_idx,
                start_byte: split_start,
                end_byte: AtomicU64::new(cur_end),
                downloaded_bytes: AtomicU64::new(0),
                write_lock: std::sync::Mutex::new(()),
            });
            guard.push(created.clone());
            // Advance unclaimed cursor past newly pushed slot so only this thief handles it
            let _ = self.next_unclaimed.compare_exchange(
                new_idx,
                new_idx + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
            created
        };

        Some(new_slot)
    }

    fn snapshot(&self) -> (Vec<(usize, u64, u64)>, Vec<u64>) {
        let Ok(guard) = self.slots.lock() else {
            return (Vec::new(), Vec::new());
        };
        let mut defs = Vec::with_capacity(guard.len());
        let mut vals = Vec::with_capacity(guard.len());
        for slot in guard.iter() {
            defs.push((
                slot.index,
                slot.start_byte,
                slot.end_byte.load(Ordering::Acquire),
            ));
            vals.push(slot.downloaded_bytes.load(Ordering::Relaxed));
        }
        (defs, vals)
    }
}

struct SegmentWorkerContext {
    client: reqwest::Client,
    fallback_client: reqwest::Client,
    url: String,
    destination_path: PathBuf,
    supports_range: bool,
    initial_size: u64,
    total_downloaded: Arc<AtomicU64>,
    total_size: Arc<AtomicU64>,
    controller: DownloadController,
    is_aborted: Arc<AtomicBool>,
    wrote_any_chunk: Arc<AtomicBool>,
    limiter: Option<Arc<BandwidthLimiter>>,
    cookie: Option<String>,
    referer: Option<String>,
    user_agent: Option<String>,
    auth_username: Option<String>,
    auth_password: Option<String>,
}

async fn download_segment_slot(slot: Arc<SegmentSlot>, ctx: &SegmentWorkerContext) -> Result<()> {
    let has_browser_identity = ctx
        .cookie
        .as_deref()
        .map_or(false, |s| !s.trim().is_empty())
        || ctx
            .user_agent
            .as_deref()
            .map_or(false, |s| !s.trim().is_empty());

    let max_retries = 2usize;
    for attempt in 0..=max_retries {
        if ctx.controller.is_stopped() || ctx.is_aborted.load(Ordering::Relaxed) {
            return Ok(());
        }

        let (already_in_seg, cur_end) = {
            let _wg = slot.write_lock.lock().unwrap();
            (
                slot.downloaded_bytes.load(Ordering::Relaxed),
                slot.end_byte.load(Ordering::Acquire),
            )
        };
        let req_start = if ctx.supports_range && ctx.initial_size > 0 {
            slot.start_byte + already_in_seg
        } else {
            0
        };

        if ctx.supports_range && ctx.initial_size > 0 && req_start > cur_end {
            return Ok(());
        }

        let range_hdr = if ctx.supports_range && ctx.initial_size > 0 {
            Some(format!("bytes={}-{}", req_start, cur_end))
        } else {
            None
        };

        let res_attempt = send_with_redirects(
            &ctx.client,
            reqwest::Method::GET,
            &ctx.url,
            ctx.cookie.as_deref(),
            ctx.referer.as_deref(),
            ctx.user_agent.as_deref(),
            DEFAULT_USER_AGENT,
            range_hdr.as_deref(),
            ctx.auth_username.as_deref(),
            ctx.auth_password.as_deref(),
        )
        .await;

        let response = match res_attempt {
            Ok(res) if res.status().is_success() => res,
            Ok(res)
                if !has_browser_identity
                    && (res.status() == reqwest::StatusCode::FORBIDDEN
                        || res.status() == reqwest::StatusCode::UNAUTHORIZED) =>
            {
                send_with_redirects(
                    &ctx.fallback_client,
                    reqwest::Method::GET,
                    &ctx.url,
                    ctx.cookie.as_deref(),
                    ctx.referer.as_deref(),
                    None,
                    FALLBACK_USER_AGENT,
                    range_hdr.as_deref(),
                    ctx.auth_username.as_deref(),
                    ctx.auth_password.as_deref(),
                )
                .await
                .context("Gagal mengirim request segmen (fallback)")?
            }
            Ok(res) => res,
            Err(e) => {
                if !has_browser_identity && attempt == 0 {
                    match send_with_redirects(
                        &ctx.fallback_client,
                        reqwest::Method::GET,
                        &ctx.url,
                        ctx.cookie.as_deref(),
                        ctx.referer.as_deref(),
                        None,
                        FALLBACK_USER_AGENT,
                        range_hdr.as_deref(),
                        ctx.auth_username.as_deref(),
                        ctx.auth_password.as_deref(),
                    )
                    .await
                    {
                        Ok(r) => r,
                        Err(_) => {
                            if attempt < max_retries {
                                tokio::time::sleep(Duration::from_millis(500 * (attempt as u64 + 1))).await;
                                continue;
                            }
                            return Err(anyhow::anyhow!("Gagal mengirim request segmen: {}", e));
                        }
                    }
                } else if attempt < max_retries {
                    tokio::time::sleep(Duration::from_millis(500 * (attempt as u64 + 1))).await;
                    continue;
                } else {
                    return Err(anyhow::anyhow!("Gagal mengirim request segmen: {}", e));
                }
            }
        };

        let status = response.status();
        if !status.is_success() {
            let is_transient = matches!(
                status,
                reqwest::StatusCode::TOO_MANY_REQUESTS
                    | reqwest::StatusCode::REQUEST_TIMEOUT
                    | reqwest::StatusCode::BAD_GATEWAY
                    | reqwest::StatusCode::SERVICE_UNAVAILABLE
                    | reqwest::StatusCode::GATEWAY_TIMEOUT
            );
            if is_transient && attempt < max_retries {
                let delay = if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    1200 * (attempt as u64 + 1)
                } else {
                    600 * (attempt as u64 + 1)
                };
                tokio::time::sleep(Duration::from_millis(delay)).await;
                continue;
            }
            anyhow::bail!("Server merespons dengan status error: {}", status);
        }

        if is_unexpected_html_response(
            &ctx.url,
            response.url().as_str(),
            response.headers(),
            Some(&ctx.destination_path),
        ) {
            anyhow::bail!(
                "Unduhan dibatalkan: Server mengembalikan halaman web HTML (bukan berkas asli). Tautan mungkin kedaluwarsa atau membutuhkan Cookie sesi aktif dari browser."
            );
        }

        if let Some(cl) = response.content_length() {
            if ctx.total_size.load(Ordering::Relaxed) == 0 {
                ctx.total_size.store(cl, Ordering::Relaxed);
            }
        }

        if ctx.supports_range
            && ctx.initial_size > 0
            && req_start > 0
            && response.status() == reqwest::StatusCode::OK
        {
            if already_in_seg > 0 {
                anyhow::bail!("Server tidak mendukung Range resume untuk segmen ini");
            }
            return Ok(());
        }

        let write_offset = if ctx.supports_range && ctx.initial_size > 0 {
            req_start
        } else {
            slot.downloaded_bytes.store(0, Ordering::Relaxed);
            ctx.total_downloaded.store(0, Ordering::Relaxed);
            0
        };

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&ctx.destination_path)
            .context("Gagal membuka file tujuan untuk menulis segmen")?;
        file.seek(SeekFrom::Start(write_offset))?;

        let mut stream = response.bytes_stream();
        let mut stream_err: Option<anyhow::Error> = None;

        while let Some(chunk_result) = stream.next().await {
            if ctx.controller.is_stopped() || ctx.is_aborted.load(Ordering::Relaxed) {
                let _ = file.flush();
                return Ok(());
            }
            if let (Some(lim), Some(override_bps)) = (&ctx.limiter, ctx.controller.speed_limit()) {
                lim.set_limit(override_bps);
            }

            match chunk_result {
                Ok(chunk) => {
                    let len = chunk.len() as u64;
                    if let Some(ref lim) = ctx.limiter {
                        lim.throttle(len, &ctx.controller, &ctx.is_aborted).await;
                        if ctx.controller.is_stopped() || ctx.is_aborted.load(Ordering::Relaxed) {
                            let _ = file.flush();
                            return Ok(());
                        }
                    }

                    let mut reached_end = false;
                    {
                        let _wg = slot.write_lock.lock().unwrap();
                        let cur_end_now = slot.end_byte.load(Ordering::Acquire);
                        let cur_done = slot.downloaded_bytes.load(Ordering::Relaxed);
                        let cur_pos = slot.start_byte + cur_done;

                        if ctx.supports_range && ctx.initial_size > 0 && cur_pos > cur_end_now {
                            reached_end = true;
                        } else {
                            let slice_to_write = if ctx.supports_range && ctx.initial_size > 0 {
                                let allowed = (cur_end_now - cur_pos + 1) as usize;
                                &chunk[..chunk.len().min(allowed)]
                            } else {
                                &chunk[..]
                            };
                            if !slice_to_write.is_empty() {
                                file.write_all(slice_to_write)?;
                                let written = slice_to_write.len() as u64;
                                ctx.wrote_any_chunk.store(true, Ordering::Relaxed);
                                slot.downloaded_bytes.fetch_add(written, Ordering::Relaxed);
                                ctx.total_downloaded.fetch_add(written, Ordering::Relaxed);
                            }
                            if ctx.supports_range
                                && ctx.initial_size > 0
                                && cur_pos + (slice_to_write.len() as u64) > cur_end_now
                            {
                                reached_end = true;
                            }
                        }
                    }

                    if reached_end {
                        file.flush()?;
                        return Ok(());
                    }
                }
                Err(e) => {
                    stream_err = Some(anyhow::anyhow!("Error saat menerima data chunk: {}", e));
                    break;
                }
            }
        }

        file.flush()?;

        if let Some(err) = stream_err {
            if attempt < max_retries && ctx.supports_range && ctx.initial_size > 0 {
                tokio::time::sleep(Duration::from_millis(500 * (attempt as u64 + 1))).await;
                continue;
            }
            return Err(err);
        }

        return Ok(());
    }

    Ok(())
}

fn build_segment_progresses(segments: &[(usize, u64, u64)], seg_values: &[u64]) -> Vec<SegmentProgress> {
    segments
        .iter()
        .enumerate()
        .map(|(idx, &(s_idx, start, end))| {
            let downloaded = seg_values.get(idx).copied().unwrap_or(0);
            let seg_len = end.saturating_sub(start) + 1;
            SegmentProgress {
                segment_index: s_idx,
                start_byte: start,
                end_byte: end,
                current_byte: start + downloaded,
                downloaded_bytes: downloaded,
                is_completed: seg_len > 0 && downloaded >= seg_len,
            }
        })
        .collect()
}

pub(crate) fn save_checkpoint(
    meta_path: &Path,
    url: &str,
    total_size: u64,
    segments: &[(usize, u64, u64)],
    seg_values: &[u64],
) {
    let cp = DownloadCheckpoint {
        url: url.to_string(),
        total_size,
        segments: segments
            .iter()
            .enumerate()
            .map(|(i, &(index, start_byte, end_byte))| SegmentCheckpoint {
                index,
                start_byte,
                end_byte,
                downloaded_bytes: seg_values.get(i).copied().unwrap_or(0),
            })
            .collect(),
    };
    if let Ok(json) = serde_json::to_string(&cp) {
        let _ = std::fs::write(meta_path, json);
    }
}
