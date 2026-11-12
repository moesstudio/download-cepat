use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Connecting,
    Downloading,
    Paused,
    Assembling,
    Completed,
    Error,
    Canceled,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Connecting => "connecting",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Assembling => "assembling",
            Self::Completed => "completed",
            Self::Error => "error",
            Self::Canceled => "canceled",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "connecting" => Self::Connecting,
            "downloading" => Self::Downloading,
            "paused" => Self::Paused,
            "assembling" => Self::Assembling,
            "completed" => Self::Completed,
            "error" => Self::Error,
            "canceled" => Self::Canceled,
            _ => Self::Queued,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoFormat {
    pub format_id: String,
    pub resolution: String,      // e.g. "1080p (Full HD)", "720p (HD)", "Audio (MP3)"
    pub extension: String,       // "mp4", "mp3", "m4a", "webm"
    pub file_size_approx: Option<u64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub download_url: String,
    pub is_audio_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub duration_seconds: Option<u64>,
    pub formats: Vec<VideoFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectResult {
    pub url: String,
    pub final_url: String,
    pub file_name: String,
    pub file_size: Option<u64>,
    pub supports_range: bool,
    pub mime_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub suggested_category: String,
    pub media_info: Option<MediaInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentProgress {
    pub segment_index: usize,
    pub start_byte: u64,
    pub end_byte: u64,
    pub current_byte: u64,
    pub downloaded_bytes: u64,
    pub is_completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressTelemetry {
    pub task_id: String,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub speed_bytes_per_sec: u64,
    pub progress_percent: f32,
    pub eta_seconds: Option<u64>,
    pub status: TaskStatus,
    pub segments: Vec<SegmentProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTask {
    pub id: String,
    pub url: String,
    pub final_url: Option<String>,
    pub file_name: String,
    pub file_size: Option<u64>,
    pub downloaded_bytes: u64,
    pub status: TaskStatus,
    pub error_message: Option<String>,
    pub category_id: String,
    pub destination_path: String,
    pub mime_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub supports_range: bool,
    pub num_connections: usize,
    pub speed_limit_bytes_per_sec: u64,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub extensions: Vec<String>,
    pub save_directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChecksums {
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrabbedAsset {
    pub url: String,
    pub file_name: String,
    pub extension: String,
    pub category_id: String,
    pub resource_type: String,
    pub source_page: String,
}
