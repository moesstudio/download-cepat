import React from 'react';
import { 
  File, 
  FileArchive, 
  FileVideo, 
  FileAudio, 
  FileCode, 
  FileText,
  DownloadCloud,
  CheckCircle2,
  AlertCircle,
  PauseCircle,
  Clock,
  XCircle
} from 'lucide-react';
import { DownloadTask } from '../types.ts';
import { SegmentedProgressBar } from './SegmentedProgressBar.tsx';
import { formatEta } from '../utils/formatters.ts';
import { useI18n } from '../i18n/index.tsx';

interface TaskDataTableProps {
  tasks: DownloadTask[];
  selectedTaskId: string | null;
  markedTaskIds?: string[];
  onSelectTask: (taskId: string) => void;
  onToggleMarkTask?: (taskId: string) => void;
  onToggleMarkAll?: () => void;
  onOpenDetails: (task: DownloadTask) => void;
  onContextMenu?: (task: DownloadTask, event: React.MouseEvent) => void;
}

export const TaskDataTable: React.FC<TaskDataTableProps> = ({
  tasks,
  selectedTaskId,
  markedTaskIds = [],
  onSelectTask,
  onToggleMarkTask,
  onToggleMarkAll,
  onOpenDetails,
  onContextMenu,
}) => {
  const { t, lang } = useI18n();

  const formatBytes = (bytes: number | null) => {
    if (bytes === null || bytes === undefined || bytes === 0) return t('common.unknown');
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(2))} ${sizes[i]}`;
  };

  const formatSpeed = (bytesPerSec: number) => {
    if (bytesPerSec <= 0) return '-';
    return `${formatBytes(bytesPerSec)}/s`;
  };

  const getFileIcon = (_fileName: string, categoryId: string) => {
    switch (categoryId) {
      case 'compressed': return <FileArchive className="w-4 h-4 text-amber-600 shrink-0" />;
      case 'video': return <FileVideo className="w-4 h-4 text-emerald-600 shrink-0" />;
      case 'music': return <FileAudio className="w-4 h-4 text-purple-600 shrink-0" />;
      case 'programs': return <FileCode className="w-4 h-4 text-red-600 shrink-0" />;
      case 'documents': return <FileText className="w-4 h-4 text-blue-600 shrink-0" />;
      default: return <File className="w-4 h-4 text-text-muted shrink-0" />;
    }
  };

  const getStatusBadge = (task: DownloadTask) => {
    switch (task.status) {
      case 'downloading':
        return (
          <span className="inline-flex items-center space-x-1 text-xs text-accent font-medium">
            <span className="w-1.5 h-1.5 rounded-full bg-accent animate-ping mr-1" />
            {t('status.downloading')}
          </span>
        );
      case 'completed':
        return (
          <span className="inline-flex items-center space-x-1 text-xs text-success font-medium">
            <CheckCircle2 className="w-3.5 h-3.5 mr-0.5" />
            {t('status.completed')}
          </span>
        );
      case 'paused':
        return (
          <span className="inline-flex items-center space-x-1 text-xs text-warning font-medium">
            <PauseCircle className="w-3.5 h-3.5 mr-0.5" />
            {t('status.paused')}
          </span>
        );
      case 'assembling':
        return (
          <span className="inline-flex items-center space-x-1 text-xs text-blue-600 font-medium">
            <Clock className="w-3.5 h-3.5 mr-0.5 animate-spin" />
            {t('status.assembling')}
          </span>
        );
      case 'error':
        return (
          <span
            className="inline-flex items-center space-x-1 text-xs text-danger font-medium"
            title={task.errorMessage || t('toast.downloadError')}
          >
            <AlertCircle className="w-3.5 h-3.5 mr-0.5" />
            {t('status.failed')}
          </span>
        );
      case 'canceled':
        return (
          <span className="inline-flex items-center space-x-1 text-xs text-text-muted font-medium">
            <XCircle className="w-3.5 h-3.5 mr-0.5 text-danger/70" />
            {t('status.canceled')}
          </span>
        );
      case 'connecting':
        return (
          <span className="inline-flex items-center space-x-1 text-xs text-accent font-medium">
            <Clock className="w-3.5 h-3.5 mr-0.5 animate-spin" />
            {t('status.connecting')}
          </span>
        );
      default:
        return <span className="text-xs text-text-muted">{t('status.queued')}</span>;
    }
  };

  if (tasks.length === 0) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center p-8 text-center bg-bg select-none">
        <div className="w-16 h-16 rounded-full bg-surface border border-border flex items-center justify-center mb-3 text-text-muted">
          <DownloadCloud className="w-8 h-8 text-accent/60" />
        </div>
        <h3 className="text-sm font-semibold text-text mb-1">{t('table.emptyTitle')}</h3>
        <p className="text-xs text-text-muted max-w-sm mb-4">
          {t('table.emptySubtitle')}
        </p>
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-auto bg-bg select-none">
      <table className="w-full text-left border-collapse text-xs">
        <thead className="sticky top-0 bg-surface border-b border-border z-10 font-semibold text-text-muted">
          <tr>
            <th className="py-2.5 px-3 w-10 text-center">
              <input
                type="checkbox"
                aria-label={t('table.selectAllAria')}
                checked={tasks.length > 0 && tasks.every((t) => markedTaskIds.includes(t.id))}
                onChange={onToggleMarkAll}
                className="rounded border-border text-accent focus:ring-accent cursor-pointer w-3.5 h-3.5"
                title={t('table.selectAllAria')}
              />
            </th>
            <th className="py-2.5 px-2 w-8 text-center">#</th>
            <th className="py-2.5 px-3">{t('table.colFileName')}</th>
            <th className="py-2.5 px-3 w-28">{t('table.colSize')}</th>
            <th className="py-2.5 px-3 w-32">{t('table.colStatus')}</th>
            <th className="py-2.5 px-3 w-44">{t('table.colProgress')}</th>
            <th className="py-2.5 px-3 w-28">{t('table.colSpeed')}</th>
            <th className="py-2.5 px-3 w-24">{t('table.colEta')}</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-border/60">
          {tasks.map((task, idx) => {
            const isSelected = selectedTaskId === task.id;
            const isMarked = markedTaskIds.includes(task.id);
            return (
              <tr
                key={task.id}
                onClick={() => onSelectTask(task.id)}
                onDoubleClick={() => onOpenDetails(task)}
                onContextMenu={(e) => {
                  if (onContextMenu) {
                    e.preventDefault();
                    onSelectTask(task.id);
                    onContextMenu(task, e);
                  }
                }}
                className={`transition-all duration-150 cursor-pointer border-l-4 ${
                  isSelected
                    ? 'bg-blue-600/30 dark:bg-blue-500/35 border-l-blue-500 shadow-xs font-semibold'
                    : isMarked
                    ? 'bg-blue-900/20 border-l-blue-400/50'
                    : 'hover:bg-slate-700/20 border-l-transparent'
                }`}
              >
                <td className="py-2.5 px-3 text-center" onClick={(e) => e.stopPropagation()}>
                  <input
                    type="checkbox"
                    aria-label={t('table.markTaskAria', { name: task.fileName })}
                    checked={isMarked}
                    onChange={() => onToggleMarkTask?.(task.id)}
                    className="rounded border-border text-accent focus:ring-accent cursor-pointer w-3.5 h-3.5"
                    title={t('table.markTaskAria', { name: task.fileName })}
                  />
                </td>
                <td className="py-2.5 px-2 text-center text-text-muted font-mono text-[11px]">
                  {idx + 1}
                </td>
                <td className="py-2.5 px-3 font-medium text-text">
                  <div className="flex items-center space-x-2 truncate max-w-md">
                    {getFileIcon(task.fileName, task.categoryId)}
                    <span className="truncate" title={task.fileName}>{task.fileName}</span>
                    {task.resolution && (
                      <span className="text-[10px] px-1.5 py-0.2 rounded bg-accent/15 text-accent font-semibold font-mono shrink-0">
                        {task.resolution}
                      </span>
                    )}
                  </div>
                </td>
                <td className="py-2.5 px-3 font-mono text-text-muted">
                  {formatBytes(task.fileSize)}
                </td>
                <td className="py-2.5 px-3">
                  {getStatusBadge(task)}
                </td>
                <td className="py-2.5 px-3">
                  <div className="flex items-center space-x-2">
                    <SegmentedProgressBar
                      progressPercent={task.progressPercent}
                      status={task.status}
                      variant="compact"
                    />
                    <span className="font-mono text-[11px] text-text-muted shrink-0 w-10 text-right">
                      {task.progressPercent.toFixed(0)}%
                    </span>
                  </div>
                </td>
                <td className="py-2.5 px-3 font-mono font-medium text-accent">
                  {formatSpeed(task.speedBytesPerSec)}
                </td>
                <td className="py-2.5 px-3 font-mono text-text-muted">
                  {formatEta(task.etaSeconds, false, lang)}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
};
