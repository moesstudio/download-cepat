use anyhow::Result;
use std::env;
use std::path::PathBuf;
use tokio::sync::broadcast;

use core_engine::{Downloader, ProgressTelemetry, UrlInspector};

#[tokio::main]
async fn main() -> Result<()> {
    println!("============================================================");
    println!("  DOWNLOAD CEPAT - High Performance Segmented Engine CLI    ");
    println!("============================================================");

    let args: Vec<String> = env::args().collect();
    let url = if args.len() > 1 {
        &args[1]
    } else {
        // Safe test target: 10MB test file from speed.cloudflare.com
        "https://speed.cloudflare.com/__down?bytes=10485760"
    };

    println!("\n[*] Menginspeksi URL: {}", url);
    let inspector = UrlInspector::new()?;
    let info = inspector.inspect(url).await?;

    println!("[+] Nama File        : {}", info.file_name);
    println!("[+] Ukuran File      : {} bytes ({:.2} MB)", 
        info.file_size.unwrap_or(0),
        info.file_size.unwrap_or(0) as f64 / (1024.0 * 1024.0)
    );
    println!("[+] Dukungan Range   : {}", if info.supports_range { "Ya (Multi-Connection Aktif)" } else { "Tidak (Single Connection)" });
    println!("[+] Tipe MIME        : {}", info.mime_type.as_deref().unwrap_or("unknown"));
    println!("[+] Kategori Otomatis: {}", info.suggested_category);

    let dest_dir = PathBuf::from("downloads_test");
    std::fs::create_dir_all(&dest_dir)?;
    let dest_path = dest_dir.join(&info.file_name);

    println!("\n[*] Menyimpan ke     : {:?}", dest_path);

    let num_connections = if info.supports_range { 8 } else { 1 };
    println!("[*] Membuka {} koneksi simultan HTTP Range...", num_connections);

    let (tx, mut rx) = broadcast::channel::<ProgressTelemetry>(128);
    let downloader = Downloader::new()?;

    // Task monitor loop
    let monitor = tokio::spawn(async move {
        while let Ok(telemetry) = rx.recv().await {
            let speed_mb = telemetry.speed_bytes_per_sec as f64 / (1024.0 * 1024.0);
            let downloaded_mb = telemetry.downloaded_bytes as f64 / (1024.0 * 1024.0);
            let total_mb = telemetry.total_bytes.unwrap_or(0) as f64 / (1024.0 * 1024.0);

            let bar_len: usize = 30;
            let filled = ((telemetry.progress_percent / 100.0) * bar_len as f32) as usize;
            let bar: String = "█".repeat(filled) + &"░".repeat(bar_len.saturating_sub(filled));

            print!("\r[Progres] [{}] {:>5.1}% | {:.2}/{:.2} MB | {:>6.2} MB/s | ETA: {:>3}s",
                bar, telemetry.progress_percent, downloaded_mb, total_mb, speed_mb, telemetry.eta_seconds.unwrap_or(0)
            );
            use std::io::Write;
            let _ = std::io::stdout().flush();

            if telemetry.progress_percent >= 100.0 {
                break;
            }
        }
    });

    let task_id = uuid::Uuid::new_v4().to_string();
    downloader.download(
        &task_id,
        &info.final_url,
        &dest_path,
        info.file_size,
        info.supports_range,
        num_connections,
        Some(tx),
        None,
    ).await?;

    let _ = monitor.await;

    println!("\n\n[✓] Unduhan selesai 100%!");
    let metadata = std::fs::metadata(&dest_path)?;
    println!("[✓] Ukuran file di disk: {} bytes ({:.2} MB)", metadata.len(), metadata.len() as f64 / (1024.0 * 1024.0));
    println!("============================================================");

    Ok(())
}
