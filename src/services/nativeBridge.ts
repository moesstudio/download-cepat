import { invoke } from '@tauri-apps/api/core';
import {
  AppUpdateInfo,
  BuildInfo,
  FileChecksums,
  GrabbedAsset,
  InspectResult,
  OpenDownloadPopupParams,
  PopupPrebufferState,
  PopupPromoteParams,
  PopupWindowSession,
  PostDownloadAction,
  StartPopupPreDownloadParams,
  UpdateInstallResult,
} from '../types.ts';

// Check if running inside Tauri native desktop environment
export const isTauri = (): boolean => {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
};

export interface NativeHeaders {
  cookie?: string;
  referer?: string;
  userAgent?: string;
  proxyUrl?: string;
  authUsername?: string;
  authPassword?: string;
  allowUrlRefresh?: boolean;
}

export interface InspectDetailedResult {
  result: InspectResult | null;
  error?: string;
}

export const inspectNativeUrlDetailed = async (
  url: string,
  headers?: NativeHeaders
): Promise<InspectDetailedResult> => {
  if (isTauri()) {
    try {
      const res = await invoke<InspectResult>('inspect_url', {
        url,
        cookie: headers?.cookie || null,
        referer: headers?.referer || null,
        userAgent: headers?.userAgent || null,
        user_agent: headers?.userAgent || null,
        proxyUrl: headers?.proxyUrl || null,
        proxy_url: headers?.proxyUrl || null,
        authUsername: headers?.authUsername || null,
        auth_username: headers?.authUsername || null,
        authPassword: headers?.authPassword || null,
        auth_password: headers?.authPassword || null,
      });
      return { result: res };
    } catch (err) {
      console.warn('Native URL inspection error:', err);
      return { result: null, error: String(err) };
    }
  }
  return { result: null };
};

export const inspectNativeUrl = async (
  url: string,
  headers?: NativeHeaders
): Promise<InspectResult | null> => {
  const { result } = await inspectNativeUrlDetailed(url, headers);
  return result;
};

export const openNativeFolder = async (path: string): Promise<{ success: boolean; message: string }> => {
  if (isTauri()) {
    try {
      const res = await invoke<string>('open_folder', { path });
      return { success: true, message: res };
    } catch (err) {
      console.error('Failed to open native folder:', err);
      return { success: false, message: String(err) };
    }
  } else {
    // Web browser fallback
    return {
      success: true,
      message: `Save folder: ${path}. (Opens Finder/Explorer directly in the desktop app)`,
    };
  }
};

export const openNativeFile = async (path: string): Promise<{ success: boolean; message: string }> => {
  if (isTauri()) {
    try {
      const res = await invoke<string>('open_file', { path });
      return { success: true, message: res };
    } catch (err) {
      console.error('Failed to open native file:', err);
      return { success: false, message: String(err) };
    }
  } else {
    // Web browser fallback
    return {
      success: true,
      message: `Opening file: ${path}. (Opens the default app directly on desktop)`,
    };
  }
};

export const triggerRealDownload = async (
  taskId: string,
  url: string,
  destinationPath: string,
  numConnections: number = 8,
  formatId?: string,
  headers?: NativeHeaders,
  speedLimitKbs?: number,
  resume: boolean = false
): Promise<void> => {
  if (isTauri()) {
    try {
      await invoke('start_real_download', {
        taskId,
        task_id: taskId,
        url,
        destinationPath,
        destination_path: destinationPath,
        numConnections,
        num_connections: numConnections,
        formatId: formatId || null,
        format_id: formatId || null,
        cookie: headers?.cookie || null,
        referer: headers?.referer || null,
        userAgent: headers?.userAgent || null,
        user_agent: headers?.userAgent || null,
        speedLimitKbs: speedLimitKbs && speedLimitKbs > 0 ? speedLimitKbs : null,
        speed_limit_kbs: speedLimitKbs && speedLimitKbs > 0 ? speedLimitKbs : null,
        resume,
        proxyUrl: headers?.proxyUrl || null,
        proxy_url: headers?.proxyUrl || null,
        authUsername: headers?.authUsername || null,
        auth_username: headers?.authUsername || null,
        authPassword: headers?.authPassword || null,
        auth_password: headers?.authPassword || null,
        allowUrlRefresh: Boolean(headers?.allowUrlRefresh),
        allow_url_refresh: Boolean(headers?.allowUrlRefresh),
      });
    } catch (err) {
      console.error('Failed to trigger real download:', err);
    }
  }
};

