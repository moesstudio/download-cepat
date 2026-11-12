import React, { useState, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import {
  BuildInfo,
  Category,
  CategoryRule,
  ClipboardMonitorConfig,
  DownloadSegment,
  DownloadTask,
  FileChecksums,
  IncomingDownloadMeta,
  PostDownloadAction,
  ProxyConfig,
  SchedulerConfig,
  SiteLogin,
  TaskStatus,
} from './types.ts';
import { CategorySidebar } from './components/CategorySidebar.tsx';
import { Toolbar } from './components/Toolbar.tsx';
import { TaskDataTable } from './components/TaskDataTable.tsx';
import { AddUrlModal } from './components/AddUrlModal.tsx';
import { DownloadProgressModal } from './components/DownloadProgressModal.tsx';
import { DownloadCompleteModal } from './components/DownloadCompleteModal.tsx';
import { SpeedLimiterModal, SettingsTab } from './components/SpeedLimiterModal.tsx';
import { TaskContextMenu } from './components/TaskContextMenu.tsx';
import { TaskPropertiesModal } from './components/TaskPropertiesModal.tsx';
import { BatchAndGrabberModal, BatchQueueItem } from './components/BatchAndGrabberModal.tsx';
import { SchedulerModal } from './components/SchedulerModal.tsx';
import { MiniDropTarget } from './components/MiniDropTarget.tsx';
import {
  openNativeFolder,
  openNativeFile,
  triggerRealDownload,
  pauseRealDownload,
  cancelRealDownload,
  stopAllRealDownloads,
  ensureFileCreated,
  updateDownloadSpeedLimit,
  readClipboardText,
  readTextFilePreview,
  runPostDownloadAction,
  getBuildInfo,
  setTrayLanguage,
  isTauri,
} from './services/nativeBridge.ts';
import {
  DEFAULT_CATEGORY_RULES,
  DEFAULT_CLIPBOARD_CONFIG,
  DEFAULT_PROXY_CONFIG,
  DEFAULT_SCHEDULER_CONFIG,
  deriveFileNameFromUrl,
  findMatchingSiteLogin,
  formatProxyUrl,
  isWithinScheduleWindow,
  matchCategoryForFile,
  parseUrlListText,
  shouldInterceptClipboardUrl,
} from './utils/downloadUtils.ts';
import {
  I18nProvider,
  LANGUAGE_OPTIONS,
  LanguageCode,
  isValidLanguage,
  translate,
} from './i18n/index.tsx';

const INITIAL_CATEGORIES: Category[] = [
  { id: 'compressed', name: 'Compressed', icon: 'archive', extensions: ['.zip', '.rar', '.7z', '.iso'], count: 1 },
  { id: 'documents', name: 'Documents', icon: 'file-text', extensions: ['.pdf', '.docx', '.xlsx'], count: 1 },
  { id: 'music', name: 'Music', icon: 'music', extensions: ['.mp3', '.flac', '.wav'], count: 1 },
  { id: 'programs', name: 'Programs', icon: 'cpu', extensions: ['.dmg', '.exe', '.pkg'], count: 1 },
  { id: 'video', name: 'Video', icon: 'video', extensions: ['.mp4', '.mkv', '.mov'], count: 1 },
];

const INITIAL_TASKS: DownloadTask[] = [
  {
    id: 'task-music',
    url: 'https://www.youtube.com/watch?v=Jdj13fGMmmA&list=RDgusshrOoODw&index=14',
    fileName: 'Yura Yunita - Dunia Tipu-Tipu.mp3',
    fileSize: 13141681, // 13.1 MB Real Full MP3
    downloadedBytes: 13141681,
    status: 'completed',
    categoryId: 'music',
    destinationPath: '~/Downloads/Music/Yura Yunita - Dunia Tipu-Tipu.mp3',
    supportsRange: true,
    numConnections: 8,
    speedLimitBytesPerSec: 0,
    speedBytesPerSec: 0,
    etaSeconds: 0,
    progressPercent: 100.0,
    createdAt: new Date(Date.now() - 1000 * 60 * 15).toISOString(),
    completedAt: new Date(Date.now() - 1000 * 60 * 14).toISOString(),
  },
  {
    id: 'task-video',
    url: 'https://www.youtube.com/watch?v=Jdj13fGMmmA',
    fileName: 'Yura Yunita - Dunia Tipu-Tipu.mp4',
    fileSize: 10326243, // 9.85 MB Real MP4 Video
    downloadedBytes: 10326243,
    status: 'completed',
    categoryId: 'video',
    destinationPath: '~/Downloads/Video/Yura Yunita - Dunia Tipu-Tipu.mp4',
    supportsRange: true,
    numConnections: 8,
    speedLimitBytesPerSec: 0,
    speedBytesPerSec: 0,
    etaSeconds: 0,
    progressPercent: 100.0,
    createdAt: new Date(Date.now() - 1000 * 60 * 20).toISOString(),
    completedAt: new Date(Date.now() - 1000 * 60 * 19).toISOString(),
  },
  {
    id: 'task-compressed',
    url: 'https://github.com/moesstudio/download-cepat/archive/refs/heads/main.zip',
    fileName: 'download-cepat-main.zip',
    fileSize: 3339657, // 3.18 MB Real Zip
    downloadedBytes: 3339657,
    status: 'completed',
    categoryId: 'compressed',
    destinationPath: '~/Downloads/Compressed/download-cepat-main.zip',
    supportsRange: false,
    numConnections: 1,
    speedLimitBytesPerSec: 0,
    speedBytesPerSec: 0,
    etaSeconds: 0,
    progressPercent: 100.0,
    createdAt: new Date(Date.now() - 1000 * 60 * 25).toISOString(),
    completedAt: new Date(Date.now() - 1000 * 60 * 24).toISOString(),
  },
  {
    id: 'task-programs',
    url: 'https://github.com/alacritty/alacritty/releases/download/v0.17.0/Alacritty-v0.17.0.dmg',
    fileName: 'Alacritty-v0.17.0.dmg',
    fileSize: 6136240, // 5.85 MB Real DMG
    downloadedBytes: 6136240,
    status: 'completed',
    categoryId: 'programs',
    destinationPath: '~/Downloads/Programs/Alacritty-v0.17.0.dmg',
    supportsRange: true,
    numConnections: 8,
    speedLimitBytesPerSec: 0,
    speedBytesPerSec: 0,
    etaSeconds: 0,
    progressPercent: 100.0,
    createdAt: new Date(Date.now() - 1000 * 60 * 30).toISOString(),
    completedAt: new Date(Date.now() - 1000 * 60 * 29).toISOString(),
  },
  {
    id: 'task-documents',
    url: 'https://www.rfc-editor.org/rfc/rfc9110.pdf',
    fileName: 'RFC_9110_HTTP_Semantics.pdf',
    fileSize: 2858365, // 2.73 MB Real PDF
    downloadedBytes: 2858365,
    status: 'completed',
    categoryId: 'documents',
    destinationPath: '~/Downloads/Documents/RFC_9110_HTTP_Semantics.pdf',
    supportsRange: true,
    numConnections: 8,
    speedLimitBytesPerSec: 0,
    speedBytesPerSec: 0,
    etaSeconds: 0,
    progressPercent: 100.0,
    createdAt: new Date(Date.now() - 1000 * 60 * 35).toISOString(),
    completedAt: new Date(Date.now() - 1000 * 60 * 34).toISOString(),
  }
];

export const App: React.FC = () => {
  const [lang, setLang] = useState<LanguageCode>(() => {
    try {
      const saved = localStorage.getItem('download_cepat_lang');
      if (isValidLanguage(saved)) return saved;
    } catch {}
    return 'id';
  });
  const langRef = React.useRef<LanguageCode>(lang);
  useEffect(() => {
    langRef.current = lang;
    try {
      localStorage.setItem('download_cepat_lang', lang);
    } catch {}
    if (typeof document !== 'undefined') {
      document.documentElement.lang = lang;
    }
    void setTrayLanguage(lang);
  }, [lang]);

  const [buildInfo, setBuildInfo] = useState<BuildInfo | undefined>(undefined);
  useEffect(() => {
    getBuildInfo().then(setBuildInfo).catch(() => {});
  }, []);

  const [tasks, setTasks] = useState<DownloadTask[]>(() => {
    try {
      const saved = localStorage.getItem('download_cepat_tasks');
      if (saved !== null) {
        const parsed = JSON.parse(saved);
        if (Array.isArray(parsed)) {
          return parsed.map((t: DownloadTask) => {
            if (t.status === 'downloading' || t.status === 'connecting') {
              return { ...t, status: 'paused' as const, speedBytesPerSec: 0, etaSeconds: 0 };
            }
            return t;
          });
        }
      }
    } catch (e) {}
    return INITIAL_TASKS;
  });

  useEffect(() => {
    try {
      const sanitized = tasks.map(({ cookie, ...rest }) => rest);
      localStorage.setItem('download_cepat_tasks', JSON.stringify(sanitized));
    } catch (e) {}
  }, [tasks]);

  // Persisted App Settings
  interface AppSettings {
    speedLimitKbs: number;
    maxTasks: number;
    defaultConnections: number;
  }

  const [settings, setSettings] = useState<AppSettings>(() => {
    try {
      const saved = localStorage.getItem('download_cepat_settings');
      if (saved) return JSON.parse(saved);
    } catch {}
    return { speedLimitKbs: 0, maxTasks: 2, defaultConnections: 8 };
  });

  useEffect(() => {
    try {
      localStorage.setItem('download_cepat_settings', JSON.stringify(settings));
    } catch {}
  }, [settings]);

  const speedLimitKbs = settings.speedLimitKbs;
  const setSpeedLimitKbs = (limitKbs: number) => {
    setSettings((prev) => ({ ...prev, speedLimitKbs: limitKbs }));
  };
  const settingsRef = React.useRef(settings);
  useEffect(() => {
    settingsRef.current = settings;
  }, [settings]);
  const tasksRef = React.useRef(tasks);
  useEffect(() => {
    tasksRef.current = tasks;
  }, [tasks]);

  const [selectedCategoryId, setSelectedCategoryId] = useState('all');
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(tasks[0]?.id || null);
  const [markedTaskIds, setMarkedTaskIds] = useState<string[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [incomingUrl, setIncomingUrl] = useState<string | undefined>(undefined);
  const [incomingMeta, setIncomingMeta] = useState<IncomingDownloadMeta | undefined>(undefined);

  // Persisted Advanced Feature Configs
  const [proxyConfig, setProxyConfig] = useState<ProxyConfig>(() => {
    try {
      const saved = localStorage.getItem('download_cepat_proxy');
      if (saved) return JSON.parse(saved);
    } catch {}
    return DEFAULT_PROXY_CONFIG;
  });
  useEffect(() => {
    try {
      localStorage.setItem('download_cepat_proxy', JSON.stringify(proxyConfig));
    } catch {}
  }, [proxyConfig]);

  const [siteLogins, setSiteLogins] = useState<SiteLogin[]>(() => {
    try {
      const saved = localStorage.getItem('download_cepat_logins');
      if (saved) return JSON.parse(saved);
    } catch {}
    return [];
  });
  useEffect(() => {
    try {
      localStorage.setItem('download_cepat_logins', JSON.stringify(siteLogins));
    } catch {}
  }, [siteLogins]);

  const [categoryRules, setCategoryRules] = useState<CategoryRule[]>(() => {
    try {
      const saved = localStorage.getItem('download_cepat_category_rules');
      if (saved) return JSON.parse(saved);
    } catch {}
    return DEFAULT_CATEGORY_RULES;
  });
  useEffect(() => {
    try {
      localStorage.setItem('download_cepat_category_rules', JSON.stringify(categoryRules));
    } catch {}
  }, [categoryRules]);

  const dynamicCategories = React.useMemo<Category[]>(() => {
    return INITIAL_CATEGORIES.map((base) => {
      const rule = categoryRules.find((r) => r.id === base.id);
      if (!rule) return base;
      return {
        ...base,
        name: rule.name || base.name,
        extensions: rule.extensions.map((ext) => (ext.startsWith('.') ? ext : `.${ext}`)),
        saveDir: rule.saveDir,
      };
    });
  }, [categoryRules]);

  const [schedulerConfig, setSchedulerConfig] = useState<SchedulerConfig>(() => {
    try {
      const saved = localStorage.getItem('download_cepat_scheduler');
      if (saved) return JSON.parse(saved);
    } catch {}
    return DEFAULT_SCHEDULER_CONFIG;
  });
  useEffect(() => {
    try {
      localStorage.setItem('download_cepat_scheduler', JSON.stringify(schedulerConfig));
    } catch {}
  }, [schedulerConfig]);

  const [clipboardConfig, setClipboardConfig] = useState<ClipboardMonitorConfig>(() => {
    try {
      const saved = localStorage.getItem('download_cepat_clipboard');
      if (saved) return JSON.parse(saved);
    } catch {}
    return DEFAULT_CLIPBOARD_CONFIG;
  });
  useEffect(() => {
    try {
      localStorage.setItem('download_cepat_clipboard', JSON.stringify(clipboardConfig));
    } catch {}
  }, [clipboardConfig]);

  const [isMiniDropOpen, setIsMiniDropOpen] = useState<boolean>(() => {
    try {
      return localStorage.getItem('download_cepat_minidrop') === 'true';
    } catch {
      return false;
    }
  });
  useEffect(() => {
    try {
      localStorage.setItem('download_cepat_minidrop', String(isMiniDropOpen));
    } catch {}
  }, [isMiniDropOpen]);

  // Modals
  const [isAddUrlOpen, setIsAddUrlOpen] = useState(false);
  const [isProgressOpen, setIsProgressOpen] = useState(false);
  const [isCompleteOpen, setIsCompleteOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [settingsInitialTab, setSettingsInitialTab] = useState<SettingsTab>('network');
  const [isBatchOpen, setIsBatchOpen] = useState(false);
  const [batchInitialTab, setBatchInitialTab] = useState<'batch' | 'grabber' | 'browser'>('batch');
  const [batchInitialText, setBatchInitialText] = useState('');
  const [browserPageLinks, setBrowserPageLinks] = useState<BatchQueueItem[]>([]);
  const [isSchedulerOpen, setIsSchedulerOpen] = useState(false);
  const [propertiesTaskId, setPropertiesTaskId] = useState<string | null>(null);
  const [propertiesInitialTab, setPropertiesInitialTab] = useState<'info' | 'rename' | 'checksum'>('info');
  const [isPropertiesOpen, setIsPropertiesOpen] = useState(false);
  const [contextMenuState, setContextMenuState] = useState<{
    task: DownloadTask;
    position: { x: number; y: number };
  } | null>(null);
  const [postActionCountdown, setPostActionCountdown] = useState<{
    action: PostDownloadAction;
    secondsLeft: number;
  } | null>(null);

  const [activeDetailTaskId, setActiveDetailTaskId] = useState<string | null>(null);
  const activeDetailTask = React.useMemo(() => {
    return tasks.find((t) => t.id === activeDetailTaskId) || null;
  }, [tasks, activeDetailTaskId]);
  const propertiesTask = React.useMemo(() => {
    return tasks.find((t) => t.id === propertiesTaskId) || null;
  }, [tasks, propertiesTaskId]);

  const [toast, setToast] = useState<{ message: string; type: 'info' | 'success' | 'warning' } | null>(null);
  const toastTimerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  const showToast = (message: string, type: 'info' | 'success' | 'warning' = 'info') => {
    if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    setToast({ message, type });
    toastTimerRef.current = setTimeout(() => setToast(null), type === 'warning' ? 6500 : 4000);
  };

  const handleNotifyLanguageChange = (nextLang: LanguageCode) => {
    const opt = LANGUAGE_OPTIONS.find((o) => o.code === nextLang);
    if (opt) {
      showToast(
        translate(nextLang, 'toast.languageChanged', { langName: opt.nativeName }),
        'info'
      );
    }
  };

  // Real network download telemetry from Rust Core
  useEffect(() => {
    if (!isTauri()) return;

    let unlistenFn: (() => void) | undefined;
    listen<{
      task_id: string;
      downloaded_bytes: number;
      total_bytes: number;
      speed_bytes_per_sec: number;
      progress_percent: number;
      eta_seconds?: number | null;
      status: string;
      destination_path: string;
      segments?: DownloadSegment[];
      error?: string;
    }>('download-telemetry', (event) => {
      const p = event.payload;
      const isCompleted = p.status === 'completed';
      const isError = p.status === 'error';
      const isPaused = p.status === 'paused';
      const isCanceled = p.status === 'canceled';
      const isStopped = isCompleted || isError || isPaused || isCanceled;

      if (isError) {
        showToast(p.error || translate(langRef.current, 'toast.downloadError'), 'warning');
      }

      setTasks((prev) =>
        prev.map((t) => {
          if (t.id === p.task_id) {
            if (t.status === 'canceled' && !isCanceled) {
              return t;
            }
            const nextStatus: TaskStatus = isCompleted
              ? 'completed'
              : isError
              ? 'error'
              : isPaused
              ? 'paused'
              : isCanceled
              ? 'canceled'
              : 'downloading';
            const nextDest = p.destination_path || t.destinationPath;
            const destBase = nextDest.split('/').pop();
            const nextFileName =
              destBase && nextDest !== t.destinationPath ? destBase : t.fileName;

            const nextDownloaded = isError || isCanceled
              ? 0
              : isPaused
              ? p.downloaded_bytes > 0
                ? p.downloaded_bytes
                : t.downloadedBytes
              : p.downloaded_bytes;

            const nextTotal =
              p.total_bytes > 0
                ? p.total_bytes
                : isCompleted
                ? p.downloaded_bytes
                : t.fileSize;

            const nextSpeed = isStopped ? 0 : p.speed_bytes_per_sec;

            const nextEta = isStopped
              ? 0
              : typeof p.eta_seconds === 'number' && p.eta_seconds > 0
              ? p.eta_seconds
              : nextTotal && nextTotal > nextDownloaded && nextSpeed > 0
              ? Math.ceil((nextTotal - nextDownloaded) / nextSpeed)
              : t.etaSeconds;

            const nextSegments = isCanceled
              ? []
              : Array.isArray(p.segments) && p.segments.length > 0
                ? p.segments
                : isCompleted && t.segments && t.segments.length > 0
                ? t.segments.map((seg) => {
                    const segTotal = Math.max(1, seg.endByte - seg.startByte + 1);
                    return {
                      ...seg,
                      downloadedBytes: segTotal,
                      currentByte: seg.startByte + segTotal,
                      isCompleted: true,
                    };
                  })
                : t.segments;

            return {
              ...t,
              fileName: nextFileName,
              downloadedBytes: isError ? t.downloadedBytes : nextDownloaded,
              fileSize: nextTotal,
              speedBytesPerSec: nextSpeed,
              etaSeconds: nextEta,
              progressPercent: isError || isCanceled
                ? 0
                : isPaused
                ? p.progress_percent > 0
                  ? p.progress_percent
                  : t.progressPercent
                : p.progress_percent,
              status: nextStatus,
              errorMessage: isError
                ? p.error || translate(langRef.current, 'toast.downloadError')
                : undefined,
              completedAt: isCompleted ? new Date().toISOString() : t.completedAt,
              destinationPath: nextDest,
              segments: nextSegments,
            };
          }
          return t;
        })
      );

      if (isCompleted) {
        setIsProgressOpen(false);
        setIsCompleteOpen(true);
        showToast(translate(langRef.current, 'toast.downloadComplete'), 'success');
      }
    }).then((unlisten) => {
      unlistenFn = unlisten;
    }).catch((err) => {
      console.warn('Tauri event listen not available:', err);
    });

    return () => {
      if (unlistenFn) unlistenFn();
    };
  }, []);

  // Web fallback simulation loop (ONLY active in Web/Vitest demo mode, DISABLED in Tauri native app)
  useEffect(() => {
    if (isTauri()) return;

    const interval = setInterval(() => {
      setTasks((prev) =>
        prev.map((t) => {
          if (t.status !== 'downloading') return t;

          // Simulated high speed download update (between 12 MB/s to 18 MB/s)
          const delta = Math.floor(Math.random() * 4000000 + 12000000);
          const newDownloaded = Math.min((t.fileSize || 0), t.downloadedBytes + delta);
          const percent = t.fileSize ? (newDownloaded / t.fileSize) * 100 : 0;
          const isDone = percent >= 100;

          if (isDone) {
            ensureFileCreated(t.destinationPath, t.fileName);
            return {
              ...t,
              downloadedBytes: newDownloaded,
              progressPercent: 100,
              status: 'completed',
              speedBytesPerSec: 0,
              etaSeconds: 0,
              completedAt: new Date().toISOString(),
            };
          }

          const rem = (t.fileSize || 0) - newDownloaded;
          const eta = Math.ceil(rem / (delta * 2));

          return {
            ...t,
            downloadedBytes: newDownloaded,
            progressPercent: percent,
            speedBytesPerSec: delta * 2,
            etaSeconds: eta,
          };
        })
      );
    }, 500);

    return () => clearInterval(interval);
  }, []);

  // Global keyboard shortcuts (Cmd+N for New Download)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'n') {
        e.preventDefault();
        setIsAddUrlOpen(true);
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  // Listen for background tasks started from Browser Dropdown menu & manual modal trigger
  useEffect(() => {
    let unlistenExternal: (() => void) | undefined;
    let unlistenBrowser: (() => void) | undefined;

    if (isTauri()) {
      listen<{
        id: string;
        url: string;
        fileName: string;
        destinationPath: string;
        categoryId: string;
        formatId?: string;
        resolution?: string;
        numConnections?: number;
        status: string;
      }>('external-task-added', (event) => {
        const p = event.payload;
        if (!p || !p.id) return;
        const currentSettings = settingsRef.current;
        const currentTasks = tasksRef.current;
        const activeCount = currentTasks.filter(
          (t) => t.status === 'downloading' || t.status === 'connecting'
        ).length;
        const isQueued = activeCount >= currentSettings.maxTasks;
        const newTask: DownloadTask = {
          id: p.id,
          url: p.url,
          fileName: p.fileName,
          destinationPath: p.destinationPath,
          fileSize: null,
          downloadedBytes: 0,
          speedBytesPerSec: isQueued ? 0 : 2_500_000,
          progressPercent: 0,
          status: isQueued ? 'queued' : 'downloading',
          categoryId: p.categoryId,
          formatId: p.formatId,
          resolution: p.resolution,
          supportsRange: true,
          numConnections: p.numConnections || currentSettings.defaultConnections,
          speedLimitBytesPerSec: currentSettings.speedLimitKbs * 1024,
          createdAt: new Date().toISOString(),
        };
        setTasks((prev) => [newTask, ...prev.filter((t) => t.id !== p.id)]);
        setSelectedTaskId(p.id);
        if (isQueued) {
          showToast(
            translate(langRef.current, 'toast.bgQueued', { name: p.fileName }),
            'info'
          );
        } else {
          showToast(
            translate(langRef.current, 'toast.bgStarted', {
              name: p.fileName,
              res: p.resolution || 'Original',
            }),
            'success'
          );
        }
      }).then((unsub) => {
        unlistenExternal = unsub;
      }).catch((e) => console.warn('Could not listen for external-task-added:', e));

      listen<IncomingDownloadMeta>('browser-add-download', (event) => {
        if (event.payload?.url) {
          setIncomingMeta(event.payload);
          setIncomingUrl(event.payload.url);
          setIsAddUrlOpen(true);
          showToast(
            translate(langRef.current, 'toast.browserLinkReceived', {
              target: event.payload.fileName || event.payload.url,
            }),
            'info'
          );
        }
      }).then((unsub) => {
        unlistenBrowser = unsub;
      }).catch((e) => console.warn('Could not listen for browser-add-download:', e));
    }
    return () => {
      if (unlistenExternal) unlistenExternal();
      if (unlistenBrowser) unlistenBrowser();
    };
  }, []);

  // Listen for Chrome Extension "Download All Links", Tray actions, and OS File Drop
  useEffect(() => {
    if (!isTauri()) return;
    let unlistenLinks: (() => void) | undefined;
    let unlistenTrayAdd: (() => void) | undefined;
    let unlistenTrayPause: (() => void) | undefined;
    let unlistenDragDrop: (() => void) | undefined;

    listen<{
      links: Array<{ url: string; label?: string }>;
      pageUrl?: string;
      cookie?: string;
      userAgent?: string;
    }>('browser-add-links', (event) => {
      const payload = event.payload;
      if (!payload || !Array.isArray(payload.links) || payload.links.length === 0) return;
      const mapped: BatchQueueItem[] = payload.links.map((item) => ({
        url: item.url,
        fileName: deriveFileNameFromUrl(item.url),
        cookie: payload.cookie,
        referer: payload.pageUrl,
        userAgent: payload.userAgent,
      }));
      setBrowserPageLinks(mapped);
      setBatchInitialTab('browser');
      setIsBatchOpen(true);
      showToast(
        translate(langRef.current, 'toast.browserLinksReceived', { count: mapped.length }),
        'info'
      );
    }).then((u) => { unlistenLinks = u; }).catch(() => {});

    listen('tray-add-url', () => {
      setIsAddUrlOpen(true);
    }).then((u) => { unlistenTrayAdd = u; }).catch(() => {});

    listen('tray-pause-all', () => {
      setTasks((prev) =>
        prev.map((t) =>
          t.status === 'downloading' || t.status === 'connecting'
            ? { ...t, status: 'paused', speedBytesPerSec: 0, etaSeconds: 0 }
            : t
        )
      );
      showToast(translate(langRef.current, 'toast.trayPausedAll'), 'info');
    }).then((u) => { unlistenTrayPause = u; }).catch(() => {});

    getCurrentWebview()
      .onDragDropEvent(async (event) => {
        if (event.payload.type !== 'drop') return;
        const paths = event.payload.paths || [];
        const collectedUrls: string[] = [];
        for (const path of paths) {
          if (/\.(txt|ef2|lst)$/i.test(path)) {
            try {
              const content = await readTextFilePreview(path);
              collectedUrls.push(...parseUrlListText(content));
            } catch (err) {
              showToast(String(err), 'warning');
            }
          }
        }
        const unique = Array.from(new Set(collectedUrls));
        if (unique.length === 1) {
          setIncomingUrl(unique[0]);
          setIsAddUrlOpen(true);
        } else if (unique.length > 1) {
          setBatchInitialText(unique.join('\n'));
          setBatchInitialTab('batch');
          setIsBatchOpen(true);
        }
      })
      .then((u) => { unlistenDragDrop = u; })
      .catch(() => {});

    return () => {
      if (unlistenLinks) unlistenLinks();
      if (unlistenTrayAdd) unlistenTrayAdd();
      if (unlistenTrayPause) unlistenTrayPause();
      if (unlistenDragDrop) unlistenDragDrop();
    };
  }, []);

  // Background Clipboard Monitor (only when explicitly enabled by user)
  const lastClipboardRef = React.useRef<string>('');
  useEffect(() => {
    if (!clipboardConfig.enabled) return;
    let active = true;
    const poll = async () => {
      const text = (await readClipboardText()).trim();
      if (!active || !text || text === lastClipboardRef.current) return;
      if (shouldInterceptClipboardUrl(text, clipboardConfig)) {
        lastClipboardRef.current = text;
        setIncomingUrl(text);
        setIsAddUrlOpen(true);
        showToast(translate(langRef.current, 'toast.clipboardDetected'), 'info');
      }
    };
    void poll();
    const timer = setInterval(poll, 2000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [clipboardConfig]);

  // Automatic Queue Promotion, Schedule Window Enforcement & Post-Download Automation
  const schedulerRef = React.useRef(schedulerConfig);
  const proxyRef = React.useRef(proxyConfig);
  const loginsRef = React.useRef(siteLogins);
  const hadActiveQueueRef = React.useRef(false);
  useEffect(() => { schedulerRef.current = schedulerConfig; }, [schedulerConfig]);
  useEffect(() => { proxyRef.current = proxyConfig; }, [proxyConfig]);
  useEffect(() => { loginsRef.current = siteLogins; }, [siteLogins]);

  const promoteQueuedTasks = React.useCallback(() => {
    const currentTasks = tasksRef.current;
    const currentSettings = settingsRef.current;
    const sched = schedulerRef.current;

    const activeTasks = currentTasks.filter(
      (t) => t.status === 'downloading' || t.status === 'connecting'
    );
    const queuedTasks = currentTasks.filter((t) => t.status === 'queued');

    if (sched.enabled && !isWithinScheduleWindow(sched)) {
      if (activeTasks.length > 0) {
        void stopAllRealDownloads();
        setTasks((prev) =>
          prev.map((t) =>
            t.status === 'downloading' || t.status === 'connecting'
              ? { ...t, status: 'queued', speedBytesPerSec: 0, etaSeconds: 0 }
              : t
          )
        );
        showToast(translate(langRef.current, 'toast.outsideSchedule'), 'info');
      }
      return;
    }

    if (queuedTasks.length > 0 && activeTasks.length < currentSettings.maxTasks) {
      const slots = currentSettings.maxTasks - activeTasks.length;
      const toStart = queuedTasks.slice(0, slots);
      const startIds = new Set(toStart.map((t) => t.id));
      const activeProxy = formatProxyUrl(proxyRef.current);

      setTasks((prev) =>
        prev.map((t) =>
          startIds.has(t.id)
            ? {
                ...t,
                status: 'downloading',
                errorMessage: undefined,
                speedBytesPerSec: isTauri() ? 0 : 8_000_000,
              }
            : t
        )
      );

      toStart.forEach((task) => {
        const login = findMatchingSiteLogin(task.url, loginsRef.current);
        triggerRealDownload(
          task.id,
          task.url,
          task.destinationPath,
          task.numConnections || currentSettings.defaultConnections,
          task.formatId,
          {
            cookie: task.cookie,
            referer: task.referer,
            userAgent: task.userAgent,
            proxyUrl: task.proxyUrl || activeProxy,
            authUsername: task.authUsername || login?.username,
            authPassword: task.authPassword || login?.password,
          },
          task.speedLimitBytesPerSec > 0
            ? Math.round(task.speedLimitBytesPerSec / 1024)
            : currentSettings.speedLimitKbs,
          task.downloadedBytes > 0
        );
      });
    }
  }, []);

  useEffect(() => {
    promoteQueuedTasks();

    const activeOrQueued = tasks.filter(
      (t) => t.status === 'downloading' || t.status === 'connecting' || t.status === 'queued'
    ).length;
    if (activeOrQueued > 0) {
      hadActiveQueueRef.current = true;
    } else if (hadActiveQueueRef.current) {
      hadActiveQueueRef.current = false;
      const action = schedulerConfig.postAction;
      if (action === 'notify_open') {
        showToast(translate(langRef.current, 'toast.allQueueDone'), 'success');
      } else if (action === 'sleep' || action === 'quit') {
        setPostActionCountdown({ action, secondsLeft: 10 });
      }
    }
  }, [tasks, settings.maxTasks, schedulerConfig, promoteQueuedTasks]);

  useEffect(() => {
    if (!schedulerConfig.enabled) return;
    const timer = setInterval(promoteQueuedTasks, 15000);
    return () => clearInterval(timer);
  }, [schedulerConfig.enabled, promoteQueuedTasks]);

  useEffect(() => {
    if (!postActionCountdown) return;
    if (postActionCountdown.secondsLeft <= 0) {
      const act = postActionCountdown.action;
      setPostActionCountdown(null);
      void runPostDownloadAction(act);
      return;
    }
    const timer = setTimeout(() => {
      setPostActionCountdown((prev) =>
        prev ? { ...prev, secondsLeft: prev.secondsLeft - 1 } : null
      );
    }, 1000);
    return () => clearTimeout(timer);
  }, [postActionCountdown]);

  // Compute category counts
  const categoryCounts = React.useMemo(() => {
    const counts: Record<string, number> = {
      all: tasks.length,
      unfinished: tasks.filter((t) => ['downloading', 'paused', 'queued', 'connecting'].includes(t.status)).length,
      completed: tasks.filter((t) => t.status === 'completed').length,
    };
    INITIAL_CATEGORIES.forEach((cat) => {
      counts[cat.id] = tasks.filter((t) => t.categoryId === cat.id).length;
    });
    return counts;
  }, [tasks]);

  // Filter tasks by selected category & search
  const filteredTasks = tasks.filter((t) => {
    if (selectedCategoryId === 'unfinished') {
      if (!['downloading', 'paused', 'queued', 'connecting'].includes(t.status)) return false;
    } else if (selectedCategoryId === 'completed') {
      if (t.status !== 'completed') return false;
    } else if (selectedCategoryId !== 'all') {
      if (t.categoryId !== selectedCategoryId) return false;
    }

    if (searchQuery.trim()) {
      const q = searchQuery.toLowerCase();
      return t.fileName.toLowerCase().includes(q) || t.url.toLowerCase().includes(q);
    }

    return true;
  });

  const selectedTask = tasks.find((t) => t.id === selectedTaskId);

  // Concurrency queue scheduler: automatically start queued tasks when active slots free up
  useEffect(() => {
    const activeCount = tasks.filter((t) => t.status === 'downloading' || t.status === 'connecting').length;
    if (activeCount < settings.maxTasks) {
      const nextQueued = tasks.find((t) => t.status === 'queued');
      if (nextQueued) {
        setTasks((prev) =>
          prev.map((t) =>
            t.id === nextQueued.id ? { ...t, status: 'downloading', speedBytesPerSec: 2_500_000 } : t
          )
        );
        triggerRealDownload(
          nextQueued.id,
          nextQueued.url,
          nextQueued.destinationPath,
          nextQueued.numConnections || settings.defaultConnections,
          nextQueued.formatId,
          {
            cookie: nextQueued.cookie,
            referer: nextQueued.referer,
            userAgent: nextQueued.userAgent,
          },
          settings.speedLimitKbs,
          false
        );
        showToast(
          translate(langRef.current, 'toast.queueNextStarted', { name: nextQueued.fileName }),
          'info'
        );
      }
    }
  }, [tasks, settings.maxTasks, settings.defaultConnections, settings.speedLimitKbs]);

  // Actions
  const handleStartNewDownload = (
    url: string,
    fileName: string,
    destinationDir: string,
    categoryId: string,
    numConnections: number,
    formatId?: string,
    resolution?: string,
    meta?: {
      cookie?: string;
      referer?: string;
      userAgent?: string;
      proxyUrl?: string;
      authUsername?: string;
      authPassword?: string;
      fileSize?: number | null;
      supportsRange?: boolean;
    }
  ) => {
    const destinationPath = `${destinationDir}/${fileName}`;
    const activeCount = tasks.filter((t) => t.status === 'downloading' || t.status === 'connecting').length;
    const scheduleBlocked = schedulerConfig.enabled && !isWithinScheduleWindow(schedulerConfig);
    const isQueued = activeCount >= settings.maxTasks || scheduleBlocked;
    const taskStatus: TaskStatus = isQueued ? 'queued' : 'downloading';

    const resolvedProxy = meta?.proxyUrl || formatProxyUrl(proxyConfig);
    const matchedLogin = findMatchingSiteLogin(url, siteLogins);
    const authUsername = meta?.authUsername || matchedLogin?.username;
    const authPassword = meta?.authPassword || matchedLogin?.password;

    const newTask: DownloadTask = {
      id: `task-${Date.now()}-${Math.floor(Math.random() * 1000)}`,
      url,
      fileName,
      fileSize: meta?.fileSize ?? (isTauri() ? null : 45000000),
      downloadedBytes: 0,
      status: taskStatus,
      categoryId,
      destinationPath,
      supportsRange: meta?.supportsRange ?? true,
      numConnections: numConnections || settings.defaultConnections,
      speedLimitBytesPerSec: speedLimitKbs * 1024,
      speedBytesPerSec: isQueued ? 0 : (isTauri() ? 0 : 15000000),
      etaSeconds: isQueued ? 0 : 3,
      progressPercent: 0,
      formatId,
      resolution,
      cookie: meta?.cookie,
      referer: meta?.referer,
      userAgent: meta?.userAgent,
      proxyUrl: resolvedProxy,
      authUsername,
      authPassword,
      httpLogs: [
        {
          timestamp: new Date().toLocaleTimeString(),
          event: isQueued ? 'ANTREAN' : 'MULAI',
          detail: url,
        },
      ],
      createdAt: new Date().toISOString(),
    };

    setTasks((prev) => [newTask, ...prev]);
    setSelectedTaskId(newTask.id);
    setActiveDetailTaskId(newTask.id);
    setIsProgressOpen(!isQueued);

    if (isQueued) {
      showToast(
        scheduleBlocked
          ? translate(langRef.current, 'toast.taskScheduled', {
              name: fileName,
              time: schedulerConfig.startTime,
            })
          : translate(langRef.current, 'toast.taskQueuedMax', {
              name: fileName,
              max: settings.maxTasks,
            }),
        'info'
      );
    } else {
      triggerRealDownload(
        newTask.id,
        url,
        destinationPath,
        newTask.numConnections,
        formatId,
        {
          cookie: newTask.cookie,
          referer: newTask.referer,
          userAgent: newTask.userAgent,
          proxyUrl: resolvedProxy,
          authUsername,
          authPassword,
        },
        settings.speedLimitKbs,
        false
      );
    }
  };

  const handleResume = (id?: string) => {
    const targetId = id || selectedTaskId;
    if (!targetId) return;
    const task = tasks.find((t) => t.id === targetId);
    if (!task) return;

    const activeCount = tasks.filter((t) => (t.status === 'downloading' || t.status === 'connecting') && t.id !== targetId).length;
    const isQueued = activeCount >= settings.maxTasks;
    const newStatus: TaskStatus = isQueued ? 'queued' : 'downloading';
    const activeProxy = task.proxyUrl || formatProxyUrl(proxyConfig);
    const login = findMatchingSiteLogin(task.url, siteLogins);

    setTasks((prev) =>
      prev.map((t) =>
        t.id === targetId
          ? { ...t, status: newStatus, errorMessage: undefined, speedBytesPerSec: isQueued ? 0 : (isTauri() ? 0 : 5000000) }
          : t
      )
    );
    setSelectedTaskId(targetId);
    setActiveDetailTaskId(targetId);
    setIsProgressOpen(!isQueued);

    if (isQueued) {
      showToast(
        translate(langRef.current, 'toast.taskQueuedMax', {
          name: task.fileName,
          max: settings.maxTasks,
        }),
        'info'
      );
    } else {
      triggerRealDownload(
        task.id,
        task.url,
        task.destinationPath,
        task.numConnections || settings.defaultConnections,
        task.formatId,
        {
          cookie: task.cookie,
          referer: task.referer,
          userAgent: task.userAgent,
          proxyUrl: activeProxy,
          authUsername: task.authUsername || login?.username,
          authPassword: task.authPassword || login?.password,
        },
        task.speedLimitBytesPerSec > 0
          ? Math.round(task.speedLimitBytesPerSec / 1024)
          : settings.speedLimitKbs,
        true
      );
      showToast(translate(langRef.current, 'toast.resuming', { name: task.fileName }), 'info');
    }
  };

  const handleRefreshUrlAndResume = (
    taskId: string,
    newUrl: string,
    headers?: { cookie?: string; referer?: string; userAgent?: string }
  ) => {
    const existing = tasks.find((t) => t.id === taskId);
    if (!existing) return;
    const activeCount = tasks.filter(
      (t) => (t.status === 'downloading' || t.status === 'connecting') && t.id !== taskId
    ).length;
    const isQueued = activeCount >= settings.maxTasks;
    const refreshedAt = new Date().toISOString();
    const activeProxy = existing.proxyUrl || formatProxyUrl(proxyConfig);
    const login = findMatchingSiteLogin(newUrl, siteLogins);

    setTasks((prev) =>
      prev.map((t) =>
        t.id === taskId
          ? {
              ...t,
              url: newUrl,
              cookie: headers?.cookie ?? t.cookie,
              referer: headers?.referer ?? t.referer,
              userAgent: headers?.userAgent ?? t.userAgent,
              lastUrlRefreshedAt: refreshedAt,
              errorMessage: undefined,
              status: isQueued ? 'queued' : 'downloading',
              httpLogs: [
                ...(t.httpLogs || []),
                {
                  timestamp: new Date().toLocaleTimeString(),
                  event: 'REFRESH_URL',
                  detail: newUrl,
                },
              ],
            }
          : t
      )
    );

    if (!isQueued) {
      setActiveDetailTaskId(taskId);
      setIsProgressOpen(true);
      triggerRealDownload(
        taskId,
        newUrl,
        existing.destinationPath,
        existing.numConnections || settings.defaultConnections,
        existing.formatId,
        {
          cookie: headers?.cookie ?? existing.cookie,
          referer: headers?.referer ?? existing.referer,
          userAgent: headers?.userAgent ?? existing.userAgent,
          proxyUrl: activeProxy,
          authUsername: existing.authUsername || login?.username,
          authPassword: existing.authPassword || login?.password,
          allowUrlRefresh: true,
        },
        existing.speedLimitBytesPerSec > 0
          ? Math.round(existing.speedLimitBytesPerSec / 1024)
          : settings.speedLimitKbs,
        true
      );
    }
    showToast(translate(langRef.current, 'toast.urlRefreshed'), 'success');
  };

  const handleUpdateTaskSpeedLimit = (taskId: string, limitKbs: number) => {
    setTasks((prev) =>
      prev.map((t) =>
        t.id === taskId ? { ...t, speedLimitBytesPerSec: limitKbs * 1024 } : t
      )
    );
    void updateDownloadSpeedLimit(limitKbs, taskId);
    showToast(
      limitKbs > 0
        ? translate(langRef.current, 'toast.taskLimitSet', { limit: limitKbs })
        : translate(langRef.current, 'toast.taskLimitCleared'),
      'success'
    );
  };

  const handleUpdateTaskPath = (
    taskId: string,
    newFileName: string,
    newDestinationPath: string
  ) => {
    setTasks((prev) =>
      prev.map((t) =>
        t.id === taskId
          ? { ...t, fileName: newFileName, destinationPath: newDestinationPath }
          : t
      )
    );
  };

  const handleUpdateChecksums = (taskId: string, checksums: FileChecksums) => {
    setTasks((prev) =>
      prev.map((t) => (t.id === taskId ? { ...t, checksums } : t))
    );
  };

  const handleMovePriority = (taskId: string, direction: 'up' | 'down') => {
    setTasks((prev) => {
      const idx = prev.findIndex((t) => t.id === taskId);
      if (idx === -1) return prev;
      const targetIdx = direction === 'up' ? idx - 1 : idx + 1;
      if (targetIdx < 0 || targetIdx >= prev.length) return prev;
      const copy = [...prev];
      const [item] = copy.splice(idx, 1);
      copy.splice(targetIdx, 0, item);
      return copy;
    });
  };

  const handleEnqueueBatch = (items: BatchQueueItem[]) => {
    if (items.length === 0) return;
    const baseTime = Date.now();
    const activeProxy = formatProxyUrl(proxyConfig);
    const created: DownloadTask[] = items.map((item, idx) => {
      const fileName = item.fileName || deriveFileNameFromUrl(item.url);
      const { categoryId, saveDir } = matchCategoryForFile(fileName, categoryRules);
      const login = findMatchingSiteLogin(item.url, siteLogins);
      return {
        id: `task-${baseTime}-${idx}`,
        url: item.url,
        fileName,
        fileSize: isTauri() ? null : 25_000_000,
        downloadedBytes: 0,
        status: 'queued' as TaskStatus,
        categoryId,
        destinationPath: `${saveDir}/${fileName}`,
        supportsRange: true,
        numConnections: settings.defaultConnections,
        speedLimitBytesPerSec: settings.speedLimitKbs * 1024,
        speedBytesPerSec: 0,
        etaSeconds: 0,
        progressPercent: 0,
        cookie: item.cookie,
        referer: item.referer,
        userAgent: item.userAgent,
        proxyUrl: activeProxy,
        authUsername: login?.username,
        authPassword: login?.password,
        createdAt: new Date(baseTime + idx).toISOString(),
      };
    });

    setTasks((prev) => [...created, ...prev]);
    setSelectedTaskId(created[0].id);
    showToast(
      translate(langRef.current, 'toast.batchEnqueued', { count: created.length }),
      'success'
    );
  };

  const handleDropUrls = (urls: string[]) => {
    if (urls.length === 1) {
      setIncomingUrl(urls[0]);
      setIsAddUrlOpen(true);
    } else if (urls.length > 1) {
      setBatchInitialText(urls.join('\n'));
      setBatchInitialTab('batch');
      setIsBatchOpen(true);
    }
  };

  const handlePause = (id?: string) => {
    const targetId = id || selectedTaskId;
    if (!targetId) return;
    void pauseRealDownload(targetId);
    setTasks((prev) =>
      prev.map((t) =>
        t.id === targetId ? { ...t, status: 'paused', speedBytesPerSec: 0, etaSeconds: 0 } : t
      )
    );
  };

  const handleStopAll = () => {
    void stopAllRealDownloads();
    setTasks((prev) =>
      prev.map((t) =>
        t.status === 'downloading' || t.status === 'connecting'
          ? { ...t, status: 'paused', speedBytesPerSec: 0, etaSeconds: 0 }
          : t
      )
    );
  };

  const handleToggleMarkTask = (id: string) => {
    setMarkedTaskIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]
    );
  };

  const handleToggleMarkAll = () => {
    if (filteredTasks.length === 0) return;
    const allMarked = filteredTasks.every((t) => markedTaskIds.includes(t.id));
    if (allMarked) {
      setMarkedTaskIds((prev) => prev.filter((id) => !filteredTasks.some((t) => t.id === id)));
    } else {
      setMarkedTaskIds((prev) => Array.from(new Set([...prev, ...filteredTasks.map((t) => t.id)])));
    }
  };

  const getSafeCleanupPath = (task: DownloadTask, excludingIds: Set<string>): string | undefined => {
    if (task.status === 'completed' || !task.destinationPath) {
      return undefined;
    }
    const ownedByRemainingCompleted = tasks.some(
      (other) =>
        other.id !== task.id &&
        !excludingIds.has(other.id) &&
        other.status === 'completed' &&
        other.destinationPath === task.destinationPath
    );
    return ownedByRemainingCompleted ? undefined : task.destinationPath;
  };

  const handleCancel = (id?: string) => {
    const targetId = id || selectedTaskId;
    if (!targetId) return;
    const task = tasks.find((t) => t.id === targetId);
    if (!task || task.status === 'completed') return;

    void cancelRealDownload(task.id, getSafeCleanupPath(task, new Set()));
    setTasks((prev) =>
      prev.map((t) =>
        t.id === targetId
          ? {
              ...t,
              status: 'canceled',
              downloadedBytes: 0,
              progressPercent: 0,
              speedBytesPerSec: 0,
              etaSeconds: 0,
              segments: [],
              errorMessage: undefined,
            }
          : t
      )
    );
    if (activeDetailTaskId === targetId) {
      setIsProgressOpen(false);
    }
    showToast(translate(langRef.current, 'toast.taskCanceled', { name: task.fileName }), 'info');
  };

  const handleDeleteMarked = () => {
    if (markedTaskIds.length === 0) return;
    const deleteSet = new Set(markedTaskIds);
    const toDelete = tasks.filter((t) => deleteSet.has(t.id));
    const count = toDelete.length;
    const cleanedIncompleteCount = toDelete.filter((t) => t.status !== 'completed').length;

    toDelete
      .filter((t) => t.status !== 'completed')
      .forEach((t) => {
        void cancelRealDownload(t.id, getSafeCleanupPath(t, deleteSet));
      });

    setTasks((prev) => prev.filter((t) => !deleteSet.has(t.id)));
    if (selectedTaskId && deleteSet.has(selectedTaskId)) {
      setSelectedTaskId(null);
    }
    if (activeDetailTaskId && deleteSet.has(activeDetailTaskId)) {
      setActiveDetailTaskId(null);
      setIsProgressOpen(false);
      setIsCompleteOpen(false);
    }
    setMarkedTaskIds([]);
    showToast(
      cleanedIncompleteCount > 0
        ? translate(langRef.current, 'toast.markedDeletedCleaned', { count })
        : translate(langRef.current, 'toast.markedDeleted', { count }),
      'info'
    );
  };

  const handleClearMarked = () => {
    setMarkedTaskIds([]);
  };

  const handleDelete = () => {
    if (!selectedTaskId) return;
    const taskToDelete = tasks.find((t) => t.id === selectedTaskId);
    if (taskToDelete && taskToDelete.status !== 'completed') {
      void cancelRealDownload(
        taskToDelete.id,
        getSafeCleanupPath(taskToDelete, new Set([taskToDelete.id]))
      );
    }
    if (activeDetailTaskId === selectedTaskId) {
      setActiveDetailTaskId(null);
      setIsProgressOpen(false);
      setIsCompleteOpen(false);
    }
    setTasks((prev) => prev.filter((t) => t.id !== selectedTaskId));
    setMarkedTaskIds((prev) => prev.filter((id) => id !== selectedTaskId));
    setSelectedTaskId(null);
    if (taskToDelete) {
      showToast(
        taskToDelete.status !== 'completed'
          ? translate(langRef.current, 'toast.deletedCleaned', { name: taskToDelete.fileName })
          : translate(langRef.current, 'toast.deletedListOnly', { name: taskToDelete.fileName }),
        'info'
      );
    }
  };

  const handleOpenFolder = async (taskToOpen?: DownloadTask) => {
    const targetPath = taskToOpen?.destinationPath || selectedTask?.destinationPath || '~/Downloads';
    showToast(translate(langRef.current, 'toast.openingFolder', { path: targetPath }), 'info');
    const res = await openNativeFolder(targetPath);
    if (!res.success) {
      showToast(res.message, 'warning');
    }
  };

  const handleOpenFile = async (task: DownloadTask) => {
    showToast(translate(langRef.current, 'toast.openingFile', { name: task.fileName }), 'info');
    const res = await openNativeFile(task.destinationPath);
    if (!res.success) {
      showToast(res.message, 'warning');
    }
  };

  const handleOpenDetails = (task: DownloadTask) => {
    setActiveDetailTaskId(task.id);
    if (task.status === 'completed') {
      setIsCompleteOpen(true);
    } else {
      setIsProgressOpen(true);
    }
  };

  return (
    <I18nProvider lang={lang} setLang={setLang}>
      <div
        onDragOver={(e) => e.preventDefault()}
        onDrop={async (e) => {
          if (e.defaultPrevented) return;
          const collected: string[] = [];
          const uriList = e.dataTransfer?.getData('text/uri-list');
          const plain = e.dataTransfer?.getData('text/plain');
          if (uriList) collected.push(...parseUrlListText(uriList));
          if (plain) collected.push(...parseUrlListText(plain));
          if (e.dataTransfer?.files && e.dataTransfer.files.length > 0) {
            for (const f of Array.from(e.dataTransfer.files)) {
              if (/\.(txt|ef2|lst)$/i.test(f.name)) {
                const txt = await f.text();
                collected.push(...parseUrlListText(txt));
              }
            }
          }
          const unique = Array.from(new Set(collected));
          if (unique.length > 0) {
            e.preventDefault();
            handleDropUrls(unique);
          }
        }}
        className="flex h-screen w-screen overflow-hidden bg-bg text-text"
      >
        {/* S01: Left Navigation Sidebar */}
        <CategorySidebar
          selectedCategory={selectedCategoryId}
          onSelectCategory={setSelectedCategoryId}
          categories={dynamicCategories}
          counts={categoryCounts}
          defaultConnections={settings.defaultConnections}
          version={buildInfo?.version}
          author={buildInfo?.author}
          onOpenBuildInfo={() => {
            setSettingsInitialTab('about');
            setIsSettingsOpen(true);
          }}
        />

        {/* Main Workspace Area */}
        <main className="flex-1 flex flex-col min-w-0 overflow-hidden">
          {/* Top Control Toolbar */}
          <Toolbar
            onAddUrl={() => setIsAddUrlOpen(true)}
            onOpenBatchGrabber={() => {
              setBatchInitialTab('batch');
              setIsBatchOpen(true);
            }}
            onOpenScheduler={() => setIsSchedulerOpen(true)}
            onToggleMiniDrop={() => setIsMiniDropOpen((v) => !v)}
            isMiniDropOpen={isMiniDropOpen}
            isSchedulerEnabled={schedulerConfig.enabled}
            onResume={() => handleResume()}
            onPause={() => handlePause()}
            onStopAll={handleStopAll}
            onDelete={handleDelete}
            onOpenFolder={handleOpenFolder}
            onOpenSettings={() => {
              setSettingsInitialTab('network');
              setIsSettingsOpen(true);
            }}
            onOpenBuildInfo={() => {
              setSettingsInitialTab('about');
              setIsSettingsOpen(true);
            }}
            onChangeLanguage={handleNotifyLanguageChange}
            searchQuery={searchQuery}
            onSearchChange={setSearchQuery}
            hasSelectedTask={Boolean(selectedTaskId)}
            markedCount={markedTaskIds.length}
            onDeleteMarked={handleDeleteMarked}
            onClearMarked={handleClearMarked}
          />

          {/* S01: Task Data Table */}
          <TaskDataTable
            tasks={filteredTasks}
            selectedTaskId={selectedTaskId}
            markedTaskIds={markedTaskIds}
            onSelectTask={setSelectedTaskId}
            onToggleMarkTask={handleToggleMarkTask}
            onToggleMarkAll={handleToggleMarkAll}
            onOpenDetails={handleOpenDetails}
            onContextMenu={(task, e) =>
              setContextMenuState({
                task,
                position: { x: e.clientX, y: e.clientY },
              })
            }
          />

          {/* Status Bar */}
          <footer className="h-6 border-t border-border bg-surface px-3 flex items-center justify-between text-[11px] text-text-muted shrink-0 select-none">
            <div className="flex items-center space-x-3">
              <span>{translate(lang, 'footer.totalTasks', { count: tasks.length })}</span>
              <span>•</span>
              <span className="text-accent font-medium">
                {translate(lang, 'footer.activeTasks', {
                  active: categoryCounts.unfinished || 0,
                  max: settings.maxTasks,
                })}
              </span>
              {markedTaskIds.length > 0 && (
                <>
                  <span>•</span>
                  <span className="text-accent font-semibold bg-accent/10 px-1.5 py-0.5 rounded">
                    {translate(lang, 'footer.markedTasks', { count: markedTaskIds.length })}
                  </span>
                </>
              )}
              {proxyConfig.enabled && proxyConfig.host && (
                <>
                  <span>•</span>
                  <span className="font-mono text-accent">
                    {translate(lang, 'footer.proxyActive', {
                      protocol: proxyConfig.protocol.toUpperCase(),
                    })}
                  </span>
                </>
              )}
            </div>
            <div className="flex items-center space-x-2">
              {schedulerConfig.enabled && (
                <span className="font-mono text-accent">
                  {translate(lang, 'footer.scheduleWindow', {
                    start: schedulerConfig.startTime,
                    stop: schedulerConfig.stopTime,
                  })}
                </span>
              )}
              {clipboardConfig.enabled && (
                <span className="font-mono text-success">{translate(lang, 'footer.clipboardOn')}</span>
              )}
              <span className="font-mono text-text-muted">
                {translate(lang, 'footer.threadCount', { count: settings.defaultConnections })}
              </span>
              {speedLimitKbs > 0 && (
                <span className="text-warning font-mono">
                  {translate(lang, 'footer.limitSpeed', { limit: speedLimitKbs })}
                </span>
              )}
              <span className="font-mono text-success">{translate(lang, 'footer.engineReady')}</span>
            </div>
          </footer>
        </main>

      {/* S02: Add URL Modal Dialog */}
      <AddUrlModal
        isOpen={isAddUrlOpen}
        onClose={() => {
          setIsAddUrlOpen(false);
          setIncomingUrl(undefined);
          setIncomingMeta(undefined);
        }}
        onStartDownload={handleStartNewDownload}
        categories={dynamicCategories}
        categoryRules={categoryRules}
        proxyConfig={proxyConfig}
        siteLogins={siteLogins}
        initialUrl={incomingUrl}
        incomingMeta={incomingMeta}
        defaultConnections={settings.defaultConnections}
      />

      {/* S04: Active Download Segmented Progress Dialog */}
      <DownloadProgressModal
        task={activeDetailTask}
        isOpen={isProgressOpen}
        onClose={() => setIsProgressOpen(false)}
        onPause={handlePause}
        onResume={handleResume}
        onCancel={handleCancel}
        onOpenProperties={(task) => {
          setPropertiesTaskId(task.id);
          setPropertiesInitialTab('info');
          setIsPropertiesOpen(true);
        }}
      />

      {/* S05: Download Complete Dialog */}
      <DownloadCompleteModal
        task={activeDetailTask}
        isOpen={isCompleteOpen}
        onClose={() => setIsCompleteOpen(false)}
        onOpenFile={handleOpenFile}
        onOpenFolder={handleOpenFolder}
        onVerifyChecksum={(task) => {
          setPropertiesTaskId(task.id);
          setPropertiesInitialTab('checksum');
          setIsPropertiesOpen(true);
        }}
      />

      {/* S06: Speed Limiter & Settings Dialog */}
      <SpeedLimiterModal
        isOpen={isSettingsOpen}
        initialTab={settingsInitialTab}
        onClose={() => setIsSettingsOpen(false)}
        speedLimitKbs={speedLimitKbs}
        maxTasks={settings.maxTasks}
        defaultConnections={settings.defaultConnections}
        proxyConfig={proxyConfig}
        siteLogins={siteLogins}
        categoryRules={categoryRules}
        clipboardConfig={clipboardConfig}
        buildInfo={buildInfo}
        onChangeLanguage={handleNotifyLanguageChange}
        onSaveSpeedLimit={(limitKbs) => {
          setSpeedLimitKbs(limitKbs);
          void updateDownloadSpeedLimit(limitKbs);
        }}
        onSaveSettings={(newSettings) => {
          setSettings({
            speedLimitKbs: newSettings.speedLimitKbs,
            maxTasks: newSettings.maxTasks,
            defaultConnections: newSettings.defaultConnections,
          });
          if (newSettings.proxyConfig) setProxyConfig(newSettings.proxyConfig);
          if (newSettings.siteLogins) setSiteLogins(newSettings.siteLogins);
          if (newSettings.categoryRules) setCategoryRules(newSettings.categoryRules);
          if (newSettings.clipboardConfig) setClipboardConfig(newSettings.clipboardConfig);
          void updateDownloadSpeedLimit(newSettings.speedLimitKbs);
          showToast(
            translate(langRef.current, 'toast.settingsSaved', {
              threads: newSettings.defaultConnections,
              max: newSettings.maxTasks,
            }),
            'success'
          );
        }}
      />

      {/* Context Menu */}
      <TaskContextMenu
        task={contextMenuState?.task || null}
        position={contextMenuState?.position || null}
        onClose={() => setContextMenuState(null)}
        onResume={(id) => handleResume(id)}
        onPause={(id) => handlePause(id)}
        onRefreshAddress={(task) => {
          setPropertiesTaskId(task.id);
          setPropertiesInitialTab('info');
          setIsPropertiesOpen(true);
        }}
        onOpenFile={handleOpenFile}
        onOpenFolder={handleOpenFolder}
        onOpenProperties={(task, tab = 'info') => {
          setPropertiesTaskId(task.id);
          setPropertiesInitialTab(tab);
          setIsPropertiesOpen(true);
        }}
        onMovePriority={handleMovePriority}
        onCopyUrl={(copiedUrl) => {
          navigator.clipboard?.writeText(copiedUrl).catch(() => {});
          showToast(translate(langRef.current, 'toast.urlCopied'), 'info');
        }}
        onDelete={(id) => {
          setSelectedTaskId(id);
          const taskToDelete = tasks.find((t) => t.id === id);
          if (taskToDelete && taskToDelete.status !== 'completed') {
            void cancelRealDownload(id, getSafeCleanupPath(taskToDelete, new Set([id])));
          }
          setTasks((prev) => prev.filter((t) => t.id !== id));
          setMarkedTaskIds((prev) => prev.filter((x) => x !== id));
        }}
      />

      {/* Task Properties / Refresh Address / Rename / Checksum Modal */}
      <TaskPropertiesModal
        task={propertiesTask}
        isOpen={isPropertiesOpen}
        initialTab={propertiesInitialTab}
        onClose={() => setIsPropertiesOpen(false)}
        onRefreshUrlAndResume={handleRefreshUrlAndResume}
        onUpdateTaskSpeedLimit={handleUpdateTaskSpeedLimit}
        onUpdateTaskPath={handleUpdateTaskPath}
        onUpdateChecksums={handleUpdateChecksums}
        onNotify={showToast}
      />

      {/* Batch & Site Grabber & Browser Links Modal */}
      <BatchAndGrabberModal
        isOpen={isBatchOpen}
        initialTab={batchInitialTab}
        initialBatchText={batchInitialText}
        initialBrowserLinks={browserPageLinks}
        defaultHeaders={{
          proxyUrl: formatProxyUrl(proxyConfig),
        }}
        onClose={() => {
          setIsBatchOpen(false);
          setBatchInitialText('');
        }}
        onEnqueueBatch={handleEnqueueBatch}
      />

      {/* Scheduler & Priority Queue Modal */}
      <SchedulerModal
        isOpen={isSchedulerOpen}
        config={schedulerConfig}
        queuedAndActiveTasks={tasks.filter(
          (t) => t.status === 'queued' || t.status === 'paused' || t.status === 'downloading'
        )}
        onClose={() => setIsSchedulerOpen(false)}
        onSaveConfig={(nextCfg) => {
          setSchedulerConfig(nextCfg);
          showToast(translate(langRef.current, 'toast.schedulerSaved'), 'success');
        }}
        onMovePriority={handleMovePriority}
        onStartQueueNow={() => {
          setSchedulerConfig((prev) => ({ ...prev, enabled: false }));
          setTasks((prev) =>
            prev.map((t) => (t.status === 'paused' ? { ...t, status: 'queued' } : t))
          );
          showToast(translate(langRef.current, 'toast.startingQueueNow'), 'info');
        }}
      />

      {/* Floating Mini Drop Target */}
      <MiniDropTarget
        visible={isMiniDropOpen}
        activeCount={tasks.filter((t) => t.status === 'downloading').length}
        totalSpeedBytesPerSec={tasks.reduce(
          (acc, t) => acc + (t.status === 'downloading' ? t.speedBytesPerSec : 0),
          0
        )}
        onClose={() => setIsMiniDropOpen(false)}
        onDropUrls={handleDropUrls}
      />

      {/* Post-Download Sleep / Quit Cancelable Countdown Guard */}
      {postActionCountdown && (
        <div className="fixed top-4 right-4 z-50 bg-surface border border-warning rounded-lg shadow-pop p-3 text-xs space-y-2 max-w-xs">
          <p className="font-bold text-warning">
            {postActionCountdown.action === 'sleep'
              ? translate(lang, 'postAction.sleeping', {
                  sec: postActionCountdown.secondsLeft,
                })
              : translate(lang, 'postAction.quitting', {
                  sec: postActionCountdown.secondsLeft,
                })}
          </p>
          <p className="text-[11px] text-text-muted">
            {translate(lang, 'postAction.allDone')}
          </p>
          <button
            type="button"
            onClick={() => setPostActionCountdown(null)}
            className="w-full py-1.5 rounded bg-danger text-white font-semibold hover:opacity-90"
          >
            {translate(lang, 'postAction.cancel')}
          </button>
        </div>
      )}
      {/* Toast Notification Banner */}
      {toast && (
        <div
          className={`fixed bottom-8 right-6 z-50 max-w-md px-4 py-3 rounded-lg shadow-pop border text-xs font-medium flex items-center space-x-2.5 animate-in fade-in slide-in-from-bottom-2 duration-150 ${
            toast.type === 'warning'
              ? 'bg-surface border-warning/50 text-warning shadow-warning/5'
              : toast.type === 'success'
              ? 'bg-surface border-success/50 text-success shadow-success/5'
              : 'bg-surface border-border text-text'
          }`}
        >
          <span className="w-2 h-2 rounded-full shrink-0 bg-accent animate-pulse" />
          <span className="leading-snug">{toast.message}</span>
        </div>
      )}
      </div>
    </I18nProvider>
  );
};

export default App;
