import React, { useEffect, useMemo, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import {
  AlertCircle,
  CheckCircle2,
  Clock,
  Download,
  ExternalLink,
  FileCheck2,
  Film,
  FolderOpen,
  HardDrive,
  Link2,
  Loader2,
  Magnet,
  Minus,
  Music,
  Pause,
  Play,
  RotateCw,
  Sparkles,
  Square,
  Upload,
  X,
  Zap,
} from 'lucide-react';
import {
  CategoryRule,
  DownloadSegment,
  PopupPrebufferProgressPayload,
  PopupPrebufferState,
  PopupPromoteParams,
  PopupWindowSession,
  ProxyConfig,
  SiteLogin,
  TaskStatus,
} from '../types.ts';
import {
  cancelPopupDownload,
  checkDiskSpace,
  closePopupWindow,
  getPopupSession,
  inspectTorrentBytes,
  isTauri,
  minimizePopupWindow,
  openMainWindow,
  openNativeFile,
  openNativeFolder,
  pauseRealDownload,
  promotePopupDownload,
  queuePopupLater,
  readClipboardText,
  startPopupPreDownload,
  triggerRealDownload,
} from '../services/nativeBridge.ts';
import {
  DEFAULT_CATEGORY_RULES,
  DEFAULT_PROXY_CONFIG,
  deriveFileNameFromUrl,
  findMatchingSiteLogin,
  formatDiskBytes,
  formatProxyUrl,
  getParentDir,
  isSupportedDownloadUrl,
  isTorrentReference,
  joinDestinationPath,
  matchCategoryForFile,
} from '../utils/downloadUtils.ts';
import { formatEta } from '../utils/formatters.ts';
import { SegmentedProgressBar } from './SegmentedProgressBar.tsx';
import { SpeedGraphCanvas } from './SpeedGraphCanvas.tsx';
import { useI18n } from '../i18n/index.tsx';

interface PopupDownloadWindowProps {
  initialSession?: PopupWindowSession | null;
  popupIdOverride?: string;
}

interface MiniTaskState {
  taskId: string;
  url: string;
  fileName: string;
  destinationPath: string;
  categoryId: string;
  numConnections: number;
  formatId?: string | null;
  resolution?: string | null;
  cookie?: string | null;
  referer?: string | null;
  userAgent?: string | null;
  proxyUrl?: string | null;
  authUsername?: string | null;
  authPassword?: string | null;
  speedLimitKbs: number;
  downloadedBytes: number;
  fileSize: number | null;
  speedBytesPerSec: number;
  progressPercent: number;
  etaSeconds?: number | null;
  status: TaskStatus;
  segments?: DownloadSegment[];
  errorMessage?: string;
  peers?: number;
  seeds?: number;
}

function loadSavedCategoryRules(): CategoryRule[] {
  try {
    const raw = localStorage.getItem('download_cepat_category_rules');
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed) && parsed.length > 0) {
        const hasTorrents = parsed.some((r: CategoryRule) => r?.id === 'torrents');
        if (hasTorrents) return parsed;
        const defaultTorrents = DEFAULT_CATEGORY_RULES.find((r) => r.id === 'torrents');
        return defaultTorrents ? [...parsed, defaultTorrents] : parsed;
      }
    }
  } catch {}
  return DEFAULT_CATEGORY_RULES;
}

function loadSavedProxyConfig(): ProxyConfig {
  try {
    const raw = localStorage.getItem('download_cepat_proxy');
    if (raw) return JSON.parse(raw);
  } catch {}
  return DEFAULT_PROXY_CONFIG;
}

function loadSavedSiteLogins(): SiteLogin[] {
  try {
    const raw = localStorage.getItem('download_cepat_logins');
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) return parsed;
    }
  } catch {}
  return [];
}

function loadSavedSpeedLimitKbs(): number {
  try {
    const raw = localStorage.getItem('download_cepat_settings');
    if (raw) {
      const parsed = JSON.parse(raw);
      const n = Number(parsed?.speedLimitKbs);
      return Number.isFinite(n) && n > 0 ? Math.round(n) : 0;
    }
  } catch {}
  return 0;
}

