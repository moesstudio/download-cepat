import React, { useState } from 'react';
import { Download, Zap, X } from 'lucide-react';
import { parseUrlListText } from '../utils/downloadUtils.ts';
import { useI18n } from '../i18n/index.tsx';

interface MiniDropTargetProps {
  visible: boolean;
  activeCount: number;
  totalSpeedBytesPerSec: number;
  onClose: () => void;
  onDropUrls: (urls: string[]) => void;
}

export const MiniDropTarget: React.FC<MiniDropTargetProps> = ({
  visible,
  activeCount,
  totalSpeedBytesPerSec,
  onClose,
  onDropUrls,
}) => {
  const { t } = useI18n();
  const [isDragOver, setIsDragOver] = useState(false);

  if (!visible) return null;

  const speedMb = (totalSpeedBytesPerSec / (1024 * 1024)).toFixed(2);

  const handleDrop = async (e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(false);

    const collected: string[] = [];
    const uriList = e.dataTransfer.getData('text/uri-list');
    const plain = e.dataTransfer.getData('text/plain');
    if (uriList) collected.push(...parseUrlListText(uriList));
    if (plain) collected.push(...parseUrlListText(plain));

    if (e.dataTransfer.files && e.dataTransfer.files.length > 0) {
      for (const file of Array.from(e.dataTransfer.files)) {
        if (/\.(txt|ef2|lst)$/i.test(file.name)) {
          const text = await file.text();
          collected.push(...parseUrlListText(text));
        }
      }
    }

    const unique = Array.from(new Set(collected));
    if (unique.length > 0) {
      onDropUrls(unique);
    }
  };

  return (
    <div
      data-testid="mini-drop-target"
      onDragOver={(e) => {
        e.preventDefault();
        setIsDragOver(true);
      }}
      onDragLeave={() => setIsDragOver(false)}
      onDrop={handleDrop}
      className={`fixed bottom-9 right-4 z-40 w-44 rounded-xl border p-3 shadow-pop transition-all select-none ${
        isDragOver
          ? 'bg-accent/20 border-accent scale-105'
          : 'bg-surface/95 backdrop-blur-md border-border'
      }`}
    >
      <div className="flex items-center justify-between mb-1.5">
        <div className="flex items-center space-x-1.5 text-xs font-bold text-text">
          <Zap className="w-3.5 h-3.5 text-accent" />
          <span>{t('minidrop.title')}</span>
        </div>
        <button
          type="button"
          onClick={onClose}
          aria-label={t('minidrop.closeAria')}
          className="text-text-muted hover:text-text"
        >
          <X className="w-3.5 h-3.5" />
        </button>
      </div>

      <div className="flex flex-col items-center justify-center border border-dashed border-border-input/50 rounded-lg py-2 px-2 text-center bg-bg/50">
        <Download
          className={`w-4 h-4 mb-1 ${isDragOver ? 'text-accent animate-bounce' : 'text-accent'}`}
        />
        <p className="text-[10px] font-medium text-text">
          {isDragOver ? t('minidrop.dropHere') : t('minidrop.hint')}
        </p>
        <p className="text-[10px] font-mono text-text-muted mt-1">
          {activeCount > 0 ? t('minidrop.activeCount', { count: activeCount, speed: speedMb }) : t('minidrop.ready')}
        </p>
      </div>
    </div>
  );
};
