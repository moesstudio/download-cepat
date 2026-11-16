// Download Cepat - In-Page Video Sniffer & Format Selector

let cachedUrl = '';
let cachedData = null;
let activeInspectUrl = '';
let activeInspectPromise = null;
let inspectGeneration = 0;
let dcContentLang = 'id';

function resetInspectCacheForUrl(nextUrl) {
  if (nextUrl === cachedUrl) return;
  cachedUrl = nextUrl;
  cachedData = null;
  activeInspectUrl = '';
  activeInspectPromise = null;
  inspectGeneration += 1;
}

const DC_CONTENT_I18N = {
  id: {
    variableSize: 'Ukuran bervariasi',
    downloadVideo: 'Download Video ⚡',
    retryBtn: '🔄 Coba Lagi',
    checkingFormats: 'Memeriksa resolusi yang tersedia di server...',
    disconnectedTitle: 'Download Cepat belum terhubung',
    disconnectedDesc: 'Pastikan aplikasi Download Cepat di desktop Anda sedang terbuka.',
    notFoundTitle: 'Format video tidak ditemukan',
    notFoundHint: 'Coba muat ulang halaman atau klik Coba Lagi.',
    chooseQuality: 'Pilih Kualitas Video',
    audioOnly: '🎵 Audio Saja (MP3 & M4A)',
    bgStarted: 'Unduhan Dimulai di Latar Belakang!',
    startFailed: 'Gagal memulai unduhan',
    dismissTooltip: 'Sembunyikan tombol untuk video ini',
    closeMenu: 'Tutup menu'
  },
  en: {
    variableSize: 'Variable size',
    downloadVideo: 'Download Video ⚡',
    retryBtn: '🔄 Retry',
    checkingFormats: 'Checking available resolutions from server...',
    disconnectedTitle: 'Download Cepat is not connected',
    disconnectedDesc: 'Make sure the Download Cepat desktop app is running.',
    notFoundTitle: 'Video formats not found',
    notFoundHint: 'Try reloading the page or click Retry.',
    chooseQuality: 'Select Video Quality',
    audioOnly: '🎵 Audio Only (MP3 & M4A)',
    bgStarted: 'Download Started in Background!',
    startFailed: 'Failed to start download',
    dismissTooltip: 'Dismiss button for this video',
    closeMenu: 'Close menu'
  },
  ms: {
    variableSize: 'Saiz berubah-ubah',
    downloadVideo: 'Muat Turun Video ⚡',
    retryBtn: '🔄 Cuba Lagi',
    checkingFormats: 'Memeriksa resolusi yang tersedia di pelayan...',
    disconnectedTitle: 'Download Cepat belum bersambung',
    disconnectedDesc: 'Pastikan aplikasi desktop Download Cepat anda sedang dibuka.',
    notFoundTitle: 'Format video tidak ditemui',
    notFoundHint: 'Cuba muat semula halaman atau klik Cuba Lagi.',
    chooseQuality: 'Pilih Kualiti Video',
    audioOnly: '🎵 Audio Sahaja (MP3 & M4A)',
    bgStarted: 'Muat Turun Dimulakan di Latar Belakang!',
    startFailed: 'Gagal memulakan muat turun',
    dismissTooltip: 'Sembunyikan butang untuk video ini',
    closeMenu: 'Tutup menu'
  },
  ja: {
    variableSize: 'サイズ可変',
    downloadVideo: '動画をダウンロード ⚡',
    retryBtn: '🔄 再試行',
    checkingFormats: 'サーバーから利用可能な解像度を確認中...',
    disconnectedTitle: 'Download Cepat に接続されていません',
    disconnectedDesc: 'デスクトップ版 Download Cepat が起動していることを確認してください。',
    notFoundTitle: '動画形式が見つかりません',
    notFoundHint: 'ページを再読み込みするか、再試行をクリックしてください。',
    chooseQuality: '画質を選択',
    audioOnly: '🎵 音声のみ (MP3 & M4A)',
    bgStarted: 'バックグラウンドでダウンロード開始！',
    startFailed: 'ダウンロードを開始できませんでした',
    dismissTooltip: 'この動画のボタンを非表示',
    closeMenu: 'メニューを閉じる'
  }
};

function dcText(key) {
  const dict = DC_CONTENT_I18N[dcContentLang] || DC_CONTENT_I18N.id;
  return dict[key] || DC_CONTENT_I18N.id[key] || '';
}