export const pauseRealDownload = async (taskId: string): Promise<boolean> => {
  if (isTauri()) {
    try {
      return await invoke<boolean>('pause_real_download', {
        taskId,
        task_id: taskId,
      });
    } catch (err) {
      console.warn('Failed to pause real download:', err);
    }
  }
  return false;
};

export const cancelRealDownload = async (
  taskId: string,
  destinationPath?: string
): Promise<boolean> => {
  if (isTauri()) {
    try {
      return await invoke<boolean>('cancel_real_download', {
        taskId,
        task_id: taskId,
        destinationPath: destinationPath ?? null,
        destination_path: destinationPath ?? null,
      });
    } catch (err) {
      console.warn('Failed to cancel real download:', err);
    }
  }
  return false;
};

export const stopAllRealDownloads = async (): Promise<number> => {
  if (isTauri()) {
    try {
      return await invoke<number>('stop_all_real_downloads');
    } catch (err) {
      console.warn('Failed to stop all real downloads:', err);
    }
  }
  return 0;
};

export const ensureFileCreated = async (destinationPath: string, fileName: string): Promise<void> => {
  if (isTauri()) {
    try {
      await invoke('ensure_file_created', {
        destinationPath,
        destination_path: destinationPath,
        fileName,
        file_name: fileName,
      });
    } catch (err) {
      console.error('Failed to ensure file created:', err);
    }
  }
};

export const getAutostartStatus = async (): Promise<boolean> => {
  if (isTauri()) {
    try {
      return await invoke<boolean>('get_autostart_status');
    } catch (err) {
      console.error('Failed to get autostart status:', err);
      return false;
    }
  }
  return false;
};

export const setAutostart = async (enable: boolean): Promise<boolean> => {
  if (isTauri()) {
    try {
      return await invoke<boolean>('set_autostart', { enable });
    } catch (err) {
      console.error('Failed to set autostart:', err);
      return false;
    }
  }
  return enable;
};

export const installBrowserExtensionHost = async (): Promise<{ success: boolean; message: string }> => {
  if (isTauri()) {
    try {
      const msg = await invoke<string>('install_browser_extension_host');
      return { success: true, message: msg };
    } catch (err) {
      return { success: false, message: String(err) };
    }
  }
  return { success: true, message: 'Extension connector configured (browser preview mode).' };
};

export const OFFICIAL_CHROME_WEBSTORE_URL =
  'https://chromewebstore.google.com/detail/lmahadjehdfibjjgcnbajbgapbkghida?utm_source=item-share-cb';
export const OFFICIAL_CHROME_EXTENSION_ID = 'lmahadjehdfibjjgcnbajbgapbkghida';

export const openExternalUrl = async (url: string): Promise<{ success: boolean; message: string }> => {
  if (isTauri()) {
    try {
      const msg = await invoke<string>('open_external_url', { url });
      return { success: true, message: msg };
    } catch {
      window.open(url, '_blank');
      return { success: true, message: 'URL opened in browser' };
    }
  }
  window.open(url, '_blank');
  return { success: true, message: 'URL opened in browser' };
};

export const openExtensionFolder = async (): Promise<{ success: boolean; message: string }> => {
  if (isTauri()) {
    try {
      const msg = await invoke<string>('open_extension_folder');
      return { success: true, message: msg };
    } catch (err) {
      return { success: false, message: String(err) };
    }
  }
  return { success: true, message: 'Extension folder: /extension' };
};

