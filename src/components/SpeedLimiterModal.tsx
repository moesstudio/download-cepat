import React, { useState, useEffect } from 'react';
import { 
  X, 
  SlidersHorizontal, 
  Gauge, 
  Network, 
  Power, 
  Chrome, 
  FolderOpen, 
  CheckCircle2, 
  Sparkles,
  Shield,
  FolderCog,
  ClipboardCheck,
  HardDrive,
  Plus,
  Trash2,
  Languages,
  BadgeCheck,
} from 'lucide-react';
import {
  BuildInfo,
  CategoryRule,
  ClipboardMonitorConfig,
  ProxyConfig,
  SiteLogin,
} from '../types.ts';
import { 
  getAutostartStatus, 
  setAutostart, 
  installBrowserExtensionHost, 
  openExtensionFolder,
  checkDiskSpace,
  FALLBACK_BUILD_INFO,
} from '../services/nativeBridge.ts';
import {
  DEFAULT_CATEGORY_RULES,
  DEFAULT_CLIPBOARD_CONFIG,
  DEFAULT_PROXY_CONFIG,
  formatDiskBytes,
} from '../utils/downloadUtils.ts';
import { LANGUAGE_OPTIONS, LanguageCode, useI18n } from '../i18n/index.tsx';

export type SettingsTab =
  | 'network'
  | 'startup'
  | 'browser'
  | 'proxy'
  | 'categories'
  | 'clipboard'
  | 'about';

interface SpeedLimiterModalProps {
  isOpen: boolean;
  onClose: () => void;
  initialTab?: SettingsTab;
  speedLimitKbs: number;
  maxTasks?: number;
  defaultConnections?: number;
  proxyConfig?: ProxyConfig;
  siteLogins?: SiteLogin[];
  categoryRules?: CategoryRule[];
  clipboardConfig?: ClipboardMonitorConfig;
  buildInfo?: BuildInfo;
  onChangeLanguage?: (next: LanguageCode) => void;
  onSaveSpeedLimit?: (limitKbs: number) => void;
  onSaveSettings?: (settings: {
    speedLimitKbs: number;
    maxTasks: number;
    defaultConnections: number;
    proxyConfig?: ProxyConfig;
    siteLogins?: SiteLogin[];
    categoryRules?: CategoryRule[];
    clipboardConfig?: ClipboardMonitorConfig;
  }) => void;
}

