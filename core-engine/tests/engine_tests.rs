use core_engine::inspector::categorize_filename;
use core_engine::model::{DownloadTask, TaskStatus};
use core_engine::storage::Storage;

#[test]
fn test_categorize_filename() {
    assert_eq!(categorize_filename("archive.zip"), "compressed");
    assert_eq!(categorize_filename("backup.tar.gz"), "compressed");
    assert_eq!(categorize_filename("movie.mp4"), "video");
    assert_eq!(categorize_filename("song.flac"), "music");
    assert_eq!(categorize_filename("setup.exe"), "programs");
    assert_eq!(categorize_filename("installer.dmg"), "programs");
    assert_eq!(categorize_filename("document.pdf"), "documents");
    assert_eq!(categorize_filename("random_unknown.xyz"), "general");
}

#[test]
fn test_storage_lifecycle() {
    let storage = Storage::in_memory().expect("In-memory SQLite should initialize");

    // 1. Verify default categories are seeded
    let categories = storage.get_categories().expect("Should read categories");
    assert_eq!(categories.len(), 6);
    assert!(categories.iter().any(|c| c.id == "compressed"));

    // 2. Insert task
    let task = DownloadTask {
        id: "test-task-1".to_string(),
        url: "https://example.com/file.zip".to_string(),
        final_url: Some("https://example.com/file.zip".to_string()),
        file_name: "file.zip".to_string(),
        file_size: Some(10485760),
        downloaded_bytes: 0,
        status: TaskStatus::Queued,
        error_message: None,
        category_id: "compressed".to_string(),
        destination_path: "/tmp/file.zip".to_string(),
        mime_type: Some("application/zip".to_string()),
        etag: Some("test-etag-123".to_string()),
        last_modified: None,
        supports_range: true,
        num_connections: 8,
        speed_limit_bytes_per_sec: 0,
        created_at: "2026-11-11T00:00:00Z".to_string(),
        completed_at: None,
    };

    storage.insert_task(&task).expect("Should insert task");

    // 3. Read back tasks
    let tasks = storage.get_all_tasks().expect("Should get all tasks");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "test-task-1");
    assert_eq!(tasks[0].status, TaskStatus::Queued);

    // 4. Update progress
    storage.update_task_progress("test-task-1", TaskStatus::Downloading, 5242880, None)
        .expect("Should update progress");

    let updated_tasks = storage.get_all_tasks().expect("Should get tasks");
    assert_eq!(updated_tasks[0].status, TaskStatus::Downloading);
    assert_eq!(updated_tasks[0].downloaded_bytes, 5242880);

    // 5. Complete task
    storage.update_task_progress("test-task-1", TaskStatus::Completed, 10485760, None)
        .expect("Should mark complete");

    let completed_tasks = storage.get_all_tasks().expect("Should get tasks");
    assert_eq!(completed_tasks[0].status, TaskStatus::Completed);
    assert!(completed_tasks[0].completed_at.is_some());
}

#[test]
fn test_video_stream_detection() {
    use core_engine::inspector::detect_video_stream;

    // Video platforms (YouTube, etc.) are handled by yt-dlp in src-tauri, not synthetic formats
    let yt_url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    let yt_fallback = detect_video_stream(yt_url, yt_url, "dQw4w9WgXcQ.mp4", Some("text/html"), None);
    assert!(yt_fallback.is_none());

    // Test direct MP4 stream
    let mp4_url = "https://cdn.example.com/videos/nature_documentary.mp4";
    let media_direct = detect_video_stream(mp4_url, mp4_url, "nature_documentary.mp4", Some("video/mp4"), Some(120_000_000));
    assert!(media_direct.is_some());
    assert_eq!(media_direct.unwrap().formats[0].extension, "mp4");

    // Test non-video URL
    let zip_url = "https://example.com/archive.zip";
    let non_video = detect_video_stream(zip_url, zip_url, "archive.zip", Some("application/zip"), None);
    assert!(non_video.is_none());
}