export const updateDownloadSpeedLimit = async (
  limitKbs: number,
  taskId?: string
): Promise<number> => {
  if (isTauri()) {
    try {
      return await invoke<number>('update_download_speed_limit', {
        taskId: taskId || null,
        task_id: taskId || null,
        limitKbs,
        limit_kbs: limitKbs,
      });
    } catch (err) {
      console.warn('Failed to update live speed limit:', err);
    }
  }
  return 0;
};

export const verifyFileChecksum = async (path: string): Promise<FileChecksums | null> => {
  if (isTauri()) {
    try {
      return await invoke<FileChecksums>('verify_file_checksum', { path });
    } catch (err) {
      throw new Error(String(err));
    }
  }
  return {
    md5: 'd41d8cd98f00b204e9800998ecf8427e',
    sha1: 'da39a3ee5e6b4b0d3255bfef95601890afd80709',
    sha256: 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
    sizeBytes: 0,
  };
};

export const checkDiskSpace = async (dir: string): Promise<number | null> => {
  if (isTauri()) {
    try {
      return await invoke<number>('check_disk_space', { dir });
    } catch {
      return null;
    }
  }
  return 250 * 1024 * 1024 * 1024; // 250 GB fallback in web/test preview
};

export interface GrabSiteOptions {
  url: string;
  maxDepth?: number;
  sameHostOnly?: boolean;
  headers?: NativeHeaders;
}

export const grabSiteLinks = async (opts: GrabSiteOptions): Promise<GrabbedAsset[]> => {
  if (isTauri()) {
    try {
      return await invoke<GrabbedAsset[]>('grab_site_links', {
        url: opts.url,
        maxDepth: opts.maxDepth ?? 1,
        max_depth: opts.maxDepth ?? 1,
        sameHostOnly: opts.sameHostOnly ?? true,
        same_host_only: opts.sameHostOnly ?? true,
        cookie: opts.headers?.cookie || null,
        referer: opts.headers?.referer || null,
        userAgent: opts.headers?.userAgent || null,
        user_agent: opts.headers?.userAgent || null,
        proxyUrl: opts.headers?.proxyUrl || null,
        proxy_url: opts.headers?.proxyUrl || null,
        authUsername: opts.headers?.authUsername || null,
        auth_username: opts.headers?.authUsername || null,
        authPassword: opts.headers?.authPassword || null,
        auth_password: opts.headers?.authPassword || null,
      });
    } catch (err) {
      throw new Error(String(err));
    }
  }
  // Safe preview mock only when outside desktop binary
  const cleanBase = opts.url.replace(/\/$/, '');
  return [
    {
      url: `${cleanBase}/assets/guide.pdf`,
      fileName: 'guide.pdf',
      extension: 'pdf',
      categoryId: 'documents',
      resourceType: 'file',
      sourcePage: opts.url,
    },
    {
      url: `${cleanBase}/media/demo.mp4`,
      fileName: 'demo.mp4',
      extension: 'mp4',
      categoryId: 'video',
      resourceType: 'media',
      sourcePage: opts.url,
    },
  ];
};

export const renameOrMoveDownload = async (
  oldPath: string,
  newPath: string
): Promise<string> => {
  if (isTauri()) {
    return await invoke<string>('rename_or_move_download', {
      oldPath,
      old_path: oldPath,
      newPath,
      new_path: newPath,
    });
  }
  return newPath;
};

export const readClipboardText = async (): Promise<string> => {
  if (isTauri()) {
    try {
      return await invoke<string>('read_clipboard_text');
    } catch {
      return '';
    }
  }
  if (typeof navigator !== 'undefined' && navigator.clipboard?.readText) {
    try {
      return await navigator.clipboard.readText();
    } catch {
      return '';
    }
  }
  return '';
};

export const readTextFilePreview = async (path: string): Promise<string> => {
  if (isTauri()) {
    return await invoke<string>('read_text_file_preview', { path });
  }
  return '';
};

