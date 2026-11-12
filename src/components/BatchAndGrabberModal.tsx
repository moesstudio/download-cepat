import React, { useEffect, useState } from 'react';
import {
  X,
  Layers,
  Globe,
  Link2,
  Upload,
  CheckSquare,
  Search,
  Loader2,
  Download,
} from 'lucide-react';
import { GrabbedAsset } from '../types.ts';
import { grabSiteLinks, NativeHeaders } from '../services/nativeBridge.ts';
import { deriveFileNameFromUrl, parseUrlListText } from '../utils/downloadUtils.ts';
import { useI18n } from '../i18n/index.tsx';

export interface BatchQueueItem {
  url: string;
  fileName: string;
  cookie?: string;
  referer?: string;
  userAgent?: string;
}

interface BatchAndGrabberModalProps {
  isOpen: boolean;
  initialTab?: 'batch' | 'grabber' | 'browser';
  initialBatchText?: string;
  initialBrowserLinks?: BatchQueueItem[];
  defaultHeaders?: NativeHeaders;
  onClose: () => void;
  onEnqueueBatch: (items: BatchQueueItem[]) => void;
}

export const BatchAndGrabberModal: React.FC<BatchAndGrabberModalProps> = ({
  isOpen,
  initialTab = 'batch',
  initialBatchText = '',
  initialBrowserLinks = [],
  defaultHeaders,
  onClose,
  onEnqueueBatch,
}) => {
  const { t } = useI18n();
  const [activeTab, setActiveTab] = useState<'batch' | 'grabber' | 'browser'>(initialTab);

  // Batch & Wildcard state
  const [batchText, setBatchText] = useState(initialBatchText);
  const [expandedUrls, setExpandedUrls] = useState<string[]>([]);
  const [selectedBatchUrls, setSelectedBatchUrls] = useState<Set<string>>(new Set());

  // Site Grabber state
  const [grabUrl, setGrabUrl] = useState('');
  const [maxDepth, setMaxDepth] = useState(1);
  const [sameHostOnly, setSameHostOnly] = useState(true);
  const [isScanning, setIsScanning] = useState(false);
  const [grabError, setGrabError] = useState<string | null>(null);
  const [grabAssets, setGrabAssets] = useState<GrabbedAsset[]>([]);
  const [grabFilter, setGrabFilter] = useState<'all' | 'file' | 'media' | 'image'>('all');
  const [selectedGrabUrls, setSelectedGrabUrls] = useState<Set<string>>(new Set());

  // Browser page links state
  const [browserLinks, setBrowserLinks] = useState<BatchQueueItem[]>(initialBrowserLinks);
  const [browserFilter, setBrowserFilter] = useState('');
  const [selectedBrowserUrls, setSelectedBrowserUrls] = useState<Set<string>>(new Set());

  useEffect(() => {
    if (!isOpen) return;
    setActiveTab(initialTab);
    if (initialBatchText) {
      setBatchText(initialBatchText);
      const urls = parseUrlListText(initialBatchText);
      setExpandedUrls(urls);
      setSelectedBatchUrls(new Set(urls));
    }
    if (initialBrowserLinks.length > 0) {
      setBrowserLinks(initialBrowserLinks);
      setSelectedBrowserUrls(new Set(initialBrowserLinks.map((b) => b.url)));
    }
  }, [isOpen, initialTab, initialBatchText, initialBrowserLinks]);

  if (!isOpen) return null;

  const handleBatchTextChange = (txt: string) => {
    setBatchText(txt);
    const urls = parseUrlListText(txt);
    setExpandedUrls(urls);
    setSelectedBatchUrls(new Set(urls));
  };

  const handleImportFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const text = await file.text();
    handleBatchTextChange(text);
  };

  const handleRunGrabber = async () => {
    const target = grabUrl.trim();
    if (!target) return;
    setIsScanning(true);
    setGrabError(null);
    try {
      const assets = await grabSiteLinks({
        url: target,
        maxDepth,
        sameHostOnly,
        headers: defaultHeaders,
      });
      setGrabAssets(assets);
      setSelectedGrabUrls(new Set(assets.map((a) => a.url)));
    } catch (err) {
      setGrabError(String(err));
    } finally {
      setIsScanning(false);
    }
  };

  const filteredGrabAssets = grabAssets.filter((a) => {
    if (grabFilter === 'all') return true;
    return a.resourceType === grabFilter;
  });

  const filteredBrowserLinks = browserLinks.filter((item) => {
    if (!browserFilter.trim()) return true;
    const q = browserFilter.toLowerCase();
    return item.url.toLowerCase().includes(q) || item.fileName.toLowerCase().includes(q);
  });

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-xs p-4 select-none">
      <div className="bg-bg border border-border rounded-lg shadow-pop w-full max-w-2xl overflow-hidden animate-in fade-in zoom-in-95 duration-150">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-surface">
          <div className="flex items-center space-x-2">
            <Layers className="w-4 h-4 text-accent" />
            <h2 className="text-sm font-bold text-text">{t('batch.title')}</h2>
          </div>
          <button
            onClick={onClose}
            aria-label={t('batch.closeAria')}
            className="p-1 rounded-md text-text-muted hover:text-text hover:bg-surface-hover"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Navigation Tabs */}
        <div className="flex border-b border-border bg-surface/60 px-4 space-x-1 text-xs">
          <button
            type="button"
            onClick={() => setActiveTab('batch')}
            className={`px-3 py-2 font-semibold border-b-2 flex items-center space-x-1.5 ${
              activeTab === 'batch'
                ? 'border-accent text-accent'
                : 'border-transparent text-text-muted hover:text-text'
            }`}
          >
            <Layers className="w-3.5 h-3.5" />
            <span>{t('batch.tabBatch')}</span>
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('grabber')}
            className={`px-3 py-2 font-semibold border-b-2 flex items-center space-x-1.5 ${
              activeTab === 'grabber'
                ? 'border-accent text-accent'
                : 'border-transparent text-text-muted hover:text-text'
            }`}
          >
            <Globe className="w-3.5 h-3.5" />
            <span>{t('batch.tabGrabber')}</span>
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('browser')}
            className={`px-3 py-2 font-semibold border-b-2 flex items-center space-x-1.5 ${
              activeTab === 'browser'
                ? 'border-accent text-accent'
                : 'border-transparent text-text-muted hover:text-text'
            }`}
          >
            <Link2 className="w-3.5 h-3.5" />
            <span>{t('batch.tabBrowser', { count: browserLinks.length })}</span>
          </button>
        </div>

        {/* Body */}
        <div className="p-4 text-xs space-y-3 max-h-[68vh] overflow-y-auto">
          {activeTab === 'batch' && (
            <>
              <div className="flex items-center justify-between gap-3">
                <p className="text-[11px] text-text-muted">
                  {t('batch.patternHint')}
                </p>
                <label className="flex items-center space-x-1.5 px-2.5 py-1 rounded-md bg-surface border border-border hover:bg-surface-hover cursor-pointer font-medium shrink-0">
                  <Upload className="w-3.5 h-3.5 text-accent" />
                  <span>{t('batch.importFile')}</span>
                  <input
                    type="file"
                    accept=".txt,.ef2,.lst"
                    onChange={handleImportFile}
                    className="hidden"
                  />
                </label>
              </div>

              <textarea
                rows={4}
                value={batchText}
                onChange={(e) => handleBatchTextChange(e.target.value)}
                aria-label={t('batch.patternLabel')}
                placeholder={'https://cdn.example.com/part_[01-05].zip\nhttps://example.com/manual.pdf'}
                className="w-full p-2.5 rounded-md bg-surface border border-border-input/40 font-mono text-[11px] text-text focus:outline-none focus:ring-1 focus:ring-accent"
              />

              <div className="space-y-1.5">
                <div className="flex items-center justify-between text-[11px] font-semibold text-text">
                  <span>
                    {t('batch.previewSummary', {
                      selected: selectedBatchUrls.size,
                      total: expandedUrls.length,
                    })}
                  </span>
                  {expandedUrls.length > 0 && (
                    <button
                      type="button"
                      onClick={() => {
                        if (selectedBatchUrls.size === expandedUrls.length) {
                          setSelectedBatchUrls(new Set());
                        } else {
                          setSelectedBatchUrls(new Set(expandedUrls));
                        }
                      }}
                      className="text-accent hover:underline"
                    >
                      {selectedBatchUrls.size === expandedUrls.length
                        ? t('batch.clearAll')
                        : t('batch.selectAll')}
                    </button>
                  )}
                </div>

                <div className="max-h-44 overflow-y-auto border border-border rounded-md divide-y divide-border/50 bg-surface/50">
                  {expandedUrls.length === 0 ? (
                    <p className="p-3 text-center text-text-muted text-[11px]">
                      {t('batch.emptyPreview')}
                    </p>
                  ) : (
                    expandedUrls.map((u) => (
                      <label
                        key={u}
                        className="flex items-center space-x-2 px-2.5 py-1.5 hover:bg-surface-hover cursor-pointer"
                      >
                        <input
                          type="checkbox"
                          checked={selectedBatchUrls.has(u)}
                          onChange={() => {
                            const next = new Set(selectedBatchUrls);
                            if (next.has(u)) next.delete(u);
                            else next.add(u);
                            setSelectedBatchUrls(next);
                          }}
                          className="rounded border-border text-accent"
                        />
                        <span className="font-mono text-[11px] truncate text-text">{u}</span>
                      </label>
                    ))
                  )}
                </div>
              </div>
            </>
          )}

          {activeTab === 'grabber' && (
            <>
              <div className="flex space-x-2">
                <input
                  type="url"
                  value={grabUrl}
                  onChange={(e) => setGrabUrl(e.target.value)}
                  placeholder="https://docs.example.com/downloads/"
                  aria-label={t('batch.grabUrlAria')}
                  className="flex-1 px-3 py-1.5 rounded-md bg-surface border border-border-input/40 font-mono text-text"
                />
                <select
                  value={maxDepth}
                  onChange={(e) => setMaxDepth(Number(e.target.value))}
                  aria-label={t('batch.grabDepthAria')}
                  className="px-2.5 py-1.5 rounded-md bg-surface border border-border text-text"
                >
                  <option value={1}>{t('batch.grabDepthOption', { n: 1 })}</option>
                  <option value={2}>{t('batch.grabDepthOption', { n: 2 })}</option>
                  <option value={3}>{t('batch.grabDepthOption', { n: 3 })}</option>
                </select>
                <button
                  type="button"
                  disabled={isScanning || !grabUrl.trim()}
                  onClick={handleRunGrabber}
                  className="flex items-center space-x-1.5 px-3 py-1.5 rounded-md bg-accent text-on-accent font-semibold hover:bg-accent-hover disabled:opacity-40"
                >
                  {isScanning ? (
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  ) : (
                    <Search className="w-3.5 h-3.5" />
                  )}
                  <span>{t('batch.grabScan')}</span>
                </button>
              </div>

              <div className="flex items-center justify-between text-[11px]">
                <label className="flex items-center space-x-1.5 cursor-pointer text-text-muted">
                  <input
                    type="checkbox"
                    checked={sameHostOnly}
                    onChange={(e) => setSameHostOnly(e.target.checked)}
                    className="rounded border-border text-accent"
                  />
                  <span>{t('batch.grabSameHost')}</span>
                </label>

                <div className="flex space-x-1">
                  {(['all', 'file', 'media', 'image'] as const).map((f) => (
                    <button
                      key={f}
                      type="button"
                      onClick={() => setGrabFilter(f)}
                      className={`px-2 py-0.5 rounded capitalize ${
                        grabFilter === f
                          ? 'bg-accent text-on-accent font-semibold'
                          : 'bg-surface border border-border text-text-muted'
                      }`}
                    >
                      {f === 'all' ? t('batch.grabFilterAll') : f}
                    </button>
                  ))}
                </div>
              </div>

              {grabError && (
                <p className="p-2 rounded bg-danger/10 border border-danger/30 text-danger text-[11px]">
                  {grabError}
                </p>
              )}

              <div className="max-h-52 overflow-y-auto border border-border rounded-md divide-y divide-border/50 bg-surface/50">
                {filteredGrabAssets.length === 0 ? (
                  <p className="p-4 text-center text-text-muted text-[11px]">
                    {t('batch.grabNoResult')}
                  </p>
                ) : (
                  filteredGrabAssets.map((asset) => (
                    <label
                      key={asset.url}
                      className="flex items-center justify-between px-2.5 py-1.5 hover:bg-surface-hover cursor-pointer gap-2"
                    >
                      <div className="flex items-center space-x-2 min-w-0">
                        <input
                          type="checkbox"
                          checked={selectedGrabUrls.has(asset.url)}
                          onChange={() => {
                            const next = new Set(selectedGrabUrls);
                            if (next.has(asset.url)) next.delete(asset.url);
                            else next.add(asset.url);
                            setSelectedGrabUrls(next);
                          }}
                          className="rounded border-border text-accent shrink-0"
                        />
                        <span className="font-medium text-text truncate">{asset.fileName}</span>
                      </div>
                      <div className="flex items-center space-x-1.5 shrink-0">
                        <span className="px-1.5 py-0.5 rounded bg-surface border border-border text-[10px] font-mono">
                          .{asset.extension || 'file'}
                        </span>
                        <span className="text-[10px] text-text-muted uppercase">
                          {asset.resourceType}
                        </span>
                      </div>
                    </label>
                  ))
                )}
              </div>
            </>
          )}

          {activeTab === 'browser' && (
            <>
              <div className="flex items-center justify-between space-x-2">
                <input
                  type="text"
                  value={browserFilter}
                  onChange={(e) => setBrowserFilter(e.target.value)}
                  placeholder={t('batch.browserFilterPlaceholder')}
                  className="flex-1 px-2.5 py-1.5 rounded-md bg-surface border border-border-input/40 text-text text-xs"
                />
                <button
                  type="button"
                  onClick={() => {
                    const visible = filteredBrowserLinks.map((b) => b.url);
                    const allSelected = visible.every((u) => selectedBrowserUrls.has(u));
                    const next = new Set(selectedBrowserUrls);
                    if (allSelected) visible.forEach((u) => next.delete(u));
                    else visible.forEach((u) => next.add(u));
                    setSelectedBrowserUrls(next);
                  }}
                  className="flex items-center space-x-1 px-2.5 py-1.5 rounded-md bg-surface border border-border text-text hover:bg-surface-hover"
                >
                  <CheckSquare className="w-3.5 h-3.5 text-accent" />
                  <span>{t('batch.toggleVisible')}</span>
                </button>
              </div>

              <div className="max-h-52 overflow-y-auto border border-border rounded-md divide-y divide-border/50 bg-surface/50">
                {filteredBrowserLinks.length === 0 ? (
                  <p className="p-4 text-center text-text-muted text-[11px]">
                    {t('batch.browserEmpty')}
                  </p>
                ) : (
                  filteredBrowserLinks.map((item) => (
                    <label
                      key={item.url}
                      className="flex items-center space-x-2 px-2.5 py-1.5 hover:bg-surface-hover cursor-pointer"
                    >
                      <input
                        type="checkbox"
                        checked={selectedBrowserUrls.has(item.url)}
                        onChange={() => {
                          const next = new Set(selectedBrowserUrls);
                          if (next.has(item.url)) next.delete(item.url);
                          else next.add(item.url);
                          setSelectedBrowserUrls(next);
                        }}
                        className="rounded border-border text-accent"
                      />
                      <span className="font-mono text-[11px] truncate text-text" title={item.url}>
                        {item.fileName || item.url}
                      </span>
                    </label>
                  ))
                )}
              </div>
            </>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-4 py-3 border-t border-border bg-surface">
          <span className="text-[11px] text-text-muted">
            {activeTab === 'batch' && t('batch.readyBatchCount', { count: selectedBatchUrls.size })}
            {activeTab === 'grabber' && t('batch.readyGrabCount', { count: selectedGrabUrls.size })}
            {activeTab === 'browser' &&
              t('batch.readyBrowserCount', { count: selectedBrowserUrls.size })}
          </span>
          <div className="flex space-x-2">
            <button
              type="button"
              onClick={onClose}
              className="px-3 py-1.5 rounded-md text-text hover:bg-surface-hover text-xs font-medium"
            >
              {t('common.cancel')}
            </button>
            <button
              type="button"
              onClick={() => {
                let items: BatchQueueItem[] = [];
                if (activeTab === 'batch') {
                  items = expandedUrls
                    .filter((u) => selectedBatchUrls.has(u))
                    .map((u) => ({ url: u, fileName: deriveFileNameFromUrl(u) }));
                } else if (activeTab === 'grabber') {
                  items = grabAssets
                    .filter((a) => selectedGrabUrls.has(a.url))
                    .map((a) => ({
                      url: a.url,
                      fileName: a.fileName || deriveFileNameFromUrl(a.url),
                      referer: a.sourcePage,
                    }));
                } else {
                  items = browserLinks.filter((b) => selectedBrowserUrls.has(b.url));
                }
                if (items.length > 0) {
                  onEnqueueBatch(items);
                  onClose();
                }
              }}
              className="flex items-center space-x-1.5 px-4 py-1.5 rounded-md bg-accent text-on-accent text-xs font-semibold hover:bg-accent-hover transition-colors"
            >
              <Download className="w-3.5 h-3.5" />
              <span>{t('batch.addToQueue')}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