#[tokio::test]
async fn test_html_redirect_rejected_and_cookies_forwarded() {
    use core_engine::downloader::Downloader;
    use core_engine::inspector::UrlInspector;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind local mock");
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let payload = b"7z\xBC\xAF\x27\x1C_REAL_ARCHIVE_BYTES_1234567890";
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let mut total = 0usize;
                loop {
                    let n = sock.read(&mut buf[total..]).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    total += n;
                    if buf[..total].windows(4).any(|w| w == b"\r\n\r\n") || total >= buf.len() {
                        break;
                    }
                }
                let req = String::from_utf8_lossy(&buf[..total]).to_string();
                let req_lower = req.to_lowercase();

                if req.starts_with("HEAD /download/test.7z") || req.starts_with("GET /download/test.7z") {
                    let has_cookie = req_lower.contains("cookie: accounttoken=secret-gofile-session");
                    let has_referer = req_lower.contains("referer: https://gofile.io/");
                    let has_ua = req_lower.contains("user-agent: customtestchrome/1.0");

                    if has_cookie && has_referer && has_ua {
                        if req.starts_with("HEAD ") {
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/x-7z-compressed\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                                payload.len()
                            );
                            let _ = sock.write_all(resp.as_bytes()).await;
                        } else {
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/x-7z-compressed\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                                payload.len()
                            );
                            let _ = sock.write_all(resp.as_bytes()).await;
                            let _ = sock.write_all(payload).await;
                        }
                    } else {
                        let resp = format!(
                            "HTTP/1.1 302 Found\r\nLocation: http://{}/d/507c8a88\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            addr
                        );
                        let _ = sock.write_all(resp.as_bytes()).await;
                    }
                } else if req.contains("/d/507c8a88") {
                    let html = "<!doctype html><html><body>Gofile Landing</body></html>";
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        html.len(),
                        html
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                }
            });
        }
    });

    let test_url = format!("http://{}/download/test.7z", addr);
    let inspector = UrlInspector::new().expect("inspector init");

    // 1. Without cookies -> redirected to HTML landing page -> MUST reject cleanly
    let err_no_cookie = inspector.inspect(&test_url).await;
    assert!(err_no_cookie.is_err(), "Missing cookies redirecting to HTML must fail inspection");
    let err_msg = format!("{}", err_no_cookie.err().unwrap());
    assert!(err_msg.contains("HTML"), "Error should mention HTML response: {}", err_msg);

    // 2. Downloader without cookies -> must refuse saving HTML into .7z and delete temp file
    let downloader = Downloader::new().expect("downloader init");
    let bad_dest = std::env::temp_dir().join(format!("dc_bad_{}.7z", std::process::id()));
    let _ = std::fs::remove_file(&bad_dest);
    let dl_err = downloader
        .download_with_headers("t-bad", &test_url, &bad_dest, None, false, 1, None, None, None, None, None)
        .await;
    assert!(dl_err.is_err(), "Downloader must reject HTML response for .7z destination");
    assert!(!bad_dest.exists(), "Downloader must not leave an HTML file masquerading as .7z");

    // 3. With Cookie + Referer + User-Agent -> inspect succeeds AND download saves exact binary bytes
    let ok_inspect = inspector
        .inspect_with_headers(
            &test_url,
            Some("accountToken=secret-gofile-session"),
            Some("https://gofile.io/"),
            Some("CustomTestChrome/1.0"),
        )
        .await
        .expect("Authenticated inspect should succeed");

    assert_eq!(ok_inspect.file_name, "test.7z");
    assert_eq!(ok_inspect.file_size, Some(36));

    let good_dest = std::env::temp_dir().join(format!("dc_good_{}.7z", std::process::id()));
    let _ = std::fs::remove_file(&good_dest);
    downloader
        .download_with_headers(
            "t-good",
            &ok_inspect.final_url,
            &good_dest,
            ok_inspect.file_size,
            false,
            1,
            None,
            None,
            Some("accountToken=secret-gofile-session"),
            Some("https://gofile.io/"),
            Some("CustomTestChrome/1.0"),
        )
        .await
        .expect("Authenticated download should succeed");

    let saved = std::fs::read(&good_dest).expect("read saved 7z");
    assert_eq!(saved.as_slice(), b"7z\xBC\xAF\x27\x1C_REAL_ARCHIVE_BYTES_1234567890");
    let _ = std::fs::remove_file(&good_dest);

    server_handle.abort();
}

#[test]
fn test_chunk_range_calculation() {
    let total_size = 100_000_000u64;
    let num_connections = 8;
    let chunk_size = total_size / (num_connections as u64);

    let mut segments = Vec::new();
    for i in 0..num_connections {
        let start = (i as u64) * chunk_size;
        let end = if i == num_connections - 1 {
            total_size - 1
        } else {
            start + chunk_size - 1
        };
        segments.push((start, end));
    }

    assert_eq!(segments.len(), 8);
    assert_eq!(segments[0], (0, 12_499_999));
    assert_eq!(segments[7], (87_500_000, 99_999_999));

    // Ensure contiguous non-overlapping coverage
    for i in 0..7 {
        assert_eq!(segments[i].1 + 1, segments[i + 1].0);
    }
}