function escapeHtml(value) {
  return String(value == null ? '' : value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function updateFloatingButtonLabels() {
  document.querySelectorAll('.download-cepat-btn-label').forEach((el) => {
    el.textContent = dcText('downloadVideo');
  });
  document.querySelectorAll('.download-cepat-close-btn').forEach((el) => {
    el.setAttribute('title', dcText('dismissTooltip'));
    el.setAttribute('aria-label', dcText('dismissTooltip'));
  });
  document.querySelectorAll('.dc-dropdown-close-btn').forEach((el) => {
    el.setAttribute('title', dcText('closeMenu'));
    el.setAttribute('aria-label', dcText('closeMenu'));
  });
}

function closeDropdown(dropdown) {
  if (!dropdown) return;
  dropdown.style.display = 'none';
  const pill = dropdown.parentElement ? dropdown.parentElement.querySelector('.download-cepat-pill-wrapper') : null;
  if (pill) {
    pill.style.opacity = '0.9';
    pill.style.transform = 'scale(1)';
  }
}

function clearVideoDismissedState() {
  document.querySelectorAll('video').forEach((v) => {
    delete v.dataset.downloadCepatHooked;
    delete v.dataset.downloadCepatDismissed;
  });
  document.querySelectorAll('.download-cepat-video-container').forEach((c) => c.remove());
}

if (typeof chrome !== 'undefined' && chrome.storage && chrome.storage.local) {
  chrome.storage.local.get(['dcLang'], (res) => {
    if (res && DC_CONTENT_I18N[res.dcLang]) {
      dcContentLang = res.dcLang;
      updateFloatingButtonLabels();
    }
  });
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area === 'local' && changes.dcLang && DC_CONTENT_I18N[changes.dcLang.newValue]) {
      dcContentLang = changes.dcLang.newValue;
      updateFloatingButtonLabels();
    }
  });
}

