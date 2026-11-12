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
  Info
} from 'lucide-react';
import { LANGUAGE_OPTIONS, LanguageCode, useI18n } from '../i18n/index.tsx';

interface ToolbarProps {
  onAddUrl: () => void;
  onOpenBatchGrabber?: () => void;
  onOpenScheduler?: () => void;
  onToggleMiniDrop?: () => void;
  isMiniDropOpen?: boolean;
  isSchedulerEnabled?: boolean;
  onResume: () => void;
  onPause: () => void;
  onStopAll: () => void;
  onDelete: () => void;
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
  onOpenBatchGrabber,
  onOpenScheduler,
  onToggleMiniDrop,
  isMiniDropOpen = false,
  isSchedulerEnabled = false,
  onResume,
  onPause,
  onStopAll,
  onDelete,
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
    <div className="h-14 border-b border-border bg-surface px-3 flex items-center justify-between gap-2 shrink-0 select-none">
      {/* Action Buttons */}
      <div className="flex items-center space-x-1 overflow-x-auto min-w-0">
        <button
          onClick={onAddUrl}
          title={t('toolbar.addUrlTitle')}
          className="flex items-center space-x-1.5 px-3 py-1.5 rounded-md bg-accent text-on-accent text-xs font-semibold shadow-sm hover:bg-accent-hover transition-colors focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-1 shrink-0"
        >
          <Plus className="w-4 h-4" />
          <span>{t('toolbar.addUrl')}</span>
        </button>

        {onOpenBatchGrabber && (
          <button
            onClick={onOpenBatchGrabber}
            className="flex items-center space-x-1 px-2.5 py-1.5 rounded-md text-xs font-medium text-text bg-bg border border-border hover:bg-surface-hover transition-colors shrink-0"
            title={t('toolbar.batchGrabTitle')}
          >
            <Layers className="w-3.5 h-3.5 text-accent" />
            <span>{t('toolbar.batchGrab')}</span>
          </button>
        )}

        {onOpenScheduler && (
          <button
            onClick={onOpenScheduler}
            className="relative flex items-center space-x-1 px-2.5 py-1.5 rounded-md text-xs font-medium text-text hover:bg-surface-hover transition-colors shrink-0"
            title={t('toolbar.scheduleTitle')}
          >
            <Clock className="w-3.5 h-3.5 text-accent" />
            <span>{t('toolbar.schedule')}</span>
            {isSchedulerEnabled && (
              <span className="w-1.5 h-1.5 rounded-full bg-success" />
            )}
          </button>
        )}

        <div className="h-5 w-px bg-border mx-1 shrink-0" />

        <button
          onClick={onResume}
          disabled={!hasSelectedTask}
          className="flex items-center space-x-1 px-2 py-1.5 rounded-md text-xs font-medium text-text hover:bg-surface-hover disabled:opacity-40 disabled:pointer-events-none transition-colors shrink-0"
          title={t('context.resume')}
        >
          <Play className="w-3.5 h-3.5 text-success fill-success" />
          <span>{t('toolbar.resume')}</span>
        </button>

        <button
          onClick={onPause}
          disabled={!hasSelectedTask}
          className="flex items-center space-x-1 px-2 py-1.5 rounded-md text-xs font-medium text-text hover:bg-surface-hover disabled:opacity-40 disabled:pointer-events-none transition-colors shrink-0"
          title={t('context.pause')}
        >
          <Pause className="w-3.5 h-3.5 text-warning fill-warning" />
          <span>{t('toolbar.pause')}</span>
        </button>

        <button
          onClick={onStopAll}
          className="flex items-center space-x-1 px-2 py-1.5 rounded-md text-xs font-medium text-text hover:bg-surface-hover transition-colors shrink-0"
          title={t('toolbar.stopAll')}
        >
          <Square className="w-3.5 h-3.5 text-danger fill-danger" />
          <span>{t('toolbar.stopAll')}</span>
        </button>

        {markedCount > 0 ? (
          <div className="flex items-center space-x-1 shrink-0">
            <button
              onClick={onDeleteMarked}
              className="flex items-center space-x-1.5 px-2.5 py-1.5 rounded-md text-xs font-semibold bg-danger text-white hover:bg-danger/90 shadow-sm transition-all"
              title={t('toolbar.deleteMarked', { count: markedCount })}
            >
              <Trash2 className="w-3.5 h-3.5" />
              <span>{t('toolbar.deleteMarked', { count: markedCount })}</span>
            </button>
            <button
              onClick={onClearMarked}
              className="px-2 py-1.5 rounded-md text-[11px] text-text-muted hover:text-text hover:bg-surface-hover transition-colors"
              title={t('toolbar.clearMarkedTitle')}
            >
              {t('common.cancel')}
            </button>
          </div>
        ) : (
          <button
            onClick={onDelete}
            disabled={!hasSelectedTask}
            className="flex items-center space-x-1 px-2 py-1.5 rounded-md text-xs font-medium text-danger hover:bg-danger/10 disabled:opacity-40 disabled:pointer-events-none transition-colors shrink-0"
            title={t('toolbar.delete')}
          >
            <Trash2 className="w-3.5 h-3.5" />
            <span>{t('toolbar.delete')}</span>
          </button>
        )}

        <div className="h-5 w-px bg-border mx-1 shrink-0" />

        <button
          onClick={onOpenFolder}
          className="flex items-center space-x-1 px-2 py-1.5 rounded-md text-xs font-medium text-text hover:bg-surface-hover transition-colors shrink-0"
          title={t('toolbar.openFolder')}
        >
          <FolderOpen className="w-3.5 h-3.5 text-text-muted" />
          <span>{t('toolbar.openFolder')}</span>
        </button>
      </div>

      {/* Search, Language, Info & Settings */}
      <div className="flex items-center space-x-1.5 shrink-0">
        <div className="relative">
          <Search className="w-3.5 h-3.5 text-text-muted absolute left-2.5 top-1/2 -translate-y-1/2" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => onSearchChange(e.target.value)}
            placeholder={t('toolbar.searchPlaceholder')}
            className="w-36 lg:w-44 pl-8 pr-2.5 py-1.5 rounded-md bg-bg border border-border text-xs text-text placeholder:text-text-muted focus:outline-none focus:ring-1 focus:ring-accent"
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

        {onToggleMiniDrop && (
          <button
            onClick={onToggleMiniDrop}
            className={`p-1.5 rounded-md transition-colors ${
              isMiniDropOpen
                ? 'bg-accent/15 text-accent'
                : 'text-text-muted hover:text-text hover:bg-surface-hover'
            }`}
            title={t('toolbar.dropTargetTitle')}
            aria-label={t('toolbar.dropTargetTitle')}
          >
            <ArrowDownToLine className="w-4 h-4" />
          </button>
        )}

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