#[tokio::test]
async fn test_pause_and_resume_preserves_checkpoint_bytes() {
    use core_engine::downloader::{checkpoint_path_for, DownloadController, DownloadOptions, Downloader};
    use core_engine::model::ProgressTelemetry;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::broadcast;

    let payload: Vec<u8> = (0..131_072u32).map(|i| ((i * 31 + 7) % 251) as u8).collect();
    let payload_arc = std::sync::Arc::new(payload);
    let requested_ranges = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind test server");
    let addr = listener.local_addr().unwrap();

    let server_payload = payload_arc.clone();
    let server_ranges = requested_ranges.clone();
    let server_handle = tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let data = server_payload.clone();
            let ranges = server_ranges.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let range_line = req
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                    .map(|l| l.to_string());

                if req.starts_with("HEAD ") {
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                        data.len()
                    );
                    let _ = sock.write_all(hdr.as_bytes()).await;
                    return;
                }

                if let Some(rl) = range_line {
                    ranges.lock().await.push(rl.clone());
                    let val = rl.split(':').nth(1).unwrap_or("").trim().trim_start_matches("bytes=");
                    let mut parts = val.split('-');
                    let start: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let end: usize = parts
                        .next()
                        .and_then(|s| if s.is_empty() { None } else { s.parse().ok() })
                        .unwrap_or(data.len() - 1)
                        .min(data.len() - 1);
                    let slice = &data[start..=end];
                    let hdr = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                        slice.len(),
                        start,
                        end,
                        data.len()
                    );
                    if sock.write_all(hdr.as_bytes()).await.is_err() {
                        return;
                    }
                    for chunk in slice.chunks(2048) {
                        if sock.write_all(chunk).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(45)).await;
                    }
                } else {
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                        data.len()
                    );
                    let _ = sock.write_all(hdr.as_bytes()).await;
                    let _ = sock.write_all(&data).await;
                }
            });
        }
    });

    let url = format!("http://{}/large.bin", addr);
    let dest = std::env::temp_dir().join(format!("dc_pause_resume_{}.bin", std::process::id()));
    let meta = checkpoint_path_for(&dest);
    let _ = std::fs::remove_file(&dest);
    let _ = std::fs::remove_file(&meta);

    let downloader = Downloader::new().expect("downloader init");
    let (tx, mut rx) = broadcast::channel::<ProgressTelemetry>(128);
    let controller = DownloadController::new();
    let ctrl_pauser = controller.clone();

    let pauser = tokio::spawn(async move {
        while let Ok(t) = rx.recv().await {
            if t.downloaded_bytes >= 16_384 && t.status == TaskStatus::Downloading {
                ctrl_pauser.pause();
                break;
            }
        }
    });

    downloader
        .download_with_options(
            "t-pause",
            &url,
            &dest,
            Some(payload_arc.len() as u64),
            true,
            4,
            Some(tx),
            Some(controller.clone()),
            DownloadOptions::default(),
        )
        .await
        .expect("paused download should return Ok");

    let _ = pauser.await;
    assert!(controller.is_paused(), "Controller should be paused");
    assert!(meta.exists(), "Checkpoint .dcmeta file must remain after pause");

    let count_before_resume = requested_ranges.lock().await.len();

    // Resume into the same destination
    let (tx2, mut rx2) = broadcast::channel::<ProgressTelemetry>(128);
    let controller2 = DownloadController::new();
    downloader
        .download_with_options(
            "t-pause",
            &url,
            &dest,
            Some(payload_arc.len() as u64),
            true,
            4,
            Some(tx2),
            Some(controller2),
            DownloadOptions::default(),
        )
        .await
        .expect("resumed download should succeed");

    let mut saw_completed = false;
    while let Ok(t) = rx2.try_recv() {
        if t.status == TaskStatus::Completed {
            saw_completed = true;
        }
    }
    assert!(saw_completed, "Resume should emit TaskStatus::Completed");
    assert!(!meta.exists(), "Completed download should delete .dcmeta checkpoint");

    // Resumed range headers should start after > 0 bytes on at least one chunk
    let all_ranges = requested_ranges.lock().await;
    let resume_ranges = &all_ranges[count_before_resume..];
    assert!(
        resume_ranges.iter().any(|r| !r.contains("bytes=0-") && !r.contains("bytes=32768-") && !r.contains("bytes=65536-") && !r.contains("bytes=98304-")),
        "At least one resumed chunk should continue mid-segment instead of restarting from segment origin: {:?}",
        resume_ranges
    );

    let saved = std::fs::read(&dest).expect("read resumed file");
    assert_eq!(saved, *payload_arc, "Resumed file contents must match payload byte-for-byte");

    let _ = std::fs::remove_file(&dest);
    server_handle.abort();
}

