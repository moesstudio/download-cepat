// Download Cepat - In-Page Video Sniffer & Format Selector

let cachedUrl = '';
let cachedData = null;
let isInspecting = false;
let dcContentLang = 'id';

const DC_CONTENT_I18N = {
  id: {
    variableSize: 'Ukuran bervariasi',
    downloadVideo: 'Download Video ⚡',
    retryBtn: '🔄 Coba Lagi',
    checkingFormats: 'Memeriksa resolusi yang tersedia di server...',
    disconnectedTitle: 'Download Cepat belum terhubung',
    disconnectedDesc: 'Pastikan aplikasi Download Cepat di desktop Anda sedang terbuka.',
    chooseQuality: 'Pilih Kualitas Video',
    audioOnly: '🎵 Audio Saja (MP3 & M4A)',
    bgStarted: 'Unduhan Dimulai di Latar Belakang!',
    startFailed: 'Gagal memulai unduhan'
  },
  en: {
    variableSize: 'Variable size',
    downloadVideo: 'Download Video ⚡',
    retryBtn: '🔄 Retry',
    checkingFormats: 'Checking available resolutions from server...',
    disconnectedTitle: 'Download Cepat is not connected',
    disconnectedDesc: 'Make sure the Download Cepat desktop app is running.',
    chooseQuality: 'Select Video Quality',
    audioOnly: '🎵 Audio Only (MP3 & M4A)',
    bgStarted: 'Download Started in Background!',
    startFailed: 'Failed to start download'
  },
  ms: {
    variableSize: 'Saiz berubah-ubah',
    downloadVideo: 'Muat Turun Video ⚡',
    retryBtn: '🔄 Cuba Lagi',
    checkingFormats: 'Memeriksa resolusi yang tersedia di pelayan...',
    disconnectedTitle: 'Download Cepat belum bersambung',
    disconnectedDesc: 'Pastikan aplikasi desktop Download Cepat anda sedang dibuka.',
    chooseQuality: 'Pilih Kualiti Video',
    audioOnly: '🎵 Audio Sahaja (MP3 & M4A)',
    bgStarted: 'Muat Turun Dimulakan di Latar Belakang!',
    startFailed: 'Gagal memulakan muat turun'
  },
  ja: {
    variableSize: 'サイズ可変',
    downloadVideo: '動画をダウンロード ⚡',
    retryBtn: '🔄 再試行',
    checkingFormats: 'サーバーから利用可能な解像度を確認中...',
    disconnectedTitle: 'Download Cepat に接続されていません',
    disconnectedDesc: 'デスクトップ版 Download Cepat が起動していることを確認してください。',
    chooseQuality: '画質を選択',
    audioOnly: '🎵 音声のみ (MP3 & M4A)',
    bgStarted: 'バックグラウンドでダウンロード開始！',
    startFailed: 'ダウンロードを開始できませんでした'
  }
};

function dcText(key) {
  const dict = DC_CONTENT_I18N[dcContentLang] || DC_CONTENT_I18N.id;
  return dict[key] || DC_CONTENT_I18N.id[key] || '';
}

function updateFloatingButtonLabels() {
  document.querySelectorAll('.download-cepat-btn-label').forEach((el) => {
    el.textContent = dcText('downloadVideo');
  });
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

  document.body.appendChild(toast);

  setTimeout(() => {
    toast.style.opacity = '0';
    toast.style.transition = 'opacity 0.4s ease';
    setTimeout(() => toast.remove(), 400);
  }, 4000);
}

