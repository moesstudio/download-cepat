import React from 'react';
import { DownloadSegment, TaskStatus } from '../types.ts';
import { useI18n } from '../i18n/index.tsx';

interface SegmentedProgressBarProps {
  progressPercent: number;
  status: TaskStatus;
  segments?: DownloadSegment[];
  variant?: 'compact' | 'detailed';
  numConnections?: number;
}

export const SegmentedProgressBar: React.FC<SegmentedProgressBarProps> = ({
  progressPercent,
  status,
  segments,
  variant = 'compact',
  numConnections = 8,
}) => {
  const { t } = useI18n();
  if (variant === 'compact') {
    return (
      <div 
        className="w-full bg-surface-hover rounded-pill h-2 overflow-hidden relative"
        role="progressbar"
        aria-valuenow={progressPercent}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <div
          className={`h-full transition-all duration-300 ease-out ${
            status === 'completed'
              ? 'bg-success'
              : status === 'error'
              ? 'bg-danger'
              : status === 'paused'
              ? 'bg-warning'
              : 'bg-accent'
          }`}
          style={{ width: `${Math.min(100, Math.max(0, progressPercent))}%` }}
        />
      </div>
    );
  }

  // Detailed mode: real multi-connection segment progress visualizer
  const fallbackTotal = 1000;
  const fallbackPct = Math.min(100, Math.max(0, progressPercent));
  const isAllDone = status === 'completed' || fallbackPct >= 100;
  const displaySegments =
    segments && segments.length > 0
      ? segments
      : Array.from({ length: Math.max(1, numConnections) }).map((_, i) => ({
          segmentIndex: i,
          startByte: 0,
          endByte: fallbackTotal - 1,
          currentByte: Math.round((fallbackPct / 100) * fallbackTotal),
          downloadedBytes: (fallbackPct / 100) * fallbackTotal,
          isCompleted: isAllDone,
        }));

  return (
    <div className="space-y-2">
      {/* Overall Progress Bar */}
      <div className="w-full bg-surface-hover rounded-md h-5 overflow-hidden border border-border relative flex items-center justify-center">
        <div
          className={`absolute left-0 top-0 bottom-0 transition-all duration-200 ${
            status === 'completed'
              ? 'bg-success'
              : status === 'error'
              ? 'bg-danger'
              : status === 'paused'
              ? 'bg-warning'
              : 'bg-accent'
          }`}
          style={{ width: `${Math.min(100, Math.max(0, progressPercent))}%` }}
        />
        <span className="relative z-10 text-xs font-mono font-bold text-text drop-shadow-sm">
          {progressPercent.toFixed(1)}%
        </span>
      </div>

      {/* Segment Threads Grid */}
      <div className="space-y-1">
        <div className="flex justify-between text-xs text-text-muted">
          <span>{t('segment.parallelConnections', { count: displaySegments.length })}</span>
          <span className="font-mono">
            {status === 'assembling'
              ? `⚡ ${t('segment.assemblingFile')}`
              : t('segment.completedSummary', {
                  done: displaySegments.filter((s) => s.isCompleted || isAllDone).length,
                  total: displaySegments.length,
                })}
          </span>
        </div>
        <div className="grid grid-cols-8 gap-1 p-1.5 bg-surface rounded-md border border-border">
          {displaySegments.map((seg, idx) => {
            const isCompleted = isAllDone || seg.isCompleted;
            const segTotal = Math.max(1, seg.endByte - seg.startByte + 1);
            const rawPct = (seg.downloadedBytes / segTotal) * 100;
            const segPercent = isCompleted
              ? 100
              : Math.min(100, Math.max(0, Number.isFinite(rawPct) ? rawPct : 0));

            return (
              <div
                key={seg.segmentIndex ?? idx}
                className="h-4 rounded-sm overflow-hidden bg-surface-hover relative border border-border/50"
                title={t('segment.threadTooltip', { id: idx + 1, pct: segPercent.toFixed(1) })}
              >
                <div
                  className={`h-full transition-all duration-200 ${
                    isCompleted
                      ? 'bg-success'
                      : status === 'downloading'
                      ? 'bg-accent'
                      : status === 'paused'
                      ? 'bg-warning'
                      : 'bg-accent/40'
                  }`}
                  style={{ width: `${segPercent.toFixed(1)}%` }}
                />
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
};