export const runPostDownloadAction = async (action: PostDownloadAction): Promise<string> => {
  if (isTauri()) {
    return await invoke<string>('run_post_download_action', { action });
  }
  return action;
};

export const OFFICIAL_RELEASES_URL = 'https://github.com/moesstudio/download-cepat/releases';

export const FALLBACK_BUILD_INFO: BuildInfo = {
  appName: 'Download Cepat',
  version: '1.0.4',
  author: 'Ujang Mustamar',
  publisher: 'Ujang Mustamar',
  copyright: 'Copyright © 2026 Ujang Mustamar',
  license: 'MIT',
  repository: 'https://github.com/moesstudio/download-cepat',
  engine: 'Download Cepat Core Engine (Rust Multi-Chunk HTTP/FTP)',
  os: typeof navigator !== 'undefined' ? navigator.platform || 'desktop' : 'desktop',
  arch: 'universal',
  buildProfile: 'release',
  tauriVersion: '2.12.x',
};

export interface RuntimeDownloadSettingsPayload {
  defaultConnections: number;
  maxTasks: number;
  speedLimitKbs: number;
  proxyUrl?: string | null;
  videoSaveDir: string;
  musicSaveDir: string;
  torrentsSaveDir: string;
}

export const syncRuntimeSettings = async (
  settings: RuntimeDownloadSettingsPayload
): Promise<void> => {
  if (isTauri()) {
    try {
      await invoke('sync_runtime_settings', { settings });
    } catch (err) {
      console.warn('Failed to sync runtime download settings:', err);
    }
  }
};

export const inspectTorrentBytes = async (bytes: number[]): Promise<InspectResult | null> => {
  if (bytes.length === 0) return null;
  if (isTauri()) {
    return await invoke<InspectResult>('inspect_torrent_bytes', { bytes });
  }
  const pseudoHash = Array.from(bytes.slice(0, 20))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')
    .padEnd(40, '0')
    .slice(0, 40);
  return {
    url: `dc-torrent://${pseudoHash}`,
    finalUrl: `dc-torrent://${pseudoHash}`,
    fileName: `torrent_${pseudoHash.slice(0, 8)}`,
    fileSize: null,
    supportsRange: true,
    mimeType: 'application/x-bittorrent',
    suggestedCategory: 'torrents',
    torrentInfo: {
      infoHash: pseudoHash,
      name: `torrent_${pseudoHash.slice(0, 8)}`,
      totalSize: 0,
      pieceLength: 262144,
      pieceCount: 0,
      trackers: [],
      files: [],
      isMultiFile: false,
      isMagnet: false,
    },
  };
};

export const openDownloadPopup = async (params: OpenDownloadPopupParams): Promise<string> => {
  if (isTauri()) {
    return await invoke<string>('open_download_popup', { params });
  }
  return `dl-popup-preview-${Date.now()}`;
};

export const getPopupSession = async (popupId: string): Promise<PopupWindowSession | null> => {
  if (isTauri()) {
    try {
      return await invoke<PopupWindowSession | null>('get_popup_session', {
        popupId,
        popup_id: popupId,
      });
    } catch (err) {
      console.warn('Failed to load popup session:', err);
      return null;
    }
  }
  return null;
};

export const startPopupPreDownload = async (
  params: StartPopupPreDownloadParams
): Promise<PopupPrebufferState | null> => {
  if (isTauri()) {
    try {
      return await invoke<PopupPrebufferState | null>('start_popup_pre_download', { params });
    } catch (err) {
      console.warn('Failed starting popup pre-download:', err);
      return null;
    }
  }
  return null;
};

