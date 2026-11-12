import React, { useEffect, useState } from 'react';
import {
  X,
  Clock,
  ArrowUp,
  ArrowDown,
  Power,
  Play,
  ListOrdered,
} from 'lucide-react';
import { DownloadTask, PostDownloadAction, SchedulerConfig } from '../types.ts';
import { useI18n } from '../i18n/index.tsx';

interface SchedulerModalProps {
  isOpen: boolean;
  config: SchedulerConfig;
  queuedAndActiveTasks: DownloadTask[];
  onClose: () => void;
  onSaveConfig: (config: SchedulerConfig) => void;
  onMovePriority: (taskId: string, direction: 'up' | 'down') => void;
  onStartQueueNow: () => void;
}

export const SchedulerModal: React.FC<SchedulerModalProps> = ({
  isOpen,
  config,
  queuedAndActiveTasks,
  onClose,
  onSaveConfig,
  onMovePriority,
  onStartQueueNow,
}) => {
  const { t } = useI18n();
  const [draft, setDraft] = useState<SchedulerConfig>(config);
  const dayLabels = [
    t('scheduler.daySun'),
    t('scheduler.dayMon'),
    t('scheduler.dayTue'),
    t('scheduler.dayWed'),
    t('scheduler.dayThu'),
    t('scheduler.dayFri'),
    t('scheduler.daySat'),
  ];

  useEffect(() => {
    if (isOpen) setDraft(config);
  }, [isOpen, config]);

  if (!isOpen) return null;

  const toggleDay = (dayIndex: number) => {
    setDraft((prev) => {
      const exists = prev.days.includes(dayIndex);
      const nextDays = exists
        ? prev.days.filter((d) => d !== dayIndex)
        : [...prev.days, dayIndex].sort((a, b) => a - b);
      return { ...prev, days: nextDays };
    });
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-xs p-4 select-none">
      <div className="bg-bg border border-border rounded-lg shadow-pop w-full max-w-xl overflow-hidden animate-in fade-in zoom-in-95 duration-150">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-surface">
          <div className="flex items-center space-x-2">
            <Clock className="w-4 h-4 text-accent" />
            <h2 className="text-sm font-bold text-text">{t('scheduler.title')}</h2>
          </div>
          <button
            onClick={onClose}
            aria-label={t('common.close')}
            className="p-1 rounded-md text-text-muted hover:text-text hover:bg-surface-hover"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="p-4 space-y-4 text-xs max-h-[70vh] overflow-y-auto">
          {/* Time Window */}
          <div className="p-3 bg-surface rounded-md border border-border space-y-3">
            <label className="flex items-center justify-between cursor-pointer">
              <span className="font-semibold text-text">{t('scheduler.enableLabel')}</span>
              <input
                type="checkbox"
                checked={draft.enabled}
                onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
                className="rounded border-border text-accent"
              />
            </label>

            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1">
                <label className="text-[11px] font-medium text-text-muted">{t('scheduler.startTime')}</label>
                <input
                  type="time"
                  value={draft.startTime}
                  onChange={(e) => setDraft({ ...draft, startTime: e.target.value })}
                  aria-label={t('scheduler.startTime')}
                  className="w-full px-2.5 py-1.5 rounded-md bg-bg border border-border-input/40 font-mono text-text"
                />
              </div>
              <div className="space-y-1">
                <label className="text-[11px] font-medium text-text-muted">{t('scheduler.stopTime')}</label>
                <input
                  type="time"
                  value={draft.stopTime}
                  onChange={(e) => setDraft({ ...draft, stopTime: e.target.value })}
                  aria-label={t('scheduler.stopTime')}
                  className="w-full px-2.5 py-1.5 rounded-md bg-bg border border-border-input/40 font-mono text-text"
                />
              </div>
            </div>

            <div className="space-y-1">
              <span className="text-[11px] font-medium text-text-muted">{t('scheduler.activeDays')}</span>
              <div className="flex space-x-1.5 pt-1">
                {dayLabels.map((label, idx) => {
                  const active = draft.days.includes(idx);
                  return (
                    <button
                      key={idx}
                      type="button"
                      onClick={() => toggleDay(idx)}
                      className={`px-2.5 py-1 rounded-md font-semibold text-[11px] transition-colors ${
                        active
                          ? 'bg-accent text-on-accent'
                          : 'bg-bg border border-border text-text-muted hover:text-text'
                      }`}
                    >
                      {label}
                    </button>
                  );
                })}
              </div>
            </div>
          </div>

          {/* Post-download Action */}
          <div className="p-3 bg-surface rounded-md border border-border space-y-2">
            <div className="flex items-center space-x-1.5 font-semibold text-text">
              <Power className="w-3.5 h-3.5 text-accent" />
              <span>{t('scheduler.postActionLabel')}</span>
            </div>
            <select
              value={draft.postAction}
              onChange={(e) =>
                setDraft({ ...draft, postAction: e.target.value as PostDownloadAction })
              }
              aria-label={t('scheduler.postActionLabel')}
              className="w-full px-2.5 py-1.5 rounded-md bg-bg border border-border text-text"
            >
              <option value="none">{t('scheduler.postNone')}</option>
              <option value="notify_open">{t('scheduler.postNotify')}</option>
              <option value="sleep">{t('scheduler.postSleep')}</option>
              <option value="quit">{t('scheduler.postQuit')}</option>
            </select>
          </div>

          {/* Queue Priority Order */}
          <div className="p-3 bg-surface rounded-md border border-border space-y-2">
            <div className="flex items-center justify-between">
              <div className="flex items-center space-x-1.5 font-semibold text-text">
                <ListOrdered className="w-3.5 h-3.5 text-accent" />
                <span>{t('scheduler.queueTitle', { count: queuedAndActiveTasks.length })}</span>
              </div>
              <button
                type="button"
                onClick={onStartQueueNow}
                className="flex items-center space-x-1 px-2.5 py-1 rounded bg-accent/15 text-accent font-semibold hover:bg-accent/25"
              >
                <Play className="w-3 h-3" />
                <span>{t('scheduler.startQueueNow')}</span>
              </button>
            </div>

            <div className="max-h-40 overflow-y-auto divide-y divide-border/50 border border-border rounded bg-bg">
              {queuedAndActiveTasks.length === 0 ? (
                <p className="p-3 text-center text-text-muted text-[11px]">
                  {t('scheduler.queueEmpty')}
                </p>
              ) : (
                queuedAndActiveTasks.map((task, index) => (
                  <div
                    key={task.id}
                    className="flex items-center justify-between px-2.5 py-1.5 hover:bg-surface/60"
                  >
                    <div className="flex items-center space-x-2 min-w-0">
                      <span className="font-mono text-[10px] text-text-muted w-4">
                        #{index + 1}
                      </span>
                      <span className="truncate text-text font-medium">{task.fileName}</span>
                      <span className="text-[10px] px-1.5 rounded bg-surface text-text-muted uppercase">
                        {task.status}
                      </span>
                    </div>
                    <div className="flex items-center space-x-1 shrink-0">
                      <button
                        type="button"
                        aria-label={`Up ${task.fileName}`}
                        onClick={() => onMovePriority(task.id, 'up')}
                        className="p-1 rounded hover:bg-surface-hover text-text-muted hover:text-text"
                      >
                        <ArrowUp className="w-3.5 h-3.5" />
                      </button>
                      <button
                        type="button"
                        aria-label={`Down ${task.fileName}`}
                        onClick={() => onMovePriority(task.id, 'down')}
                        className="p-1 rounded hover:bg-surface-hover text-text-muted hover:text-text"
                      >
                        <ArrowDown className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end space-x-2 px-4 py-3 border-t border-border bg-surface">
          <button
            type="button"
            onClick={onClose}
            className="px-3 py-1.5 rounded-md text-text hover:bg-surface-hover font-medium text-xs"
          >
            {t('common.cancel')}
          </button>
          <button
            type="button"
            onClick={() => {
              onSaveConfig(draft);
              onClose();
            }}
            className="px-4 py-1.5 rounded-md bg-accent text-on-accent text-xs font-semibold hover:bg-accent-hover"
          >
            {t('scheduler.saveSchedule')}
          </button>
        </div>
      </div>
    </div>
  );
};
