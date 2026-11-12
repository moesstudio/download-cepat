use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::model::{Category, DownloadTask, TaskStatus};

pub struct Storage {
    conn: Arc<Mutex<Connection>>,
}

impl Storage {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path).context("Gagal membuka database SQLite")?;
        let storage = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        storage.migrate()?;
        Ok(storage)
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let storage = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        storage.migrate()?;
        Ok(storage)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS categories (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL UNIQUE,
              icon TEXT NOT NULL,
              extensions TEXT NOT NULL,
              save_directory TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS queue_configs (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL,
              is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
              max_concurrent_tasks INTEGER NOT NULL DEFAULT 1 CHECK (max_concurrent_tasks BETWEEN 1 AND 10),
              schedule_start_time TEXT,
              schedule_stop_time TEXT,
              action_on_complete TEXT NOT NULL DEFAULT 'none' CHECK (action_on_complete IN ('none', 'sleep', 'shutdown', 'exit')),
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS download_tasks (
              id TEXT PRIMARY KEY,
              url TEXT NOT NULL,
              final_url TEXT,
              file_name TEXT NOT NULL,
              file_size INTEGER DEFAULT NULL,
              downloaded_bytes INTEGER NOT NULL DEFAULT 0,
              status TEXT NOT NULL DEFAULT 'queued' 
                CHECK (status IN ('queued', 'connecting', 'downloading', 'paused', 'assembling', 'completed', 'error', 'canceled')),
              error_message TEXT,
              category_id TEXT NOT NULL REFERENCES categories(id) ON DELETE RESTRICT,
              queue_id TEXT REFERENCES queue_configs(id) ON DELETE SET NULL,
              destination_path TEXT NOT NULL,
              temp_directory TEXT NOT NULL,
              mime_type TEXT,
              etag TEXT,
              last_modified TEXT,
              supports_range INTEGER NOT NULL DEFAULT 1 CHECK (supports_range IN (0, 1)),
              num_connections INTEGER NOT NULL DEFAULT 8 CHECK (num_connections BETWEEN 1 AND 32),
              speed_limit_bytes_per_sec INTEGER NOT NULL DEFAULT 0,
              position_in_queue INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
              completed_at TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_tasks_status ON download_tasks(status);
            CREATE INDEX IF NOT EXISTS idx_tasks_category ON download_tasks(category_id);
            CREATE INDEX IF NOT EXISTS idx_tasks_created ON download_tasks(created_at DESC);

            INSERT OR IGNORE INTO categories (id, name, icon, extensions, save_directory) VALUES
              ('compressed', 'Compressed', 'archive', '["zip","rar","7z","tar","gz","bz2","xz","iso"]', '~/Downloads/Compressed'),
              ('documents', 'Documents', 'file-text', '["pdf","doc","docx","xls","xlsx","ppt","pptx","txt"]', '~/Downloads/Documents'),
              ('music', 'Music', 'music', '["mp3","wav","flac","aac","ogg","m4a"]', '~/Downloads/Music'),
              ('programs', 'Programs', 'cpu', '["exe","msi","dmg","pkg","deb","rpm","appimage"]', '~/Downloads/Programs'),
              ('video', 'Video', 'video', '["mp4","mkv","avi","mov","webm","flv","ts"]', '~/Downloads/Video'),
              ('general', 'General', 'folder', '[]', '~/Downloads');
        "#)?;

        Ok(())
    }

    pub fn insert_task(&self, task: &DownloadTask) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO download_tasks (
                id, url, final_url, file_name, file_size, downloaded_bytes,
                status, error_message, category_id, destination_path, temp_directory,
                mime_type, etag, last_modified, supports_range, num_connections,
                speed_limit_bytes_per_sec, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
            "#,
            params![
                task.id,
                task.url,
                task.final_url,
                task.file_name,
                task.file_size,
                task.downloaded_bytes as i64,
                task.status.as_str(),
                task.error_message,
                task.category_id,
                task.destination_path,
                "/tmp", // temp_directory
                task.mime_type,
                task.etag,
                task.last_modified,
                if task.supports_range { 1 } else { 0 },
                task.num_connections as i64,
                task.speed_limit_bytes_per_sec as i64,
                task.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn update_task_progress(&self, task_id: &str, status: TaskStatus, downloaded_bytes: u64, error: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let completed_at = if status == TaskStatus::Completed { Some(now.clone()) } else { None };

        conn.execute(
            r#"
            UPDATE download_tasks 
            SET status = ?1, downloaded_bytes = ?2, error_message = ?3, updated_at = ?4, completed_at = COALESCE(?5, completed_at)
            WHERE id = ?6
            "#,
            params![
                status.as_str(),
                downloaded_bytes as i64,
                error,
                now,
                completed_at,
                task_id,
            ],
        )?;
        Ok(())
    }

    pub fn get_all_tasks(&self) -> Result<Vec<DownloadTask>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, url, final_url, file_name, file_size, downloaded_bytes,
                   status, error_message, category_id, destination_path,
                   mime_type, etag, last_modified, supports_range, num_connections,
                   speed_limit_bytes_per_sec, created_at, completed_at
            FROM download_tasks
            ORDER BY created_at DESC
            "#
        )?;

        let rows = stmt.query_map([], |row| {
            let status_str: String = row.get(6)?;
            let supports_range_int: i32 = row.get(13)?;
            let file_size: Option<i64> = row.get(4)?;
            let downloaded_bytes: i64 = row.get(5)?;
            let num_connections: i64 = row.get(14)?;
            let speed_limit: i64 = row.get(15)?;

            Ok(DownloadTask {
                id: row.get(0)?,
                url: row.get(1)?,
                final_url: row.get(2)?,
                file_name: row.get(3)?,
                file_size: file_size.map(|v| v as u64),
                downloaded_bytes: downloaded_bytes as u64,
                status: TaskStatus::from_str(&status_str),
                error_message: row.get(7)?,
                category_id: row.get(8)?,
                destination_path: row.get(9)?,
                mime_type: row.get(10)?,
                etag: row.get(11)?,
                last_modified: row.get(12)?,
                supports_range: supports_range_int == 1,
                num_connections: num_connections as usize,
                speed_limit_bytes_per_sec: speed_limit as u64,
                created_at: row.get(16)?,
                completed_at: row.get(17)?,
            })
        })?;

        let mut tasks = Vec::new();
        for r in rows {
            tasks.push(r?);
        }
        Ok(tasks)
    }

    pub fn get_categories(&self) -> Result<Vec<Category>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, icon, extensions, save_directory FROM categories")?;
        let rows = stmt.query_map([], |row| {
            let ext_str: String = row.get(3)?;
            let extensions: Vec<String> = serde_json::from_str(&ext_str).unwrap_or_default();
            Ok(Category {
                id: row.get(0)?,
                name: row.get(1)?,
                icon: row.get(2)?,
                extensions,
                save_directory: row.get(4)?,
            })
        })?;

        let mut categories = Vec::new();
        for r in rows {
            categories.push(r?);
        }
        Ok(categories)
    }
}
