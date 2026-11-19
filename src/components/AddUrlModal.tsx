import React, { useState, useEffect, useRef } from 'react';
import {
  X,
  Link2,
  Folder,
  Download,
  Loader2,
  CheckCircle2,
  Film,
  Music,
  Sparkles,
  AlertCircle,
  RotateCw,
  KeyRound,
  HardDrive,
  Magnet,
  Upload,
} from 'lucide-react';
import {
  Category,
  CategoryRule,
  IncomingDownloadMeta,
  InspectResult,
  ProxyConfig,
  SiteLogin,
  VideoFormat,
} from '../types.ts';
import {
  checkDiskSpace,
  inspectNativeUrlDetailed,
  inspectTorrentBytes,
  isTauri,
  NativeHeaders,
  readClipboardText,
} from '../services/nativeBridge.ts';
import {
  deriveFileNameFromUrl,
  findMatchingSiteLogin,
  formatDiskBytes,
  formatProxyUrl,
  isMagnetUri,
  isSupportedDownloadUrl,
  isTorrentReference,
  matchCategoryForFile,
} from '../utils/downloadUtils.ts';
import { useI18n } from '../i18n/index.tsx';

interface AddUrlModalProps {
  isOpen: boolean;
  onClose: () => void;
  onStartDownload: (
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
  ) => void;
  categories: Category[];
  categoryRules?: CategoryRule[];
  proxyConfig?: ProxyConfig;
  siteLogins?: SiteLogin[];
  initialUrl?: string;
  initialMode?: 'url' | 'torrent';
  incomingMeta?: IncomingDownloadMeta;
  defaultConnections?: number;
  isAndroid?: boolean;
}

