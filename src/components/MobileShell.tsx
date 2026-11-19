import React, { useState } from 'react';
import {
  Plus,
  Search,
  Settings,
  DownloadCloud,
  Play,
  Pause,
  MoreVertical,
  FolderOpen,
  Share2,
  Magnet,
  CheckCircle2,
  Activity,
  Trash2,
  X,
  PauseCircle,
} from 'lucide-react';
import { Category, DownloadTask } from '../types.ts';
import { SegmentedProgressBar } from './SegmentedProgressBar.tsx';
import { useI18n } from '../i18n/index.tsx';

export type MobileNavTab = 'all' | 'active' | 'done';

interface MobileShellProps {
  tasks: DownloadTask[];
  categories: Category[];
  activeCategory: string;
  onSelectCategory: (id: string) => void;
  searchQuery: string;
  onSearchChange: (q: string) => void;
  defaultConnections: number;
  updateAvailableVersion?: string | null;
  onAddUrl: () => void;
  onAddTorrent: () => void;
  onPauseTask: (id: string) => void;
  onResumeTask: (id: string) => void;
  onPauseAll: () => void;
  onClearCompleted: () => void;
  onOpenFile: (task: DownloadTask) => void;
  onShareFile: (task: DownloadTask) => void;
  onOpenTaskMenu: (task: DownloadTask) => void;
  onOpenProgress: (task: DownloadTask) => void;
  onOpenSettings: (tab?: 'network' | 'about') => void;
}