export const PopupDownloadWindow: React.FC<PopupDownloadWindowProps> = ({
  initialSession,
  popupIdOverride,
}) => {
  const { t, lang } = useI18n();

  const resolvedPopupId = useMemo(() => {
    if (popupIdOverride) return popupIdOverride;
    if (initialSession?.popupId) return initialSession.popupId;
    if (typeof window !== 'undefined') {
      return new URLSearchParams(window.location.search).get('popupId') || '';
    }
    return '';
  }, [popupIdOverride, initialSession?.popupId]);

  const [session, setSession] = useState<PopupWindowSession | null>(initialSession ?? null);
  const [loading, setLoading] = useState<boolean>(!initialSession);
  const [categoryRules, setCategoryRules] = useState<CategoryRule[]>(() => loadSavedCategoryRules());

  // Editable confirm state
  const [mode, setMode] = useState<'confirm' | 'mini_progress'>(() =>
    initialSession?.mode === 'mini_progress' ? 'mini_progress' : 'confirm'
  );
  const [url, setUrl] = useState<string>(initialSession?.url || '');
  const [fileName, setFileName] = useState<string>(initialSession?.fileName || '');
  const [categoryId, setCategoryId] = useState<string>(initialSession?.categoryId || 'general');
  const [saveDir, setSaveDir] = useState<string>(() =>
    initialSession?.destinationPath ? getParentDir(initialSession.destinationPath) : '~/Downloads'
  );
  const [numConnections, setNumConnections] = useState<number>(
    initialSession?.numConnections || 8
  );
  const [selectedFormatId, setSelectedFormatId] = useState<string>(
    initialSession?.formatId || ''
  );
  const [selectedResolution, setSelectedResolution] = useState<string | undefined>(
    initialSession?.resolution || undefined
  );
  const [prebuffer, setPrebuffer] = useState<PopupPrebufferState>(
    initialSession?.prebuffer || {
      status: 'idle',
      runId: 0,
      url: '',
      downloadedBytes: 0,
      totalBytes: null,
      speedBytesPerSec: 0,
      progressPercent: 0,
      supportsRange: false,
    }
  );
  const [freeDiskBytes, setFreeDiskBytes] = useState<number | null>(null);
  const [isPromoting, setIsPromoting] = useState(false);
  const [isQueuingLater, setIsQueuingLater] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);

  const [miniTask, setMiniTask] = useState<MiniTaskState | null>(() => {
    if (!initialSession || initialSession.mode !== 'mini_progress') return null;
    const isCompleted = initialSession.prebuffer?.status === 'completed';
    return {
      taskId: initialSession.taskId || initialSession.popupId,
      url: initialSession.url,
      fileName: initialSession.fileName,
      destinationPath: initialSession.destinationPath,
      categoryId: initialSession.categoryId || 'general',
      numConnections: initialSession.numConnections || 8,
      formatId: initialSession.formatId,
      resolution: initialSession.resolution,
      cookie: initialSession.cookie,
      referer: initialSession.referer,
      userAgent: initialSession.userAgent,
      proxyUrl: initialSession.proxyUrl,
      authUsername: initialSession.authUsername,
      authPassword: initialSession.authPassword,
      speedLimitKbs: initialSession.speedLimitBytesPerSec
        ? Math.round(initialSession.speedLimitBytesPerSec / 1024)
        : loadSavedSpeedLimitKbs(),
      downloadedBytes: initialSession.prebuffer?.downloadedBytes || 0,
      fileSize: initialSession.fileSize ?? initialSession.prebuffer?.totalBytes ?? null,
      speedBytesPerSec: 0,
      progressPercent: isCompleted ? 100 : 0,
      etaSeconds: isCompleted ? 0 : null,
      status: isCompleted ? 'completed' : 'downloading',
    };
  });

  const userEditedNameRef = useRef(false);
  const userEditedCatRef = useRef(false);
  const lastProbedUrlRef = useRef<string>(initialSession?.url || '');
  const torrentFileInputRef = useRef<HTMLInputElement | null>(null);
  const taskIdRef = useRef<string | null>(initialSession?.taskId || null);
  const earlyTelemetryMapRef = useRef<Map<string, any>>(new Map());

  const applyCategoryDefaults = React.useCallback(
    (nextCatId: string, rules: CategoryRule[]) => {
      setCategoryId(nextCatId);
      const found = rules.find((r) => r.id === nextCatId);
      if (found?.saveDir) {
        setSaveDir(found.saveDir);
      } else if (nextCatId === 'general') {
        setSaveDir('~/Downloads');
      } else {
        setSaveDir(`~/Downloads/${nextCatId.charAt(0).toUpperCase()}${nextCatId.slice(1)}`);
      }
    },
    []
  );

  const handleUrlInputChange = React.useCallback(
    (nextUrl: string) => {
      setUrl(nextUrl);
      setLocalError(null);
      const trimmed = nextUrl.trim();
      if (!isSupportedDownloadUrl(trimmed)) return;
      const derived = deriveFileNameFromUrl(trimmed);
      if (!userEditedNameRef.current) {
        setFileName(derived);
      }
      if (!userEditedCatRef.current) {
        const rules = loadSavedCategoryRules();
        setCategoryRules(rules);
        const matched = matchCategoryForFile(derived, rules, trimmed);
        applyCategoryDefaults(matched.categoryId, rules);
      }
    },
    [applyCategoryDefaults]
  );

  // Sync categoryRules if edited in main window
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === 'download_cepat_category_rules') {
        setCategoryRules(loadSavedCategoryRules());
      }
    };
    window.addEventListener('storage', onStorage);
    return () => window.removeEventListener('storage', onStorage);
  }, []);

  // Load initial popup session from Rust
  useEffect(() => {
    if (initialSession || !resolvedPopupId) {
      setLoading(false);
      return;
    }
    let mounted = true;
    getPopupSession(resolvedPopupId)
      .then((loaded) => {
        if (!mounted) return;
        setSession(loaded);
        if (loaded) {
          const initialModeVal = loaded.mode === 'mini_progress' ? 'mini_progress' : 'confirm';
          setMode(initialModeVal);
          setUrl(loaded.url || '');
          lastProbedUrlRef.current = loaded.url || '';
          setFileName(loaded.fileName || '');
          const loadedRules = loadSavedCategoryRules();
          setCategoryRules(loadedRules);
          const initialCat = loaded.categoryId || 'general';
          setCategoryId(initialCat);
          setSaveDir(
            loaded.destinationPath
              ? getParentDir(loaded.destinationPath)
              : loadedRules.find((r) => r.id === initialCat)?.saveDir || '~/Downloads'
          );
          setNumConnections(loaded.numConnections || 8);
          setSelectedFormatId(loaded.formatId || '');
          setSelectedResolution(loaded.resolution || undefined);
          setPrebuffer(loaded.prebuffer);
          if (loaded.prebuffer?.mediaInfo?.formats?.length && !loaded.formatId) {
            setSelectedFormatId(loaded.prebuffer.mediaInfo.formats[0].formatId);
            setSelectedResolution(loaded.prebuffer.mediaInfo.formats[0].resolution);
          }
          taskIdRef.current = loaded.taskId || null;

          if (initialModeVal === 'mini_progress') {
            const isComp = loaded.prebuffer?.status === 'completed';
            const isQueuedIdle = loaded.prebuffer?.status === 'idle';
            const early = loaded.taskId ? earlyTelemetryMapRef.current.get(loaded.taskId) : undefined;
            setMiniTask({
              taskId: loaded.taskId || loaded.popupId,
              url: loaded.url,
              fileName: loaded.fileName,
              destinationPath: loaded.destinationPath,
              categoryId: initialCat,
              numConnections: loaded.numConnections || 8,
              formatId: loaded.formatId,
              resolution: loaded.resolution,
              cookie: loaded.cookie,
              referer: loaded.referer,
              userAgent: loaded.userAgent,
              proxyUrl: loaded.proxyUrl,
              authUsername: loaded.authUsername,
              authPassword: loaded.authPassword,
              speedLimitKbs: loaded.speedLimitBytesPerSec
                ? Math.round(loaded.speedLimitBytesPerSec / 1024)
                : loadSavedSpeedLimitKbs(),
              downloadedBytes: early?.downloaded_bytes ?? loaded.prebuffer?.downloadedBytes ?? 0,
              fileSize:
                (early?.total_bytes > 0 ? early.total_bytes : null) ??
                loaded.fileSize ??
                loaded.prebuffer?.totalBytes ??
                null,
              speedBytesPerSec: early?.speed_bytes_per_sec ?? 0,
              progressPercent: early?.progress_percent ?? (isComp ? 100 : 0),
              etaSeconds: early?.eta_seconds ?? (isComp ? 0 : null),
              status:
                early?.status === 'completed' || isComp
                  ? 'completed'
                  : early?.status === 'error'
                  ? 'error'
                  : early?.status === 'paused'
                  ? 'paused'
                  : isQueuedIdle
                  ? 'queued'
                  : 'downloading',
              segments: early?.segments,
              errorMessage: early?.error,
              peers: early?.peers,
              seeds: early?.seeds,
            });
          } else if (!loaded.url && loaded.initialMode !== 'torrent') {
            readClipboardText()
              .then((clip) => {
                if (mounted && clip && isSupportedDownloadUrl(clip.trim())) {
                  handleUrlInputChange(clip.trim());
                }
              })
              .catch(() => {});
          }
        }
      })
      .finally(() => {
        if (mounted) setLoading(false);
      });
    return () => {
      mounted = false;
    };
  }, [initialSession, resolvedPopupId, handleUrlInputChange]);

  // Check disk free space when saveDir changes
  useEffect(() => {
    if (!saveDir) return;
    checkDiskSpace(saveDir).then(setFreeDiskBytes).catch(() => {});
  }, [saveDir]);

  // Keep browser document.title synced with popup mode/status
  useEffect(() => {
    if (typeof document === 'undefined') return;
    if (mode === 'confirm') {
      document.title = fileName
        ? `Download - ${fileName}`
        : session?.initialMode === 'torrent'
        ? t('toolbar.addTorrentTitle')
        : t('popup.confirmTitle');
      return;
    }
    const name = miniTask?.fileName || fileName || 'Download';
    if (miniTask?.status === 'completed') {
      document.title = t('popup.miniTitleComplete');
    } else if (miniTask?.status === 'paused') {
      document.title = t('popup.miniTitlePaused', { name });
    } else if (miniTask?.status === 'error') {
      document.title = t('popup.miniTitleError', { name });
    } else {
      document.title = t('popup.miniTitleDownloading', { name });
    }
  }, [mode, fileName, miniTask?.fileName, miniTask?.status, session?.initialMode, t]);

  // Subscribe to prebuffer & download telemetry events from Tauri runtime
  useEffect(() => {
    if (!isTauri() || !resolvedPopupId) return;

    let unlistenPre: (() => void) | undefined;
    let unlistenTelemetry: (() => void) | undefined;

    listen<PopupPrebufferProgressPayload>('popup-prebuffer-progress', (event) => {
      const payload = event.payload;
      if (!payload || payload.popupId !== resolvedPopupId) return;
      const st = payload.state;
      setPrebuffer(st);
      setLocalError(null);

      if (!userEditedNameRef.current && st.fileName && st.fileName.trim()) {
        setFileName(st.fileName.trim());
      }

      if (!userEditedCatRef.current) {
        const currentRules = loadSavedCategoryRules();
        setCategoryRules(currentRules);
        const derivedName = st.fileName || deriveFileNameFromUrl(st.url || '');
        const ruleMatch = matchCategoryForFile(derivedName, currentRules, st.url);
        const nextCat =
          ruleMatch.categoryId !== 'general'
            ? ruleMatch.categoryId
            : st.suggestedCategory || 'general';
        applyCategoryDefaults(nextCat, currentRules);
      }

      if (st.mediaInfo?.formats?.length) {
        setSelectedFormatId((prev) => {
          const valid = st.mediaInfo!.formats.some((f) => f.formatId === prev);
          if (valid) return prev;
          setSelectedResolution(st.mediaInfo!.formats[0].resolution);
          return st.mediaInfo!.formats[0].formatId;
        });
      }
    })
      .then((u) => {
        unlistenPre = u;
      })
      .catch(() => {});

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
      peers?: number;
      seeds?: number;
    }>('download-telemetry', (event) => {
      const p = event.payload;
      if (!p || !p.task_id) return;

      earlyTelemetryMapRef.current.set(p.task_id, p);
      if (earlyTelemetryMapRef.current.size > 30) {
        const firstKey = earlyTelemetryMapRef.current.keys().next().value;
        if (firstKey) earlyTelemetryMapRef.current.delete(firstKey);
      }

      if (taskIdRef.current !== p.task_id) return;

      const isCompleted = p.status === 'completed';
      const isError = p.status === 'error';
      const isPaused = p.status === 'paused';
      const isCanceled = p.status === 'canceled';
      const isAssembling = p.status === 'assembling';
      const isConnecting = p.status === 'connecting';
      const isStopped = isCompleted || isError || isPaused || isCanceled;

      setMiniTask((prev) => {
        if (!prev) return prev;
        if (prev.status === 'canceled' && !isCanceled) return prev;
        const nextStatus: TaskStatus = isCompleted
          ? 'completed'
          : isError
          ? 'error'
          : isPaused
          ? 'paused'
          : isCanceled
          ? 'canceled'
          : isAssembling
          ? 'assembling'
          : isConnecting
          ? 'connecting'
          : 'downloading';
        const nextDest = p.destination_path || prev.destinationPath;
        const baseName = nextDest.split(/[\\/]/).pop();
        const nextName = baseName && nextDest !== prev.destinationPath ? baseName : prev.fileName;
        const nextDownloaded =
          isError || isCanceled
            ? 0
            : isPaused
            ? p.downloaded_bytes > 0
              ? p.downloaded_bytes
              : prev.downloadedBytes
            : p.downloaded_bytes;
        const nextTotal =
          p.total_bytes > 0
            ? p.total_bytes
            : isCompleted
            ? p.downloaded_bytes
            : prev.fileSize;
        const nextSpeed = isStopped ? 0 : p.speed_bytes_per_sec;
        const nextEta = isStopped
          ? 0
          : typeof p.eta_seconds === 'number' && p.eta_seconds > 0
          ? p.eta_seconds
          : nextTotal && nextTotal > nextDownloaded && nextSpeed > 0
          ? Math.ceil((nextTotal - nextDownloaded) / nextSpeed)
          : prev.etaSeconds;
        const nextSegments = isCanceled
          ? []
          : Array.isArray(p.segments) && p.segments.length > 0
          ? p.segments
          : isCompleted && prev.segments && prev.segments.length > 0
          ? prev.segments.map((seg) => {
              const segTotal = Math.max(1, seg.endByte - seg.startByte + 1);
              return {
                ...seg,
                downloadedBytes: segTotal,
                currentByte: seg.startByte + segTotal,
                isCompleted: true,
              };
            })
          : prev.segments;

        return {
          ...prev,
          fileName: nextName,
          destinationPath: nextDest,
          downloadedBytes: isError ? prev.downloadedBytes : nextDownloaded,
          fileSize: nextTotal,
          speedBytesPerSec: nextSpeed,
          etaSeconds: nextEta,
          progressPercent:
            isError || isCanceled
              ? 0
              : isPaused
              ? p.progress_percent > 0
                ? p.progress_percent
                : prev.progressPercent
              : p.progress_percent,
          status: nextStatus,
          segments: nextSegments,
          errorMessage: isError ? p.error || t('toast.downloadError') : undefined,
          peers: typeof p.peers === 'number' ? p.peers : prev.peers,
          seeds: typeof p.seeds === 'number' ? p.seeds : prev.seeds,
        };
      });
    })
      .then((u) => {
        unlistenTelemetry = u;
      })
      .catch(() => {});

    return () => {
      if (unlistenPre) unlistenPre();
      if (unlistenTelemetry) unlistenTelemetry();
    };
  }, [resolvedPopupId, applyCategoryDefaults, t]);

  // Debounced URL pre-check when user types/pastes a new URL in confirm mode
  useEffect(() => {
    if (mode !== 'confirm' || !resolvedPopupId) return;
    const trimmed = url.trim();
    if (!trimmed || trimmed === lastProbedUrlRef.current) return;
    const timer = setTimeout(() => {
      lastProbedUrlRef.current = trimmed;
      void startPopupPreDownload({
        popupId: resolvedPopupId,
        url: trimmed,
        cookie: session?.cookie ?? null,
        referer: session?.referer ?? null,
        userAgent: session?.userAgent ?? null,
        force: true,
      });
    }, 350);
    return () => clearTimeout(timer);
  }, [url, mode, resolvedPopupId, session?.cookie, session?.referer, session?.userAgent]);

  const handleRecheckUrl = () => {
    const trimmed = url.trim();
    if (!trimmed || !resolvedPopupId) return;
    lastProbedUrlRef.current = trimmed;
    setLocalError(null);
    void startPopupPreDownload({
      popupId: resolvedPopupId,
      url: trimmed,
      cookie: session?.cookie ?? null,
      referer: session?.referer ?? null,
      userAgent: session?.userAgent ?? null,
      force: true,
    });
  };

  const handleTorrentFilePick = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) return;
    setLocalError(null);
    try {
      const buf = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buf));
      const res = await inspectTorrentBytes(bytes);
      if (!res) return;
      userEditedNameRef.current = false;
      userEditedCatRef.current = false;
      setUrl(res.url);
      lastProbedUrlRef.current = res.url;
      setFileName(res.fileName || file.name.replace(/\.torrent$/i, ''));
      const currentRules = loadSavedCategoryRules();
      setCategoryRules(currentRules);
      applyCategoryDefaults('torrents', currentRules);
      setPrebuffer((prev) => ({
        ...prev,
        status: 'skipped',
        url: res.url,
        fileName: res.fileName,
        totalBytes: res.fileSize,
        supportsRange: true,
        suggestedCategory: 'torrents',
        mimeType: res.mimeType || 'application/x-bittorrent',
        torrentInfo: res.torrentInfo || null,
        mediaInfo: null,
        error: null,
      }));
      if (resolvedPopupId) {
        void startPopupPreDownload({
          popupId: resolvedPopupId,
          url: res.url,
          force: true,
        });
      }
    } catch (err) {
      setLocalError(String(err));
    }
  };

  const handleSelectFormat = (formatId: string) => {
    setSelectedFormatId(formatId);
    const formats = prebuffer.mediaInfo?.formats || [];
    const fmt = formats.find((f) => f.formatId === formatId);
    if (!fmt) return;
    setSelectedResolution(fmt.resolution);

    if (!userEditedNameRef.current && fileName) {
      const dotIdx = fileName.lastIndexOf('.');
      const stem = dotIdx > 0 ? fileName.slice(0, dotIdx) : fileName;
      setFileName(`${stem}.${fmt.extension}`);
    }

    if (!userEditedCatRef.current) {
      const nextCat = fmt.isAudioOnly ? 'music' : 'video';
      applyCategoryDefaults(nextCat, categoryRules);
    }
  };

  const buildPromoteParams = (): PopupPromoteParams => {
    const cleanUrl = url.trim();
    const resolvedName = (fileName || deriveFileNameFromUrl(cleanUrl) || 'download.bin').trim();
    const resolvedDest = joinDestinationPath(saveDir || '~/Downloads', resolvedName);
    const activeProxy = formatProxyUrl(loadSavedProxyConfig()) || session?.proxyUrl || null;
    const matchedLogin = findMatchingSiteLogin(cleanUrl, loadSavedSiteLogins());
    const speedLimitKbs = loadSavedSpeedLimitKbs();
    const currentFmt = prebuffer.mediaInfo?.formats?.find((f) => f.formatId === selectedFormatId);

    return {
      popupId: resolvedPopupId,
      url: cleanUrl,
      fileName: resolvedName,
      destinationPath: resolvedDest,
      categoryId,
      numConnections,
      formatId: prebuffer.mediaInfo ? selectedFormatId || currentFmt?.formatId || null : null,
      resolution: prebuffer.mediaInfo
        ? selectedResolution || currentFmt?.resolution || null
        : session?.resolution ?? null,
      cookie: session?.cookie ?? null,
      referer: session?.referer ?? null,
      userAgent: session?.userAgent ?? null,
      proxyUrl: activeProxy,
      authUsername: matchedLogin?.username || session?.authUsername || null,
      authPassword: matchedLogin?.password || session?.authPassword || null,
      speedLimitKbs: speedLimitKbs > 0 ? speedLimitKbs : null,
      fileSize:
        currentFmt?.fileSizeApprox ??
        prebuffer.totalBytes ??
        prebuffer.torrentInfo?.totalSize ??
        session?.fileSize ??
        null,
      mimeType: prebuffer.mimeType ?? session?.mimeType ?? null,
    };
  };

  const handleStartDownload = async () => {
    const cleanUrl = url.trim();
    if (!cleanUrl || isPromoting) return;
    setIsPromoting(true);
    setLocalError(null);
    try {
      const params = buildPromoteParams();
      const updated = await promotePopupDownload(params);
      setSession(updated);
      taskIdRef.current = updated.taskId || resolvedPopupId;

      const isAlreadyDone = updated.prebuffer?.status === 'completed';
      const early = updated.taskId ? earlyTelemetryMapRef.current.get(updated.taskId) : undefined;
      const totalBytes =
        (early?.total_bytes > 0 ? early.total_bytes : null) ??
        updated.fileSize ??
        updated.prebuffer?.totalBytes ??
        params.fileSize ??
        null;
      const dlBytes =
        early?.downloaded_bytes ??
        updated.prebuffer?.downloadedBytes ??
        (isAlreadyDone && totalBytes ? totalBytes : 0);

      setMiniTask({
        taskId: updated.taskId || resolvedPopupId,
        url: updated.url,
        fileName: updated.fileName,
        destinationPath: updated.destinationPath,
        categoryId: updated.categoryId,
        numConnections: updated.numConnections,
        formatId: updated.formatId,
        resolution: updated.resolution,
        cookie: updated.cookie,
        referer: updated.referer,
        userAgent: updated.userAgent,
        proxyUrl: updated.proxyUrl,
        authUsername: updated.authUsername,
        authPassword: updated.authPassword,
        speedLimitKbs: params.speedLimitKbs || 0,
        downloadedBytes: dlBytes,
        fileSize: totalBytes,
        speedBytesPerSec: early?.speed_bytes_per_sec ?? 0,
        progressPercent:
          early?.progress_percent ?? (isAlreadyDone || early?.status === 'completed' ? 100 : 0),
        etaSeconds: early?.eta_seconds ?? (isAlreadyDone ? 0 : null),
        status:
          early?.status === 'completed' || isAlreadyDone
            ? 'completed'
            : early?.status === 'error'
            ? 'error'
            : 'downloading',
        segments: early?.segments,
        errorMessage: early?.error,
        peers: early?.peers,
        seeds: early?.seeds,
      });
      setMode('mini_progress');
    } catch (err) {
      setLocalError(String(err));
    } finally {
      setIsPromoting(false);
    }
  };

  const handleDownloadLater = async () => {
    const cleanUrl = url.trim();
    if (!cleanUrl || isQueuingLater) return;
    setIsQueuingLater(true);
    setLocalError(null);
    try {
      await queuePopupLater(buildPromoteParams());
    } catch (err) {
      setLocalError(String(err));
    } finally {
      setIsQueuingLater(false);
    }
  };

  const handleCancelConfirm = async () => {
    if (resolvedPopupId) {
      await cancelPopupDownload(resolvedPopupId);
    }
  };

  const handleMiniPauseOrResume = async () => {
    if (!miniTask) return;
    if (miniTask.status === 'paused' || miniTask.status === 'error' || miniTask.status === 'queued') {
      setMiniTask((prev) =>
        prev ? { ...prev, status: 'downloading', errorMessage: undefined } : prev
      );
      await triggerRealDownload(
        miniTask.taskId,
        miniTask.url,
        miniTask.destinationPath,
        miniTask.numConnections,
        miniTask.formatId || undefined,
        {
          cookie: miniTask.cookie || undefined,
          referer: miniTask.referer || undefined,
          userAgent: miniTask.userAgent || undefined,
          proxyUrl: miniTask.proxyUrl || undefined,
          authUsername: miniTask.authUsername || undefined,
          authPassword: miniTask.authPassword || undefined,
        },
        miniTask.speedLimitKbs > 0 ? miniTask.speedLimitKbs : undefined,
        true
      );
    } else {
      await pauseRealDownload(miniTask.taskId);
      setMiniTask((prev) =>
        prev ? { ...prev, status: 'paused', speedBytesPerSec: 0, etaSeconds: 0 } : prev
      );
    }
  };

  const handleMiniCancel = async () => {
    if (resolvedPopupId) {
      await cancelPopupDownload(resolvedPopupId);
    }
  };

  const getCategoryLabel = (rule: CategoryRule) => {
    switch (rule.id) {
      case 'compressed':
        return t('sidebar.catCompressed');
      case 'documents':
        return t('sidebar.catDocuments');
      case 'music':
        return t('sidebar.catMusic');
      case 'programs':
        return t('sidebar.catPrograms');
      case 'video':
        return t('sidebar.catVideo');
      case 'torrents':
        return t('sidebar.catTorrents');
      default:
        return rule.name;
    }
  };

  if (loading) {
    return (
      <div className="h-screen w-screen bg-bg text-text flex items-center justify-center">
        <Loader2 className="w-6 h-6 text-accent animate-spin" />
      </div>
    );
  }

  if (!session && !initialSession) {
    return (
      <div className="h-screen w-screen bg-bg text-text flex flex-col items-center justify-center p-6 text-center space-y-3 select-none">
        <AlertCircle className="w-8 h-8 text-warning" />
        <p className="text-xs text-text-muted">Popup session unavailable or expired.</p>
        <button
          type="button"
          onClick={() => void closePopupWindow(resolvedPopupId)}
          className="px-4 py-1.5 rounded-md bg-accent text-on-accent text-xs font-semibold"
        >
          {t('common.close')}
        </button>
      </div>
    );
  }

  // ==================== MODE B: MINI PROGRESS WINDOW ====================
  if (mode === 'mini_progress' && miniTask) {
    const isCompleted = miniTask.status === 'completed';
    const isPaused = miniTask.status === 'paused' || miniTask.status === 'queued';
    const isError = miniTask.status === 'error';
    const isTorrent =
      miniTask.categoryId === 'torrents' ||
      isTorrentReference(miniTask.url) ||
      typeof miniTask.peers === 'number' ||
      typeof miniTask.seeds === 'number';

    return (
      <div className="h-screen w-screen bg-bg text-text flex flex-col overflow-hidden select-none">
        {/* Draggable Window Title Bar */}
        <header
          data-tauri-drag-region
          className="flex items-center justify-between px-3.5 py-2.5 bg-surface border-b border-border shrink-0"
        >
          <div data-tauri-drag-region className="flex items-center space-x-2 min-w-0">
            {isCompleted ? (
              <CheckCircle2 className="w-4 h-4 text-success shrink-0" />
            ) : isTorrent ? (
              <Magnet className="w-4 h-4 text-accent shrink-0" />
            ) : (
              <Zap className="w-4 h-4 text-accent shrink-0" />
            )}
            <span
              data-tauri-drag-region
              className="text-xs font-bold text-text truncate"
              title={miniTask.fileName}
            >
              {miniTask.fileName}
            </span>
            {miniTask.resolution && (
              <span className="px-1.5 py-0.5 rounded bg-accent/15 text-accent text-[10px] font-mono font-semibold shrink-0">
                {miniTask.resolution}
              </span>
            )}
          </div>
          <div className="flex items-center space-x-1 shrink-0">
            <button
              type="button"
              onClick={() => void minimizePopupWindow(resolvedPopupId)}
              title={t('popup.minimizeBtn')}
              aria-label={t('popup.minimizeBtn')}
              className="p-1 rounded hover:bg-surface-hover text-text-muted hover:text-text transition-colors"
            >
              <Minus className="w-3.5 h-3.5" />
            </button>
            <button
              type="button"
              onClick={() => void closePopupWindow(resolvedPopupId)}
              title={t('common.close')}
              aria-label={t('common.close')}
              className="p-1 rounded hover:bg-surface-hover text-text-muted hover:text-text transition-colors"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        </header>

        {/* Progress Body */}
        <div className="flex-1 overflow-y-auto p-3.5 space-y-3 text-xs">
          {/* Torrent swarm peers badge if applicable */}
          {isTorrent && (
            <div className="flex items-center justify-between px-2.5 py-1 rounded bg-surface border border-border text-[11px]">
              <span className="inline-flex items-center space-x-1.5 text-accent font-medium">
                <Magnet className="w-3.5 h-3.5" />
                <span>{t('sidebar.catTorrents')}</span>
              </span>
              <span className="font-mono text-text-muted">
                {t('torrent.peersSeeds', {
                  peers: miniTask.peers ?? 0,
                  seeds: miniTask.seeds ?? 0,
                })}
              </span>
            </div>
          )}

          {/* 3 Metric Highlights */}
          <div className="grid grid-cols-3 gap-2 p-2 bg-surface rounded-md border border-border text-center">
            <div>
              <p className="text-[10px] text-text-muted uppercase font-semibold">
                {t('progress.speed')}
              </p>
              <p className="font-mono text-xs font-bold text-accent">
                {(miniTask.speedBytesPerSec / (1024 * 1024)).toFixed(2)} MB/s
              </p>
            </div>
            <div>
              <p className="text-[10px] text-text-muted uppercase font-semibold">
                {t('progress.downloaded')}
              </p>
              <p className="font-mono text-xs font-bold text-text truncate">
                {formatDiskBytes(miniTask.downloadedBytes, '0 B')} /{' '}
                {formatDiskBytes(miniTask.fileSize, t('common.unknown'))}
              </p>
            </div>
            <div>
              <p className="text-[10px] text-text-muted uppercase font-semibold">
                {t('progress.remaining')}
              </p>
              <p className="font-mono text-xs font-bold text-text">
                {isCompleted ? '0s' : formatEta(miniTask.etaSeconds, false, lang)}
              </p>
            </div>
          </div>

          <SegmentedProgressBar
            progressPercent={miniTask.progressPercent}
            status={miniTask.status}
            variant="detailed"
            numConnections={miniTask.numConnections || 8}
            segments={miniTask.segments}
          />

          <SpeedGraphCanvas currentSpeedBytesPerSec={miniTask.speedBytesPerSec} height={64} />

          {isError && miniTask.errorMessage && (
            <div className="p-2 rounded bg-danger/10 border border-danger/40 text-red-300 flex items-start space-x-2 text-[11px]">
              <AlertCircle className="w-4 h-4 text-danger shrink-0 mt-0.5" />
              <span className="leading-snug">{miniTask.errorMessage}</span>
            </div>
          )}

          <div className="text-[10px] text-text-muted flex justify-between gap-2 pt-1 border-t border-border/60">
            <span className="shrink-0">{t('progress.savePath')}</span>
            <span className="font-mono text-text truncate" title={miniTask.destinationPath}>
              {miniTask.destinationPath}
            </span>
          </div>
        </div>

        {/* Footer Controls */}
        <footer className="px-3.5 py-2.5 bg-surface border-t border-border flex items-center justify-between gap-2 shrink-0">
          <div className="flex items-center space-x-1.5">
            {isCompleted ? (
              <>
                <button
                  type="button"
                  onClick={() => void openNativeFile(miniTask.destinationPath)}
                  className="flex items-center space-x-1 px-2.5 py-1.5 rounded-md bg-success text-white font-semibold text-xs hover:opacity-90"
                >
                  <FileCheck2 className="w-3.5 h-3.5" />
                  <span>{t('common.openFile')}</span>
                </button>
                <button
                  type="button"
                  onClick={() => void openNativeFolder(miniTask.destinationPath)}
                  className="flex items-center space-x-1 px-2.5 py-1.5 rounded-md bg-bg border border-border text-text hover:bg-surface-hover text-xs font-medium"
                >
                  <FolderOpen className="w-3.5 h-3.5" />
                  <span>{t('common.openFolder')}</span>
                </button>
              </>
            ) : (
              <>
                <button
                  type="button"
                  onClick={() => void handleMiniPauseOrResume()}
                  className="flex items-center space-x-1 px-2.5 py-1.5 rounded-md bg-bg border border-border text-text hover:bg-surface-hover text-xs font-medium"
                >
                  {isPaused || isError ? (
                    <>
                      <Play className="w-3.5 h-3.5 text-success fill-success" />
                      <span>{t('progress.resumeBtn')}</span>
                    </>
                  ) : (
                    <>
                      <Pause className="w-3.5 h-3.5 text-warning fill-warning" />
                      <span>{t('common.pause')}</span>
                    </>
                  )}
                </button>
                <button
                  type="button"
                  onClick={() => void handleMiniCancel()}
                  className="flex items-center space-x-1 px-2.5 py-1.5 rounded-md bg-danger/10 text-danger hover:bg-danger/20 text-xs font-medium"
                >
                  <Square className="w-3.5 h-3.5 fill-danger" />
                  <span>{t('progress.cancelBtn')}</span>
                </button>
              </>
            )}
          </div>

          <div className="flex items-center space-x-1.5">
            <button
              type="button"
              onClick={() => void openMainWindow()}
              className="flex items-center space-x-1 px-2.5 py-1.5 rounded-md bg-bg border border-border text-text-muted hover:text-text hover:bg-surface-hover text-[11px] font-medium"
            >
              <ExternalLink className="w-3 h-3" />
              <span>{t('popup.openMain')}</span>
            </button>
            <button
              type="button"
              onClick={() =>
                void (isCompleted
                  ? closePopupWindow(resolvedPopupId)
                  : minimizePopupWindow(resolvedPopupId))
              }
              className="px-2.5 py-1.5 rounded-md bg-accent text-on-accent text-xs font-semibold hover:bg-accent-hover"
            >
              {isCompleted ? t('common.close') : t('popup.minimizeBtn')}
            </button>
          </div>
        </footer>
      </div>
    );
  }

  // ==================== MODE A: CONFIRM + SILENT PRE-DOWNLOAD ====================
  const isTorrentMode =
    session?.initialMode === 'torrent' ||
    Boolean(prebuffer.torrentInfo) ||
    isTorrentReference(url) ||
    categoryId === 'torrents';

  const currentTotalSize =
    prebuffer.totalBytes ?? prebuffer.torrentInfo?.totalSize ?? session?.fileSize ?? null;
  const activeFmt = prebuffer.mediaInfo?.formats?.find((f) => f.formatId === selectedFormatId);
  const displayError = localError || (prebuffer.status === 'error' ? prebuffer.error : null);

  return (
    <div className="h-screen w-screen bg-bg text-text flex flex-col overflow-hidden select-none">
      {/* Header */}
      <header
        data-tauri-drag-region
        className="flex items-center justify-between px-4 py-2.5 bg-surface border-b border-border shrink-0"
      >
        <div data-tauri-drag-region className="flex items-center space-x-2">
          {isTorrentMode ? (
            <Magnet className="w-4 h-4 text-accent" />
          ) : (
            <Link2 className="w-4 h-4 text-accent" />
          )}
          <h1 data-tauri-drag-region className="text-sm font-bold text-text">
            {isTorrentMode ? t('toolbar.addTorrentTitle') : t('popup.confirmTitle')}
          </h1>
          {session?.cookie || session?.referer ? (
            <span className="px-1.5 py-0.5 rounded-full bg-emerald-500/15 text-emerald-400 text-[10px] font-semibold">
              {t('addUrl.browserSessionBadge')}
            </span>
          ) : null}
        </div>
        <div className="flex items-center space-x-1">
          <button
            type="button"
            onClick={() => void minimizePopupWindow(resolvedPopupId)}
            title={t('popup.minimizeBtn')}
            aria-label={t('popup.minimizeBtn')}
            className="p-1 rounded-md text-text-muted hover:text-text hover:bg-surface-hover transition-colors"
          >
            <Minus className="w-4 h-4" />
          </button>
          <button
            type="button"
            onClick={() => void handleCancelConfirm()}
            title={t('common.cancel')}
            aria-label={t('common.close')}
            className="p-1 rounded-md text-text-muted hover:text-text hover:bg-surface-hover transition-colors"
          >
            <X className="w-4 h-4" />
          </button>
        </div>
      </header>

      {/* Body */}
      <div className="flex-1 overflow-y-auto p-3.5 space-y-3 text-xs">
        {/* Silent Pre-Download Live Banner */}
        {url.trim() && (
          <div
            data-testid="popup-prebuffer-banner"
            className={`p-2.5 rounded-md border space-y-1.5 text-[11px] ${
              prebuffer.status === 'completed'
                ? 'bg-emerald-500/10 border-emerald-500/40 text-emerald-300'
                : prebuffer.status === 'error'
                ? 'bg-red-500/10 border-red-500/40 text-red-300'
                : 'bg-accent/10 border-accent/30 text-text'
            }`}
          >
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center space-x-1.5 font-medium truncate">
                {prebuffer.status === 'completed' ? (
                  <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400 shrink-0" />
                ) : prebuffer.status === 'downloading' ? (
                  <Loader2 className="w-3.5 h-3.5 text-accent animate-spin shrink-0" />
                ) : prebuffer.status === 'inspecting' ? (
                  <Loader2 className="w-3.5 h-3.5 text-accent animate-spin shrink-0" />
                ) : prebuffer.status === 'error' ? (
                  <AlertCircle className="w-3.5 h-3.5 text-red-400 shrink-0" />
                ) : isTorrentMode ? (
                  <Magnet className="w-3.5 h-3.5 text-accent shrink-0" />
                ) : (
                  <Zap className="w-3.5 h-3.5 text-accent shrink-0" />
                )}
                <span className="truncate">
                  {prebuffer.status === 'completed'
                    ? t('popup.prebufferComplete')
                    : prebuffer.status === 'downloading'
                    ? t('popup.prebufferActive', {
                        size: formatDiskBytes(prebuffer.downloadedBytes, '0 B'),
                        pct: Math.round(prebuffer.progressPercent || 0),
                      })
                    : prebuffer.status === 'paused'
                    ? t('popup.prebufferPaused', {
                        size: formatDiskBytes(prebuffer.downloadedBytes, '0 B'),
                      })
                    : prebuffer.status === 'skipped' && isTorrentMode
                    ? t('popup.prebufferTorrentHint')
                    : prebuffer.status === 'inspecting'
                    ? t('popup.prebufferWaiting')
                    : prebuffer.status === 'error'
                    ? t('addUrl.inspectErrorTitle')
                    : t('popup.prebufferWaiting')}
                </span>
              </div>
              {prebuffer.status === 'downloading' && prebuffer.speedBytesPerSec > 0 && (
                <span className="font-mono text-[10px] text-accent shrink-0">
                  {(prebuffer.speedBytesPerSec / (1024 * 1024)).toFixed(2)} MB/s
                </span>
              )}
            </div>

            {(prebuffer.status === 'downloading' || prebuffer.status === 'completed') && (
              <div className="w-full h-1.5 bg-bg/60 rounded-full overflow-hidden">
                <div
                  className={`h-full transition-all duration-200 ${
                    prebuffer.status === 'completed' ? 'bg-emerald-400' : 'bg-accent'
                  }`}
                  style={{
                    width: `${
                      prebuffer.status === 'completed'
                        ? 100
                        : Math.min(100, Math.max(8, prebuffer.progressPercent || 8))
                    }%`,
                  }}
                />
              </div>
            )}
          </div>
        )}

        {/* URL Input + .torrent file browse */}
        <div className="space-y-1">
          <div className="flex items-center justify-between">
            <label className="font-semibold text-text">{t('addUrl.urlLabel')}</label>
            <div className="flex items-center space-x-1.5">
              <input
                ref={torrentFileInputRef}
                type="file"
                accept=".torrent,application/x-bittorrent"
                onChange={handleTorrentFilePick}
                className="hidden"
              />
              <button
                type="button"
                onClick={() => torrentFileInputRef.current?.click()}
                className="inline-flex items-center space-x-1 px-2 py-0.5 rounded bg-surface border border-border hover:bg-surface-hover text-[11px] font-medium text-accent transition-colors"
              >
                <Upload className="w-3 h-3" />
                <span>{t('addUrl.pickTorrentFile')}</span>
              </button>
              <button
                type="button"
                onClick={handleRecheckUrl}
                disabled={!url.trim()}
                className="inline-flex items-center space-x-1 px-2 py-0.5 rounded bg-surface border border-border hover:bg-surface-hover text-[11px] font-medium text-text-muted hover:text-text disabled:opacity-40"
              >
                <RotateCw className="w-3 h-3" />
                <span>{t('addUrl.recheckUrl')}</span>
              </button>
            </div>
          </div>
          <input
            type="text"
            value={url}
            onChange={(e) => handleUrlInputChange(e.target.value)}
            placeholder={t('addUrl.urlPlaceholder')}
            className="w-full px-3 py-1.5 rounded-md bg-surface border border-border-input/40 text-text placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent font-mono text-[11px]"
            autoFocus={!url}
          />
        </div>

        {/* Inspect / Server Error */}
        {displayError && (
          <div className="p-2.5 bg-red-500/10 border border-red-500/40 rounded-md text-[11px] text-red-300 flex items-start space-x-2">
            <AlertCircle className="w-4 h-4 text-red-400 shrink-0 mt-0.5" />
            <span className="leading-snug flex-1">{displayError}</span>
          </div>
        )}

        {/* Video Quality Card */}
        {prebuffer.mediaInfo && prebuffer.mediaInfo.formats.length > 0 && (
          <div className="p-2.5 bg-blue-500/10 border border-blue-500/30 rounded-lg space-y-2">
            <div className="flex items-center justify-between">
              <div className="flex items-center space-x-1.5 text-accent font-semibold text-xs">
                <Film className="w-3.5 h-3.5" />
                <span>{t('addUrl.mediaDetected')}</span>
              </div>
              <span className="flex items-center space-x-1 text-[10px] bg-blue-500/15 text-accent font-medium px-2 py-0.5 rounded-full">
                <Sparkles className="w-3 h-3" />
                <span>{t('addUrl.multiQualityBadge')}</span>
              </span>
            </div>
            <div className="grid grid-cols-2 gap-1.5 max-h-32 overflow-y-auto">
              {prebuffer.mediaInfo.formats.map((fmt) => {
                const isSelected = selectedFormatId === fmt.formatId;
                return (
                  <button
                    key={fmt.formatId}
                    type="button"
                    onClick={() => handleSelectFormat(fmt.formatId)}
                    className={`flex items-center justify-between p-1.5 rounded-md border text-left transition-all ${
                      isSelected
                        ? 'bg-accent text-on-accent border-accent'
                        : 'bg-surface hover:bg-surface-hover border-border text-text'
                    }`}
                  >
                    <div className="truncate mr-1.5">
                      <div className="flex items-center space-x-1 text-[11px] font-semibold truncate">
                        {fmt.isAudioOnly ? (
                          <Music className="w-3 h-3 shrink-0" />
                        ) : (
                          <Film className="w-3 h-3 shrink-0" />
                        )}
                        <span className="truncate">{fmt.resolution}</span>
                      </div>
                    </div>
                    <span className="text-[10px] font-mono shrink-0">
                      {formatDiskBytes(fmt.fileSizeApprox, t('common.unknown'))}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
        )}

        {/* BitTorrent Card */}
        {isTorrentMode && (
          <div className="p-2.5 bg-surface rounded-md border border-border space-y-1 text-[11px]">
            <div className="flex items-center justify-between">
              <span className="inline-flex items-center space-x-1.5 font-semibold text-accent">
                <Magnet className="w-3.5 h-3.5" />
                <span>{t('addUrl.torrentDetected')}</span>
              </span>
              {prebuffer.torrentInfo && (
                <span className="font-mono text-text-muted">
                  {prebuffer.torrentInfo.files.length > 0
                    ? `${t('addUrl.torrentFilesCount', {
                        count: prebuffer.torrentInfo.files.length,
                      })} • ${formatDiskBytes(prebuffer.torrentInfo.totalSize, t('common.unknown'))}`
                    : formatDiskBytes(prebuffer.torrentInfo.totalSize, t('common.unknown'))}
                </span>
              )}
            </div>
            {prebuffer.torrentInfo?.infoHash && (
              <p className="font-mono text-[10px] text-text-muted truncate">
                {t('addUrl.torrentInfoHash')}: {prebuffer.torrentInfo.infoHash}
              </p>
            )}
            {prebuffer.torrentInfo?.isMagnet && prebuffer.torrentInfo.files.length === 0 && (
              <p className="text-[10px] text-text-muted">{t('addUrl.torrentMagnetHint')}</p>
            )}
          </div>
        )}

        {/* Standard Info Strip */}
        {!prebuffer.mediaInfo && !isTorrentMode && url.trim() && (
          <div className="p-2 bg-surface rounded-md border border-border flex items-center justify-between text-[11px]">
            <span className="font-medium text-text-muted">
              {t('addUrl.originalSize')}{' '}
              <strong className="text-text font-mono">
                {formatDiskBytes(currentTotalSize, t('common.unknown'))}
              </strong>
            </span>
            <span className="font-mono text-[10px] text-text-muted">
              {prebuffer.supportsRange ? `${numConnections} ${t('common.threadsUnit')}` : t('addUrl.singleThread')}
            </span>
          </div>
        )}

        {/* File Name */}
        <div className="space-y-1">
          <label className="font-semibold text-text">{t('addUrl.saveAsLabel')}</label>
          <input
            type="text"
            value={fileName}
            onChange={(e) => {
              userEditedNameRef.current = true;
              setFileName(e.target.value);
            }}
            placeholder={t('addUrl.saveAsPlaceholder')}
            className="w-full px-3 py-1.5 rounded-md bg-surface border border-border-input/40 text-text focus:outline-none focus:ring-1 focus:ring-accent font-mono"
          />
        </div>

        {/* Category & Connections */}
        <div className="grid grid-cols-2 gap-2.5">
          <div className="space-y-1">
            <label className="font-semibold text-text">{t('addUrl.categoryLabel')}</label>
            <select
              value={categoryId}
              onChange={(e) => {
                userEditedCatRef.current = true;
                applyCategoryDefaults(e.target.value, categoryRules);
              }}
              className="w-full px-2.5 py-1.5 rounded-md bg-surface border border-border-input/40 text-text focus:outline-none focus:ring-1 focus:ring-accent"
            >
              <option value="general">{t('addUrl.categoryGeneral')}</option>
              {categoryRules.map((r) => (
                <option key={r.id} value={r.id}>
                  {getCategoryLabel(r)}
                </option>
              ))}
            </select>
          </div>
          <div className="space-y-1">
            <label className="font-semibold text-text">{t('addUrl.connectionsLabel')}</label>
            <select
              value={numConnections}
              onChange={(e) => setNumConnections(Number(e.target.value))}
              className="w-full px-2.5 py-1.5 rounded-md bg-surface border border-border-input/40 text-text focus:outline-none focus:ring-1 focus:ring-accent font-mono"
            >
              <option value={1}>{t('addUrl.conn1')}</option>
              <option value={4}>{t('addUrl.conn4')}</option>
              <option value={8}>{t('addUrl.conn8')}</option>
              <option value={16}>{t('addUrl.conn16')}</option>
              <option value={32}>{t('addUrl.conn32')}</option>
            </select>
          </div>
        </div>

        {/* Save Folder & Free Space */}
        <div className="space-y-1">
          <label className="font-semibold text-text">{t('addUrl.saveFolderLabel')}</label>
          <input
            type="text"
            value={saveDir}
            onChange={(e) => {
              userEditedCatRef.current = true;
              setSaveDir(e.target.value);
            }}
            className="w-full px-3 py-1.5 rounded-md bg-surface border border-border-input/40 text-text font-mono text-[11px]"
          />
          <div className="flex items-center justify-between text-[10px] text-text-muted pt-0.5">
            <span className="inline-flex items-center space-x-1">
              <HardDrive className="w-3 h-3" />
              <span>
                {t('addUrl.diskFree')}{' '}
                <strong className="font-mono text-text">
                  {formatDiskBytes(freeDiskBytes, t('common.unknown'))}
                </strong>
              </span>
            </span>
            {freeDiskBytes !== null &&
              (activeFmt?.fileSizeApprox ?? currentTotalSize ?? 0) > freeDiskBytes && (
                <span className="text-danger font-semibold">{t('addUrl.diskLowWarning')}</span>
              )}
          </div>
        </div>
      </div>

      {/* Footer Actions */}
      <footer className="px-4 py-2.5 bg-surface border-t border-border flex items-center justify-between gap-2 shrink-0">
        <div className="flex items-center space-x-1.5">
          <button
            type="button"
            onClick={() => void handleCancelConfirm()}
            className="px-2.5 py-1.5 rounded-md text-text-muted hover:text-text hover:bg-surface-hover text-xs font-medium"
          >
            {t('common.cancel')}
          </button>
          <button
            type="button"
            onClick={() => void minimizePopupWindow(resolvedPopupId)}
            className="flex items-center space-x-1 px-2.5 py-1.5 rounded-md bg-bg border border-border text-text-muted hover:text-text hover:bg-surface-hover text-xs font-medium"
          >
            <Minus className="w-3.5 h-3.5" />
            <span>{t('popup.minimizeBtn')}</span>
          </button>
        </div>

        <div className="flex items-center space-x-2">
          <button
            type="button"
            disabled={!url.trim() || isQueuingLater || isPromoting}
            onClick={() => void handleDownloadLater()}
            className="flex items-center space-x-1 px-3 py-1.5 rounded-md bg-bg border border-border text-text hover:bg-surface-hover text-xs font-medium disabled:opacity-40"
          >
            <Clock className="w-3.5 h-3.5 text-warning" />
            <span>{t('popup.downloadLater')}</span>
          </button>
          <button
            type="button"
            disabled={!url.trim() || isPromoting || isQueuingLater}
            onClick={() => void handleStartDownload()}
            className="flex items-center space-x-1.5 px-3.5 py-1.5 rounded-md bg-accent text-on-accent text-xs font-semibold hover:bg-accent-hover shadow-sm disabled:opacity-40"
          >
            <Download className="w-3.5 h-3.5" />
            <span>{t('popup.startDownload')}</span>
          </button>
        </div>
      </footer>
    </div>
  );
};