#[tokio::test]
async fn test_rate_limited_429_steps_down_and_succeeds() {
    use core_engine::downloader::{DownloadOptions, Downloader};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let payload: Vec<u8> = (0..65_536u32).map(|i| ((i * 17 + 3) % 253) as u8).collect();
    let payload_arc = std::sync::Arc::new(payload);
    let seen_range = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen_full = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind 429 test server");
    let addr = listener.local_addr().unwrap();

    let sp = payload_arc.clone();
    let sr = seen_range.clone();
    let sf = seen_full.clone();
    let server_handle = tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let data = sp.clone();
            let sr = sr.clone();
            let sf = sf.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let has_range = req
                    .lines()
                    .any(|l| l.to_ascii_lowercase().starts_with("range:"));

                if has_range {
                    sr.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let resp = "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 12\r\nConnection: close\r\n\r\nrate limited";
                    let _ = sock.write_all(resp.as_bytes()).await;
                } else {
                    sf.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                        data.len()
                    );
                    let _ = sock.write_all(hdr.as_bytes()).await;
                    let _ = sock.write_all(&data).await;
                }
            });
        }
    });

    let url = format!("http://{}/limited.bin", addr);
    let dest = std::env::temp_dir().join(format!("dc_stepdown_{}.bin", std::process::id()));
    let _ = std::fs::remove_file(&dest);

    let downloader = Downloader::new().expect("downloader init");
    downloader
        .download_with_options(
            "t-429",
            &url,
            &dest,
            Some(payload_arc.len() as u64),
            true,
            8,
            None,
            None,
            DownloadOptions::default(),
        )
        .await
        .expect("Step-down fallback after 429 should succeed");

    assert!(seen_range.load(std::sync::atomic::Ordering::SeqCst) > 0);
    assert_eq!(seen_full.load(std::sync::atomic::Ordering::SeqCst), 1);

    let saved = std::fs::read(&dest).expect("read stepdown output");
    assert_eq!(saved, *payload_arc);

    let _ = std::fs::remove_file(&dest);
    server_handle.abort();
}

#[tokio::test]
async fn test_redirect_cookie_scoping_and_speed_limiter() {
    use core_engine::downloader::{DownloadOptions, Downloader};
    use core_engine::UrlInspector;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    // Cross-domain server bound to localhost (different domain from 127.0.0.1)
    let ext_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ext");
    let ext_port = ext_listener.local_addr().unwrap().port();
    let ext_cookie_seen = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ext_flag = ext_cookie_seen.clone();
    let ext_handle = tokio::spawn(async move {
        if let Ok((mut sock, _)) = ext_listener.accept().await {
            let mut buf = [0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            if req.lines().any(|l| l.to_ascii_lowercase().starts_with("cookie:")) {
                ext_flag.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            let resp = "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\nCROSS_OKAY";
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });

    // Main server on 127.0.0.1: /hop1 -> 302 to 127.0.0.1/payload.bin (same host preserves cookie),
    // and /cross -> 302 to localhost:<ext_port>/ext.bin (unrelated host strips cookie)
    let payload = vec![0x5Au8; 96 * 1024];
    let payload_arc = std::sync::Arc::new(payload);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind main");
    let addr = listener.local_addr().unwrap();
    let main_sp = payload_arc.clone();
    let main_handle = tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let data = main_sp.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let first = req.lines().next().unwrap_or("");
                let has_cookie = req
                    .lines()
                    .any(|l| l.eq_ignore_ascii_case("cookie: session=top-secret"));

                if first.contains("/cross") {
                    let resp = format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://localhost:{}/ext.bin\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        ext_port
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                } else if first.contains("/hop1") {
                    let resp = format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://{}/payload.bin\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        addr
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                } else if first.contains("/payload.bin") {
                    if !has_cookie {
                        let resp = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        let _ = sock.write_all(resp.as_bytes()).await;
                        return;
                    }
                    let method_head = first.starts_with("HEAD ");
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                        data.len()
                    );
                    let _ = sock.write_all(hdr.as_bytes()).await;
                    if !method_head {
                        let _ = sock.write_all(&data).await;
                    }
                }
            });
        }
    });

    let inspector = UrlInspector::new().expect("inspector");
    // 1. Cross-domain redirect MUST strip cookie
    let cross_res = inspector
        .inspect_with_headers(&format!("http://{}/cross", addr), Some("session=top-secret"), None, None)
        .await
        .expect("cross inspect should succeed");
    assert_eq!(cross_res.file_name, "ext.bin");
    assert!(!ext_cookie_seen.load(std::sync::atomic::Ordering::SeqCst), "Cookie MUST be stripped on unrelated domain redirect");

    // 2. Same-domain redirect preserves cookie and token-bucket throttles 96KB @ 64KB/s (~0.45s+)
    let same_res = inspector
        .inspect_with_headers(&format!("http://{}/hop1", addr), Some("session=top-secret"), None, None)
        .await
        .expect("same-domain hop inspect should keep cookie");
    assert_eq!(same_res.file_name, "payload.bin");
    assert_eq!(same_res.file_size, Some(98_304));

    let dest = std::env::temp_dir().join(format!("dc_speed_{}.bin", std::process::id()));
    let _ = std::fs::remove_file(&dest);
    let downloader = Downloader::new().expect("downloader");
    let started = std::time::Instant::now();
    downloader
        .download_with_options(
            "t-speed",
            &format!("http://{}/hop1", addr),
            &dest,
            same_res.file_size,
            false,
            1,
            None,
            None,
            DownloadOptions {
                cookie: Some("session=top-secret"),
                referer: None,
                user_agent: None,
                speed_limit_bytes_per_sec: Some(64 * 1024),
                ..Default::default()
            },
        )
        .await
        .expect("throttled same-domain download should succeed");

    let elapsed = started.elapsed();
    assert!(
        elapsed >= std::time::Duration::from_millis(350),
        "Token bucket speed limiter should throttle transfer; elapsed {:?}",
        elapsed
    );
    let saved = std::fs::read(&dest).expect("read speed output");
    assert_eq!(saved.len(), 98_304);

    let _ = std::fs::remove_file(&dest);
    main_handle.abort();
    ext_handle.abort();
}