function formatBytes(bytes) {
  if (!bytes) return dcText('variableSize');
  if (bytes >= 1024 * 1024 * 1024) return `~${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  if (bytes >= 1024 * 1024) return `~${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  if (bytes >= 1024) return `~${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

function showInPageToast(message, isError = false) {
  const existing = document.getElementById('download-cepat-toast');
  if (existing) existing.remove();

  const toast = document.createElement('div');
  toast.id = 'download-cepat-toast';
  toast.style.cssText = `
    position: fixed;
    top: 20px;
    right: 20px;
    z-index: 2147483647;
    background: ${isError ? '#dc2626' : '#0f172a'};
    color: #ffffff;
    border: 1px solid ${isError ? '#ef4444' : '#3b82f6'};
    border-radius: 8px;
    padding: 12px 18px;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    font-size: 12px;
    font-weight: 500;
    box-shadow: 0 12px 28px rgba(0,0,0,0.45);
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: 380px;
    animation: dcFadeIn 0.25s ease-out;
  `;

  toast.innerHTML = `
    <span style="font-size: 16px;">${isError ? '⚠️' : '⚡'}</span>
    <div style="flex: 1; line-height: 1.4;">${message}</div>
  `;

  const toastRoot = document.body || document.documentElement;
  if (!toastRoot) return;
  toastRoot.appendChild(toast);

  setTimeout(() => {
    toast.style.opacity = '0';
    toast.style.transition = 'opacity 0.4s ease';
    setTimeout(() => toast.remove(), 400);
  }, 4000);
}

function isSubFrameWindow() {
  try {
    return window.self !== window.top;
  } catch (_) {
    return true;
  }
}

function initVideoSniffer() {
  if (
    isSubFrameWindow() &&
    window.innerWidth > 0 &&
    window.innerHeight > 0 &&
    (window.innerWidth < 140 || window.innerHeight < 80)
  ) {
    return;
  }

  const currentUrl = window.location.href;
  if (currentUrl !== cachedUrl) {
    resetInspectCacheForUrl(currentUrl);
    clearVideoDismissedState();
  }

  const videos = document.querySelectorAll('video');

  videos.forEach((video) => {
    if (video.dataset.downloadCepatDismissed === 'true') return;
    if (video.dataset.downloadCepatHooked) return;

    const rect = typeof video.getBoundingClientRect === 'function'
      ? video.getBoundingClientRect()
      : null;
    if (
      rect &&
      (rect.width > 0 || rect.height > 0) &&
      (rect.width < 120 || rect.height < 68)
    ) {
      if (!video.dataset.dcPendingResize) {
        video.dataset.dcPendingResize = 'true';
        video.addEventListener('loadedmetadata', initVideoSniffer, { once: true });
        video.addEventListener('playing', initVideoSniffer, { once: true });
      }
      return;
    }

    video.dataset.downloadCepatHooked = 'true';

    // Find suitable parent (like player container)
    const playerContainer = video.closest('#movie_player') ||
      video.closest('.html5-video-player') ||
      video.parentElement;

    if (!playerContainer) return;

    if (window.getComputedStyle(playerContainer).position === 'static') {
      playerContainer.style.position = 'relative';
    }

    // Check if widget already attached
    if (playerContainer.querySelector('.download-cepat-video-container')) return;

    const container = document.createElement('div');
    container.className = 'download-cepat-video-container';
    container.style.cssText = `
      position: absolute;
      top: 14px;
      right: 14px;
      z-index: 2147483646;
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
      user-select: none;
    `;

    // 1. Floating Pill Wrapper: holds Download Action Button + Dismiss/Close Button
    const pill = document.createElement('div');
    pill.className = 'download-cepat-pill-wrapper';
    pill.style.cssText = `
      display: inline-flex;
      align-items: stretch;
      background: linear-gradient(135deg, #1d4ed8, #2563eb);
      color: #ffffff;
      border-radius: 9999px;
      box-shadow: 0 4px 16px rgba(0,0,0,0.35);
      border: 1px solid rgba(255, 255, 255, 0.25);
      opacity: 0.9;
      transition: all 0.2s ease;
      overflow: hidden;
    `;

    // 1a. Main Download Button Toggle
    const btn = document.createElement('div');
    btn.className = 'download-cepat-btn';
    btn.style.cssText = `
      padding: 6px 12px 6px 14px;
      font-size: 11px;
      font-weight: 700;
      display: flex;
      align-items: center;
      gap: 6px;
      cursor: pointer;
      transition: background 0.15s ease;
    `;

    btn.innerHTML = `
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
        <polyline points="7 10 12 15 17 10"></polyline>
        <line x1="12" y1="15" x2="12" y2="3"></line>
      </svg>
      <span class="download-cepat-btn-label">${dcText('downloadVideo')}</span>
      <span style="font-size: 9px; opacity: 0.8; margin-left: 2px;">▼</span>
    `;

    // 1b. Close / Dismiss Button for this video
    const closeBtn = document.createElement('div');
    closeBtn.className = 'download-cepat-close-btn';
    closeBtn.setAttribute('title', dcText('dismissTooltip'));
    closeBtn.setAttribute('aria-label', dcText('dismissTooltip'));
    closeBtn.style.cssText = `
      padding: 6px 10px;
      display: flex;
      align-items: center;
      justify-content: center;
      cursor: pointer;
      border-left: 1px solid rgba(255, 255, 255, 0.25);
      color: rgba(255, 255, 255, 0.85);
      transition: background 0.15s ease, color 0.15s ease;
    `;
    closeBtn.innerHTML = `
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <line x1="18" y1="6" x2="6" y2="18"></line>
        <line x1="6" y1="6" x2="18" y2="18"></line>
      </svg>
    `;

    pill.appendChild(btn);
    pill.appendChild(closeBtn);

    pill.addEventListener('mouseenter', () => {
      pill.style.opacity = '1';
      pill.style.transform = 'scale(1.03)';
    });

    pill.addEventListener('mouseleave', () => {
      if (dropdown.style.display !== 'block') {
        pill.style.opacity = '0.9';
        pill.style.transform = 'scale(1)';
      }
    });

    let hoverPrewarmTimer = null;
    btn.addEventListener('mouseenter', () => {
      btn.style.background = 'rgba(255, 255, 255, 0.15)';
      const hoverUrl = window.location.href;
      if (cachedUrl !== hoverUrl || !cachedData) {
        clearTimeout(hoverPrewarmTimer);
        hoverPrewarmTimer = setTimeout(() => {
          if (video.dataset.downloadCepatDismissed === 'true') return;
          prewarmFormats(window.location.href);
        }, 80);
      }
    });
    btn.addEventListener('mouseleave', () => {
      btn.style.background = 'transparent';
      clearTimeout(hoverPrewarmTimer);
    });

    closeBtn.addEventListener('mouseenter', () => {
      closeBtn.style.background = 'rgba(239, 68, 68, 0.55)';
      closeBtn.style.color = '#ffffff';
    });
    closeBtn.addEventListener('mouseleave', () => {
      closeBtn.style.background = 'transparent';
      closeBtn.style.color = 'rgba(255, 255, 255, 0.85)';
    });

    closeBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      e.preventDefault();
      clearTimeout(hoverPrewarmTimer);
      video.dataset.downloadCepatDismissed = 'true';
      container.remove();
    });

    // 2. Floating Dropdown Menu
    const dropdown = document.createElement('div');
    dropdown.className = 'download-cepat-dropdown';
    dropdown.style.cssText = `
      position: absolute;
      top: calc(100% + 8px);
      right: 0;
      width: 320px;
      max-width: calc(100vw - 24px);
      max-height: min(420px, calc(100vh - 64px));
      overflow-y: auto;
      background: #0f172a;
      color: #f8fafc;
      border: 1px solid rgba(255, 255, 255, 0.15);
      border-radius: 10px;
      box-shadow: 0 18px 40px rgba(0, 0, 0, 0.65);
      display: none;
      padding: 6px 0;
      scrollbar-width: thin;
    `;

    container.appendChild(pill);
    container.appendChild(dropdown);
    playerContainer.appendChild(container);

    // Toggle Dropdown on Button Click
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      e.preventDefault();

      if (dropdown.style.display === 'block') {
        closeDropdown(dropdown);
        return;
      }

      dropdown.style.display = 'block';
      pill.style.opacity = '1';

      loadAvailableFormats(dropdown);
    });

    // Close dropdown on outside click
    document.addEventListener('click', (e) => {
      if (!container.contains(e.target)) {
        closeDropdown(dropdown);
      }
    });
  });
}

function sendExtensionBridgeMessage(message) {
  return new Promise((resolve) => {
    if (typeof chrome === 'undefined' || !chrome.runtime || !chrome.runtime.sendMessage) {
      resolve(null);
      return;
    }
    try {
      chrome.runtime.sendMessage(message, (resp) => {
        if (chrome.runtime.lastError || !resp) {
          resolve(null);
        } else {
          resolve(resp);
        }
      });
    } catch (_) {
      resolve(null);
    }
  });
}

function resolveMediaExtension(mediaUrl, mimeType) {
  const lowerUrl = (mediaUrl || '').split('?')[0].split('#')[0].toLowerCase();
  if (lowerUrl.endsWith('.m3u8') || lowerUrl.endsWith('.mpd') || lowerUrl.endsWith('.mp4')) return 'mp4';
  if (lowerUrl.endsWith('.ts')) return 'ts';
  if (lowerUrl.endsWith('.webm')) return 'webm';
  if (lowerUrl.endsWith('.mov')) return 'mov';
  if (lowerUrl.endsWith('.mp3')) return 'mp3';
  if (lowerUrl.endsWith('.m4a')) return 'm4a';
  if (lowerUrl.endsWith('.aac')) return 'aac';
  if (lowerUrl.endsWith('.flac')) return 'flac';
  if (lowerUrl.endsWith('.wav')) return 'wav';

  const mime = (mimeType || '').split(';')[0].trim().toLowerCase();
  if (mime === 'audio/mpeg' || mime === 'audio/mp3') return 'mp3';
  if (mime === 'audio/mp4' || mime === 'audio/x-m4a') return 'm4a';
  if (mime === 'audio/aac') return 'aac';
  if (mime === 'audio/webm' || mime === 'video/webm') return 'webm';
  if (mime === 'video/quicktime') return 'mov';
  if (mime === 'video/mp2t' || mime === 'application/mp2t') return 'ts';
  return mime.startsWith('audio/') ? 'm4a' : 'mp4';
}

function renderDropdownStatusMessage(dropdown, titleText, detailText, allowRetry = false) {
  dropdown.innerHTML = `
    <div style="padding: 6px 12px 8px; border-bottom: 1px solid rgba(255,255,255,0.08); font-size: 10px; font-weight: 600; text-transform: uppercase; color: #94a3b8; display: flex; justify-content: space-between; align-items: center;">
      <span style="color: #38bdf8;">Download Cepat ⚡</span>
      <button type="button" class="dc-dropdown-close-btn" title="${dcText('closeMenu')}" style="
        background: rgba(255, 255, 255, 0.08);
        border: 1px solid rgba(255, 255, 255, 0.15);
        color: #94a3b8;
        border-radius: 4px;
        cursor: pointer;
        width: 20px;
        height: 20px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        padding: 0;
        font-size: 11px;
        line-height: 1;
        transition: all 0.15s ease;
      ">✕</button>
    </div>
    <div style="padding: 14px; color: #f87171; font-size: 11px; text-align: center; line-height: 1.5;">
      ⚠️ <strong>${escapeHtml(titleText)}</strong><br>
      <span style="color: #94a3b8; font-size: 10px;">${escapeHtml(detailText)}</span>
      ${allowRetry ? `<div style="margin-top: 8px;"><button class="dc-retry-btn" style="background: #2563eb; color: #fff; border: none; border-radius: 6px; padding: 4px 10px; font-size: 10px; font-weight: 600; cursor: pointer;">${escapeHtml(dcText('retryBtn'))}</button></div>` : ''}
    </div>
  `;

  const ddClose = dropdown.querySelector('.dc-dropdown-close-btn');
  if (ddClose) {
    ddClose.addEventListener('mouseenter', () => {
      ddClose.style.background = 'rgba(239, 68, 68, 0.4)';
      ddClose.style.color = '#ffffff';
    });
    ddClose.addEventListener('mouseleave', () => {
      ddClose.style.background = 'rgba(255, 255, 255, 0.08)';
      ddClose.style.color = '#94a3b8';
    });
    ddClose.addEventListener('click', (e) => {
      e.stopPropagation();
      e.preventDefault();
      closeDropdown(dropdown);
    });
  }

  if (allowRetry) {
    const retryBtn = dropdown.querySelector('.dc-retry-btn');
    if (retryBtn) {
      retryBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        cachedData = null;
        loadAvailableFormats(dropdown, true);
      });
    }
  }
}

async function fetchFormatsForUrl(targetUrl, { forceFresh = false } = {}) {
  if (!targetUrl || (!targetUrl.startsWith('http://') && !targetUrl.startsWith('https://'))) {
    throw new Error('URL tidak didukung');
  }

  if (!forceFresh && cachedData && cachedUrl === targetUrl) {
    return { data: cachedData, stale: false };
  }

  if (!forceFresh && activeInspectPromise && activeInspectUrl === targetUrl) {
    return activeInspectPromise;
  }

  if (forceFresh) {
    cachedData = null;
    cachedUrl = targetUrl;
    activeInspectPromise = null;
    activeInspectUrl = '';
    inspectGeneration += 1;
  }

  const reqGen = inspectGeneration;
  const reqPromise = (async () => {
    let data = null;
    const bridgeResp = await sendExtensionBridgeMessage({
      action: 'INSPECT_VIDEO',
      url: targetUrl
    });
    if (bridgeResp) {
      if (!bridgeResp.ok) {
        throw new Error(bridgeResp.error || 'Bridge error');
      }
      data = bridgeResp.data;
    } else {
      const res = await fetch(`http://127.0.0.1:18291/inspect?url=${encodeURIComponent(targetUrl)}`);
      if (!res.ok) {
        throw new Error(`HTTP error ${res.status}`);
      }
      data = await res.json();
    }

    const isCurrent = reqGen === inspectGeneration && window.location.href === targetUrl;
    if (isCurrent && data && data.status === 'ok' && Array.isArray(data.formats) && data.formats.length > 0) {
      cachedUrl = targetUrl;
      cachedData = data;
    }
    return { data, stale: !isCurrent, reqGen };
  })().finally(() => {
    if (activeInspectPromise === reqPromise) {
      activeInspectPromise = null;
      activeInspectUrl = '';
    }
  });

  activeInspectUrl = targetUrl;
  activeInspectPromise = reqPromise;
  return reqPromise;
}