export const MobileShell: React.FC<MobileShellProps> = ({
  tasks,
  categories,
  activeCategory,
  onSelectCategory,
  searchQuery,
  onSearchChange,
  defaultConnections,
  updateAvailableVersion,
  onAddUrl,
  onAddTorrent,
  onPauseTask,
  onResumeTask,
  onPauseAll,
  onClearCompleted,
  onOpenFile,
  onShareFile,
  onOpenTaskMenu,
  onOpenProgress,
  onOpenSettings,
}) => {
  const { t } = useI18n();
  const [navTab, setNavTab] = useState<MobileNavTab>('all');
  const [isSearchOpen, setIsSearchOpen] = useState(false);

  const activeTasksCount = tasks.filter(
    (tk) => tk.status === 'downloading' || tk.status === 'connecting' || tk.status === 'assembling'
  ).length;
  const completedTasksCount = tasks.filter((tk) => tk.status === 'completed').length;
  const totalSpeedBps = tasks.reduce(
    (acc, tk) => acc + (tk.status === 'downloading' ? tk.speedBytesPerSec : 0),
    0
  );

  const formatBytes = (bytes?: number | null) => {
    if (!bytes || bytes <= 0) return t('common.unknown');
    if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${bytes} B`;
  };

  const formatSpeed = (bps: number) => {
    if (!bps || bps <= 0) return '0 KB/s';
    if (bps >= 1024 * 1024) return `${(bps / (1024 * 1024)).toFixed(2)} MB/s`;
    return `${(bps / 1024).toFixed(0)} KB/s`;
  };

  const formatEta = (secs?: number | null) => {
    if (secs === null || secs === undefined || secs < 0) return '--';
    if (secs < 60) return `${Math.ceil(secs)}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m ${Math.ceil(secs % 60)}s`;
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    return `${h}h ${m}m`;
  };

  const getCategoryLabel = (id: string, fallback: string) => {
    const keyMap: Record<string, Parameters<typeof t>[0]> = {
      all: 'sidebar.statusAll',
      compressed: 'sidebar.catCompressed',
      documents: 'sidebar.catDocuments',
      music: 'sidebar.catMusic',
      programs: 'sidebar.catPrograms',
      video: 'sidebar.catVideo',
      torrents: 'sidebar.catTorrents',
    };
    const key = keyMap[id];
    return key ? t(key) : fallback;
  };

  const visibleTasks = tasks.filter((task) => {
    if (navTab === 'active') {
      if (task.status === 'completed') return false;
    } else if (navTab === 'done') {
      if (task.status !== 'completed') return false;
    }

    if (
      navTab === 'all' &&
      activeCategory !== 'all' &&
      activeCategory !== 'unfinished' &&
      activeCategory !== 'finished' &&
      activeCategory !== 'completed'
    ) {
      if (task.categoryId !== activeCategory) return false;
    }

    if (searchQuery.trim()) {
      const q = searchQuery.toLowerCase();
      if (
        !task.fileName.toLowerCase().includes(q) &&
        !task.url.toLowerCase().includes(q)
      ) {
        return false;
      }
    }
    return true;
  });

  const statusBadgeColor = (status: DownloadTask['status']) => {
    switch (status) {
      case 'completed':
        return 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30';
      case 'downloading':
      case 'connecting':
      case 'assembling':
        return 'bg-accent/15 text-accent border-accent/30';
      case 'paused':
        return 'bg-warning/15 text-warning border-warning/30';
      case 'error':
        return 'bg-danger/15 text-danger border-danger/30';
      default:
        return 'bg-surface-hover text-text-muted border-border';
    }
  };

  return (
    <div
      className="flex flex-col h-[100dvh] w-full bg-bg text-text overflow-hidden select-none"
      data-testid="mobile-shell"
    >
      {/* Top App Bar */}
      <header className="shrink-0 bg-surface border-b border-border pt-[env(safe-area-inset-top)]">
        <div className="flex items-center justify-between px-3.5 h-14 gap-2">
          <div className="flex items-center space-x-2.5 min-w-0">
            <img
              src="/logo.png"
              alt="Download Cepat"
              className="w-8 h-8 rounded-lg shadow-sm object-cover border border-border/50 shrink-0"
            />
            <div className="min-w-0">
              <h1 className="text-sm font-bold text-text truncate">Download Cepat</h1>
              <p className="text-[11px] font-mono text-text-muted truncate">
                {totalSpeedBps > 0
                  ? `${formatSpeed(totalSpeedBps)} • ${activeTasksCount}`
                  : `${defaultConnections} ${t('common.threadsUnit')}`}
              </p>
            </div>
          </div>

          <div className="flex items-center space-x-1 shrink-0">
            {updateAvailableVersion && (
              <button
                type="button"
                onClick={() => onOpenSettings('about')}
                className="px-2 py-1 rounded-full bg-emerald-500/15 text-emerald-400 border border-emerald-500/30 text-[10px] font-bold"
              >
                v{updateAvailableVersion}
              </button>
            )}
            <button
              type="button"
              onClick={onAddTorrent}
              aria-label={t('toolbar.addTorrent')}
              className="p-2.5 rounded-lg text-text-muted hover:text-text hover:bg-surface-hover"
            >
              <Magnet className="w-4 h-4" />
            </button>
            {activeTasksCount > 0 && (
              <button
                type="button"
                onClick={onPauseAll}
                aria-label={t('toolbar.stopAll')}
                className="p-2.5 rounded-lg text-warning hover:bg-surface-hover"
              >
                <PauseCircle className="w-4 h-4" />
              </button>
            )}
            <button
              type="button"
              onClick={() => {
                setIsSearchOpen((prev) => !prev);
                if (isSearchOpen) onSearchChange('');
              }}
              aria-label={t('toolbar.searchPlaceholder')}
              className="p-2.5 rounded-lg text-text-muted hover:text-text hover:bg-surface-hover"
            >
              {isSearchOpen ? <X className="w-4 h-4" /> : <Search className="w-4 h-4" />}
            </button>
          </div>
        </div>

        {isSearchOpen && (
          <div className="px-3.5 pb-2.5">
            <input
              type="search"
              value={searchQuery}
              onChange={(e) => onSearchChange(e.target.value)}
              placeholder={t('toolbar.searchPlaceholder')}
              autoFocus
              className="w-full px-3 py-2 rounded-lg bg-bg border border-border text-xs text-text placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent"
            />
          </div>
        )}

        {/* Horizontal Category Chips (visible on All tab) */}
        {navTab === 'all' && (
          <div className="flex items-center space-x-1.5 px-3.5 pb-2.5 overflow-x-auto no-scrollbar">
            <button
              type="button"
              onClick={() => onSelectCategory('all')}
              className={`px-3 py-1.5 rounded-full text-[11px] font-semibold whitespace-nowrap border transition-colors ${
                activeCategory === 'all'
                  ? 'bg-accent text-on-accent border-accent'
                  : 'bg-bg text-text-muted border-border hover:text-text'
              }`}
            >
              {t('sidebar.statusAll')} ({tasks.length})
            </button>
            {categories.map((cat) => (
              <button
                key={cat.id}
                type="button"
                onClick={() => onSelectCategory(cat.id)}
                className={`px-3 py-1.5 rounded-full text-[11px] font-semibold whitespace-nowrap border transition-colors ${
                  activeCategory === cat.id
                    ? 'bg-accent text-on-accent border-accent'
                    : 'bg-bg text-text-muted border-border hover:text-text'
                }`}
              >
                {getCategoryLabel(cat.id, cat.name)} ({cat.count})
              </button>
            ))}
          </div>
        )}

        {/* Quick Clear Done Bar (on Done tab) */}
        {navTab === 'done' && completedTasksCount > 0 && (
          <div className="flex items-center justify-between px-3.5 pb-2.5 text-[11px] text-text-muted">
            <span>{t('mobile.savedToDownloads')}</span>
            <button
              type="button"
              onClick={onClearCompleted}
              className="inline-flex items-center space-x-1 px-2.5 py-1 rounded-md bg-bg border border-border text-text hover:bg-surface-hover font-medium"
            >
              <Trash2 className="w-3 h-3" />
              <span>{t('toolbar.clearDone')}</span>
            </button>
          </div>
        )}
      </header>

      {/* Main Scrollable Download Card List */}
      <main className="flex-1 overflow-y-auto p-3 space-y-2.5 relative">
        {visibleTasks.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center text-center px-6 py-12 space-y-3 text-text-muted">
            <DownloadCloud className="w-12 h-12 opacity-30" />
            <p className="text-sm font-medium text-text">{t('table.emptyTitle')}</p>
            <button
              type="button"
              onClick={onAddUrl}
              className="inline-flex items-center space-x-2 px-4 py-2.5 rounded-full bg-accent text-on-accent text-xs font-bold shadow-sm"
            >
              <Plus className="w-4 h-4" />
              <span>{t('toolbar.addUrl')}</span>
            </button>
          </div>
        ) : (
          visibleTasks.map((task) => {
            const isCompleted = task.status === 'completed';
            const isDownloading =
              task.status === 'downloading' ||
              task.status === 'connecting' ||
              task.status === 'assembling';

            return (
              <div
                key={task.id}
                className="bg-surface border border-border rounded-xl p-3 space-y-2.5 shadow-xs"
              >
                <div className="flex items-start justify-between gap-2">
                  <div
                    className="min-w-0 flex-1 cursor-pointer"
                    onClick={() => onOpenProgress(task)}
                  >
                    <p className="text-xs font-bold text-text truncate">{task.fileName}</p>
                    <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] font-mono text-text-muted mt-0.5">
                      <span>
                        {formatBytes(task.downloadedBytes)} / {formatBytes(task.fileSize)}
                      </span>
                      {isDownloading && (
                        <>
                          <span className="text-accent font-semibold">
                            {formatSpeed(task.speedBytesPerSec)}
                          </span>
                          <span>ETA {formatEta(task.etaSeconds)}</span>
                        </>
                      )}
                      {task.peers !== undefined && task.peers > 0 && (
                        <span>
                          P:{task.peers} S:{task.seeds ?? 0}
                        </span>
                      )}
                    </div>
                  </div>

                  <span
                    className={`px-2 py-0.5 rounded-full border text-[10px] font-semibold shrink-0 ${statusBadgeColor(
                      task.status
                    )}`}
                  >
                    {task.status === 'completed'
                      ? t('status.completed')
                      : task.status === 'paused'
                      ? t('status.paused')
                      : task.status === 'error'
                      ? t('status.failed')
                      : `${task.progressPercent.toFixed(0)}%`}
                  </span>
                </div>

                {!isCompleted && (
                  <SegmentedProgressBar
                    progressPercent={task.progressPercent}
                    status={task.status}
                    segments={task.segments}
                    variant="compact"
                    numConnections={task.numConnections}
                  />
                )}

                {task.errorMessage && (
                  <p className="text-[11px] text-danger leading-snug line-clamp-2">
                    {task.errorMessage}
                  </p>
                )}

                <div className="flex items-center justify-between pt-0.5">
                  <div className="flex items-center space-x-1.5">
                    {isCompleted ? (
                      <>
                        <button
                          type="button"
                          onClick={() => onOpenFile(task)}
                          className="min-h-[36px] px-3 rounded-lg bg-accent/15 text-accent hover:bg-accent/25 text-[11px] font-semibold inline-flex items-center space-x-1.5"
                        >
                          <FolderOpen className="w-3.5 h-3.5" />
                          <span>{t('common.openFile')}</span>
                        </button>
                        <button
                          type="button"
                          onClick={() => onShareFile(task)}
                          className="min-h-[36px] px-3 rounded-lg bg-bg border border-border text-text hover:bg-surface-hover text-[11px] font-semibold inline-flex items-center space-x-1.5"
                        >
                          <Share2 className="w-3.5 h-3.5 text-emerald-400" />
                          <span>{t('mobile.shareFile')}</span>
                        </button>
                      </>
                    ) : isDownloading ? (
                      <button
                        type="button"
                        onClick={() => onPauseTask(task.id)}
                        className="min-h-[36px] px-3 rounded-lg bg-warning/15 text-warning text-[11px] font-semibold inline-flex items-center space-x-1.5"
                      >
                        <Pause className="w-3.5 h-3.5" />
                        <span>{t('common.pause')}</span>
                      </button>
                    ) : (
                      <button
                        type="button"
                        onClick={() => onResumeTask(task.id)}
                        className="min-h-[36px] px-3 rounded-lg bg-accent/15 text-accent text-[11px] font-semibold inline-flex items-center space-x-1.5"
                      >
                        <Play className="w-3.5 h-3.5" />
                        <span>{t('common.resume')}</span>
                      </button>
                    )}
                  </div>

                  <button
                    type="button"
                    onClick={() => onOpenTaskMenu(task)}
                    aria-label={`${t('context.properties')} ${task.fileName}`}
                    className="p-2 rounded-lg text-text-muted hover:text-text hover:bg-surface-hover"
                  >
                    <MoreVertical className="w-4 h-4" />
                  </button>
                </div>
              </div>
            );
          })
        )}

        {/* Spacer above FAB & Bottom Nav */}
        <div className="h-16" />
      </main>

      {/* Floating Action Button (+) */}
      <button
        type="button"
        onClick={onAddUrl}
        aria-label={t('toolbar.addUrl')}
        className="fixed right-4 bottom-[calc(4.5rem+env(safe-area-inset-bottom))] z-30 w-14 h-14 rounded-full bg-accent text-on-accent shadow-pop flex items-center justify-center hover:bg-accent-hover active:scale-95 transition-transform"
      >
        <Plus className="w-6 h-6" />
      </button>

      {/* Touch Bottom Navigation */}
      <nav
        className="shrink-0 bg-surface border-t border-border pb-[env(safe-area-inset-bottom)]"
        aria-label="Mobile Navigation"
      >
        <div className="grid grid-cols-4 h-16">
          <button
            type="button"
            onClick={() => setNavTab('all')}
            className={`flex flex-col items-center justify-center space-y-1 text-[11px] font-semibold ${
              navTab === 'all' ? 'text-accent' : 'text-text-muted'
            }`}
          >
            <DownloadCloud className="w-5 h-5" />
            <span className="truncate max-w-full px-1">{t('sidebar.statusAll')}</span>
          </button>

          <button
            type="button"
            onClick={() => setNavTab('active')}
            className={`flex flex-col items-center justify-center space-y-1 text-[11px] font-semibold relative ${
              navTab === 'active' ? 'text-accent' : 'text-text-muted'
            }`}
          >
            <div className="relative">
              <Activity className="w-5 h-5" />
              {activeTasksCount > 0 && (
                <span className="absolute -top-1.5 -right-2.5 px-1.5 min-w-[16px] h-4 rounded-full bg-accent text-on-accent text-[9px] font-bold flex items-center justify-center">
                  {activeTasksCount}
                </span>
              )}
            </div>
            <span className="truncate max-w-full px-1">{t('sidebar.statusUnfinished')}</span>
          </button>

          <button
            type="button"
            onClick={() => setNavTab('done')}
            className={`flex flex-col items-center justify-center space-y-1 text-[11px] font-semibold ${
              navTab === 'done' ? 'text-accent' : 'text-text-muted'
            }`}
          >
            <CheckCircle2 className="w-5 h-5" />
            <span className="truncate max-w-full px-1">{t('sidebar.statusFinished')}</span>
          </button>

          <button
            type="button"
            onClick={() => onOpenSettings('network')}
            aria-label={t('toolbar.settingsTitle')}
            className="flex flex-col items-center justify-center space-y-1 text-[11px] font-semibold text-text-muted hover:text-text relative"
          >
            <div className="relative">
              <Settings className="w-5 h-5" />
              {updateAvailableVersion && (
                <span className="absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full bg-emerald-400" />
              )}
            </div>
            <span className="truncate max-w-full px-1">{t('mobile.navSettings')}</span>
          </button>
        </div>
      </nav>
    </div>
  );
};