#[tokio::test]
async fn test_concurrent_segments_and_eta_telemetry() {
    use core_engine::downloader::{DownloadOptions, Downloader};
    use core_engine::model::ProgressTelemetry;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::broadcast;

    let payload = vec![0xA5u8; 262_144]; // 256 KB -> 8 segments x 32 KB
    let payload_arc = std::sync::Arc::new(payload);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();

    let sp = payload_arc.clone();
    let server_handle = tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let data = sp.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let range_line = req
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                    .map(|l| l.to_string());

                if let Some(rl) = range_line {
                    let val = rl.split(':').nth(1).unwrap_or("").trim().trim_start_matches("bytes=");
                    let mut parts = val.split('-');
                    let start: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let end: usize = parts
                        .next()
                        .and_then(|s| if s.is_empty() { None } else { s.parse().ok() })
                        .unwrap_or(data.len() - 1)
                        .min(data.len() - 1);
                    let slice = &data[start..=end];
                    let hdr = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                        slice.len(),
                        start,
                        end,
                        data.len()
                    );
                    if sock.write_all(hdr.as_bytes()).await.is_err() {
                        return;
                    }
                    for chunk in slice.chunks(4096) {
                        if sock.write_all(chunk).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(55)).await;
                    }
                }
            });
        }
    });

    let url = format!("http://{}/multi.bin", addr);
    let dest = std::env::temp_dir().join(format!("dc_parallel_eta_{}.bin", std::process::id()));
    let _ = std::fs::remove_file(&dest);

    let (tx, mut rx) = broadcast::channel::<ProgressTelemetry>(128);
    let collector = tokio::spawn(async move {
        let mut collected = Vec::new();
        while let Ok(t) = rx.recv().await {
            let done = t.status == TaskStatus::Completed;
            collected.push(t);
            if done {
                break;
            }
        }
        collected
    });

    let downloader = Downloader::new().expect("downloader");
    downloader
        .download_with_options(
            "t-parallel",
            &url,
            &dest,
            Some(payload_arc.len() as u64),
            true,
            8,
            Some(tx),
            None,
            DownloadOptions::default(),
        )
        .await
        .expect("parallel download should succeed");

    let events = collector.await.expect("collector");
    let active_events: Vec<&ProgressTelemetry> = events
        .iter()
        .filter(|e| e.status == TaskStatus::Downloading && e.downloaded_bytes > 0)
        .collect();

    assert!(!active_events.is_empty(), "Should receive active Downloading telemetry events");
    assert!(
        active_events.iter().any(|e| e.eta_seconds.is_some()),
        "At least one active Downloading telemetry event must populate eta_seconds"
    );
    assert!(
        active_events.iter().any(|e| {
            e.segments.len() == 8 && e.segments.iter().filter(|s| s.downloaded_bytes > 0).count() >= 6
        }),
        "Multiple segments must actively download in parallel simultaneously"
    );

    let _ = std::fs::remove_file(&dest);
    server_handle.abort();
}

