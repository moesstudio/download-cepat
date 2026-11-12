import React from 'react';
import { X, Pause, Play, Square, Zap, AlertCircle, RefreshCw } from 'lucide-react';
import { DownloadTask } from '../types.ts';
import { SegmentedProgressBar } from './SegmentedProgressBar.tsx';
import { SpeedGraphCanvas } from './SpeedGraphCanvas.tsx';
import { formatEta } from '../utils/formatters.ts';
import { useI18n } from '../i18n/index.tsx';

interface DownloadProgressModalProps {
  task: DownloadTask | null;
  isOpen: boolean;
  onClose: () => void;
  onPause: (taskId: string) => void;
  onResume: (taskId: string) => void;
  onCancel: (taskId: string) => void;
  onOpenProperties?: (task: DownloadTask) => void;
}

export const DownloadProgressModal: React.FC<DownloadProgressModalProps> = ({
  task,
  isOpen,
  onClose,
  onPause,
  onResume,
  onCancel,
  onOpenProperties,
}) => {
  const { t, lang } = useI18n();
  if (!isOpen || !task) return null;

  const formatBytes = (bytes: number | null) => {
    if (bytes === null || bytes === undefined || bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(2))} ${sizes[i]}`;
  };

  const isPaused = task.status === 'paused';

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-xs p-4 select-none">
      <div className="bg-bg border border-border rounded-lg shadow-pop w-full max-w-xl overflow-hidden animate-in fade-in zoom-in-95 duration-150">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-surface">
          <div className="flex items-center space-x-2 truncate">
            <Zap className="w-4 h-4 text-accent shrink-0" />
            <h2 className="text-sm font-bold text-text truncate" title={task.fileName}>
              {task.fileName}
            </h2>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded-md text-text-muted hover:text-text hover:bg-surface-hover transition-colors shrink-0"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Content */}
        <div className="p-4 space-y-4 text-xs">
          {/* Key Metric Highlights */}
          <div className="grid grid-cols-3 gap-2 p-2.5 bg-surface rounded-md border border-border text-center">
            <div>
              <p className="text-[10px] text-text-muted uppercase font-semibold">{t('progress.speed')}</p>
              <p className="font-mono text-sm font-bold text-accent">
                {(task.speedBytesPerSec / (1024 * 1024)).toFixed(2)} MB/s
              </p>
            </div>
            <div>
              <p className="text-[10px] text-text-muted uppercase font-semibold">{t('progress.downloaded')}</p>
              <p className="font-mono text-sm font-bold text-text">
                {formatBytes(task.downloadedBytes)} / {formatBytes(task.fileSize)}
              </p>
            </div>
            <div>
              <p className="text-[10px] text-text-muted uppercase font-semibold">{t('progress.remaining')}</p>
              <p className="font-mono text-sm font-bold text-text">
                {formatEta(task.etaSeconds, true, lang)}
              </p>
            </div>
          </div>

          {/* Detailed Segmented Progress */}
          <SegmentedProgressBar
            progressPercent={task.progressPercent}
            status={task.status}
            variant="detailed"
            numConnections={task.numConnections || 8}
            segments={task.segments}
          />

          {/* Real-time Speed Chart */}
          <SpeedGraphCanvas
            currentSpeedBytesPerSec={task.speedBytesPerSec}
            height={85}
          />

          {/* Error Alert if Download Failed */}
          {task.status === 'error' && task.errorMessage && (
            <div className="p-2.5 rounded-md bg-red-500/10 border border-red-500/40 text-red-300 flex items-start space-x-2 text-[11px]">
              <AlertCircle className="w-4 h-4 text-red-400 shrink-0 mt-0.5" />
              <div className="space-y-0.5">
                <p className="font-bold text-red-400">{t('progress.errorTitle')}</p>
                <p className="leading-relaxed">{task.errorMessage}</p>
              </div>
            </div>
          )}

          {/* Details info */}
          <div className="space-y-1 text-[11px] text-text-muted border-t border-border pt-2">
            <div className="flex justify-between">
              <span>{t('progress.sourceUrl')}</span>
              <span className="font-mono truncate max-w-xs text-text" title={task.url}>{task.url}</span>
            </div>
            <div className="flex justify-between">
              <span>{t('progress.savePath')}</span>
              <span className="font-mono truncate max-w-xs text-text" title={task.destinationPath}>{task.destinationPath}</span>
            </div>
          </div>
        </div>

        {/* Action Controls */}
        <div className="flex items-center justify-between px-4 py-3 border-t border-border bg-surface">
          <div className="flex items-center space-x-2">
            <button
              onClick={() => isPaused ? onResume(task.id) : onPause(task.id)}
              className="flex items-center space-x-1.5 px-3 py-1.5 rounded-md bg-surface border border-border text-text hover:bg-surface-hover text-xs font-medium transition-colors"
            >
              {isPaused ? (
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
              onClick={() => onCancel(task.id)}
              className="flex items-center space-x-1.5 px-3 py-1.5 rounded-md bg-danger/10 text-danger hover:bg-danger/20 text-xs font-medium transition-colors"
            >
              <Square className="w-3.5 h-3.5 fill-danger" />
              <span>{t('progress.cancelBtn')}</span>
            </button>
            {onOpenProperties && (
              <button
                onClick={() => {
                  onClose();
                  onOpenProperties(task);
                }}
                className="flex items-center space-x-1.5 px-2.5 py-1.5 rounded-md bg-surface border border-border text-text hover:bg-surface-hover text-xs font-medium transition-colors"
                title={t('progress.propertiesTitle')}
              >
                <RefreshCw className="w-3.5 h-3.5 text-accent" />
                <span>{t('progress.propertiesBtn')}</span>
              </button>
            )}
          </div>

          <button
            onClick={onClose}
            className="px-4 py-1.5 rounded-md bg-accent text-on-accent text-xs font-semibold hover:bg-accent-hover transition-colors"
          >
            {t('common.close')}
          </button>
        </div>
      </div>
    </div>
  );
};