export const SpeedLimiterModal: React.FC<SpeedLimiterModalProps> = ({
  isOpen,
  onClose,
  initialTab = 'network',
  speedLimitKbs,
  maxTasks: propMaxTasks = 2,
  defaultConnections: propConnections = 8,
  proxyConfig: propProxy = DEFAULT_PROXY_CONFIG,
  siteLogins: propLogins = [],
  categoryRules: propRules = DEFAULT_CATEGORY_RULES,
  clipboardConfig: propClipboard = DEFAULT_CLIPBOARD_CONFIG,
  buildInfo,
  onChangeLanguage,
  onSaveSpeedLimit,
  onSaveSettings,
}) => {
  const { lang, setLang, t } = useI18n();
  const [activeTab, setActiveTab] = useState<SettingsTab>(initialTab);
  const [isEnabled, setIsEnabled] = useState(speedLimitKbs > 0);
  const [limitVal, setLimitVal] = useState(speedLimitKbs > 0 ? speedLimitKbs : 2048);
  const [maxTasks, setMaxTasks] = useState(propMaxTasks);
  const [defaultConnections, setDefaultConnections] = useState(propConnections);

  const [proxyDraft, setProxyDraft] = useState<ProxyConfig>(propProxy);
  const [loginsDraft, setLoginsDraft] = useState<SiteLogin[]>(propLogins);
  const [newDomain, setNewDomain] = useState('');
  const [newUser, setNewUser] = useState('');
  const [newPass, setNewPass] = useState('');

  const [rulesDraft, setRulesDraft] = useState<CategoryRule[]>(propRules);
  const [clipboardDraft, setClipboardDraft] = useState<ClipboardMonitorConfig>(propClipboard);
  const [clipboardExtText, setClipboardExtText] = useState(propClipboard.extensions.join(', '));
  const [freeDiskBytes, setFreeDiskBytes] = useState<number | null>(null);

  // Autostart state
  const [isAutostart, setIsAutostart] = useState(false);
  const [autostartLoading, setAutostartLoading] = useState(false);
  const [autostartMsg, setAutostartMsg] = useState('');

  // Browser extension state
  const [connectorLoading, setConnectorLoading] = useState(false);
  const [connectorMsg, setConnectorMsg] = useState('');

  useEffect(() => {
    if (isOpen) {
      setActiveTab(initialTab);
      setIsEnabled(speedLimitKbs > 0);
      setLimitVal(speedLimitKbs > 0 ? speedLimitKbs : 2048);
      setMaxTasks(propMaxTasks);
      setDefaultConnections(propConnections);
      setProxyDraft(propProxy);
      setLoginsDraft(propLogins);
      setRulesDraft(propRules);
      setClipboardDraft(propClipboard);
      setClipboardExtText(propClipboard.extensions.join(', '));
      getAutostartStatus().then((status) => {
        setIsAutostart(status);
      });
      checkDiskSpace('~/Downloads').then((bytes) => {
        setFreeDiskBytes(bytes);
      });
    }
  }, [
    isOpen,
    initialTab,
    speedLimitKbs,
    propMaxTasks,
    propConnections,
    propProxy,
    propLogins,
    propRules,
    propClipboard,
  ]);

  const handleToggleAutostart = async (checked: boolean) => {
    setAutostartLoading(true);
    setAutostartMsg('');
    try {
      const res = await setAutostart(checked);
      setIsAutostart(res);
      setAutostartMsg(res ? t('settings.startupSavedOk') : t('settings.startupSavedOff'));
    } catch (e) {
      setAutostartMsg(String(e));
    } finally {
      setAutostartLoading(false);
    }
  };

  const handleInstallConnector = async () => {
    setConnectorLoading(true);
    setConnectorMsg('');
    try {
      const res = await installBrowserExtensionHost();
      setConnectorMsg(res.message);
    } catch (e) {
      setConnectorMsg(String(e));
    } finally {
      setConnectorLoading(false);
    }
  };

  const handleOpenFolder = async () => {
    await openExtensionFolder();
  };

  if (!isOpen) return null;

  const resolvedBuild = buildInfo || FALLBACK_BUILD_INFO;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-xs p-4 select-none">
      <div className="bg-bg border border-border rounded-lg shadow-pop w-full max-w-2xl overflow-hidden animate-in fade-in zoom-in-95 duration-150 flex flex-col max-h-[90vh]">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-surface">
          <div className="flex items-center space-x-2">
            <SlidersHorizontal className="w-4 h-4 text-accent" />
            <h2 className="text-sm font-bold text-text">{t('settings.title')}</h2>
          </div>
          <button
            onClick={onClose}
            aria-label={t('common.close')}
            className="p-1 rounded-md text-text-muted hover:text-text"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Navigation Tabs */}
        <div className="flex flex-wrap border-b border-border bg-surface/50 px-3 pt-2 gap-1 text-xs">
          <button
            onClick={() => setActiveTab('network')}
            className={`flex items-center space-x-1.5 px-2.5 py-2 border-b-2 font-medium transition-colors ${
              activeTab === 'network'
                ? 'border-accent text-accent font-semibold'
                : 'border-transparent text-text-muted hover:text-text'
            }`}
          >
            <Gauge className="w-3.5 h-3.5" />
            <span>{t('settings.tabNetwork')}</span>
          </button>
          <button
            onClick={() => setActiveTab('proxy')}
            className={`flex items-center space-x-1.5 px-2.5 py-2 border-b-2 font-medium transition-colors ${
              activeTab === 'proxy'
                ? 'border-accent text-accent font-semibold'
                : 'border-transparent text-text-muted hover:text-text'
            }`}
          >
            <Shield className="w-3.5 h-3.5" />
            <span>{t('settings.tabProxy')}</span>
          </button>
          <button
            onClick={() => setActiveTab('categories')}
            className={`flex items-center space-x-1.5 px-2.5 py-2 border-b-2 font-medium transition-colors ${
              activeTab === 'categories'
                ? 'border-accent text-accent font-semibold'
                : 'border-transparent text-text-muted hover:text-text'
            }`}
          >
            <FolderCog className="w-3.5 h-3.5" />
            <span>{t('settings.tabCategories')}</span>
          </button>
          <button
            onClick={() => setActiveTab('clipboard')}
            className={`flex items-center space-x-1.5 px-2.5 py-2 border-b-2 font-medium transition-colors ${
              activeTab === 'clipboard'
                ? 'border-accent text-accent font-semibold'
                : 'border-transparent text-text-muted hover:text-text'
            }`}
          >
            <ClipboardCheck className="w-3.5 h-3.5" />
            <span>{t('settings.tabClipboard')}</span>
          </button>
          <button
            onClick={() => setActiveTab('startup')}
            className={`flex items-center space-x-1.5 px-2.5 py-2 border-b-2 font-medium transition-colors ${
              activeTab === 'startup'
                ? 'border-accent text-accent font-semibold'
                : 'border-transparent text-text-muted hover:text-text'
            }`}
          >
            <Power className="w-3.5 h-3.5" />
            <span>{t('settings.tabStartup')}</span>
          </button>
          <button
            onClick={() => setActiveTab('browser')}
            className={`flex items-center space-x-1.5 px-2.5 py-2 border-b-2 font-medium transition-colors ${
              activeTab === 'browser'
                ? 'border-accent text-accent font-semibold'
                : 'border-transparent text-text-muted hover:text-text'
            }`}
          >
            <Chrome className="w-3.5 h-3.5" />
            <span>{t('settings.tabBrowser')}</span>
          </button>
          <button
            onClick={() => setActiveTab('about')}
            className={`flex items-center space-x-1.5 px-2.5 py-2 border-b-2 font-medium transition-colors ${
              activeTab === 'about'
                ? 'border-accent text-accent font-semibold'
                : 'border-transparent text-text-muted hover:text-text'
            }`}
          >
            <BadgeCheck className="w-3.5 h-3.5" />
            <span>{t('settings.tabAbout')}</span>
          </button>
        </div>

        {/* Tab Content */}
        <div className="p-4 space-y-4 text-xs overflow-y-auto">
          {/* TAB 1: Startup PC/Laptop */}
          {activeTab === 'startup' && (
            <div className="space-y-4">
              <div className="p-3.5 bg-surface rounded-lg border border-border space-y-3">
                <div className="flex items-start justify-between gap-3">
                  <div className="space-y-1">
                    <div className="flex items-center space-x-2">
                      <Power className="w-4 h-4 text-accent" />
                      <span className="font-semibold text-text text-sm">
                        {t('settings.startupTitle')}
                      </span>
                    </div>
                    <p className="text-text-muted leading-relaxed">
                      {t('settings.startupDesc')}
                    </p>
                  </div>
                  <label className="relative inline-flex items-center cursor-pointer shrink-0 mt-1">
                    <input
                      type="checkbox"
                      checked={isAutostart}
                      disabled={autostartLoading}
                      onChange={(e) => handleToggleAutostart(e.target.checked)}
                      className="sr-only peer"
                    />
                    <div className="w-9 h-5 bg-border peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-accent"></div>
                  </label>
                </div>

                <div className="pt-2 border-t border-border/60 flex items-center justify-between text-[11px]">
                  <span className="text-text-muted">{t('settings.startupStatusLabel')}</span>
                  <span className={`font-semibold flex items-center space-x-1 ${isAutostart ? 'text-success' : 'text-text-muted'}`}>
                    <span className={`w-2 h-2 rounded-full ${isAutostart ? 'bg-success' : 'bg-text-muted'}`} />
                    <span>
                      {isAutostart ? t('settings.startupActive') : t('settings.startupInactive')}
                    </span>
                  </span>
                </div>

                {autostartMsg && (
                  <p className="text-[11px] text-accent pt-1">{autostartMsg}</p>
                )}
              </div>

              <div className="p-3 bg-accent/5 rounded-lg border border-accent/20 text-text-muted space-y-1.5">
                <div className="flex items-center space-x-1.5 font-semibold text-text">
                  <Sparkles className="w-3.5 h-3.5 text-accent" />
                  <span>{t('settings.startupEfficiencyTitle')}</span>
                </div>
                <p className="text-[11px]">
                  {t('settings.startupEfficiencyDesc')}
                </p>
              </div>
            </div>
          )}

          {/* TAB 2: Browser Extension */}
          {activeTab === 'browser' && (
            <div className="space-y-4">
              <div className="p-3.5 bg-surface rounded-lg border border-border space-y-3">
                <div className="flex items-center justify-between">
                  <div className="flex items-center space-x-2">
                    <Chrome className="w-4 h-4 text-accent" />
                    <span className="font-semibold text-text text-sm">
                      {t('settings.browserTitle')}
                    </span>
                  </div>
                  <span className="px-2 py-0.5 rounded text-[10px] bg-accent/10 text-accent font-bold">
                    Manifest V3
                  </span>
                </div>

                <p className="text-text-muted leading-relaxed">
                  {t('settings.browserDesc')}
                </p>

                <div className="pt-2 border-t border-border/60 space-y-2">
                  <span className="font-semibold text-text block">
                    {t('settings.browserStep1Title')}
                  </span>
                  <p className="text-[11px] text-text-muted">
                    {t('settings.browserStep1Desc')}
                  </p>
                  <button
                    onClick={handleInstallConnector}
                    disabled={connectorLoading}
                    className="flex items-center space-x-1.5 px-3 py-1.5 rounded-md bg-accent text-on-accent font-semibold hover:bg-accent-hover transition-colors shadow-sm disabled:opacity-50"
                  >
                    <CheckCircle2 className="w-3.5 h-3.5" />
                    <span>
                      {connectorLoading ? t('settings.installingBtn') : t('settings.installNativeHost')}
                    </span>
                  </button>
                  {connectorMsg && (
                    <p className="text-[11px] text-success font-medium">{connectorMsg}</p>
                  )}
                </div>

                <div className="pt-2 border-t border-border/60 space-y-2">
                  <span className="font-semibold text-text block">
                    {t('settings.browserStep2Title')}
                  </span>
                  <ol className="list-decimal list-inside space-y-1 text-text-muted text-[11px]">
                    <li>{t('settings.browserStep2Item1')}</li>
                    <li>{t('settings.browserStep2Item2')}</li>
                    <li>{t('settings.browserStep2Item3')}</li>
                  </ol>
                  <button
                    onClick={handleOpenFolder}
                    className="flex items-center space-x-1.5 px-3 py-1.5 rounded-md bg-bg border border-border hover:bg-surface-hover text-text font-medium transition-colors"
                  >
                    <FolderOpen className="w-3.5 h-3.5 text-accent" />
                    <span>{t('settings.openExtFolder')}</span>
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* TAB 3: Language, Speed Limiter & Concurrency */}
          {activeTab === 'network' && (
            <div className="space-y-4">
              <div className="p-3.5 bg-surface rounded-lg border border-border space-y-2.5">
                <div className="flex items-center space-x-2">
                  <Languages className="w-4 h-4 text-accent" />
                  <span className="font-semibold text-text">{t('settings.languageTitle')}</span>
                </div>
                <p className="text-[11px] text-text-muted">
                  {t('settings.languageDesc')}
                </p>
                <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 pt-1">
                  {LANGUAGE_OPTIONS.map((opt) => {
                    const selected = lang === opt.code;
                    return (
                      <button
                        key={opt.code}
                        type="button"
                        aria-pressed={selected}
                        onClick={() => {
                          setLang(opt.code);
                          onChangeLanguage?.(opt.code);
                        }}
                        className={`flex items-center justify-between px-2.5 py-2 rounded-md border text-left transition-colors ${
                          selected
                            ? 'bg-accent text-on-accent border-accent font-semibold shadow-xs'
                            : 'bg-bg border-border text-text hover:bg-surface-hover'
                        }`}
                      >
                        <span className="truncate">{opt.nativeName}</span>
                        <span
                          className={`text-[10px] font-mono px-1 rounded ${
                            selected ? 'bg-on-accent/20 text-on-accent' : 'text-text-muted'
                          }`}
                        >
                          {opt.shortLabel}
                        </span>
                      </button>
                    );
                  })}
                </div>
              </div>

              <div className="p-3.5 bg-surface rounded-lg border border-border space-y-3">
                <div className="flex items-center justify-between">
                  <div className="flex items-center space-x-2">
                    <Gauge className="w-4 h-4 text-accent" />
                    <span className="font-semibold text-text">
                      {t('settings.speedLimiterTitle')}
                    </span>
                  </div>
                  <input
                    type="checkbox"
                    aria-label={t('settings.speedLimiterAria')}
                    checked={isEnabled}
                    onChange={(e) => setIsEnabled(e.target.checked)}
                    className="rounded border-border text-accent focus:ring-accent"
                  />
                </div>

                {isEnabled && (
                  <div className="space-y-1.5 pt-2 border-t border-border/60">
                    <div className="flex items-center space-x-2">
                      <input
                        type="number"
                        value={limitVal}
                        onChange={(e) => setLimitVal(Math.max(64, Number(e.target.value)))}
                        className="flex-1 px-3 py-1.5 rounded-md bg-bg border border-border text-text font-mono text-xs focus:ring-1 focus:ring-accent"
                      />
                      <span className="font-mono text-text-muted">{t('settings.kbPerSec')}</span>
                    </div>
                    <p className="text-[11px] text-text-muted">
                      {t('settings.speedEquivDesc', { mb: (limitVal / 1024).toFixed(1) })}
                    </p>
                  </div>
                )}
              </div>

              <div className="space-y-3 p-3.5 bg-surface rounded-lg border border-border">
                <div className="flex items-center space-x-2 text-text font-semibold">
                  <Network className="w-4 h-4 text-accent" />
                  <span>{t('settings.networkConfigTitle')}</span>
                </div>

                <div className="flex justify-between items-center">
                  <span>{t('settings.maxConcurrentLabel')}</span>
                  <select
                    value={maxTasks}
                    onChange={(e) => setMaxTasks(Number(e.target.value))}
                    className="px-2 py-1 rounded bg-bg border border-border text-text font-mono"
                  >
                    <option value={1}>{t('settings.taskUnit', { n: 1 })}</option>
                    <option value={2}>{t('settings.taskUnitRecommended', { n: 2 })}</option>
                    <option value={3}>{t('settings.taskUnit', { n: 3 })}</option>
                    <option value={5}>{t('settings.taskUnit', { n: 5 })}</option>
                  </select>
                </div>

                <div className="flex justify-between items-center">
                  <span>{t('settings.threadsPerTaskLabel')}</span>
                  <select
                    value={defaultConnections}
                    onChange={(e) => setDefaultConnections(Number(e.target.value))}
                    className="px-2 py-1 rounded bg-bg border border-border text-text font-mono"
                  >
                    <option value={4}>{t('settings.threadUnit', { n: 4 })}</option>
                    <option value={8}>{t('settings.threadUnitDefault', { n: 8 })}</option>
                    <option value={16}>{t('settings.threadUnit', { n: 16 })}</option>
                    <option value={32}>{t('settings.threadUnit', { n: 32 })}</option>
                  </select>
                </div>
              </div>
            </div>
          )}

          {/* TAB 4: Proxy & Site Logins */}
          {activeTab === 'proxy' && (
            <div className="space-y-4">
              <div className="p-3.5 bg-surface rounded-lg border border-border space-y-3">
                <div className="flex items-center justify-between">
                  <div className="flex items-center space-x-2 font-semibold text-text">
                    <Shield className="w-4 h-4 text-accent" />
                    <span>{t('settings.proxyTitle')}</span>
                  </div>
                  <input
                    type="checkbox"
                    aria-label={t('settings.proxyAria')}
                    checked={proxyDraft.enabled}
                    onChange={(e) => setProxyDraft({ ...proxyDraft, enabled: e.target.checked })}
                    className="rounded border-border text-accent"
                  />
                </div>
                <div className="grid grid-cols-4 gap-2">
                  <select
                    aria-label="Protokol Proxy"
                    value={proxyDraft.protocol}
                    onChange={(e) =>
                      setProxyDraft({
                        ...proxyDraft,
                        protocol: e.target.value as ProxyConfig['protocol'],
                      })
                    }
                    className="px-2 py-1.5 rounded bg-bg border border-border text-text font-mono"
                  >
                    <option value="http">HTTP</option>
                    <option value="https">HTTPS</option>
                    <option value="socks4">SOCKS4</option>
                    <option value="socks5">SOCKS5</option>
                  </select>
                  <input
                    type="text"
                    aria-label="Host Proxy"
                    value={proxyDraft.host}
                    onChange={(e) => setProxyDraft({ ...proxyDraft, host: e.target.value })}
                    placeholder="127.0.0.1"
                    className="col-span-2 px-2.5 py-1.5 rounded bg-bg border border-border font-mono text-text"
                  />
                  <input
                    type="text"
                    aria-label="Port Proxy"
                    value={proxyDraft.port}
                    onChange={(e) => setProxyDraft({ ...proxyDraft, port: e.target.value })}
                    placeholder="1080"
                    className="px-2.5 py-1.5 rounded bg-bg border border-border font-mono text-text"
                  />
                </div>
                <div className="grid grid-cols-2 gap-2">
                  <input
                    type="text"
                    value={proxyDraft.username || ''}
                    onChange={(e) => setProxyDraft({ ...proxyDraft, username: e.target.value })}
                    placeholder={t('settings.proxyUserPlaceholder')}
                    className="px-2.5 py-1.5 rounded bg-bg border border-border text-text"
                  />
                  <input
                    type="password"
                    value={proxyDraft.password || ''}
                    onChange={(e) => setProxyDraft({ ...proxyDraft, password: e.target.value })}
                    placeholder={t('settings.proxyPassPlaceholder')}
                    className="px-2.5 py-1.5 rounded bg-bg border border-border text-text"
                  />
                </div>
              </div>

              <div className="p-3.5 bg-surface rounded-lg border border-border space-y-3">
                <span className="font-semibold text-text block">
                  {t('settings.loginsTitle')}
                </span>
                <div className="grid grid-cols-4 gap-2">
                  <input
                    type="text"
                    value={newDomain}
                    onChange={(e) => setNewDomain(e.target.value)}
                    placeholder={t('settings.loginsDomainPlaceholder')}
                    aria-label="Domain Site Login"
                    className="col-span-1 px-2 py-1.5 rounded bg-bg border border-border font-mono text-text"
                  />
                  <input
                    type="text"
                    value={newUser}
                    onChange={(e) => setNewUser(e.target.value)}
                    placeholder={t('settings.loginsUserPlaceholder')}
                    aria-label="Username Site Login"
                    className="px-2 py-1.5 rounded bg-bg border border-border text-text"
                  />
                  <input
                    type="password"
                    value={newPass}
                    onChange={(e) => setNewPass(e.target.value)}
                    placeholder={t('settings.loginsPassPlaceholder')}
                    aria-label="Password Site Login"
                    className="px-2 py-1.5 rounded bg-bg border border-border text-text"
                  />
                  <button
                    type="button"
                    onClick={() => {
                      if (!newDomain.trim() || !newUser.trim()) return;
                      setLoginsDraft((prev) => [
                        ...prev,
                        {
                          id: `login-${Date.now()}`,
                          domain: newDomain.trim(),
                          username: newUser.trim(),
                          password: newPass,
                        },
                      ]);
                      setNewDomain('');
                      setNewUser('');
                      setNewPass('');
                    }}
                    className="flex items-center justify-center space-x-1 px-2 py-1.5 rounded bg-accent text-on-accent font-semibold hover:bg-accent-hover"
                  >
                    <Plus className="w-3.5 h-3.5" />
                    <span>{t('settings.addLogin')}</span>
                  </button>
                </div>

                {loginsDraft.length > 0 && (
                  <div className="divide-y divide-border/50 border border-border rounded bg-bg">
                    {loginsDraft.map((item) => (
                      <div
                        key={item.id}
                        className="flex items-center justify-between px-2.5 py-1.5 text-[11px]"
                      >
                        <div className="flex space-x-3 font-mono">
                          <span className="text-accent font-semibold">{item.domain}</span>
                          <span className="text-text">{item.username}</span>
                          <span className="text-text-muted">••••••</span>
                        </div>
                        <button
                          type="button"
                          onClick={() =>
                            setLoginsDraft((prev) => prev.filter((l) => l.id !== item.id))
                          }
                          aria-label={t('settings.deleteLoginAria', { domain: item.domain })}
                          className="text-danger hover:opacity-80"
                        >
                          <Trash2 className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>
          )}

          {/* TAB 5: Category Rules & Disk Space */}
          {activeTab === 'categories' && (
            <div className="space-y-4">
              <div className="p-3 bg-surface rounded-lg border border-border flex items-center justify-between">
                <div className="flex items-center space-x-2">
                  <HardDrive className="w-4 h-4 text-accent" />
                  <div>
                    <p className="font-semibold text-text">{t('settings.diskCapacityLabel')}</p>
                    <p className="text-[11px] font-mono text-success">
                      {t('settings.diskAvailableLabel', {
                        size: formatDiskBytes(freeDiskBytes, t('common.unknown')),
                      })}
                    </p>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => checkDiskSpace('~/Downloads').then(setFreeDiskBytes)}
                  className="px-2.5 py-1 rounded bg-bg border border-border text-text hover:bg-surface-hover"
                >
                  {t('settings.refreshDiskBtn')}
                </button>
              </div>

              <div className="p-3 bg-surface rounded-lg border border-border space-y-2.5">
                <span className="font-semibold text-text block">
                  {t('settings.catRulesTitle')}
                </span>
                <div className="space-y-2">
                  {rulesDraft.map((rule) => (
                    <div
                      key={rule.id}
                      className="p-2 rounded border border-border bg-bg space-y-1.5"
                    >
                      <div className="flex items-center justify-between">
                        <span className="font-semibold text-accent">{rule.name}</span>
                        <input
                          type="text"
                          value={rule.saveDir}
                          aria-label={`Folder kategori ${rule.name}`}
                          onChange={(e) =>
                            setRulesDraft((prev) =>
                              prev.map((r) =>
                                r.id === rule.id ? { ...r, saveDir: e.target.value } : r
                              )
                            )
                          }
                          className="w-60 px-2 py-1 rounded bg-surface border border-border font-mono text-[11px] text-text"
                        />
                      </div>
                      <input
                        type="text"
                        value={rule.extensions.join(', ')}
                        aria-label={`Ekstensi kategori ${rule.name}`}
                        onChange={(e) => {
                          const exts = e.target.value
                            .split(',')
                            .map((s) => s.trim().toLowerCase().replace(/^\./, ''))
                            .filter(Boolean);
                          setRulesDraft((prev) =>
                            prev.map((r) => (r.id === rule.id ? { ...r, extensions: exts } : r))
                          );
                        }}
                        className="w-full px-2 py-1 rounded bg-surface border border-border font-mono text-[11px] text-text-muted"
                      />
                    </div>
                  ))}
                </div>
              </div>
            </div>
          )}

          {/* TAB 6: Background Clipboard Monitor */}
          {activeTab === 'clipboard' && (
            <div className="space-y-4">
              <div className="p-3.5 bg-surface rounded-lg border border-border space-y-3">
                <div className="flex items-center justify-between">
                  <div className="flex items-center space-x-2">
                    <ClipboardCheck className="w-4 h-4 text-accent" />
                    <span className="font-semibold text-text">
                      {t('settings.clipboardTitle')}
                    </span>
                  </div>
                  <input
                    type="checkbox"
                    aria-label={t('settings.clipboardAria')}
                    checked={clipboardDraft.enabled}
                    onChange={(e) =>
                      setClipboardDraft({ ...clipboardDraft, enabled: e.target.checked })
                    }
                    className="rounded border-border text-accent"
                  />
                </div>
                <p className="text-[11px] text-text-muted leading-relaxed">
                  {t('settings.clipboardDesc')}
                </p>
                <div className="space-y-1">
                  <label className="font-medium text-text">
                    {t('settings.clipboardExtLabel')}
                  </label>
                  <textarea
                    rows={3}
                    aria-label="Ekstensi Clipboard Monitor"
                    value={clipboardExtText}
                    onChange={(e) => {
                      setClipboardExtText(e.target.value);
                      const list = e.target.value
                        .split(',')
                        .map((s) => s.trim().toLowerCase().replace(/^\./, ''))
                        .filter(Boolean);
                      setClipboardDraft((prev) => ({ ...prev, extensions: list }));
                    }}
                    className="w-full p-2 rounded bg-bg border border-border font-mono text-[11px] text-text"
                  />
                </div>
              </div>
            </div>
          )}

          {/* TAB 7: Official Build Info */}
          {activeTab === 'about' && (
            <div className="space-y-4">
              <div className="p-4 bg-surface rounded-lg border border-border space-y-3">
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-center space-x-2.5">
                    <BadgeCheck className="w-5 h-5 text-accent shrink-0" />
                    <div>
                      <h3 className="text-sm font-bold text-text">{t('buildInfo.heading')}</h3>
                      <p className="text-[11px] text-text-muted">{t('buildInfo.subheading')}</p>
                    </div>
                  </div>
                  <span className="px-2.5 py-1 rounded-full bg-accent/15 text-accent font-mono text-[11px] font-bold shrink-0">
                    {resolvedBuild.appName} v{resolvedBuild.version}
                  </span>
                </div>

                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5 pt-2 border-t border-border/60">
                  <div className="p-2.5 rounded-md bg-bg border border-border">
                    <p className="text-[10px] text-text-muted uppercase">
                      {t('buildInfo.authorLabel')}
                    </p>
                    <p className="font-bold text-text text-xs mt-0.5">{resolvedBuild.author}</p>
                  </div>
                  <div className="p-2.5 rounded-md bg-bg border border-border">
                    <p className="text-[10px] text-text-muted uppercase">
                      {t('buildInfo.publisherLabel')}
                    </p>
                    <p className="font-semibold text-text text-xs mt-0.5">
                      {resolvedBuild.publisher}
                    </p>
                  </div>
                  <div className="p-2.5 rounded-md bg-bg border border-border">
                    <p className="text-[10px] text-text-muted uppercase">
                      {t('buildInfo.versionLabel')}
                    </p>
                    <p className="font-mono font-semibold text-text text-xs mt-0.5">
                      v{resolvedBuild.version} ({resolvedBuild.buildProfile})
                    </p>
                  </div>
                  <div className="p-2.5 rounded-md bg-bg border border-border">
                    <p className="text-[10px] text-text-muted uppercase">
                      {t('buildInfo.licenseLabel')}
                    </p>
                    <p className="font-mono font-semibold text-text text-xs mt-0.5">
                      {resolvedBuild.license}
                    </p>
                  </div>
                  <div className="p-2.5 rounded-md bg-bg border border-border sm:col-span-2">
                    <p className="text-[10px] text-text-muted uppercase">
                      {t('buildInfo.engineLabel')}
                    </p>
                    <p className="font-mono text-text text-[11px] mt-0.5">{resolvedBuild.engine}</p>
                  </div>
                  <div className="p-2.5 rounded-md bg-bg border border-border">
                    <p className="text-[10px] text-text-muted uppercase">
                      {t('buildInfo.platformLabel')}
                    </p>
                    <p className="font-mono text-text text-[11px] mt-0.5">
                      {resolvedBuild.os} ({resolvedBuild.arch})
                    </p>
                  </div>
                  <div className="p-2.5 rounded-md bg-bg border border-border">
                    <p className="text-[10px] text-text-muted uppercase">
                      {t('buildInfo.frameworkLabel')}
                    </p>
                    <p className="font-mono text-text text-[11px] mt-0.5">
                      Tauri v{resolvedBuild.tauriVersion}
                    </p>
                  </div>
                  <div className="p-2.5 rounded-md bg-bg border border-border sm:col-span-2">
                    <p className="text-[10px] text-text-muted uppercase">
                      {t('buildInfo.repositoryLabel')}
                    </p>
                    <p className="font-mono text-accent text-[11px] break-all mt-0.5">
                      {resolvedBuild.repository}
                    </p>
                  </div>
                  <div className="p-2.5 rounded-md bg-bg border border-border sm:col-span-2">
                    <p className="text-[10px] text-text-muted uppercase">
                      {t('buildInfo.copyrightLabel')}
                    </p>
                    <p className="text-text text-[11px] font-medium mt-0.5">
                      {resolvedBuild.copyright}
                    </p>
                  </div>
                </div>
              </div>

              <div className="p-3 bg-accent/5 rounded-lg border border-accent/20 text-text text-[11px] font-medium">
                {t('buildInfo.cleanNotice')}
              </div>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end space-x-2 px-4 py-3 border-t border-border bg-surface shrink-0">
          <button onClick={onClose} className="px-3 py-1.5 text-xs text-text-muted hover:text-text font-medium">
            {t('common.close')}
          </button>
          <button
            onClick={() => {
              const finalLimit = isEnabled ? limitVal : 0;
              if (onSaveSettings) {
                onSaveSettings({
                  speedLimitKbs: finalLimit,
                  maxTasks,
                  defaultConnections,
                  proxyConfig: proxyDraft,
                  siteLogins: loginsDraft,
                  categoryRules: rulesDraft,
                  clipboardConfig: clipboardDraft,
                });
              } else if (onSaveSpeedLimit) {
                onSaveSpeedLimit(finalLimit);
              }
              onClose();
            }}
            className="px-4 py-1.5 rounded-md bg-accent text-on-accent text-xs font-semibold hover:bg-accent-hover transition-colors shadow-sm"
          >
            {t('settings.saveSettings')}
          </button>
        </div>
      </div>
    </div>
  );
};