#[tokio::test]
async fn test_cancel_mid_transfer_removes_partial_and_checkpoint_files() {
    use core_engine::downloader::{checkpoint_path_for, DownloadController, DownloadOptions, Downloader};
    use core_engine::model::ProgressTelemetry;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::broadcast;

    let payload: Vec<u8> = (0..131_072u32).map(|i| ((i * 19 + 3) % 251) as u8).collect();
    let payload_arc = std::sync::Arc::new(payload);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind cancel test server");
    let addr = listener.local_addr().unwrap();

    let server_payload = payload_arc.clone();
    let server_handle = tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let data = server_payload.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let range_line = req
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                    .map(|l| l.to_string());

                if let Some(rl) = range_line {
                    let val = rl.split(':').nth(1).unwrap_or("").trim().trim_start_matches("bytes=");
                    let mut parts = val.split('-');
                    let start: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let end: usize = parts
                        .next()
                        .and_then(|s| if s.is_empty() { None } else { s.parse().ok() })
                        .unwrap_or(data.len() - 1)
                        .min(data.len() - 1);
                    let slice = &data[start..=end];
                    let hdr = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                        slice.len(),
                        start,
                        end,
                        data.len()
                    );
                    if sock.write_all(hdr.as_bytes()).await.is_err() {
                        return;
                    }
                    for chunk in slice.chunks(2048) {
                        if sock.write_all(chunk).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(45)).await;
                    }
                }
            });
        }
    });

    let url = format!("http://{}/cancel_me.bin", addr);
    let dest = std::env::temp_dir().join(format!("dc_cancel_cleanup_{}.bin", std::process::id()));
    let meta = checkpoint_path_for(&dest);
    let _ = std::fs::remove_file(&dest);
    let _ = std::fs::remove_file(&meta);

    let downloader = Downloader::new().expect("downloader init");
    let (tx, mut rx) = broadcast::channel::<ProgressTelemetry>(128);
    let controller = DownloadController::new();
    let ctrl_canceler = controller.clone();

    let collector = tokio::spawn(async move {
        let mut saw_canceled = false;
        while let Ok(t) = rx.recv().await {
            if t.downloaded_bytes >= 8_192 && t.status == TaskStatus::Downloading {
                ctrl_canceler.cancel();
            }
            if t.status == TaskStatus::Canceled {
                assert_eq!(t.downloaded_bytes, 0);
                saw_canceled = true;
                break;
            }
        }
        saw_canceled
    });

    downloader
        .download_with_options(
            "t-cancel",
            &url,
            &dest,
            Some(payload_arc.len() as u64),
            true,
            4,
            Some(tx),
            Some(controller),
            DownloadOptions::default(),
        )
        .await
        .expect("canceled download should exit cleanly");

    let saw_canceled = collector.await.expect("collector");
    assert!(saw_canceled, "Must emit Canceled telemetry status");
    assert!(!dest.exists(), "Canceled partial destination file must be deleted from disk");
    assert!(!meta.exists(), "Canceled checkpoint .dcmeta file must be deleted from disk");

    server_handle.abort();
}

