import React, { useEffect, useState } from 'react';
import {
  X,
  RefreshCw,
  ShieldCheck,
  FolderEdit,
  Gauge,
  CheckCircle2,
  AlertTriangle,
  Info,
} from 'lucide-react';
import { DownloadTask, FileChecksums } from '../types.ts';
import {
  renameOrMoveDownload,
  verifyFileChecksum,
} from '../services/nativeBridge.ts';
import { formatDiskBytes, getParentDir } from '../utils/downloadUtils.ts';
import { useI18n } from '../i18n/index.tsx';

interface TaskPropertiesModalProps {
  task: DownloadTask | null;
  isOpen: boolean;
  initialTab?: 'info' | 'rename' | 'checksum';
  onClose: () => void;
  onRefreshUrlAndResume: (
    taskId: string,
    newUrl: string,
    headers?: { cookie?: string; referer?: string; userAgent?: string }
  ) => void;
  onUpdateTaskSpeedLimit: (taskId: string, limitKbs: number) => void;
  onUpdateTaskPath: (taskId: string, newFileName: string, newDestinationPath: string) => void;
  onUpdateChecksums: (taskId: string, checksums: FileChecksums) => void;
  onNotify: (msg: string, type?: 'info' | 'success' | 'warning') => void;
}

export const TaskPropertiesModal: React.FC<TaskPropertiesModalProps> = ({
  task,
  isOpen,
  initialTab = 'info',
  onClose,
  onRefreshUrlAndResume,
  onUpdateTaskSpeedLimit,
  onUpdateTaskPath,
  onUpdateChecksums,
  onNotify,
}) => {
  const { t } = useI18n();
  const [activeTab, setActiveTab] = useState<'info' | 'rename' | 'checksum'>(initialTab);
  const [refreshUrl, setRefreshUrl] = useState('');
  const [cookie, setCookie] = useState('');
  const [referer, setReferer] = useState('');
  const [userAgent, setUserAgent] = useState('');
  const [limitKbs, setLimitKbs] = useState(0);

  const [newFileName, setNewFileName] = useState('');
  const [newDir, setNewDir] = useState('');
  const [isMoving, setIsMoving] = useState(false);

  const [checksums, setChecksums] = useState<FileChecksums | null>(null);
  const [isHashing, setIsHashing] = useState(false);
  const [expectedHash, setExpectedHash] = useState('');

  const taskId = task?.id;
  useEffect(() => {
    if (!isOpen || !task) return;
    setActiveTab(initialTab);
    setRefreshUrl(task.url);
    setCookie(task.cookie || '');
    setReferer(task.referer || '');
    setUserAgent(task.userAgent || '');
    setLimitKbs(Math.round((task.speedLimitBytesPerSec || 0) / 1024));
    setNewFileName(task.fileName);
    setNewDir(getParentDir(task.destinationPath));
    setChecksums(task.checksums || null);
    setExpectedHash('');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, taskId, initialTab]);

  if (!isOpen || !task) return null;

  const normalizedExpected = expectedHash.trim().toLowerCase();
  const hashMatches =
    Boolean(normalizedExpected) &&
    checksums !== null &&
    [checksums.md5, checksums.sha1, checksums.sha256]
      .map((h) => h.toLowerCase())
      .includes(normalizedExpected);
  const hashMismatch =
    Boolean(normalizedExpected) && checksums !== null && !hashMatches;

  const handleComputeChecksums = async () => {
    setIsHashing(true);
    try {
      const result = await verifyFileChecksum(task.destinationPath);
      if (result) {
        setChecksums(result);
        onUpdateChecksums(task.id, result);
        onNotify(t('props.checksumComputed'), 'success');
      }
    } catch (err) {
      onNotify(String(err), 'warning');
    } finally {
      setIsHashing(false);
    }
  };

  const handleRenameMove = async () => {
    const cleanName = newFileName.trim();
    const cleanDir = newDir.trim().replace(/\/$/, '');
    if (!cleanName || !cleanDir) return;
    const targetPath = `${cleanDir}/${cleanName}`;
    setIsMoving(true);
    try {
      const resolved = await renameOrMoveDownload(task.destinationPath, targetPath);
      onUpdateTaskPath(task.id, cleanName, resolved || targetPath);
      onNotify(t('props.renamedNotify', { name: cleanName }), 'success');
    } catch (err) {
      onNotify(String(err), 'warning');
    } finally {
      setIsMoving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-xs p-4 select-none">
      <div className="bg-bg border border-border rounded-lg shadow-pop w-full max-w-2xl overflow-hidden animate-in fade-in zoom-in-95 duration-150">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-surface">
          <div className="flex items-center space-x-2 truncate">
            <Info className="w-4 h-4 text-accent shrink-0" />
            <h2 className="text-sm font-bold text-text truncate" title={task.fileName}>
              {t('props.titlePrefix', { name: task.fileName })}
            </h2>
          </div>
          <button
            onClick={onClose}
            aria-label={t('common.close')}
            className="p-1 rounded-md text-text-muted hover:text-text hover:bg-surface-hover transition-colors"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Tabs */}
        <div className="flex border-b border-border bg-surface/60 px-4 space-x-1 text-xs">
          <button
            type="button"
            onClick={() => setActiveTab('info')}
            className={`px-3 py-2 font-semibold border-b-2 transition-colors ${
              activeTab === 'info'
                ? 'border-accent text-accent'
                : 'border-transparent text-text-muted hover:text-text'
            }`}
          >
            {t('props.tabInfo')}
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('rename')}
            className={`px-3 py-2 font-semibold border-b-2 transition-colors ${
              activeTab === 'rename'
                ? 'border-accent text-accent'
                : 'border-transparent text-text-muted hover:text-text'
            }`}
          >
            {t('props.tabRename')}
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('checksum')}
            className={`px-3 py-2 font-semibold border-b-2 transition-colors ${
              activeTab === 'checksum'
                ? 'border-accent text-accent'
                : 'border-transparent text-text-muted hover:text-text'
            }`}
          >
            {t('props.tabChecksum')}
          </button>
        </div>

        <div className="p-4 space-y-4 text-xs max-h-[70vh] overflow-y-auto">
          {activeTab === 'info' && (
            <>
              <div className="grid grid-cols-2 gap-3 p-3 bg-surface rounded-md border border-border">
                <div>
                  <p className="text-[10px] text-text-muted uppercase">{t('props.sizeAndProgress')}</p>
                  <p className="font-mono font-semibold text-text">
                    {formatDiskBytes(task.downloadedBytes, t('common.unknown'))} /{' '}
                    {formatDiskBytes(task.fileSize, t('common.unknown'))} (
                    {task.progressPercent.toFixed(1)}%)
                  </p>
                </div>
                <div>
                  <p className="text-[10px] text-text-muted uppercase">{t('props.resumeSupportLabel')}</p>
                  <p className="font-mono font-semibold text-text">
                    {task.supportsRange ? t('props.rangeActive') : t('props.singleStream')} •{' '}
                    {task.numConnections} {t('common.threadsUnit')}
                  </p>
                </div>
                <div className="col-span-2">
                  <p className="text-[10px] text-text-muted uppercase">{t('props.fileLocation')}</p>
                  <p className="font-mono text-[11px] text-text break-all">
                    {task.destinationPath}
                  </p>
                </div>
              </div>

              {/* Live Per-Task Speed Cap */}
              <div className="p-3 bg-surface rounded-md border border-border space-y-2">
                <div className="flex items-center space-x-1.5 font-semibold text-text">
                  <Gauge className="w-3.5 h-3.5 text-accent" />
                  <span>{t('props.taskSpeedCapHeader')}</span>
                </div>
                <div className="flex items-center space-x-2">
                  <input
                    type="number"
                    min={0}
                    value={limitKbs}
                    onChange={(e) => setLimitKbs(Math.max(0, Number(e.target.value)))}
                    aria-label={t('props.taskSpeedLimitAria')}
                    className="w-32 px-2.5 py-1.5 rounded-md bg-bg border border-border-input/40 font-mono text-text"
                  />
                  <span className="text-text-muted">{t('props.speedHint')}</span>
                  <button
                    type="button"
                    onClick={() => onUpdateTaskSpeedLimit(task.id, limitKbs)}
                    className="ml-auto px-3 py-1.5 rounded-md bg-accent text-on-accent font-semibold hover:bg-accent-hover transition-colors"
                  >
                    {t('props.applyLive')}
                  </button>
                </div>
              </div>

              {/* Refresh Download Address */}
              <div className="p-3 bg-surface rounded-md border border-border space-y-2.5">
                <div className="flex items-center justify-between">
                  <div className="flex items-center space-x-1.5 font-semibold text-text">
                    <RefreshCw className="w-3.5 h-3.5 text-accent" />
                    <span>{t('props.refreshTitle')}</span>
                  </div>
                  {task.lastUrlRefreshedAt && (
                    <span className="text-[10px] text-success font-mono">
                      {t('props.updatedAt', {
                        time: new Date(task.lastUrlRefreshedAt).toLocaleTimeString(),
                      })}
                    </span>
                  )}
                </div>
                <p className="text-[11px] text-text-muted">
                  {t('props.refreshDesc')}
                </p>
                <div className="space-y-1">
                  <label className="font-medium text-text">{t('props.newUrlLabel')}</label>
                  <input
                    type="url"
                    value={refreshUrl}
                    onChange={(e) => setRefreshUrl(e.target.value)}
                    aria-label={t('props.newUrlAria')}
                    placeholder="https://..."
                    className="w-full px-2.5 py-1.5 rounded-md bg-bg border border-border-input/40 font-mono text-[11px] text-text"
                  />
                </div>
                <div className="grid grid-cols-2 gap-2">
                  <input
                    type="text"
                    value={referer}
                    onChange={(e) => setReferer(e.target.value)}
                    placeholder={t('props.refererPlaceholder')}
                    className="px-2.5 py-1.5 rounded-md bg-bg border border-border-input/40 font-mono text-[11px] text-text"
                  />
                  <input
                    type="text"
                    value={cookie}
                    onChange={(e) => setCookie(e.target.value)}
                    placeholder={t('props.cookiePlaceholder')}
                    className="px-2.5 py-1.5 rounded-md bg-bg border border-border-input/40 font-mono text-[11px] text-text"
                  />
                </div>
                {task.status !== 'completed' && (
                  <div className="flex justify-end">
                    <button
                      type="button"
                      onClick={() => {
                        if (!refreshUrl.trim()) return;
                        onRefreshUrlAndResume(task.id, refreshUrl.trim(), {
                          cookie: cookie.trim() || undefined,
                          referer: referer.trim() || undefined,
                          userAgent: userAgent.trim() || undefined,
                        });
                        onClose();
                      }}
                      className="flex items-center space-x-1.5 px-3 py-1.5 rounded-md bg-accent text-on-accent font-semibold hover:bg-accent-hover transition-colors"
                    >
                      <RefreshCw className="w-3.5 h-3.5" />
                      <span>{t('props.refreshAndResume')}</span>
                    </button>
                  </div>
                )}
              </div>

              {/* HTTP & Event Logs */}
              {task.httpLogs && task.httpLogs.length > 0 && (
                <div className="space-y-1">
                  <p className="font-semibold text-text">{t('props.connectionLogs')}</p>
                  <div className="max-h-28 overflow-y-auto bg-surface border border-border rounded-md p-2 font-mono text-[10px] space-y-1">
                    {task.httpLogs.map((log, i) => (
                      <div key={i} className="flex space-x-2 text-text-muted">
                        <span className="shrink-0">[{log.timestamp}]</span>
                        <span className="text-accent font-semibold">{log.event}</span>
                        <span className="truncate text-text">{log.detail}</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </>
          )}

          {activeTab === 'rename' && (
            <div className="space-y-3 p-3 bg-surface rounded-md border border-border">
              <div className="flex items-center space-x-1.5 font-semibold text-text">
                <FolderEdit className="w-4 h-4 text-accent" />
                <span>{t('props.renameHeader')}</span>
              </div>
              <p className="text-[11px] text-text-muted">
                {t('props.renameDesc')}
              </p>
              <div className="space-y-1">
                <label className="font-medium text-text">{t('props.newFileNameLabel')}</label>
                <input
                  type="text"
                  value={newFileName}
                  onChange={(e) => setNewFileName(e.target.value)}
                  aria-label={t('props.newFileNameLabel')}
                  className="w-full px-2.5 py-1.5 rounded-md bg-bg border border-border-input/40 font-mono text-text"
                />
              </div>
              <div className="space-y-1">
                <label className="font-medium text-text">{t('props.targetFolderLabel')}</label>
                <input
                  type="text"
                  value={newDir}
                  onChange={(e) => setNewDir(e.target.value)}
                  aria-label={t('props.targetFolderAria')}
                  className="w-full px-2.5 py-1.5 rounded-md bg-bg border border-border-input/40 font-mono text-text"
                />
              </div>
              <div className="flex justify-end">
                <button
                  type="button"
                  disabled={isMoving || !newFileName.trim() || !newDir.trim()}
                  onClick={handleRenameMove}
                  className="px-4 py-1.5 rounded-md bg-accent text-on-accent font-semibold hover:bg-accent-hover transition-colors disabled:opacity-40"
                >
                  {isMoving ? t('props.movingBtn') : t('props.saveRenameBtn')}
                </button>
              </div>
            </div>
          )}

          {activeTab === 'checksum' && (
            <div className="space-y-3 p-3 bg-surface rounded-md border border-border">
              <div className="flex items-center justify-between">
                <div className="flex items-center space-x-1.5 font-semibold text-text">
                  <ShieldCheck className="w-4 h-4 text-accent" />
                  <span>{t('props.checksumHeader')}</span>
                </div>
                <button
                  type="button"
                  disabled={isHashing}
                  onClick={handleComputeChecksums}
                  className="px-3 py-1.5 rounded-md bg-accent text-on-accent font-semibold hover:bg-accent-hover transition-colors disabled:opacity-40"
                >
                  {isHashing ? t('props.calculating') : t('props.calcChecksum')}
                </button>
              </div>

              {checksums && (
                <div className="space-y-2 p-2.5 bg-bg rounded-md border border-border font-mono text-[11px]">
                  <div>
                    <span className="text-text-muted block text-[10px]">MD5</span>
                    <span className="text-text break-all select-all">{checksums.md5}</span>
                  </div>
                  <div>
                    <span className="text-text-muted block text-[10px]">SHA-1</span>
                    <span className="text-text break-all select-all">{checksums.sha1}</span>
                  </div>
                  <div>
                    <span className="text-text-muted block text-[10px]">SHA-256</span>
                    <span className="text-text break-all select-all">{checksums.sha256}</span>
                  </div>
                </div>
              )}

              <div className="space-y-1">
                <label className="font-medium text-text">{t('props.compareHashLabel')}</label>
                <input
                  type="text"
                  value={expectedHash}
                  onChange={(e) => setExpectedHash(e.target.value)}
                  placeholder={t('props.refHashPlaceholder')}
                  aria-label={t('props.refHashLabel')}
                  className="w-full px-2.5 py-1.5 rounded-md bg-bg border border-border-input/40 font-mono text-[11px] text-text"
                />
                {hashMatches && (
                  <p className="flex items-center space-x-1 text-success font-semibold pt-1">
                    <CheckCircle2 className="w-4 h-4" />
                    <span>{t('props.hashMatch')}</span>
                  </p>
                )}
                {hashMismatch && (
                  <p className="flex items-center space-x-1 text-danger font-semibold pt-1">
                    <AlertTriangle className="w-4 h-4" />
                    <span>{t('props.hashMismatch')}</span>
                  </p>
                )}
              </div>
            </div>
          )}
        </div>

        <div className="flex items-center justify-end px-4 py-3 border-t border-border bg-surface">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-1.5 rounded-md bg-surface-hover text-text font-medium text-xs hover:bg-border/40"
          >
            {t('common.close')}
          </button>
        </div>
      </div>
    </div>
  );
};