function prewarmFormats(targetUrl) {
  if (!targetUrl || (!targetUrl.startsWith('http://') && !targetUrl.startsWith('https://'))) {
    return;
  }
  if (cachedData && cachedUrl === targetUrl) {
    return;
  }
  if (activeInspectPromise && activeInspectUrl === targetUrl) {
    return;
  }
  fetchFormatsForUrl(targetUrl, { forceFresh: false }).catch(() => {});
}

async function loadAvailableFormats(dropdown, forceFresh = false) {
  const targetUrl = window.location.href;
  const startGen = inspectGeneration;

  if (!forceFresh && cachedData && cachedUrl === targetUrl) {
    renderFormats(dropdown, cachedData);
    return;
  }

  dropdown.innerHTML = `
    <div style="padding: 6px 12px 8px; border-bottom: 1px solid rgba(255,255,255,0.08); font-size: 10px; font-weight: 600; text-transform: uppercase; color: #94a3b8; display: flex; justify-content: space-between; align-items: center;">
      <span style="color: #38bdf8;">Download Cepat ⚡</span>
      <button type="button" class="dc-dropdown-close-btn" title="${dcText('closeMenu')}" style="
        background: rgba(255, 255, 255, 0.08);
        border: 1px solid rgba(255, 255, 255, 0.15);
        color: #94a3b8;
        border-radius: 4px;
        cursor: pointer;
        width: 20px;
        height: 20px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        padding: 0;
        font-size: 11px;
        line-height: 1;
        transition: all 0.15s ease;
      ">✕</button>
    </div>
    <div style="padding: 16px; text-align: center; color: #94a3b8; font-size: 11px; display: flex; flex-direction: column; align-items: center; gap: 8px;">
      <div style="width: 18px; height: 18px; border: 2px solid #3b82f6; border-top-color: transparent; border-radius: 50%; animation: dcSpin 0.7s linear infinite;"></div>
      <span>${dcText('checkingFormats')}</span>
    </div>
  `;

  const ddLoadingClose = dropdown.querySelector('.dc-dropdown-close-btn');
  if (ddLoadingClose) {
    ddLoadingClose.addEventListener('mouseenter', () => {
      ddLoadingClose.style.background = 'rgba(239, 68, 68, 0.4)';
      ddLoadingClose.style.color = '#ffffff';
    });
    ddLoadingClose.addEventListener('mouseleave', () => {
      ddLoadingClose.style.background = 'rgba(255, 255, 255, 0.08)';
      ddLoadingClose.style.color = '#94a3b8';
    });
    ddLoadingClose.addEventListener('click', (e) => {
      e.stopPropagation();
      e.preventDefault();
      closeDropdown(dropdown);
    });
  }

  try {
    const isHttpTarget = targetUrl.startsWith('http://') || targetUrl.startsWith('https://');
    const result = isHttpTarget ? await fetchFormatsForUrl(targetUrl, { forceFresh }) : null;
    if (window.location.href !== targetUrl || (!forceFresh && startGen !== inspectGeneration)) {
      return;
    }
    if (!dropdown.isConnected || dropdown.style.display === 'none') {
      return;
    }

    const data = result ? result.data : null;
    if (data && data.status === 'ok' && data.formats && data.formats.length > 0) {
      renderFormats(dropdown, data);
    } else {
      // Fallback: check passive webRequest sniffed streams or direct <video> src tags on any site/iframe
      const sniffResp = await sendExtensionBridgeMessage({ action: 'GET_TAB_STREAMS' });
      if (window.location.href !== targetUrl || !dropdown.isConnected || dropdown.style.display === 'none') {
        return;
      }
      const sniffed = (sniffResp && Array.isArray(sniffResp.streams)) ? sniffResp.streams : [];
      const domSources = Array.from(document.querySelectorAll('video[src], video source[src]'))
        .map((el) => el.src)
        .filter((s) => s && (s.startsWith('http://') || s.startsWith('https://')));

      const fallbackFormats = [];
      const seenUrls = new Set();
      sniffed.forEach((s, idx) => {
        const mimeLower = (s.mimeType || '').toLowerCase();
        const urlPathLower = (s.url || '').split('?')[0].split('#')[0].toLowerCase();
        const isManifest = s.url.includes('.m3u8') || s.url.includes('.mpd') || mimeLower.includes('mpegurl') || mimeLower.includes('dash');
        if (urlPathLower.endsWith('.ts') && typeof s.fileSize === 'number' && s.fileSize < 5 * 1024 * 1024) {
          return;
        }
        if (!isManifest && typeof s.fileSize === 'number' && s.fileSize < 64 * 1024) {
          return;
        }
        if (s.url.includes('googlevideo.com/videoplayback')) {
          return;
        }
        if (!seenUrls.has(s.url)) {
          seenUrls.add(s.url);
          const ext = resolveMediaExtension(s.url, mimeLower);
          fallbackFormats.push({
            formatId: `stream_${idx}`,
            resolution: s.label || `Media Stream #${idx + 1}`,
            extension: ext,
            fileSizeApprox: s.fileSize || null,
            downloadUrl: s.url,
            isAudioOnly: mimeLower.startsWith('audio/'),
          });
        }
      });
      domSources.forEach((srcUrl, idx) => {
        if (!seenUrls.has(srcUrl)) {
          seenUrls.add(srcUrl);
          fallbackFormats.push({
            formatId: `direct_${idx}`,
            resolution: `Direct Video #${idx + 1}`,
            extension: resolveMediaExtension(srcUrl, 'video/mp4'),
            fileSizeApprox: null,
            downloadUrl: srcUrl,
            isAudioOnly: false,
          });
        }
      });

      if (fallbackFormats.length > 0) {
        const fallbackTitle = (document.title && document.title.trim())
          || (sniffResp && sniffResp.tabTitle ? sniffResp.tabTitle.trim() : '')
          || 'Video';
        const synthetic = {
          status: 'ok',
          title: fallbackTitle,
          formats: fallbackFormats,
        };
        renderFormats(dropdown, synthetic);
      } else {
        renderDropdownStatusMessage(
          dropdown,
          dcText('notFoundTitle'),
          (data && data.message) || dcText('notFoundHint'),
          true
        );
      }
    }
  } catch (err) {
    if (window.location.href !== targetUrl || !dropdown.isConnected || dropdown.style.display === 'none') {
      return;
    }
    renderDropdownStatusMessage(
      dropdown,
      dcText('disconnectedTitle'),
      dcText('disconnectedDesc'),
      true
    );
  }
}

