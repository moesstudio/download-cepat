import React from 'react';
import {
  Plus,
  Play,
  Pause,
  Square,
  Trash2,
  SlidersHorizontal,
  Search,
  FolderOpen,
  Layers,
  Clock,
  ArrowDownToLine,
  Globe,
  Info,
  Magnet,
  CheckCheck,
} from 'lucide-react';
import { LANGUAGE_OPTIONS, LanguageCode, useI18n } from '../i18n/index.tsx';

interface ToolbarProps {
  onAddUrl: () => void;
  onOpenTorrent?: () => void;
  onOpenBatchGrabber?: () => void;
  onOpenScheduler?: () => void;
  onToggleMiniDrop?: () => void;
  isMiniDropOpen?: boolean;
  isSchedulerEnabled?: boolean;
  onResume: () => void;
  onPause: () => void;
  onStopAll: () => void;
  onDelete: () => void;
  onClearCompleted?: () => void;
  completedCount?: number;
  onOpenFolder: () => void;
  onOpenSettings: () => void;
  onOpenBuildInfo?: () => void;
  onChangeLanguage?: (nextLang: LanguageCode) => void;
  searchQuery: string;
  onSearchChange: (q: string) => void;
  hasSelectedTask: boolean;
  markedCount?: number;
  onDeleteMarked?: () => void;
  onClearMarked?: () => void;
}