function initVideoSniffer() {
  const currentUrl = window.location.href;
  if (currentUrl !== cachedUrl) {
    cachedUrl = currentUrl;
    cachedData = null;
  }

  const videos = document.querySelectorAll('video');

  videos.forEach((video) => {
    if (video.dataset.downloadCepatHooked) return;
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

    // 1. Floating Main Button
    const btn = document.createElement('div');
    btn.className = 'download-cepat-btn';
    btn.style.cssText = `
      background: linear-gradient(135deg, #1d4ed8, #2563eb);
      color: #ffffff;
      padding: 6px 14px;
      border-radius: 9999px;
      font-size: 11px;
      font-weight: 700;
      display: flex;
      align-items: center;
      gap: 6px;
      box-shadow: 0 4px 16px rgba(0,0,0,0.35);
      cursor: pointer;
      opacity: 0.9;
      transition: all 0.2s ease;
      border: 1px solid rgba(255, 255, 255, 0.25);
    `;

    btn.innerHTML = `
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
        <polyline points="7 10 12 15 17 10"></polyline>
        <line x1="12" y1="15" x2="12" y2="3"></line>
      </svg>
      <span>${''}</span>
      <span class="download-cepat-btn-label">${dcText('downloadVideo')}</span>
      <span style="font-size: 9px; opacity: 0.8; margin-left: 2px;">▼</span>
    `;

    btn.addEventListener('mouseenter', () => {
      btn.style.opacity = '1';
      btn.style.transform = 'scale(1.04)';
    });

    btn.addEventListener('mouseleave', () => {
      if (dropdown.style.display !== 'block') {
        btn.style.opacity = '0.9';
        btn.style.transform = 'scale(1)';
      }
    });

    // 2. Floating Dropdown Menu
    const dropdown = document.createElement('div');
    dropdown.className = 'download-cepat-dropdown';
    dropdown.style.cssText = `
      position: absolute;
      top: calc(100% + 8px);
      right: 0;
      width: 320px;
      max-height: 420px;
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

    container.appendChild(btn);
    container.appendChild(dropdown);
    playerContainer.appendChild(container);

    // Toggle Dropdown on Button Click
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      e.preventDefault();

      if (dropdown.style.display === 'block') {
        dropdown.style.display = 'none';
        btn.style.opacity = '0.9';
        btn.style.transform = 'scale(1)';
        return;
      }

      dropdown.style.display = 'block';
      btn.style.opacity = '1';

      loadAvailableFormats(dropdown);
    });

    // Close dropdown on outside click
    document.addEventListener('click', (e) => {
      if (!container.contains(e.target)) {
        dropdown.style.display = 'none';
        btn.style.opacity = '0.9';
        btn.style.transform = 'scale(1)';
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
  return mime.startsWith('audio/') ? 'm4a' : 'mp4';
}

function renderDropdownStatusMessage(dropdown, titleText, detailText, allowRetry = false) {
  dropdown.innerHTML = `
    <div style="padding: 14px; color: #f87171; font-size: 11px; text-align: center; line-height: 1.5;">
      ⚠️ <strong>${titleText}</strong><br>
      <span style="color: #94a3b8; font-size: 10px;">${detailText}</span>
      ${allowRetry ? `<div style="margin-top: 8px;"><button class="dc-retry-btn" style="background: #2563eb; color: #fff; border: none; border-radius: 6px; padding: 4px 10px; font-size: 10px; font-weight: 600; cursor: pointer;">${dcText('retryBtn')}</button></div>` : ''}
    </div>
  `;
  if (allowRetry) {
    const retryBtn = dropdown.querySelector('.dc-retry-btn');
    if (retryBtn) {
      retryBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        cachedData = null;
        loadAvailableFormats(dropdown);
      });
    }
  }
}

async function loadAvailableFormats(dropdown) {
  const currentUrl = window.location.href;

  if (cachedData && cachedUrl === currentUrl) {
    renderFormats(dropdown, cachedData);
    return;
  }

  dropdown.innerHTML = `
    <div style="padding: 16px; text-align: center; color: #94a3b8; font-size: 11px; display: flex; flex-direction: column; align-items: center; gap: 8px;">
      <div style="width: 18px; height: 18px; border: 2px solid #3b82f6; border-top-color: transparent; border-radius: 50%; animation: dcSpin 0.7s linear infinite;"></div>
      <span>${dcText('checkingFormats')}</span>
    </div>
  `;

  if (isInspecting) return;
  isInspecting = true;

  try {
    let data = null;
    const bridgeResp = await sendExtensionBridgeMessage({
      action: 'INSPECT_VIDEO',
      url: currentUrl
    });
    if (bridgeResp) {
      if (!bridgeResp.ok) {
        throw new Error(bridgeResp.error || 'Bridge error');
      }
      data = bridgeResp.data;
    } else {
      const res = await fetch(`http://127.0.0.1:18291/inspect?url=${encodeURIComponent(currentUrl)}`);
      if (!res.ok) {
        throw new Error(`HTTP error ${res.status}`);
      }
      data = await res.json();
    }

    if (data && data.status === 'ok' && data.formats && data.formats.length > 0) {
      cachedData = data;
      cachedUrl = currentUrl;
      renderFormats(dropdown, data);
    } else {
      // Fallback: check passive webRequest sniffed streams or direct <video> src tags on any site
      const sniffResp = await sendExtensionBridgeMessage({ action: 'GET_TAB_STREAMS' });
      const sniffed = (sniffResp && Array.isArray(sniffResp.streams)) ? sniffResp.streams : [];
      const domSources = Array.from(document.querySelectorAll('video[src], video source[src]'))
        .map((el) => el.src)
        .filter((s) => s && (s.startsWith('http://') || s.startsWith('https://')));

      const fallbackFormats = [];
      const seenUrls = new Set();
      sniffed.forEach((s, idx) => {
        const mimeLower = (s.mimeType || '').toLowerCase();
        const isManifest = s.url.includes('.m3u8') || s.url.includes('.mpd') || mimeLower.includes('mpegurl') || mimeLower.includes('dash');
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
        const synthetic = {
          status: 'ok',
          title: document.title || 'Video',
          formats: fallbackFormats,
        };
        renderFormats(dropdown, synthetic);
      } else {
        renderDropdownStatusMessage(
          dropdown,
          'Format video tidak ditemukan',
          (data && data.message) || 'Coba muat ulang halaman atau klik Coba Lagi.',
          true
        );
      }
    }
  } catch (err) {
    renderDropdownStatusMessage(
      dropdown,
      dcText('disconnectedTitle'),
      dcText('disconnectedDesc'),
      true
    );
  } finally {
    isInspecting = false;
  }
}

function renderFormats(dropdown, data) {
  const formats = data.formats || [];
  const title = data.title || document.title.replace(' - YouTube', '').trim();

  let html = `
    <div style="padding: 6px 12px 8px; border-bottom: 1px solid rgba(255,255,255,0.08); font-size: 10px; font-weight: 600; text-transform: uppercase; tracking: 0.05em; color: #94a3b8; display: flex; justify-content: space-between;">
      <span>${dcText('chooseQuality')}</span>
      <span style="color: #38bdf8;">Download Cepat ⚡</span>
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
      <div class="dc-format-item" data-fmt-id="${fmt.formatId}" style="
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
          ">${badgeText}</span>
          <span style="font-size: 11px; font-weight: 500; color: #f1f5f9; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
            ${fmt.resolution}
          </span>
        </div>
        <span style="font-size: 10px; font-family: monospace; color: #94a3b8; shrink-0;">
          ${fmt.sizeLabel || formatBytes(fmt.fileSizeApprox)}
        </span>
      </div>
    `;
  });

  // Render Audio Section
  if (audioFormats.length > 0) {
    html += `
      <div style="padding: 6px 12px; background: rgba(255,255,255,0.02); font-size: 9px; font-weight: 600; text-transform: uppercase; color: #64748b; border-top: 1px solid rgba(255,255,255,0.08); border-bottom: 1px solid rgba(255,255,255,0.04);">
        ${dcText('audioOnly')}
      </div>
    `;

    audioFormats.forEach((fmt) => {
      const extUpper = (fmt.extension || 'M4A').toUpperCase();
      const isMp3 = extUpper === 'MP3';
      const badgeText = extUpper;
      const badgeColor = isMp3 ? '#059669' : '#0284c7';

      html += `
        <div class="dc-format-item" data-fmt-id="${fmt.formatId}" style="
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
            ">${badgeText}</span>
            <span style="font-size: 11px; font-weight: 500; color: #f1f5f9; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
              ${fmt.resolution}
            </span>
          </div>
          <span style="font-size: 10px; font-family: monospace; color: #94a3b8; shrink-0;">
            ${fmt.sizeLabel || formatBytes(fmt.fileSizeApprox)}
          </span>
        </div>
      `;
    });
  }

  html += `</div>`;
  dropdown.innerHTML = html;

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

      dropdown.style.display = 'none';

      // Start download in background without switching to desktop app
      try {
        const isSniffedDirect =
          selected.formatId &&
          (selected.formatId.startsWith('stream_') || selected.formatId.startsWith('direct_'));
        const payload = {
          url: (isSniffedDirect && selected.downloadUrl) ? selected.downloadUrl : window.location.href,
          title: title,
          formatId: isSniffedDirect ? '' : selected.formatId,
          resolution: selected.resolution,
          extension: selected.extension,
          isAudioOnly: selected.isAudioOnly
        };

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
          showInPageToast(`
            <strong>${dcText('bgStarted')}</strong><br>
            <span style="color: #94a3b8;">${title}</span><br>
            <span style="color: #38bdf8; font-weight: 600;">${selected.resolution}</span> (${selected.sizeLabel || formatBytes(selected.fileSizeApprox)})
          `);
        }
      } catch (err) {
        showInPageToast(`${dcText('startFailed')}: ${err.message}`, true);
      }
    });
  });
}

// Inject keyframe animation for spinner
if (!document.getElementById('download-cepat-style')) {
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
  document.head.appendChild(style);
}

// Listen to YouTube SPA video navigation
window.addEventListener('yt-navigate-finish', () => {
  cachedData = null;
  cachedUrl = window.location.href;
  isInspecting = false;
  document.querySelectorAll('.download-cepat-dropdown').forEach((dd) => {
    dd.style.display = 'none';
  });
  setTimeout(initVideoSniffer, 500);
});

// Periodic observer for dynamic video elements
const observer = new MutationObserver(() => initVideoSniffer());
observer.observe(document.body, { childList: true, subtree: true });
initVideoSniffer();
