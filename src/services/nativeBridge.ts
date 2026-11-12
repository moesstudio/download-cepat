import { invoke } from '@tauri-apps/api/core';
import {
  BuildInfo,
  FileChecksums,
  GrabbedAsset,
  InspectResult,
  PostDownloadAction,
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
      message: `Folder penyimpanan: ${path}. (Akan langsung membuka Finder/Explorer saat dijalankan di aplikasi desktop)`,
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
      message: `Membuka berkas: ${path}. (Akan langsung membuka aplikasi default di desktop)`,
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
  return { success: true, message: 'Konektor extension dikonfigurasi (mode browser preview).' };
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
  return { success: true, message: 'Folder ekstensi: /extension' };
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

export const FALLBACK_BUILD_INFO: BuildInfo = {
  appName: 'Download Cepat',
  version: '1.0.0',
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

export const setTrayLanguage = async (lang: string): Promise<void> => {
  if (isTauri()) {
    try {
      await invoke('set_tray_language', { lang });
    } catch (err) {
      console.warn('Failed to sync tray language:', err);
    }
  }
};