export const Toolbar: React.FC<ToolbarProps> = ({
  onAddUrl,
  onOpenTorrent,
  onOpenBatchGrabber,
  onOpenScheduler,
  onToggleMiniDrop,
  isMiniDropOpen = false,
  isSchedulerEnabled = false,
  onResume,
  onPause,
  onStopAll,
  onDelete,
  onClearCompleted,
  completedCount = 0,
  onOpenFolder,
  onOpenSettings,
  onOpenBuildInfo,
  onChangeLanguage,
  searchQuery,
  onSearchChange,
  hasSelectedTask,
  markedCount = 0,
  onDeleteMarked,
  onClearMarked,
}) => {
  const { t, lang, setLang } = useI18n();

  const handleLanguageSelect = (next: LanguageCode) => {
    setLang(next);
    if (onChangeLanguage) onChangeLanguage(next);
  };

  return (
    <div
      data-testid="main-toolbar"
      className="min-h-14 border-b border-border bg-surface px-2.5 py-1.5 flex flex-wrap items-center justify-between gap-2 shrink-0 select-none"
    >
      {/* IDM-Style Wrapped Icon-Above-Label Action Buttons (No Horizontal Scroll) */}
      <div className="flex flex-wrap items-center gap-1">
        <button
          onClick={onAddUrl}
          title={t('toolbar.addUrlTitle')}
          className="flex flex-col items-center justify-center min-w-[56px] px-2.5 py-1 rounded-md bg-accent text-on-accent text-[10px] leading-tight font-semibold shadow-sm hover:bg-accent-hover transition-colors focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-1"
        >
          <Plus className="w-4 h-4 mb-0.5" />
          <span className="whitespace-nowrap">{t('toolbar.addUrl')}</span>
        </button>

        {onOpenTorrent && (
          <button
            onClick={onOpenTorrent}
            title={t('toolbar.addTorrentTitle')}
            className="flex flex-col items-center justify-center min-w-[52px] px-2 py-1 rounded-md bg-bg border border-border text-text hover:bg-surface-hover text-[10px] leading-tight font-medium transition-colors"
          >
            <Magnet className="w-4 h-4 mb-0.5 text-cyan-500" />
            <span className="whitespace-nowrap">{t('toolbar.addTorrent')}</span>
          </button>
        )}

        {onOpenBatchGrabber && (
          <button
            onClick={onOpenBatchGrabber}
            title={t('toolbar.batchGrabTitle')}
            className="flex flex-col items-center justify-center min-w-[58px] px-2 py-1 rounded-md text-text bg-bg border border-border hover:bg-surface-hover text-[10px] leading-tight font-medium transition-colors"
          >
            <Layers className="w-4 h-4 mb-0.5 text-accent" />
            <span className="whitespace-nowrap">{t('toolbar.batchGrab')}</span>
          </button>
        )}

        {onOpenScheduler && (
          <button
            onClick={onOpenScheduler}
            title={t('toolbar.scheduleTitle')}
            className="relative flex flex-col items-center justify-center min-w-[52px] px-2 py-1 rounded-md text-text hover:bg-surface-hover text-[10px] leading-tight font-medium transition-colors"
          >
            <Clock className="w-4 h-4 mb-0.5 text-accent" />
            <span className="whitespace-nowrap">{t('toolbar.schedule')}</span>
            {isSchedulerEnabled && (
              <span className="w-1.5 h-1.5 rounded-full bg-success absolute top-1 right-1.5" />
            )}
          </button>
        )}

        <div className="hidden sm:block h-7 w-px bg-border mx-0.5" />

        <button
          onClick={onResume}
          disabled={!hasSelectedTask}
          title={t('context.resume')}
          className="flex flex-col items-center justify-center min-w-[48px] px-2 py-1 rounded-md text-text hover:bg-surface-hover disabled:opacity-40 disabled:pointer-events-none text-[10px] leading-tight font-medium transition-colors"
        >
          <Play className="w-4 h-4 mb-0.5 text-success fill-success" />
          <span className="whitespace-nowrap">{t('toolbar.resume')}</span>
        </button>

        <button
          onClick={onPause}
          disabled={!hasSelectedTask}
          title={t('context.pause')}
          className="flex flex-col items-center justify-center min-w-[46px] px-2 py-1 rounded-md text-text hover:bg-surface-hover disabled:opacity-40 disabled:pointer-events-none text-[10px] leading-tight font-medium transition-colors"
        >
          <Pause className="w-4 h-4 mb-0.5 text-warning fill-warning" />
          <span className="whitespace-nowrap">{t('toolbar.pause')}</span>
        </button>

        <button
          onClick={onStopAll}
          title={t('toolbar.stopAll')}
          className="flex flex-col items-center justify-center min-w-[50px] px-2 py-1 rounded-md text-text hover:bg-surface-hover text-[10px] leading-tight font-medium transition-colors"
        >
          <Square className="w-4 h-4 mb-0.5 text-danger fill-danger" />
          <span className="whitespace-nowrap">{t('toolbar.stopAll')}</span>
        </button>

        {markedCount > 0 ? (
          <div className="flex items-center space-x-1">
            <button
              onClick={onDeleteMarked}
              title={t('toolbar.deleteMarked', { count: markedCount })}
              className="flex flex-col items-center justify-center px-2.5 py-1 rounded-md text-[10px] leading-tight font-semibold bg-danger text-white hover:bg-danger/90 shadow-sm transition-all"
            >
              <Trash2 className="w-4 h-4 mb-0.5" />
              <span className="whitespace-nowrap">
                {t('toolbar.deleteMarked', { count: markedCount })}
              </span>
            </button>
            <button
              onClick={onClearMarked}
              title={t('toolbar.clearMarkedTitle')}
              className="px-2 py-1.5 rounded-md text-[10px] text-text-muted hover:text-text hover:bg-surface-hover transition-colors"
            >
              {t('common.cancel')}
            </button>
          </div>
        ) : (
          <button
            onClick={onDelete}
            disabled={!hasSelectedTask}
            title={t('toolbar.delete')}
            className="flex flex-col items-center justify-center min-w-[46px] px-2 py-1 rounded-md text-danger hover:bg-danger/10 disabled:opacity-40 disabled:pointer-events-none text-[10px] leading-tight font-medium transition-colors"
          >
            <Trash2 className="w-4 h-4 mb-0.5" />
            <span className="whitespace-nowrap">{t('toolbar.delete')}</span>
          </button>
        )}

        {onClearCompleted && (
          <button
            onClick={onClearCompleted}
            disabled={completedCount === 0}
            title={t('toolbar.clearDoneTitle')}
            className="flex flex-col items-center justify-center min-w-[54px] px-2 py-1 rounded-md text-text hover:bg-surface-hover disabled:opacity-40 disabled:pointer-events-none text-[10px] leading-tight font-medium transition-colors"
          >
            <CheckCheck className="w-4 h-4 mb-0.5 text-success" />
            <span className="whitespace-nowrap">{t('toolbar.clearDone')}</span>
          </button>
        )}

        <div className="hidden sm:block h-7 w-px bg-border mx-0.5" />

        <button
          onClick={onOpenFolder}
          title={t('toolbar.openFolder')}
          className="flex flex-col items-center justify-center min-w-[54px] px-2 py-1 rounded-md text-text hover:bg-surface-hover text-[10px] leading-tight font-medium transition-colors"
        >
          <FolderOpen className="w-4 h-4 mb-0.5 text-text-muted" />
          <span className="whitespace-nowrap">{t('toolbar.openFolder')}</span>
        </button>

        {onToggleMiniDrop && (
          <button
            onClick={onToggleMiniDrop}
            title={t('toolbar.dropTargetTitle')}
            aria-label={t('toolbar.dropTargetTitle')}
            className={`flex flex-col items-center justify-center min-w-[52px] px-2 py-1 rounded-md text-[10px] leading-tight font-medium transition-colors ${
              isMiniDropOpen
                ? 'bg-accent/15 text-accent'
                : 'text-text hover:bg-surface-hover'
            }`}
          >
            <ArrowDownToLine className="w-4 h-4 mb-0.5 text-accent" />
            <span className="whitespace-nowrap">{t('toolbar.dropTarget')}</span>
          </button>
        )}
      </div>

      {/* Search, Language, Info & Settings (wraps gracefully on non-fullscreen windows) */}
      <div className="flex flex-wrap items-center gap-1.5">
        <div className="relative">
          <Search className="w-3.5 h-3.5 text-text-muted absolute left-2.5 top-1/2 -translate-y-1/2" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => onSearchChange(e.target.value)}
            placeholder={t('toolbar.searchPlaceholder')}
            className="w-32 sm:w-36 lg:w-44 pl-8 pr-2.5 py-1.5 rounded-md bg-bg border border-border text-xs text-text placeholder:text-text-muted focus:outline-none focus:ring-1 focus:ring-accent"
          />
        </div>

        <div
          className="flex items-center space-x-1 px-1.5 py-1 rounded-md bg-bg border border-border text-xs text-text-muted"
          title={t('toolbar.languageTitle')}
        >
          <Globe className="w-3.5 h-3.5 text-accent shrink-0" />
          <select
            aria-label={t('toolbar.languageTitle')}
            value={lang}
            onChange={(e) => handleLanguageSelect(e.target.value as LanguageCode)}
            className="bg-transparent text-[11px] font-semibold text-text focus:outline-none cursor-pointer pr-0.5"
          >
            {LANGUAGE_OPTIONS.map((opt) => (
              <option key={opt.code} value={opt.code}>
                {opt.shortLabel}
              </option>
            ))}
          </select>
        </div>

        {onOpenBuildInfo && (
          <button
            onClick={onOpenBuildInfo}
            className="p-1.5 rounded-md text-text-muted hover:text-text hover:bg-surface-hover transition-colors"
            title={t('toolbar.buildInfoTitle')}
            aria-label={t('toolbar.buildInfoTitle')}
          >
            <Info className="w-4 h-4" />
          </button>
        )}

        <button
          onClick={onOpenSettings}
          className="p-1.5 rounded-md text-text-muted hover:text-text hover:bg-surface-hover transition-colors"
          title={t('toolbar.settingsTitle')}
          aria-label={t('toolbar.settingsTitle')}
        >
          <SlidersHorizontal className="w-4 h-4" />
        </button>
      </div>
    </div>
  );
};