#[tokio::test]
async fn test_dynamic_work_stealing_and_refresh_url_resume() {
    use core_engine::downloader::{checkpoint_path_for, DownloadController, DownloadOptions, Downloader};
    use core_engine::model::ProgressTelemetry;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::broadcast;

    let payload: Vec<u8> = (0..131_072u32).map(|i| ((i * 29 + 11) % 251) as u8).collect();
    let payload_arc = std::sync::Arc::new(payload);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind steal server");
    let addr = listener.local_addr().unwrap();
    let new_token_requests = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));

    let server_payload = payload_arc.clone();
    let server_new_reqs = new_token_requests.clone();
    let server_handle = tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let data = server_payload.clone();
            let req_log = server_new_reqs.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let first_line = req.lines().next().unwrap_or("");
                let path = first_line.split_whitespace().nth(1).unwrap_or("");
                let range_line = req
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                    .map(|l| l.to_string());

                if path == "/new-token.bin" {
                    if let Some(ref rl) = range_line {
                        req_log.lock().await.push(rl.clone());
                    }
                }

                if let Some(rl) = range_line {
                    let val = rl.split(':').nth(1).unwrap_or("").trim().trim_start_matches("bytes=");
                    let mut parts = val.split('-');
                    let start: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let end: usize = parts
                        .next()
                        .and_then(|s| if s.is_empty() { None } else { s.parse().ok() })
                        .unwrap_or(data.len() - 1)
                        .min(data.len() - 1);
                    let slice = &data[start..=end];
                    let hdr = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        slice.len(),
                        start,
                        end,
                        data.len()
                    );
                    if sock.write_all(hdr.as_bytes()).await.is_err() {
                        return;
                    }
                    // Intentionally throttle requests starting at byte 0 so faster worker steals its tail!
                    let slow = start == 0;
                    for chunk in slice.chunks(4096) {
                        if sock.write_all(chunk).await.is_err() {
                            return;
                        }
                        if slow {
                            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
                        }
                    }
                }
            });
        }
    });

    let downloader = Downloader::new().expect("downloader");

    // 1. Verify dynamic work-stealing produces >2 segments on slow segment 0
    let steal_dest = std::env::temp_dir().join(format!("dc_steal_{}.bin", std::process::id()));
    let _ = std::fs::remove_file(&steal_dest);
    let _ = std::fs::remove_file(checkpoint_path_for(&steal_dest));

    let (tx, mut rx) = broadcast::channel::<ProgressTelemetry>(128);
    let collector = tokio::spawn(async move {
        let mut max_segments = 0usize;
        while let Ok(t) = rx.recv().await {
            max_segments = max_segments.max(t.segments.len());
            if t.status == TaskStatus::Completed {
                break;
            }
        }
        max_segments
    });

    downloader
        .download_with_options(
            "t-steal",
            &format!("http://{}/steal.bin", addr),
            &steal_dest,
            Some(payload_arc.len() as u64),
            true,
            2,
            Some(tx),
            None,
            DownloadOptions::default(),
        )
        .await
        .expect("dynamic work-stealing download must succeed");

    let max_segs = collector.await.expect("collector");
    assert!(
        max_segs > 2,
        "Fast thread should dynamically split slow tail segment so segment count exceeds initial 2 (got {})",
        max_segs
    );
    assert_eq!(std::fs::read(&steal_dest).unwrap(), *payload_arc);
    let _ = std::fs::remove_file(&steal_dest);

    // 2. Verify Refresh Download Address resumes existing .dcmeta without restarting from byte 0
    let ref_dest = std::env::temp_dir().join(format!("dc_refresh_{}.bin", std::process::id()));
    let _ = std::fs::remove_file(&ref_dest);
    let _ = std::fs::remove_file(checkpoint_path_for(&ref_dest));

    let ctrl = DownloadController::new();
    let ctrl_p = ctrl.clone();
    let (tx1, mut rx1) = broadcast::channel::<ProgressTelemetry>(128);
    tokio::spawn(async move {
        while let Ok(t) = rx1.recv().await {
            if t.downloaded_bytes >= 32_768 && t.status == TaskStatus::Downloading {
                ctrl_p.pause();
                break;
            }
        }
    });

    downloader
        .download_with_options(
            "t-refresh",
            &format!("http://{}/old-expired.bin", addr),
            &ref_dest,
            Some(payload_arc.len() as u64),
            true,
            2,
            Some(tx1),
            Some(ctrl),
            DownloadOptions::default(),
        )
        .await
        .expect("initial paused transfer");

    downloader
        .download_with_options(
            "t-refresh",
            &format!("http://{}/new-token.bin", addr),
            &ref_dest,
            Some(payload_arc.len() as u64),
            true,
            2,
            None,
            None,
            DownloadOptions {
                allow_url_refresh: true,
                ..Default::default()
            },
        )
        .await
        .expect("refreshed URL should resume seamlessly");

    let reqs = new_token_requests.lock().await;
    assert!(!reqs.is_empty());
    assert!(
        reqs.iter().any(|r| !r.contains("bytes=0-") && !r.contains("bytes=65536-")),
        "Refreshed URL must continue mid-segment from existing checkpoint: {:?}",
        *reqs
    );
    assert_eq!(std::fs::read(&ref_dest).unwrap(), *payload_arc);

    let _ = std::fs::remove_file(&ref_dest);
    server_handle.abort();
}

