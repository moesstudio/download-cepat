import React from 'react';
import { CheckCircle2, FolderOpen, Play, ShieldCheck, X } from 'lucide-react';
import { DownloadTask } from '../types.ts';
import { useI18n } from '../i18n/index.tsx';

interface DownloadCompleteModalProps {
  task: DownloadTask | null;
  isOpen: boolean;
  onClose: () => void;
  onOpenFile: (task: DownloadTask) => void;
  onOpenFolder: (task: DownloadTask) => void;
  onVerifyChecksum?: (task: DownloadTask) => void;
}

export const DownloadCompleteModal: React.FC<DownloadCompleteModalProps> = ({
  task,
  isOpen,
  onClose,
  onOpenFile,
  onOpenFolder,
  onVerifyChecksum,
}) => {
  const { t } = useI18n();
  if (!isOpen || !task) return null;

  const formatBytes = (bytes: number | null) => {
    if (!bytes) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(2))} ${sizes[i]}`;
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-xs p-4 select-none">
      <div className="bg-bg border border-border rounded-lg shadow-pop w-full max-w-md overflow-hidden animate-in fade-in zoom-in-95 duration-150">
        <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-surface">
          <div className="flex items-center space-x-2 text-success font-bold text-sm">
            <CheckCircle2 className="w-4 h-4" />
            <span>{t('complete.title')}</span>
          </div>
          <button onClick={onClose} className="p-1 rounded-md text-text-muted hover:text-text">
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="p-5 text-center space-y-3">
          <div className="w-12 h-12 rounded-full bg-success/10 text-success mx-auto flex items-center justify-center">
            <CheckCircle2 className="w-6 h-6" />
          </div>
          <div>
            <h3 className="text-sm font-bold text-text truncate px-4" title={task.fileName}>
              {task.fileName}
            </h3>
            <p className="text-xs text-text-muted mt-1">
              {t('complete.fileSize')} <span className="font-mono font-semibold text-text">{formatBytes(task.fileSize)}</span>
            </p>
          </div>
          <p className="text-[11px] text-text-muted bg-surface p-2 rounded-md border border-border font-mono truncate" title={task.destinationPath}>
            {task.destinationPath}
          </p>
        </div>

        <div className="flex items-center justify-between px-4 py-3 border-t border-border bg-surface">
          <div className="flex space-x-2">
            <button
              onClick={() => onOpenFile(task)}
              className="flex items-center space-x-1.5 px-3 py-1.5 rounded-md bg-accent text-on-accent text-xs font-semibold hover:bg-accent-hover transition-colors shadow-sm"
            >
              <Play className="w-3.5 h-3.5 fill-on-accent" />
              <span>{t('context.openFile')}</span>
            </button>
            <button
              onClick={() => onOpenFolder(task)}
              className="flex items-center space-x-1.5 px-3 py-1.5 rounded-md bg-surface border border-border text-text hover:bg-surface-hover text-xs font-medium transition-colors"
            >
              <FolderOpen className="w-3.5 h-3.5 text-text-muted" />
              <span>{t('common.openFolder')}</span>
            </button>
            {onVerifyChecksum && (
              <button
                onClick={() => {
                  onClose();
                  onVerifyChecksum(task);
                }}
                className="flex items-center space-x-1.5 px-3 py-1.5 rounded-md bg-surface border border-border text-text hover:bg-surface-hover text-xs font-medium transition-colors"
              >
                <ShieldCheck className="w-3.5 h-3.5 text-accent" />
                <span>{t('complete.verifyChecksum')}</span>
              </button>
            )}
          </div>
          <button
            onClick={onClose}
            className="px-3 py-1.5 text-xs text-text-muted hover:text-text font-medium"
          >
            {t('common.close')}
          </button>
        </div>
      </div>
    </div>
  );
};