function renderFormats(dropdown, data) {
  const formats = data.formats || [];
  const title = data.title || document.title.replace(' - YouTube', '').trim() || 'Video';

  let html = `
    <div style="padding: 6px 12px 8px; border-bottom: 1px solid rgba(255,255,255,0.08); font-size: 10px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; color: #94a3b8; display: flex; justify-content: space-between; align-items: center;">
      <span>${escapeHtml(dcText('chooseQuality'))}</span>
      <div style="display: flex; align-items: center; gap: 8px;">
        <span style="color: #38bdf8;">Download Cepat ⚡</span>
        <button type="button" class="dc-dropdown-close-btn" title="${escapeHtml(dcText('closeMenu'))}" style="
          background: rgba(255, 255, 255, 0.08);
          border: 1px solid rgba(255, 255, 255, 0.15);
          color: #94a3b8;
          border-radius: 4px;
          cursor: pointer;
          width: 20px;
          height: 20px;
          display: inline-flex;
          align-items: center;
          justify-content: center;
          padding: 0;
          font-size: 11px;
          line-height: 1;
          transition: all 0.15s ease;
        ">✕</button>
      </div>
    </div>
    <div style="max-height: 320px; overflow-y: auto;">
  `;

  const videoFormats = formats.filter(f => !f.isAudioOnly);
  const audioFormats = formats.filter(f => f.isAudioOnly);

  // Render Video Formats
  videoFormats.forEach((fmt) => {
    const heightMatch = /^(\d+)p/.exec(fmt.formatId || '');
    const heightStr = heightMatch ? heightMatch[1] : '';
    const badgeText = heightStr ? `${heightStr}p` : (fmt.extension || 'HD').toUpperCase();
    const isHigh = heightStr ? parseInt(heightStr, 10) >= 1080 : false;
    const badgeColor = isHigh ? '#2563eb' : '#334155';

    html += `
      <div class="dc-format-item" data-fmt-id="${escapeHtml(fmt.formatId)}" style="
        padding: 8px 12px;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        cursor: pointer;
        transition: background 0.15s ease;
        border-bottom: 1px solid rgba(255,255,255,0.04);
      ">
        <div style="display: flex; align-items: center; gap: 8px; min-width: 0;">
          <span style="
            background: ${badgeColor};
            color: #ffffff;
            font-size: 9px;
            font-weight: 700;
            padding: 2px 6px;
            border-radius: 4px;
            font-family: monospace;
          ">${escapeHtml(badgeText)}</span>
          <span style="font-size: 11px; font-weight: 500; color: #f1f5f9; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
            ${escapeHtml(fmt.resolution)}
          </span>
        </div>
        <span style="font-size: 10px; font-family: monospace; color: #94a3b8; shrink-0;">
          ${escapeHtml(fmt.sizeLabel || formatBytes(fmt.fileSizeApprox))}
        </span>
      </div>
    `;
  });

  // Render Audio Section
  if (audioFormats.length > 0) {
    html += `
      <div style="padding: 6px 12px; background: rgba(255,255,255,0.02); font-size: 9px; font-weight: 600; text-transform: uppercase; color: #64748b; border-top: 1px solid rgba(255,255,255,0.08); border-bottom: 1px solid rgba(255,255,255,0.04);">
        ${escapeHtml(dcText('audioOnly'))}
      </div>
    `;

    audioFormats.forEach((fmt) => {
      const extUpper = (fmt.extension || 'M4A').toUpperCase();
      const isMp3 = extUpper === 'MP3';
      const badgeText = extUpper;
      const badgeColor = isMp3 ? '#059669' : '#0284c7';

      html += `
        <div class="dc-format-item" data-fmt-id="${escapeHtml(fmt.formatId)}" style="
          padding: 8px 12px;
          display: flex;
          align-items: center;
          justify-content: space-between;
          gap: 8px;
          cursor: pointer;
          transition: background 0.15s ease;
          border-bottom: 1px solid rgba(255,255,255,0.04);
        ">
          <div style="display: flex; align-items: center; gap: 8px; min-width: 0;">
            <span style="
              background: ${badgeColor};
              color: #ffffff;
              font-size: 9px;
              font-weight: 700;
              padding: 2px 6px;
              border-radius: 4px;
              font-family: monospace;
            ">${escapeHtml(badgeText)}</span>
            <span style="font-size: 11px; font-weight: 500; color: #f1f5f9; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
              ${escapeHtml(fmt.resolution)}
            </span>
          </div>
          <span style="font-size: 10px; font-family: monospace; color: #94a3b8; shrink-0;">
            ${escapeHtml(fmt.sizeLabel || formatBytes(fmt.fileSizeApprox))}
          </span>
        </div>
      `;
    });
  }

  html += `</div>`;
  dropdown.innerHTML = html;

  const ddClose = dropdown.querySelector('.dc-dropdown-close-btn');
  if (ddClose) {
    ddClose.addEventListener('mouseenter', () => {
      ddClose.style.background = 'rgba(239, 68, 68, 0.4)';
      ddClose.style.color = '#ffffff';
    });
    ddClose.addEventListener('mouseleave', () => {
      ddClose.style.background = 'rgba(255, 255, 255, 0.08)';
      ddClose.style.color = '#94a3b8';
    });
    ddClose.addEventListener('click', (e) => {
      e.stopPropagation();
      e.preventDefault();
      closeDropdown(dropdown);
    });
  }

  // Add click & hover listeners for items
  const items = dropdown.querySelectorAll('.dc-format-item');
  items.forEach((item) => {
    item.addEventListener('mouseenter', () => {
      item.style.background = '#1e293b';
    });
    item.addEventListener('mouseleave', () => {
      item.style.background = 'transparent';
    });

    item.addEventListener('click', async (e) => {
      e.stopPropagation();
      const fmtId = item.getAttribute('data-fmt-id');
      const selected = formats.find(f => f.formatId === fmtId);
      if (!selected) return;

      closeDropdown(dropdown);

      // Start download in background without switching to desktop app
      try {
        const isSniffedDirect =
          selected.formatId &&
          (selected.formatId.startsWith('stream_') || selected.formatId.startsWith('direct_'));
        const frameHref = (window.location && window.location.href) || '';
        const isHttpFrame = frameHref.startsWith('http://') || frameHref.startsWith('https://');
        const payload = {
          url: (isSniffedDirect && selected.downloadUrl) ? selected.downloadUrl : window.location.href,
          title: title,
          formatId: isSniffedDirect ? '' : selected.formatId,
          resolution: selected.resolution,
          extension: selected.extension,
          isAudioOnly: selected.isAudioOnly
        };
        if (isSniffedDirect && isSubFrameWindow() && isHttpFrame) {
          payload.referer = frameHref;
          payload.referrer = frameHref;
        }

        let resultData = null;
        const bridgeResp = await sendExtensionBridgeMessage({
          action: 'START_VIDEO_DOWNLOAD',
          payload
        });
        if (bridgeResp) {
          if (!bridgeResp.ok) {
            throw new Error(bridgeResp.error || 'Bridge error');
          }
          resultData = bridgeResp.data;
        } else {
          const res = await fetch('http://127.0.0.1:18291/start_download', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(payload)
          });
          if (!res.ok) {
            throw new Error(`HTTP error ${res.status}`);
          }
          resultData = await res.json().catch(() => ({ status: 'ok' }));
        }

        if (resultData) {
          const safeTitle = escapeHtml(title);
          const safeResolution = escapeHtml(selected.resolution);
          const safeSize = escapeHtml(selected.sizeLabel || formatBytes(selected.fileSizeApprox));
          showInPageToast(`
            <strong>${escapeHtml(dcText('bgStarted'))}</strong><br>
            <span style="color: #94a3b8;">${safeTitle}</span><br>
            <span style="color: #38bdf8; font-weight: 600;">${safeResolution}</span> (${safeSize})
          `);
        }
      } catch (err) {
        const errMsg = err && err.message ? err.message : String(err);
        showInPageToast(`${escapeHtml(dcText('startFailed'))}: ${escapeHtml(errMsg)}`, true);
      }
    });
  });
}