#[tokio::test]
async fn test_checksums_disk_space_ftp_and_grabber() {
    use core_engine::grabber::{grab_site_links, GrabOptions};
    use core_engine::integrity::{available_disk_space, compute_file_checksums, ensure_sufficient_disk_space};
    use core_engine::{DownloadOptions, Downloader, InspectOptions, UrlInspector};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    // 1. Checksums & disk space check
    let sample_file = std::env::temp_dir().join(format!("dc_hash_{}.bin", std::process::id()));
    std::fs::write(&sample_file, b"hello download cepat").unwrap();
    let hashes = compute_file_checksums(&sample_file).expect("compute checksums");
    assert_eq!(hashes.size_bytes, 20);
    assert_eq!(hashes.md5.len(), 32);
    assert_eq!(hashes.sha1.len(), 40);
    assert_eq!(hashes.sha256.len(), 64);
    assert!(available_disk_space(&sample_file).unwrap_or(0) > 0);
    assert!(ensure_sufficient_disk_space(&sample_file, u64::MAX).is_err());
    let _ = std::fs::remove_file(&sample_file);

    // 2. Mock FTP server (SIZE, REST, EPSV, RETR)
    let ftp_payload: Vec<u8> = b"FTP_STREAM_PAYLOAD_0123456789_ABCDEF".to_vec();
    let ftp_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ftp");
    let ftp_port = ftp_listener.local_addr().unwrap().port();
    let ftp_data_copy = ftp_payload.clone();

    let ftp_handle = tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = ftp_listener.accept().await else {
                break;
            };
            let data = ftp_data_copy.clone();
            tokio::spawn(async move {
                let (rx, mut tx) = sock.into_split();
                let mut reader = BufReader::new(rx);
                let _ = tx.write_all(b"220 Mock FTP Ready\r\n").await;
                let mut data_listener: Option<TcpListener> = None;
                let mut rest_offset = 0usize;

                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                        break;
                    }
                    let cmd = line.trim();
                    let upper = cmd.to_ascii_uppercase();
                    if upper.starts_with("USER ") {
                        let _ = tx.write_all(b"331 Password required\r\n").await;
                    } else if upper.starts_with("PASS ") {
                        let _ = tx.write_all(b"230 Logged in\r\n").await;
                    } else if upper.starts_with("TYPE ") {
                        let _ = tx.write_all(b"200 Type set to I\r\n").await;
                    } else if upper.starts_with("SIZE ") {
                        let _ = tx.write_all(format!("213 {}\r\n", data.len()).as_bytes()).await;
                    } else if upper.starts_with("REST ") {
                        rest_offset = cmd[5..].trim().parse().unwrap_or(0);
                        let _ = tx.write_all(b"350 Restarting at offset\r\n").await;
                    } else if upper == "EPSV" {
                        let dl = TcpListener::bind("127.0.0.1:0").await.unwrap();
                        let p = dl.local_addr().unwrap().port();
                        data_listener = Some(dl);
                        let _ = tx.write_all(format!("229 Entering Extended Passive Mode (|||{}|)\r\n", p).as_bytes()).await;
                    } else if upper.starts_with("RETR ") {
                        if let Some(dl) = data_listener.take() {
                            let _ = tx.write_all(b"150 Opening binary data connection\r\n").await;
                            if let Ok((mut d_sock, _)) = dl.accept().await {
                                let _ = d_sock.write_all(&data[rest_offset.min(data.len())..]).await;
                                let _ = d_sock.shutdown().await;
                            }
                            let _ = tx.write_all(b"226 Transfer complete\r\n").await;
                        }
                    } else if upper == "QUIT" {
                        let _ = tx.write_all(b"221 Goodbye\r\n").await;
                        break;
                    }
                }
            });
        }
    });

    let ftp_url = format!("ftp://127.0.0.1:{}/pub/sample.bin", ftp_port);
    let inspector = UrlInspector::new().expect("inspector");
    let ftp_info = inspector
        .inspect_with_options(
            &ftp_url,
            InspectOptions {
                auth_username: Some("tester"),
                auth_password: Some("secret"),
                ..Default::default()
            },
        )
        .await
        .expect("inspect ftp");
    assert_eq!(ftp_info.file_name, "sample.bin");
    assert_eq!(ftp_info.file_size, Some(ftp_payload.len() as u64));
    assert!(ftp_info.supports_range);

    let ftp_dest = std::env::temp_dir().join(format!("dc_ftp_{}.bin", std::process::id()));
    let _ = std::fs::remove_file(&ftp_dest);
    Downloader::new()
        .unwrap()
        .download_with_options(
            "t-ftp",
            &ftp_url,
            &ftp_dest,
            ftp_info.file_size,
            ftp_info.supports_range,
            1,
            None,
            None,
            DownloadOptions {
                auth_username: Some("tester"),
                auth_password: Some("secret"),
                ..Default::default()
            },
        )
        .await
        .expect("ftp download should succeed");
    assert_eq!(std::fs::read(&ftp_dest).unwrap(), ftp_payload);
    let _ = std::fs::remove_file(&ftp_dest);
    ftp_handle.abort();

    // 3. Site Grabber depth 2 with Basic Auth check
    let web_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind web grab");
    let web_port = web_listener.local_addr().unwrap().port();
    let web_handle = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        loop {
            let Ok((mut sock, _)) = web_listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("/");
                let has_auth = req.to_ascii_lowercase().contains("authorization: basic ");
                if !has_auth {
                    let _ = sock.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    return;
                }
                let body = if path == "/" {
                    r#"<html><body>
                        <a href="/files/pack.zip">Zip</a>
                        <img src="/img/cover.png" />
                        <a href="/sub.html">Subpage</a>
                    </body></html>"#
                } else if path == "/sub.html" {
                    r#"<html><body><video src="/media/clip.mp4"></video></body></html>"#
                } else {
                    ""
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });

    let grabbed = grab_site_links(
        &format!("http://127.0.0.1:{}/", web_port),
        GrabOptions {
            max_depth: 2,
            same_host_only: true,
            auth_username: Some("admin"),
            auth_password: Some("pass"),
            ..Default::default()
        },
    )
    .await
    .expect("grab_site_links should succeed");
    assert!(grabbed.iter().any(|g| g.file_name == "pack.zip"));
    assert!(grabbed.iter().any(|g| g.file_name == "cover.png"));
    assert!(grabbed.iter().any(|g| g.file_name == "clip.mp4"));
    web_handle.abort();
}
