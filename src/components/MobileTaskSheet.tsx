import React from 'react';
import {
  X,
  Play,
  Pause,
  FolderOpen,
  Share2,
  BarChart2,
  ShieldCheck,
  Info,
  Copy,
  Trash2,
} from 'lucide-react';
import { DownloadTask } from '../types.ts';
import { useI18n } from '../i18n/index.tsx';

interface MobileTaskSheetProps {
  task: DownloadTask | null;
  onClose: () => void;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onOpenFile: (task: DownloadTask) => void;
  onShareFile: (task: DownloadTask) => void;
  onOpenProgress: (task: DownloadTask) => void;
  onOpenProperties: (task: DownloadTask, tab?: 'info' | 'checksum') => void;
  onCopyUrl: (url: string) => void;
  onDelete: (id: string) => void;
}

export const MobileTaskSheet: React.FC<MobileTaskSheetProps> = ({
  task,
  onClose,
  onPause,
  onResume,
  onOpenFile,
  onShareFile,
  onOpenProgress,
  onOpenProperties,
  onCopyUrl,
  onDelete,
}) => {
  const { t } = useI18n();
  if (!task) return null;

  const isCompleted = task.status === 'completed';
  const isDownloading = task.status === 'downloading' || task.status === 'connecting';

  const formatBytes = (bytes: number | null | undefined) => {
    if (!bytes || bytes <= 0) return t('common.unknown');
    if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${bytes} B`;
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-end justify-center bg-black/60 backdrop-blur-xs select-none"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-label={task.fileName}
    >
      <div
        className="w-full max-w-lg bg-bg border-t border-border rounded-t-2xl shadow-pop overflow-hidden animate-in slide-in-from-bottom duration-200 pb-[env(safe-area-inset-bottom)]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex justify-center pt-2.5 pb-1">
          <div className="w-10 h-1 rounded-full bg-border" />
        </div>

        <div className="flex items-start justify-between px-4 py-2.5 border-b border-border gap-3">
          <div className="min-w-0 flex-1">
            <h3 className="text-sm font-bold text-text truncate">{task.fileName}</h3>
            <p className="text-[11px] text-text-muted font-mono mt-0.5 truncate">
              {formatBytes(task.downloadedBytes)} / {formatBytes(task.fileSize)} •{' '}
              {task.progressPercent.toFixed(1)}%
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label={t('common.close')}
            className="p-2 rounded-lg text-text-muted hover:text-text hover:bg-surface-hover"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="p-2 divide-y divide-border/40 text-xs">
          {isCompleted ? (
            <div className="pb-1 space-y-1">
              <button
                type="button"
                onClick={() => {
                  onOpenFile(task);
                  onClose();
                }}
                className="w-full min-h-[44px] px-3 rounded-lg flex items-center space-x-3 text-text hover:bg-surface-hover font-medium text-left"
              >
                <FolderOpen className="w-4 h-4 text-accent shrink-0" />
                <span>{t('common.openFile')}</span>
              </button>
              <button
                type="button"
                onClick={() => {
                  onShareFile(task);
                  onClose();
                }}
                className="w-full min-h-[44px] px-3 rounded-lg flex items-center space-x-3 text-text hover:bg-surface-hover font-medium text-left"
              >
                <Share2 className="w-4 h-4 text-emerald-400 shrink-0" />
                <span>{t('mobile.shareFile')}</span>
              </button>
            </div>
          ) : (
            <div className="pb-1 space-y-1">
              {isDownloading ? (
                <button
                  type="button"
                  onClick={() => {
                    onPause(task.id);
                    onClose();
                  }}
                  className="w-full min-h-[44px] px-3 rounded-lg flex items-center space-x-3 text-warning hover:bg-surface-hover font-medium text-left"
                >
                  <Pause className="w-4 h-4 shrink-0" />
                  <span>{t('common.pause')}</span>
                </button>
              ) : (
                <button
                  type="button"
                  onClick={() => {
                    onResume(task.id);
                    onClose();
                  }}
                  className="w-full min-h-[44px] px-3 rounded-lg flex items-center space-x-3 text-accent hover:bg-surface-hover font-medium text-left"
                >
                  <Play className="w-4 h-4 shrink-0" />
                  <span>{t('common.resume')}</span>
                </button>
              )}
              <button
                type="button"
                onClick={() => {
                  onOpenProgress(task);
                  onClose();
                }}
                className="w-full min-h-[44px] px-3 rounded-lg flex items-center space-x-3 text-text hover:bg-surface-hover font-medium text-left"
              >
                <BarChart2 className="w-4 h-4 text-accent shrink-0" />
                <span>{t('table.colProgress')}</span>
              </button>
            </div>
          )}

          <div className="pt-1 pb-1 space-y-1">
            <button
              type="button"
              onClick={() => {
                onOpenProperties(task, 'info');
                onClose();
              }}
              className="w-full min-h-[44px] px-3 rounded-lg flex items-center space-x-3 text-text hover:bg-surface-hover font-medium text-left"
            >
              <Info className="w-4 h-4 text-text-muted shrink-0" />
              <span>{t('context.properties')}</span>
            </button>
            {isCompleted && (
              <button
                type="button"
                onClick={() => {
                  onOpenProperties(task, 'checksum');
                  onClose();
                }}
                className="w-full min-h-[44px] px-3 rounded-lg flex items-center space-x-3 text-text hover:bg-surface-hover font-medium text-left"
              >
                <ShieldCheck className="w-4 h-4 text-emerald-400 shrink-0" />
                <span>{t('context.verifyChecksum')}</span>
              </button>
            )}
            <button
              type="button"
              onClick={() => {
                onCopyUrl(task.url);
                onClose();
              }}
              className="w-full min-h-[44px] px-3 rounded-lg flex items-center space-x-3 text-text hover:bg-surface-hover font-medium text-left"
            >
              <Copy className="w-4 h-4 text-text-muted shrink-0" />
              <span>{t('context.copyUrl')}</span>
            </button>
          </div>

          <div className="pt-1">
            <button
              type="button"
              onClick={() => {
                onDelete(task.id);
                onClose();
              }}
              className="w-full min-h-[44px] px-3 rounded-lg flex items-center space-x-3 text-danger hover:bg-danger/10 font-semibold text-left"
            >
              <Trash2 className="w-4 h-4 shrink-0" />
              <span>{t('common.delete')}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
