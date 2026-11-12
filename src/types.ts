export type TaskStatus = 
  | 'queued' 
  | 'connecting' 
  | 'downloading' 
  | 'paused' 
  | 'assembling' 
  | 'completed' 
  | 'error' 
  | 'canceled';

export interface DownloadSegment {
  segmentIndex: number;
  startByte: number;
  endByte: number;
  currentByte: number;
  downloadedBytes: number;
  isCompleted: boolean;
}

export interface FileChecksums {
  md5: string;
  sha1: string;
  sha256: string;
  sizeBytes: number;
}

export interface GrabbedAsset {
  url: string;
  fileName: string;
  extension: string;
  categoryId: string;
  resourceType: 'image' | 'media' | 'file' | 'page';
  sourcePage: string;
}

export interface ProxyConfig {
  enabled: boolean;
  protocol: 'http' | 'https' | 'socks4' | 'socks5';
  host: string;
  port: string;
  username?: string;
  password?: string;
}

export interface SiteLogin {
  id: string;
  domain: string;
  username: string;
  password: string;
  notes?: string;
}

export interface CategoryRule {
  id: string;
  name: string;
  extensions: string[];
  saveDir: string;
}

export type PostDownloadAction = 'none' | 'notify_open' | 'sleep' | 'quit';

export interface SchedulerConfig {
  enabled: boolean;
  startTime: string; // HH:MM
  stopTime: string;  // HH:MM
  days: number[];    // 0=Sun .. 6=Sat
  postAction: PostDownloadAction;
}

export interface ClipboardMonitorConfig {
  enabled: boolean;
  extensions: string[];
}

export interface TaskHttpLog {
  timestamp: string;
  event: string;
  detail: string;
}

export interface DownloadTask {
  id: string;
  url: string;
  fileName: string;
  fileSize: number | null;
  downloadedBytes: number;
  status: TaskStatus;
  errorMessage?: string;
  categoryId: string;
  destinationPath: string;
  mimeType?: string;
  supportsRange: boolean;
  numConnections: number;
  speedLimitBytesPerSec: number;
  speedBytesPerSec: number;
  etaSeconds?: number;
  progressPercent: number;
  createdAt: string;
  completedAt?: string;
  formatId?: string;
  resolution?: string;
  cookie?: string;
  referer?: string;
  userAgent?: string;
  proxyUrl?: string;
  authUsername?: string;
  authPassword?: string;
  priority?: number;
  lastUrlRefreshedAt?: string;
  checksums?: FileChecksums;
  httpLogs?: TaskHttpLog[];
  segments?: DownloadSegment[];
}

export interface IncomingDownloadMeta {
  url: string;
  fileName?: string;
  cookie?: string;
  referer?: string;
  userAgent?: string;
  fileSize?: number | null;
  mimeType?: string | null;
}

export interface Category {
  id: string;
  name: string;
  icon: string;
  extensions: string[];
  saveDir?: string;
  count: number;
}

export interface VideoFormat {
  formatId: string;
  resolution: string;      // e.g. "1080p (Full HD 60fps)", "720p (HD)", "Audio (MP3)"
  extension: string;       // "mp4", "mp3", "m4a", "webm"
  fileSizeApprox?: number;
  videoCodec?: string;
  audioCodec?: string;
  downloadUrl: string;
  isAudioOnly: boolean;
}

export interface MediaInfo {
  title: string;
  thumbnailUrl?: string;
  durationSeconds?: number;
  formats: VideoFormat[];
}

export interface InspectResult {
  url: string;
  finalUrl: string;
  fileName: string;
  fileSize: number | null;
  supportsRange: boolean;
  mimeType?: string;
  suggestedCategory: string;
  mediaInfo?: MediaInfo;
}

export interface BuildInfo {
  appName: string;
  version: string;
  author: string;
  publisher: string;
  copyright: string;
  license: string;
  repository: string;
  engine: string;
  os: string;
  arch: string;
  buildProfile: string;
  tauriVersion: string;
}