export const AddUrlModal: React.FC<AddUrlModalProps> = ({
  isOpen,
  onClose,
  onStartDownload,
  categories,
  categoryRules = [],
  proxyConfig,
  siteLogins = [],
  initialUrl,
  initialMode = 'url',
  incomingMeta,
  defaultConnections = 8,
  isAndroid = false,
}) => {
  const { t } = useI18n();
  const [url, setUrl] = useState('');
  const [fileName, setFileName] = useState('');
  const [selectedCategory, setSelectedCategory] = useState('general');
  const [saveDir, setSaveDir] = useState('~/Downloads');
  const [numConnections, setNumConnections] = useState(defaultConnections);
  const [isValidating, setIsValidating] = useState(false);
  const [inspectInfo, setInspectInfo] = useState<InspectResult | null>(null);
  const [inspectError, setInspectError] = useState<string | null>(null);
  const [selectedFormatId, setSelectedFormatId] = useState<string>('1080p_mp4');
  const [sessionHeaders, setSessionHeaders] = useState<NativeHeaders | undefined>(undefined);
  const [freeDiskBytes, setFreeDiskBytes] = useState<number | null>(null);
  const probeReqIdRef = useRef(0);
  const torrentFileInputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!isOpen) return;
    checkDiskSpace(saveDir).then(setFreeDiskBytes);
  }, [isOpen, saveDir]);

  useEffect(() => {
    if (!isOpen) {
      setNumConnections(defaultConnections);
    }
  }, [isOpen, defaultConnections]);

  // Auto-read clipboard or use initialUrl / incomingMeta / initialMode on open
  useEffect(() => {
    if (isOpen) {
      setNumConnections(defaultConnections);
      setInspectError(null);
      const targetUrl = incomingMeta?.url || initialUrl;
      if (targetUrl && isSupportedDownloadUrl(targetUrl)) {
        const metaHeaders: NativeHeaders | undefined =
          incomingMeta?.url === targetUrl &&
          (incomingMeta.cookie || incomingMeta.referer || incomingMeta.userAgent)
            ? {
                cookie: incomingMeta.cookie,
                referer: incomingMeta.referer,
                userAgent: incomingMeta.userAgent,
              }
            : undefined;
        setSessionHeaders(metaHeaders);
        setUrl(targetUrl);
        handleProbe(
          targetUrl,
          metaHeaders,
          incomingMeta?.url === targetUrl ? incomingMeta.fileName : undefined
        );
      } else {
        setSessionHeaders(undefined);
        if (initialMode === 'torrent') {
          const torrentRule = categoryRules.find((r) => r.id === 'torrents');
          const torrentCat = categories.find((c) => c.id === 'torrents');
          setSelectedCategory('torrents');
          setSaveDir(torrentRule?.saveDir || torrentCat?.saveDir || '~/Downloads/Torrents');
        } else {
          setSelectedCategory('general');
          setSaveDir('~/Downloads');
        }
        readClipboardText().then((nativeText) => {
          if (
            nativeText &&
            isSupportedDownloadUrl(nativeText) &&
            (initialMode !== 'torrent' || isTorrentReference(nativeText))
          ) {
            setUrl(nativeText);
            handleProbe(nativeText, undefined, undefined);
            return;
          }
          if (navigator.clipboard && typeof navigator.clipboard.readText === 'function') {
            navigator.clipboard.readText().then((text) => {
              if (
                text &&
                isSupportedDownloadUrl(text) &&
                (initialMode !== 'torrent' || isTorrentReference(text))
              ) {
                setUrl(text);
                handleProbe(text, undefined, undefined);
              }
            }).catch(() => {});
          }
        }).catch(() => {});
      }
    } else {
      probeReqIdRef.current += 1;
      setUrl('');
      setInspectInfo(null);
      setInspectError(null);
      setFileName('');
      setSelectedFormatId('1080p_mp4');
      setSessionHeaders(undefined);
    }
  }, [isOpen, initialUrl, initialMode, incomingMeta]);

  const handleTorrentFilePick = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) return;
    setInspectError(null);
    try {
      const buf = await file.arrayBuffer();
      const res = await inspectTorrentBytes(Array.from(new Uint8Array(buf)));
      if (!res) return;
      probeReqIdRef.current += 1;
      setIsValidating(false);
      setUrl(res.url);
      setFileName(res.fileName || file.name.replace(/\.torrent$/i, ''));
      const torrentRule = categoryRules.find((r) => r.id === 'torrents');
      const torrentCat = categories.find((c) => c.id === 'torrents');
      setSelectedCategory('torrents');
      setSaveDir(torrentRule?.saveDir || torrentCat?.saveDir || '~/Downloads/Torrents');
      setInspectInfo(res);
    } catch (err) {
      setInspectError(String(err));
    }
  };

  const handleProbe = (
    inputUrl: string,
    overrideHeaders?: NativeHeaders,
    suggestedFileName?: string
  ) => {
    if (!isSupportedDownloadUrl(inputUrl)) {
      setInspectInfo(null);
      setInspectError(null);
      return;
    }

    const reqId = ++probeReqIdRef.current;
    setInspectError(null);

    const isTorrent = isTorrentReference(inputUrl);
    const urlLower = inputUrl.toLowerCase();
    const isVideo = !isTorrent && (urlLower.includes('youtube.com') ||
      urlLower.includes('youtu.be') ||
      urlLower.includes('vimeo.com') ||
      urlLower.includes('tiktok.com') ||
      urlLower.includes('instagram.com') ||
      urlLower.includes('instagr.am') ||
      urlLower.includes('facebook.com') ||
      urlLower.includes('fb.watch') ||
      urlLower.includes('fb.com') ||
      urlLower.includes('x.com') ||
      urlLower.includes('twitter.com') ||
      urlLower.includes('reddit.com') ||
      urlLower.includes('redd.it') ||
      urlLower.includes('twitch.tv') ||
      urlLower.includes('dailymotion.com') ||
      urlLower.includes('soundcloud.com') ||
      urlLower.includes('threads.net') ||
      urlLower.includes('threads.com') ||
      urlLower.includes('pinterest.com') ||
      urlLower.includes('pin.it') ||
      urlLower.endsWith('.mp4') ||
      urlLower.endsWith('.mkv') ||
      urlLower.endsWith('.webm') ||
      urlLower.endsWith('.m3u8'));

    let initialDerivedName =
      suggestedFileName && suggestedFileName.trim().length > 0
        ? suggestedFileName.trim()
        : deriveFileNameFromUrl(inputUrl);

    // Instant synchronous category & filename derivation
    try {
      let cat = 'general';
      if (isTorrent) {
        cat = 'torrents';
      } else if (isVideo) {
        cat = 'video';
        const parsed = new URL(inputUrl);
        if (inputUrl.includes('v=')) {
          const vId = new URLSearchParams(parsed.search).get('v') || 'video';
          initialDerivedName = `Video_${vId}.mp4`;
        } else if (!initialDerivedName.includes('.')) {
          initialDerivedName = `${initialDerivedName}.mp4`;
        }
      } else {
        const ext = initialDerivedName.split('.').pop()?.toLowerCase();
        if (['zip', 'rar', '7z', 'iso', 'tar', 'gz', 'bz2', 'xz'].includes(ext || '')) cat = 'compressed';
        else if (['mp4', 'mkv', 'avi', 'mov', 'webm', 'flv', 'ts'].includes(ext || '')) cat = 'video';
        else if (['mp3', 'wav', 'flac', 'm4a', 'aac', 'ogg', 'opus'].includes(ext || '')) cat = 'music';
        else if (['exe', 'dmg', 'pkg', 'msi', 'deb', 'rpm', 'appimage', 'apk'].includes(ext || '')) cat = 'programs';
        else if (['pdf', 'doc', 'docx', 'xls', 'xlsx', 'epub'].includes(ext || '')) cat = 'documents';
      }

      const customMatched =
        categoryRules.length > 0
          ? matchCategoryForFile(initialDerivedName, categoryRules, inputUrl)
          : null;
      if (customMatched && customMatched.categoryId !== 'general') {
        cat = customMatched.categoryId;
      }

      const matchedRule = categoryRules.find((r) => r.id === cat);
      const foundCategory = categories.find((c) => c.id === cat);
      if (foundCategory || matchedRule || cat === 'torrents') {
        setSelectedCategory(cat);
        setSaveDir(
          matchedRule?.saveDir ||
            foundCategory?.saveDir ||
            (cat === 'torrents'
              ? '~/Downloads/Torrents'
              : foundCategory?.extensions
              ? `~/Downloads/${foundCategory.name}`
              : '~/Downloads')
        );
      } else {
        setSelectedCategory('general');
        setSaveDir('~/Downloads');
      }

      setFileName(initialDerivedName);
    } catch (e) {
      if (isTorrent) {
        setSelectedCategory('torrents');
        const torrentRule = categoryRules.find((r) => r.id === 'torrents');
        setSaveDir(torrentRule?.saveDir || '~/Downloads/Torrents');
        setFileName(initialDerivedName);
      }
    }

    setIsValidating(true);

    const activeProxy = formatProxyUrl(proxyConfig);
    const matchedLogin = findMatchingSiteLogin(inputUrl, siteLogins);
    const mergedHeaders: NativeHeaders = {
      ...overrideHeaders,
      proxyUrl: overrideHeaders?.proxyUrl || activeProxy,
      authUsername: overrideHeaders?.authUsername || matchedLogin?.username,
      authPassword: overrideHeaders?.authPassword || matchedLogin?.password,
    };

    // Real native URL inspection (forwarding captured browser cookies/referer/UA + proxy/auth)
    inspectNativeUrlDetailed(inputUrl, mergedHeaders).then(({ result: nativeResult, error }) => {
      if (reqId !== probeReqIdRef.current) return;

      if (error) {
        setIsValidating(false);
        setInspectInfo(null);
        setInspectError(error);
        return;
      }

      if (nativeResult) {
        setIsValidating(false);
        setInspectError(null);
        setInspectInfo(nativeResult);
        if (nativeResult.fileName) {
          setFileName(nativeResult.fileName);
        }
        const customCat =
          nativeResult.fileName && categoryRules.length > 0
            ? matchCategoryForFile(nativeResult.fileName, categoryRules, inputUrl)
            : null;
        const resolvedCatId =
          customCat && customCat.categoryId !== 'general'
            ? customCat.categoryId
            : nativeResult.suggestedCategory;
        if (resolvedCatId) {
          const found = categories.find((c) => c.id === resolvedCatId);
          const rule = categoryRules.find((r) => r.id === resolvedCatId);
          if (found || rule || resolvedCatId === 'torrents') {
            setSelectedCategory(resolvedCatId);
            setSaveDir(
              rule?.saveDir ||
                found?.saveDir ||
                (resolvedCatId === 'torrents'
                  ? '~/Downloads/Torrents'
                  : found?.extensions
                  ? `~/Downloads/${found.name}`
                  : '~/Downloads')
            );
          } else {
            setSelectedCategory('general');
            setSaveDir('~/Downloads');
          }
        }
        if (nativeResult.mediaInfo && nativeResult.mediaInfo.formats.length > 0) {
          setSelectedFormatId(nativeResult.mediaInfo.formats[0].formatId);
        }
        return;
      }

      // Fallback for web browser / test preview environment
      setTimeout(() => {
        if (reqId !== probeReqIdRef.current) return;
        if (!isTauri() && isTorrent) {
          const xtMatch = inputUrl.match(/btih:([a-fA-F0-9]{40}|[A-Z2-7]{32})/i);
          const dcMatch = inputUrl.match(/^dc-torrent:\/\/([^/?#]+)/i);
          const infoHash = (xtMatch?.[1] || dcMatch?.[1] || '').toLowerCase();
          setInspectInfo({
            url: inputUrl,
            finalUrl: inputUrl,
            fileName: initialDerivedName,
            fileSize: null,
            supportsRange: true,
            mimeType: 'application/x-bittorrent',
            suggestedCategory: 'torrents',
            torrentInfo: {
              infoHash,
              name: initialDerivedName,
              totalSize: 0,
              pieceLength: 262144,
              pieceCount: 0,
              trackers: [],
              files: [],
              isMultiFile: false,
              isMagnet: isMagnetUri(inputUrl),
            },
          });
          setIsValidating(false);
          return;
        }

        if (!isTauri() && isVideo) {
          const testFormats: VideoFormat[] = [
            {
              formatId: '1080p_mp4',
              resolution: '1080p (Full HD 60fps)',
              extension: 'mp4',
              fileSizeApprox: 85000000,
              videoCodec: 'H.264 / AVC',
              audioCodec: 'AAC 320kbps',
              downloadUrl: inputUrl,
              isAudioOnly: false,
            },
            {
              formatId: '720p_mp4',
              resolution: '720p (HD)',
              extension: 'mp4',
              fileSizeApprox: 46000000,
              videoCodec: 'H.264 / AVC',
              audioCodec: 'AAC 192kbps',
              downloadUrl: inputUrl,
              isAudioOnly: false,
            },
            {
              formatId: 'audio_mp3',
              resolution: 'Audio Only (MP3 320 kbps)',
              extension: 'mp3',
              fileSizeApprox: 6800000,
              videoCodec: undefined,
              audioCodec: 'MP3 320kbps',
              downloadUrl: inputUrl,
              isAudioOnly: true,
            },
          ];

          setInspectInfo({
            url: inputUrl,
            finalUrl: inputUrl,
            fileName: initialDerivedName || 'video.mp4',
            fileSize: 85000000,
            supportsRange: true,
            suggestedCategory: 'video',
            mediaInfo: {
              title: initialDerivedName || 'Online Video Stream',
              durationSeconds: 215,
              formats: testFormats,
            },
          });
          setSelectedFormatId('1080p_mp4');
          setIsValidating(false);
          return;
        }

        setInspectInfo({
          url: inputUrl,
          finalUrl: inputUrl,
          fileName: initialDerivedName || (isVideo ? 'video.mp4' : 'download.bin'),
          fileSize: null,
          supportsRange: true,
          suggestedCategory: isVideo ? 'video' : selectedCategory,
          mediaInfo: undefined,
        });
        setIsValidating(false);
      }, 200);
    });
  };

  const handleSelectFormat = (formatId: string) => {
    setSelectedFormatId(formatId);
    if (!inspectInfo?.mediaInfo) return;

    const fmt = inspectInfo.mediaInfo.formats.find(f => f.formatId === formatId);
    if (!fmt) return;

    // Update extension in filename
    const dotIdx = fileName.lastIndexOf('.');
    const nameWithoutExt = dotIdx > 0 ? fileName.slice(0, dotIdx) : fileName;
    const newName = `${nameWithoutExt}.${fmt.extension}`;
    setFileName(newName);

    // Update category & destination folder respecting user categoryRules
    const nextCategoryId = fmt.isAudioOnly ? 'music' : 'video';
    setSelectedCategory(nextCategoryId);
    const matchedRule = categoryRules.find((r) => r.id === nextCategoryId);
    const foundCat = categories.find((c) => c.id === nextCategoryId);
    const fallbackDir = nextCategoryId === 'music' ? '~/Downloads/Music' : '~/Downloads/Video';
    setSaveDir(matchedRule?.saveDir || foundCat?.saveDir || fallbackDir);
  };

  const getCategoryLabel = (cat: Category) => {
    switch (cat.id) {
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
        return cat.name;
    }
  };

  const formatBytes = (bytes?: number | null) => {
    if (bytes === null || bytes === undefined || bytes <= 0) return t('common.unknown');
    if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(2)} KB`;
    return `${bytes} B`;
  };

  if (!isOpen) return null;

  const isTorrentMode =
    initialMode === 'torrent' ||
    Boolean(inspectInfo?.torrentInfo) ||
    isTorrentReference(url) ||
    selectedCategory === 'torrents';

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-xs p-4 select-none">
      <div className="bg-bg border border-border rounded-lg shadow-pop w-full max-w-lg max-h-[92dvh] flex flex-col overflow-hidden animate-in fade-in zoom-in-95 duration-150">
        {/* Modal Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-surface shrink-0">
          <div className="flex items-center space-x-2">
            {isTorrentMode ? (
              <Magnet className="w-4 h-4 text-accent" />
            ) : (
              <Link2 className="w-4 h-4 text-accent" />
            )}
            <h2 className="text-sm font-bold text-text">
              {isTorrentMode && initialMode === 'torrent'
                ? t('toolbar.addTorrentTitle')
                : t('addUrl.title')}
            </h2>
          </div>
          <button
            onClick={onClose}
            aria-label={t('common.close')}
            className="p-1 rounded-md text-text-muted hover:text-text hover:bg-surface-hover transition-colors"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Modal Body */}
        <div className="p-4 space-y-4 text-xs overflow-y-auto flex-1">
          {/* URL Input */}
          <div className="space-y-1">
            <div className="flex items-center justify-between gap-2">
              <label className="font-semibold text-text">{t('addUrl.urlLabel')}</label>
              <div className="flex items-center space-x-1.5">
                {sessionHeaders && (sessionHeaders.cookie || sessionHeaders.referer) && (
                  <span className="inline-flex items-center space-x-1 text-[10px] px-2 py-0.5 rounded-full bg-emerald-500/15 text-emerald-400 border border-emerald-500/30 font-medium">
                    <KeyRound className="w-3 h-3" />
                    <span>{t('addUrl.browserSessionBadge')}</span>
                  </span>
                )}
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
              </div>
            </div>
            <div className="relative">
              <input
                type="text"
                value={url}
                onChange={(e) => {
                  const nextUrl = e.target.value;
                  setUrl(nextUrl);
                  const matchingHeaders =
                    incomingMeta?.url === nextUrl ? sessionHeaders : undefined;
                  if (incomingMeta?.url !== nextUrl) {
                    setSessionHeaders(undefined);
                  }
                  handleProbe(nextUrl, matchingHeaders);
                }}
                placeholder={t('addUrl.urlPlaceholder')}
                className="w-full px-3 py-2 rounded-md bg-surface border border-border-input/40 text-text placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent font-mono pr-9"
                autoFocus
              />
              {isValidating && (
                <Loader2 className="w-4 h-4 text-accent animate-spin absolute right-3 top-1/2 -translate-y-1/2" />
              )}
            </div>
          </div>

          {/* HTML-Hijack / Server Error Alert */}
          {inspectError && (
            <div className="p-3 bg-red-500/10 border border-red-500/40 rounded-lg space-y-2 text-[11px] text-red-300">
              <div className="flex items-start space-x-2">
                <AlertCircle className="w-4 h-4 text-red-400 shrink-0 mt-0.5" />
                <div className="space-y-1 flex-1">
                  <p className="font-bold text-red-400">{t('addUrl.inspectErrorTitle')}</p>
                  <p className="leading-relaxed text-red-200/90">
                    {inspectError.includes('ANDROID_VIDEO_PHASE2')
                      ? t('mobile.videoPhase2Notice')
                      : inspectError}
                  </p>
                </div>
              </div>
              <div className="flex justify-end">
                <button
                  type="button"
                  onClick={() => handleProbe(url, sessionHeaders)}
                  className="inline-flex items-center space-x-1 px-2.5 py-1 rounded bg-red-500/20 hover:bg-red-500/30 text-red-200 font-medium transition-colors"
                >
                  <RotateCw className="w-3 h-3" />
                  <span>{t('addUrl.recheckUrl')}</span>
                </button>
              </div>
            </div>
          )}

          {/* Video Stream Detected Card */}
          {inspectInfo?.mediaInfo && (
            <div className="p-3 bg-blue-500/10 border border-blue-500/30 rounded-lg space-y-2.5 animate-in fade-in duration-200">
              <div className="flex items-center justify-between">
                <div className="flex items-center space-x-1.5 text-accent font-semibold text-xs">
                  <Film className="w-4 h-4 text-accent" />
                  <span>{t('addUrl.mediaDetected')}</span>
                </div>
                <span className="flex items-center space-x-1 text-[11px] bg-blue-500/15 text-accent font-medium px-2 py-0.5 rounded-full">
                  <Sparkles className="w-3 h-3" />
                  <span>{t('addUrl.multiQualityBadge')}</span>
                </span>
              </div>

              {/* Resolution / Format Selection */}
              <div className="space-y-1">
                <label className="text-[11px] font-semibold text-text flex items-center justify-between">
                  <span>{t('addUrl.chooseQuality')}</span>
                  <span className="text-text-muted font-normal">
                    {inspectInfo.mediaInfo.formats.find(f => f.formatId === selectedFormatId)?.isAudioOnly
                      ? `🎵 ${t('addUrl.mediaAudioLabel')}`
                      : `🎬 ${t('addUrl.mediaVideoLabel')}`}
                  </span>
                </label>
                <div className="grid grid-cols-2 gap-1.5">
                  {inspectInfo.mediaInfo.formats.map((fmt) => {
                    const isSelected = selectedFormatId === fmt.formatId;
                    return (
                      <button
                        key={fmt.formatId}
                        type="button"
                        onClick={() => handleSelectFormat(fmt.formatId)}
                        className={`flex items-center justify-between p-2 rounded-md border text-left transition-all ${
                          isSelected
                            ? 'bg-accent text-on-accent border-accent shadow-xs'
                            : 'bg-surface hover:bg-surface-hover border-border text-text'
                        }`}
                      >
                        <div className="truncate mr-2">
                          <div className="flex items-center space-x-1 text-[11px] font-semibold truncate">
                            {fmt.isAudioOnly ? <Music className="w-3 h-3 shrink-0" /> : <Film className="w-3 h-3 shrink-0" />}
                            <span className="truncate">{fmt.resolution}</span>
                          </div>
                          <p className={`text-[10px] ${isSelected ? 'text-on-accent/80' : 'text-text-muted'} font-mono`}>
                            {fmt.videoCodec || fmt.audioCodec}
                          </p>
                        </div>
                        <span className={`text-[10px] font-mono shrink-0 ${isSelected ? 'text-on-accent font-bold' : 'text-text-muted'}`}>
                          {formatBytes(fmt.fileSizeApprox)}
                        </span>
                      </button>
                    );
                  })}
                </div>
              </div>
            </div>
          )}

          {/* BitTorrent Detected Card */}
          {(inspectInfo?.torrentInfo || isTorrentReference(url)) && !inspectError && (
            <div className="p-2.5 bg-cyan-500/10 rounded-md border border-cyan-500/30 space-y-1 text-[11px]">
              <div className="flex items-center justify-between">
                <span className="inline-flex items-center space-x-1.5 font-semibold text-cyan-400">
                  <Magnet className="w-3.5 h-3.5" />
                  <span>{t('addUrl.torrentDetected')}</span>
                </span>
                {inspectInfo?.torrentInfo && (
                  <span className="font-mono text-text-muted">
                    {inspectInfo.torrentInfo.files.length > 0
                      ? `${t('addUrl.torrentFilesCount', {
                          count: inspectInfo.torrentInfo.files.length,
                        })} • ${formatBytes(inspectInfo.torrentInfo.totalSize)}`
                      : formatBytes(inspectInfo.torrentInfo.totalSize)}
                  </span>
                )}
              </div>
              {inspectInfo?.torrentInfo?.infoHash && (
                <p className="font-mono text-[10px] text-text-muted truncate">
                  {t('addUrl.torrentInfoHash')}: {inspectInfo.torrentInfo.infoHash}
                </p>
              )}
              {(inspectInfo?.torrentInfo?.isMagnet || isMagnetUri(url)) &&
                (!inspectInfo?.torrentInfo || inspectInfo.torrentInfo.files.length === 0) && (
                  <p className="text-[10px] text-text-muted">{t('addUrl.torrentMagnetHint')}</p>
                )}
            </div>
          )}

          {/* Standard Metadata Info Card if not video or torrent */}
          {inspectInfo &&
            !inspectInfo.mediaInfo &&
            !inspectInfo.torrentInfo &&
            !isTorrentReference(url) &&
            !inspectError && (
              <div className="p-2.5 bg-surface rounded-md border border-border space-y-1.5 text-[11px]">
                <div className="flex items-center justify-between font-medium">
                  <span className={`flex items-center space-x-1.5 ${inspectInfo.supportsRange ? 'text-success' : 'text-warning'}`}>
                    <CheckCircle2 className="w-3.5 h-3.5 shrink-0" />
                    <span>
                      {inspectInfo.supportsRange
                        ? t('addUrl.rangeSupported')
                        : t('addUrl.rangeUnsupported')}
                    </span>
                  </span>
                  <span className="font-mono text-text shrink-0 ml-2">
                    {inspectInfo.supportsRange
                      ? `${numConnections} ${t('common.threadsUnit')}`
                      : t('addUrl.singleThread')}
                  </span>
                </div>
                <div className="flex items-center justify-between text-text-muted text-[10px]">
                  <span>
                    {t('addUrl.originalSize')}{' '}
                    <strong className="text-text font-mono">{formatBytes(inspectInfo.fileSize)}</strong>
                  </span>
                  {inspectInfo.mimeType && (
                    <span className="font-mono truncate max-w-[200px]" title={inspectInfo.mimeType}>
                      {inspectInfo.mimeType.split(';')[0]}
                    </span>
                  )}
                </div>
              </div>
            )}

          {/* File Name */}
          <div className="space-y-1">
            <label className="font-semibold text-text">{t('addUrl.saveAsLabel')}</label>
            <input
              type="text"
              value={fileName}
              onChange={(e) => setFileName(e.target.value)}
              placeholder={t('addUrl.saveAsPlaceholder')}
              className="w-full px-3 py-1.5 rounded-md bg-surface border border-border-input/40 text-text focus:outline-none focus:ring-1 focus:ring-accent font-medium font-mono"
            />
          </div>

          {/* Category & Folder */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div className="space-y-1">
              <label className="font-semibold text-text">{t('addUrl.categoryLabel')}</label>
              <select
                value={selectedCategory}
                onChange={(e) => {
                  const val = e.target.value;
                  setSelectedCategory(val);
                  const rule = categoryRules.find((r) => r.id === val);
                  const cat = categories.find(c => c.id === val);
                  setSaveDir(
                    rule?.saveDir ||
                      cat?.saveDir ||
                      (val === 'torrents'
                        ? '~/Downloads/Torrents'
                        : cat?.extensions
                        ? `~/Downloads/${cat.name}`
                        : '~/Downloads')
                  );
                }}
                className="w-full px-2.5 py-1.5 rounded-md bg-surface border border-border-input/40 text-text focus:outline-none focus:ring-1 focus:ring-accent"
              >
                <option value="general">{t('addUrl.categoryGeneral')}</option>
                {categories.map((c) => (
                  <option key={c.id} value={c.id}>{getCategoryLabel(c)}</option>
                ))}
                {!categories.some((c) => c.id === 'torrents') && (
                  <option value="torrents">{t('sidebar.catTorrents')}</option>
                )}
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

          {/* Destination Directory */}
          <div className="space-y-1">
            <label className="font-semibold text-text">{t('addUrl.saveFolderLabel')}</label>
            {isAndroid ? (
              <div className="px-3 py-2 rounded-md bg-surface border border-border text-[11px] text-accent font-medium flex items-center space-x-2">
                <Folder className="w-3.5 h-3.5 shrink-0" />
                <span>{t('mobile.savedToDownloads')}</span>
              </div>
            ) : (
              <div className="flex space-x-2">
                <input
                  type="text"
                  value={saveDir}
                  onChange={(e) => setSaveDir(e.target.value)}
                  className="flex-1 px-3 py-1.5 rounded-md bg-surface border border-border-input/40 text-text font-mono text-[11px]"
                />
                <button
                  type="button"
                  className="px-2.5 py-1.5 rounded-md bg-surface border border-border text-text hover:bg-surface-hover flex items-center space-x-1"
                >
                  <Folder className="w-3.5 h-3.5" />
                  <span>{t('addUrl.chooseBtn')}</span>
                </button>
              </div>
            )}
            <div className="flex items-center justify-between text-[10px] pt-0.5 text-text-muted">
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
                inspectInfo?.fileSize &&
                inspectInfo.fileSize > freeDiskBytes && (
                  <span className="text-danger font-semibold">
                    {t('addUrl.diskLowWarning')}
                  </span>
                )}
            </div>
          </div>
        </div>

        {/* Modal Footer */}
        <div className="flex items-center justify-end space-x-2 px-4 py-3 border-t border-border bg-surface">
          <button
            onClick={onClose}
            className="px-3 py-1.5 rounded-md text-text hover:bg-surface-hover font-medium transition-colors text-xs"
          >
            {t('common.cancel')}
          </button>
          <button
            onClick={() => {
              if (url && !inspectError) {
                const currentFmt = inspectInfo?.mediaInfo?.formats.find(f => f.formatId === selectedFormatId);
                const activeProxy = formatProxyUrl(proxyConfig);
                const matchedLogin = findMatchingSiteLogin(url, siteLogins);
                onStartDownload(
                  url,
                  fileName || 'download.bin',
                  saveDir,
                  selectedCategory,
                  numConnections,
                  inspectInfo?.mediaInfo ? selectedFormatId : undefined,
                  currentFmt?.resolution,
                  {
                    cookie: sessionHeaders?.cookie,
                    referer: sessionHeaders?.referer,
                    userAgent: sessionHeaders?.userAgent,
                    proxyUrl: activeProxy,
                    authUsername: matchedLogin?.username,
                    authPassword: matchedLogin?.password,
                    fileSize: currentFmt?.fileSizeApprox ?? inspectInfo?.fileSize ?? null,
                    supportsRange: inspectInfo?.supportsRange ?? true,
                  }
                );
                onClose();
              }
            }}
            disabled={!url || Boolean(inspectError)}
            className="flex items-center space-x-1.5 px-4 py-1.5 rounded-md bg-accent text-on-accent text-xs font-semibold shadow-sm hover:bg-accent-hover transition-colors disabled:opacity-40"
          >
            <Download className="w-3.5 h-3.5" />
            <span>{t('addUrl.startDownload')}</span>
          </button>
        </div>
      </div>
    </div>
  );
};
