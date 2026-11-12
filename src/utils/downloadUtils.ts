import {
  CategoryRule,
  ClipboardMonitorConfig,
  ProxyConfig,
  SchedulerConfig,
  SiteLogin,
} from '../types.ts';

export const DEFAULT_CATEGORY_RULES: CategoryRule[] = [
  {
    id: 'compressed',
    name: 'Compressed',
    extensions: ['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'iso'],
    saveDir: '~/Downloads/Compressed',
  },
  {
    id: 'documents',
    name: 'Documents',
    extensions: ['pdf', 'doc', 'docx', 'ppt', 'pptx', 'xls', 'xlsx', 'txt', 'csv', 'epub'],
    saveDir: '~/Downloads/Documents',
  },
  {
    id: 'music',
    name: 'Music',
    extensions: ['mp3', 'wav', 'flac', 'aac', 'm4a', 'ogg', 'opus'],
    saveDir: '~/Downloads/Music',
  },
  {
    id: 'programs',
    name: 'Programs',
    extensions: ['exe', 'msi', 'dmg', 'pkg', 'deb', 'rpm', 'appimage', 'apk'],
    saveDir: '~/Downloads/Programs',
  },
  {
    id: 'video',
    name: 'Video',
    extensions: ['mp4', 'mkv', 'webm', 'avi', 'mov', 'flv', 'wmv', 'm4v', 'm3u8', 'ts'],
    saveDir: '~/Downloads/Video',
  },
];

export const DEFAULT_CLIPBOARD_EXTENSIONS = [
  'zip', 'rar', '7z', 'tar', 'gz', 'iso',
  'exe', 'dmg', 'pkg', 'msi', 'apk',
  'mp4', 'mkv', 'webm', 'mp3', 'm4a', 'flac',
  'pdf', 'docx', 'xlsx',
];

export const DEFAULT_PROXY_CONFIG: ProxyConfig = {
  enabled: false,
  protocol: 'http',
  host: '',
  port: '',
  username: '',
  password: '',
};

export const DEFAULT_SCHEDULER_CONFIG: SchedulerConfig = {
  enabled: false,
  startTime: '00:00',
  stopTime: '06:00',
  days: [0, 1, 2, 3, 4, 5, 6],
  postAction: 'none',
};

export const DEFAULT_CLIPBOARD_CONFIG: ClipboardMonitorConfig = {
  enabled: false,
  extensions: DEFAULT_CLIPBOARD_EXTENSIONS,
};

/**
 * Expands numeric `[01-10]` or alphabetic `[a-z]` batch wildcard patterns.
 * Preserves zero-padding width (e.g., `[001-012]`) and caps expansion at 500 items.
 */
export const expandBatchPattern = (pattern: string): string[] => {
  const trimmed = pattern.trim();
  if (!trimmed) return [];

  const numMatch = trimmed.match(/\[(\d+)-(\d+)\]/);
  if (numMatch) {
    const [full, startStr, endStr] = numMatch;
    const start = parseInt(startStr, 10);
    const end = parseInt(endStr, 10);
    const width = startStr.length;
    const step = start <= end ? 1 : -1;
    const count = Math.min(500, Math.abs(end - start) + 1);
    const results: string[] = [];
    for (let i = 0; i < count; i++) {
      const val = String(start + i * step).padStart(width, '0');
      results.push(trimmed.replace(full, val));
    }
    return results;
  }

  const alphaMatch = trimmed.match(/\[([a-zA-Z])-([a-zA-Z])\]/);
  if (alphaMatch) {
    const [full, startChar, endChar] = alphaMatch;
    const startCode = startChar.charCodeAt(0);
    const endCode = endChar.charCodeAt(0);
    const step = startCode <= endCode ? 1 : -1;
    const count = Math.min(52, Math.abs(endCode - startCode) + 1);
    const results: string[] = [];
    for (let i = 0; i < count; i++) {
      results.push(trimmed.replace(full, String.fromCharCode(startCode + i * step)));
    }
    return results;
  }

  return isSupportedDownloadUrl(trimmed) ? [trimmed] : [];
};

export const isSupportedDownloadUrl = (rawUrl: string): boolean => {
  const s = rawUrl.trim().toLowerCase();
  return s.startsWith('http://') || s.startsWith('https://') || s.startsWith('ftp://');
};

/**
 * Parses raw lines, `.ef2` URL list exports (`<URL>` / plain lines), or clipboard/dropped text.
 */
export const parseUrlListText = (rawText: string): string[] => {
  const lines = rawText.split(/[\r\n]+/);
  const urls: string[] = [];
  const seen = new Set<string>();

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#') || trimmed.startsWith(';')) continue;
    // Extract all http(s)/ftp tokens or wildcard patterns from line
    const tokenMatches = trimmed.match(/(?:https?|ftp):\/\/[^\s"'<>]+/gi);
    if (tokenMatches) {
      for (const token of tokenMatches) {
        for (const expanded of expandBatchPattern(token)) {
          if (!seen.has(expanded)) {
            seen.add(expanded);
            urls.push(expanded);
          }
        }
      }
    } else if (/^<\s*(?:https?|ftp):\/\//i.test(trimmed)) {
      const cleaned = trimmed.replace(/^<\s*/, '').replace(/\s*>$/, '');
      if (isSupportedDownloadUrl(cleaned) && !seen.has(cleaned)) {
        seen.add(cleaned);
        urls.push(cleaned);
      }
    }
  }
  return urls;
};

export const deriveFileNameFromUrl = (rawUrl: string): string => {
  try {
    const parsed = new URL(rawUrl);
    const parts = parsed.pathname.split('/').filter(Boolean);
    if (parts.length > 0) {
      return decodeURIComponent(parts[parts.length - 1]);
    }
  } catch {
    // ignore malformed
  }
  return 'download.bin';
};

export const formatProxyUrl = (proxy?: ProxyConfig): string | undefined => {
  if (!proxy || !proxy.enabled) return undefined;
  const host = proxy.host.trim();
  const port = proxy.port.trim();
  if (!host || !port) return undefined;
  const auth =
    proxy.username && proxy.username.trim()
      ? `${encodeURIComponent(proxy.username.trim())}:${encodeURIComponent(proxy.password || '')}@`
      : '';
  return `${proxy.protocol}://${auth}${host}:${port}`;
};

export const findMatchingSiteLogin = (
  rawUrl: string,
  logins: SiteLogin[]
): SiteLogin | undefined => {
  try {
    const hostname = new URL(rawUrl).hostname.toLowerCase();
    return logins.find((entry) => {
      const domain = entry.domain.trim().toLowerCase().replace(/^www\./, '');
      if (!domain) return false;
      return hostname === domain || hostname.endsWith(`.${domain}`);
    });
  } catch {
    return undefined;
  }
};

export const matchCategoryForFile = (
  fileName: string,
  rules: CategoryRule[]
): { categoryId: string; saveDir: string } => {
  const dotIdx = fileName.lastIndexOf('.');
  const ext = dotIdx >= 0 ? fileName.slice(dotIdx + 1).toLowerCase() : '';
  if (ext) {
    const found = rules.find((rule) =>
      rule.extensions.some((e) => e.trim().toLowerCase().replace(/^\./, '') === ext)
    );
    if (found) {
      return {
        categoryId: found.id,
        saveDir: found.saveDir || `~/Downloads/${found.name}`,
      };
    }
  }
  return { categoryId: 'general', saveDir: '~/Downloads' };
};

export const shouldInterceptClipboardUrl = (
  text: string,
  config: ClipboardMonitorConfig
): boolean => {
  if (!config.enabled) return false;
  const trimmed = text.trim();
  if (!isSupportedDownloadUrl(trimmed) || /\s/.test(trimmed)) return false;
  try {
    const parsed = new URL(trimmed);
    const lastSeg = parsed.pathname.split('/').pop() || '';
    const ext = lastSeg.includes('.') ? lastSeg.split('.').pop()?.toLowerCase() || '' : '';
    if (!ext) return false;
    return config.extensions
      .map((e) => e.trim().toLowerCase().replace(/^\./, ''))
      .includes(ext);
  } catch {
    return false;
  }
};

const parseHhMmMinutes = (hhmm: string): number => {
  const [hStr, mStr] = hhmm.split(':');
  const h = Math.max(0, Math.min(23, Number(hStr) || 0));
  const m = Math.max(0, Math.min(59, Number(mStr) || 0));
  return h * 60 + m;
};

export const isWithinScheduleWindow = (
  config: SchedulerConfig,
  now: Date = new Date()
): boolean => {
  if (!config.enabled) return true;
  if (!config.days.includes(now.getDay())) return false;
  const currentMins = now.getHours() * 60 + now.getMinutes();
  const startMins = parseHhMmMinutes(config.startTime);
  const stopMins = parseHhMmMinutes(config.stopTime);

  if (startMins === stopMins) return true;
  if (startMins < stopMins) {
    return currentMins >= startMins && currentMins < stopMins;
  }
  // Overnight window (e.g., 23:00 -> 06:00)
  return currentMins >= startMins || currentMins < stopMins;
};

export const formatDiskBytes = (
  bytes: number | null | undefined,
  unknownLabel = 'Tidak diketahui'
): string => {
  if (bytes === null || bytes === undefined) return unknownLabel;
  if (bytes <= 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(sizes.length - 1, Math.floor(Math.log(bytes) / Math.log(k)));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(2))} ${sizes[i]}`;
};

export const getParentDir = (fullPath: string): string => {
  const clean = fullPath.replace(/\\/g, '/');
  const idx = clean.lastIndexOf('/');
  return idx > 0 ? clean.slice(0, idx) : '~/Downloads';
};