export const promotePopupDownload = async (
  params: PopupPromoteParams
): Promise<PopupWindowSession> => {
  if (isTauri()) {
    return await invoke<PopupWindowSession>('promote_popup_download', { params });
  }
  return {
    popupId: params.popupId,
    mode: 'mini_progress',
    source: 'manual',
    initialMode: 'url',
    url: params.url,
    fileName: params.fileName,
    destinationPath: params.destinationPath,
    categoryId: params.categoryId,
    numConnections: params.numConnections,
    formatId: params.formatId ?? null,
    resolution: params.resolution ?? null,
    cookie: params.cookie ?? null,
    referer: params.referer ?? null,
    userAgent: params.userAgent ?? null,
    fileSize: params.fileSize ?? null,
    mimeType: params.mimeType ?? null,
    proxyUrl: params.proxyUrl ?? null,
    authUsername: params.authUsername ?? null,
    authPassword: params.authPassword ?? null,
    speedLimitBytesPerSec: params.speedLimitKbs ? params.speedLimitKbs * 1024 : null,
    taskId: `task-preview-${Date.now()}`,
    prebuffer: {
      status: 'downloading',
      runId: 1,
      url: params.url,
      fileName: params.fileName,
      downloadedBytes: 0,
      totalBytes: params.fileSize ?? null,
      speedBytesPerSec: 0,
      progressPercent: 0,
      supportsRange: true,
    },
  };
};

export const queuePopupLater = async (params: PopupPromoteParams): Promise<void> => {
  if (isTauri()) {
    await invoke('queue_popup_later', { params });
  }
};

export const cancelPopupDownload = async (popupId: string): Promise<boolean> => {
  if (isTauri()) {
    try {
      return await invoke<boolean>('cancel_popup_download', {
        popupId,
        popup_id: popupId,
      });
    } catch (err) {
      console.warn('Failed canceling popup download:', err);
      return false;
    }
  }
  return true;
};

export const openMainWindow = async (): Promise<void> => {
  if (isTauri()) {
    try {
      await invoke('open_main_window');
    } catch (err) {
      console.warn('Failed opening main window:', err);
    }
  }
};

export const minimizePopupWindow = async (popupId: string): Promise<void> => {
  if (isTauri()) {
    try {
      await invoke('minimize_popup_window', {
        popupId,
        popup_id: popupId,
      });
    } catch (err) {
      console.warn('Failed minimizing popup window:', err);
    }
  }
};

export const closePopupWindow = async (popupId: string): Promise<void> => {
  if (isTauri()) {
    try {
      await invoke('close_popup_window', {
        popupId,
        popup_id: popupId,
      });
    } catch (err) {
      console.warn('Failed closing popup window:', err);
    }
  }
};

export const getBuildInfo = async (): Promise<BuildInfo> => {
  if (isTauri()) {
    try {
      return await invoke<BuildInfo>('get_build_info');
    } catch (err) {
      console.warn('Failed to fetch native build info:', err);
    }
  }
  return FALLBACK_BUILD_INFO;
};

export const checkForAppUpdate = async (): Promise<AppUpdateInfo> => {
  if (isTauri()) {
    return await invoke<AppUpdateInfo>('check_for_app_update');
  }
  return {
    currentVersion: FALLBACK_BUILD_INFO.version,
    latestVersion: FALLBACK_BUILD_INFO.version,
    updateAvailable: false,
    releaseUrl: OFFICIAL_RELEASES_URL,
    releaseNotes: '',
    publishedAt: null,
    assetName: null,
    assetUrl: null,
    assetSize: null,
    assetSha256: null,
    installKind: 'manual',
    inPlaceSupported: false,
  };
};

export const downloadAndInstallAppUpdate = async (): Promise<UpdateInstallResult> => {
  if (isTauri()) {
    return await invoke<UpdateInstallResult>('download_and_install_app_update');
  }
  return {
    startedInPlace: false,
    message: 'Auto-update is available in the Download Cepat desktop application.',
    assetPath: null,
  };
};

export const setTrayLanguage = async (lang: string): Promise<void> => {
  if (isTauri()) {
    try {
      await invoke('set_tray_language', { lang });
    } catch (err) {
      console.warn('Failed to sync tray language:', err);
    }
  }
};