// Inject keyframe animation for spinner
if (!document.getElementById('download-cepat-style')) {
  const styleRoot = document.head || document.documentElement;
  if (styleRoot) {
    const style = document.createElement('style');
    style.id = 'download-cepat-style';
    style.textContent = `
      @keyframes dcSpin {
        0% { transform: rotate(0deg); }
        100% { transform: rotate(360deg); }
      }
      @keyframes dcFadeIn {
        from { opacity: 0; transform: translateY(-8px); }
        to { opacity: 1; transform: translateY(0); }
      }
    `;
    styleRoot.appendChild(style);
  }
}

// Listen to YouTube SPA video navigation
window.addEventListener('yt-navigate-finish', () => {
  resetInspectCacheForUrl(window.location.href);
  clearVideoDismissedState();
  setTimeout(initVideoSniffer, 500);
});

// Periodic observer for dynamic video elements
const observer = new MutationObserver(() => initVideoSniffer());
const observeRoot = document.body || document.documentElement;
if (observeRoot) {
  observer.observe(observeRoot, { childList: true, subtree: true });
  initVideoSniffer();
} else {
  window.addEventListener('DOMContentLoaded', () => {
    const readyRoot = document.body || document.documentElement;
    if (readyRoot) {
      observer.observe(readyRoot, { childList: true, subtree: true });
      initVideoSniffer();
    }
  }, { once: true });
}
