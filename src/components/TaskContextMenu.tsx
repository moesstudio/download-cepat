import React, { useEffect, useRef } from 'react';
import {
  Play,
  Pause,
  RefreshCw,
  FolderOpen,
  FileCheck2,
  Info,
  Copy,
  Trash2,
  ArrowUp,
  ArrowDown,
  ShieldCheck,
} from 'lucide-react';
import { DownloadTask } from '../types.ts';
import { useI18n } from '../i18n/index.tsx';

interface TaskContextMenuProps {
  task: DownloadTask | null;
  position: { x: number; y: number } | null;
  onClose: () => void;
  onResume: (taskId: string) => void;
  onPause: (taskId: string) => void;
  onRefreshAddress: (task: DownloadTask) => void;
  onOpenFile: (task: DownloadTask) => void;
  onOpenFolder: (task: DownloadTask) => void;
  onOpenProperties: (task: DownloadTask, initialTab?: 'info' | 'rename' | 'checksum') => void;
  onMovePriority: (taskId: string, direction: 'up' | 'down') => void;
  onCopyUrl: (url: string) => void;
  onDelete: (taskId: string) => void;
}

export const TaskContextMenu: React.FC<TaskContextMenuProps> = ({
  task,
  position,
  onClose,
  onResume,
  onPause,
  onRefreshAddress,
  onOpenFile,
  onOpenFolder,
  onOpenProperties,
  onMovePriority,
  onCopyUrl,
  onDelete,
}) => {
  const { t } = useI18n();
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!position) return;
    const handleOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('mousedown', handleOutside);
    window.addEventListener('keydown', handleKey);
    return () => {
      window.removeEventListener('mousedown', handleOutside);
      window.removeEventListener('keydown', handleKey);
    };
  }, [position, onClose]);

  if (!task || !position) return null;

  const isRunning = task.status === 'downloading' || task.status === 'connecting';
  const isCompleted = task.status === 'completed';
  const left = typeof window !== 'undefined' ? Math.min(position.x, window.innerWidth - 250) : position.x;
  const top = typeof window !== 'undefined' ? Math.min(position.y, window.innerHeight - 360) : position.y;

  return (
    <div
      ref={menuRef}
      role="menu"
      aria-label={t('context.menuAria', { name: task.fileName })}
      style={{ left: Math.max(8, left), top: Math.max(8, top) }}
      className="fixed z-50 w-60 bg-bg border border-border rounded-lg shadow-pop py-1.5 text-xs text-text select-none"
    >
      <div className="px-3 py-1.5 border-b border-border/60 mb-1">
        <p className="font-semibold truncate" title={task.fileName}>{task.fileName}</p>
        <p className="text-[10px] text-text-muted uppercase">{task.status}</p>
      </div>

      {isRunning ? (
        <button
          role="menuitem"
          onClick={() => {
            onPause(task.id);
            onClose();
          }}
          className="w-full px-3 py-1.5 flex items-center space-x-2 hover:bg-surface-hover text-left"
        >
          <Pause className="w-3.5 h-3.5 text-warning" />
          <span>{t('context.pause')}</span>
        </button>
      ) : (
        !isCompleted && (
          <button
            role="menuitem"
            onClick={() => {
              onResume(task.id);
              onClose();
            }}
            className="w-full px-3 py-1.5 flex items-center space-x-2 hover:bg-surface-hover text-left"
          >
            <Play className="w-3.5 h-3.5 text-success" />
            <span>{t('context.resume')}</span>
          </button>
        )
      )}

      {!isCompleted && (
        <button
          role="menuitem"
          onClick={() => {
            onRefreshAddress(task);
            onClose();
          }}
          className="w-full px-3 py-1.5 flex items-center space-x-2 hover:bg-surface-hover text-left"
        >
          <RefreshCw className="w-3.5 h-3.5 text-accent" />
          <span>{t('context.refreshUrl')}</span>
        </button>
      )}

      {isCompleted && (
        <button
          role="menuitem"
          onClick={() => {
            onOpenFile(task);
            onClose();
          }}
          className="w-full px-3 py-1.5 flex items-center space-x-2 hover:bg-surface-hover text-left"
        >
          <FileCheck2 className="w-3.5 h-3.5 text-success" />
          <span>{t('context.openFile')}</span>
        </button>
      )}

      <button
        role="menuitem"
        onClick={() => {
          onOpenFolder(task);
          onClose();
        }}
        className="w-full px-3 py-1.5 flex items-center space-x-2 hover:bg-surface-hover text-left"
      >
        <FolderOpen className="w-3.5 h-3.5 text-text-muted" />
        <span>{t('context.openFolder')}</span>
      </button>

      <div className="my-1 border-t border-border/60" />

      <button
        role="menuitem"
        onClick={() => {
          onMovePriority(task.id, 'up');
          onClose();
        }}
        className="w-full px-3 py-1.5 flex items-center space-x-2 hover:bg-surface-hover text-left"
      >
        <ArrowUp className="w-3.5 h-3.5 text-text-muted" />
        <span>{t('context.priorityUp')}</span>
      </button>

      <button
        role="menuitem"
        onClick={() => {
          onMovePriority(task.id, 'down');
          onClose();
        }}
        className="w-full px-3 py-1.5 flex items-center space-x-2 hover:bg-surface-hover text-left"
      >
        <ArrowDown className="w-3.5 h-3.5 text-text-muted" />
        <span>{t('context.priorityDown')}</span>
      </button>

      <div className="my-1 border-t border-border/60" />

      <button
        role="menuitem"
        onClick={() => {
          onOpenProperties(task, 'checksum');
          onClose();
        }}
        className="w-full px-3 py-1.5 flex items-center space-x-2 hover:bg-surface-hover text-left"
      >
        <ShieldCheck className="w-3.5 h-3.5 text-accent" />
        <span>{t('context.verifyChecksum')}</span>
      </button>

      <button
        role="menuitem"
        onClick={() => {
          onOpenProperties(task, 'info');
          onClose();
        }}
        className="w-full px-3 py-1.5 flex items-center space-x-2 hover:bg-surface-hover text-left"
      >
        <Info className="w-3.5 h-3.5 text-text-muted" />
        <span>{t('context.properties')}</span>
      </button>

      <button
        role="menuitem"
        onClick={() => {
          onCopyUrl(task.url);
          onClose();
        }}
        className="w-full px-3 py-1.5 flex items-center space-x-2 hover:bg-surface-hover text-left"
      >
        <Copy className="w-3.5 h-3.5 text-text-muted" />
        <span>{t('context.copyUrl')}</span>
      </button>

      <div className="my-1 border-t border-border/60" />

      <button
        role="menuitem"
        onClick={() => {
          onDelete(task.id);
          onClose();
        }}
        className="w-full px-3 py-1.5 flex items-center space-x-2 hover:bg-danger/15 text-danger text-left"
      >
        <Trash2 className="w-3.5 h-3.5" />
        <span>{t('context.delete')}</span>
      </button>
    </div>
  );
};
