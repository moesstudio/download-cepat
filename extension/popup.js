const POPUP_I18N = {
  id: {
    checkingEngine: "Memeriksa engine...",
    desktopConnected: "● Desktop Terhubung",
    desktopOffline: "● Desktop Offline",
    activeLabel: "Aktif",
    activeSite: "Situs aktif",
    internalTab: "Tab Internal",
    enableSite: "Aktifkan Situs Ini",
    excludeSite: "Kecualikan Situs Ini",
    downloadAllBtn: "Download Semua Link Halaman ⚡",
    detectedMedia: "Media Terdeteksi di Tab",
    noStreamDetected: "Belum ada stream media terdeteksi.",
    noActiveTab: "Tidak ada tab aktif yang bisa dipindai.",
    collectingLinks: "Mengumpulkan link di halaman...",
    sendLinksFailed: "Gagal mengirim link ke desktop Download Cepat.",
    linksSent: "Terkirim {count} tautan ke Download Cepat!",
    downloadBtn: "Unduh",
    streamSent: "Stream dikirim ke Download Cepat!"
  },
  en: {
    checkingEngine: "Checking engine...",
    desktopConnected: "● Desktop Connected",
    desktopOffline: "● Desktop Offline",
    activeLabel: "Active",
    activeSite: "Active site",
    internalTab: "Internal Tab",
    enableSite: "Enable This Site",
    excludeSite: "Exclude This Site",
    downloadAllBtn: "Download All Page Links ⚡",
    detectedMedia: "Media Detected on Tab",
    noStreamDetected: "No media streams detected yet.",
    noActiveTab: "No active tab available to scan.",
    collectingLinks: "Collecting links on page...",
    sendLinksFailed: "Failed to send links to Download Cepat desktop.",
    linksSent: "Sent {count} links to Download Cepat!",
    downloadBtn: "Download",
    streamSent: "Stream sent to Download Cepat!"
  },
  ms: {
    checkingEngine: "Memeriksa enjin...",
    desktopConnected: "● Desktop Bersambung",
    desktopOffline: "● Desktop Luar Talian",
    activeLabel: "Aktif",
    activeSite: "Laman aktif",
    internalTab: "Tab Dalaman",
    enableSite: "Aktifkan Laman Ini",
    excludeSite: "Kecualikan Laman Ini",
    downloadAllBtn: "Muat Turun Semua Pautan Halaman ⚡",
    detectedMedia: "Media Dikesan di Tab",
    noStreamDetected: "Belum ada strim media dikesan.",
    noActiveTab: "Tiada tab aktif yang boleh diimbas.",
    collectingLinks: "Mengumpul pautan di halaman...",
    sendLinksFailed: "Gagal menghantar pautan ke desktop Download Cepat.",
    linksSent: "Menghantar {count} pautan ke Download Cepat!",
    downloadBtn: "Muat Turun",
    streamSent: "Strim dihantar ke Download Cepat!"
  },
  ja: {
    checkingEngine: "エンジンを確認中...",
    desktopConnected: "● デスクトップ接続済み",
    desktopOffline: "● デスクトップ未接続",
    activeLabel: "有効",
    activeSite: "現在のサイト",
    internalTab: "内部タブ",
    enableSite: "このサイトを有効化",
    excludeSite: "このサイトを除外",
    downloadAllBtn: "ページ内の全リンクを取得 ⚡",
    detectedMedia: "検出されたメディア",
    noStreamDetected: "メディアストリームはまだありません。",
    noActiveTab: "スキャン可能なアクティブタブがありません。",
    collectingLinks: "ページ内のリンクを収集中...",
    sendLinksFailed: "デスクトップアプリへの送信に失敗しました。",
    linksSent: "{count} 件のリンクを送信しました！",
    downloadBtn: "取得",
    streamSent: "ストリームを送信しました！"
  }
};

function isSupportedLang(lang) {
  return lang === "id" || lang === "en" || lang === "ms" || lang === "ja";
}

document.addEventListener("DOMContentLoaded", async () => {
  const bridgeStatus = document.getElementById("bridgeStatus");
  const toggleIntercept = document.getElementById("toggleIntercept");
  const toggleActiveLabel = document.getElementById("toggleActiveLabel");
  const langSelect = document.getElementById("langSelect");
  const siteHostEl = document.getElementById("siteHost");
  const toggleSiteBtn = document.getElementById("toggleSiteBtn");
  const downloadAllBtn = document.getElementById("downloadAllBtn");
  const feedbackEl = document.getElementById("feedback");
  const detectedMediaLabel = document.getElementById("detectedMediaLabel");
  const streamCountEl = document.getElementById("streamCount");
  const streamListEl = document.getElementById("streamList");

  let activeTab = null;
  let activeHost = "";
  let currentLang = "id";
  let desktopOnline = null;
  let excludedList = [];
  let currentStreams = [];

  const getDict = () => POPUP_I18N[currentLang] || POPUP_I18N.id;

  const renderStaticTexts = () => {
    const d = getDict();
    if (toggleActiveLabel) toggleActiveLabel.textContent = d.activeLabel;
    if (downloadAllBtn) downloadAllBtn.textContent = d.downloadAllBtn;
    if (detectedMediaLabel) detectedMediaLabel.textContent = d.detectedMedia;
    if (siteHostEl && !activeHost) {
      siteHostEl.textContent = activeTab && activeTab.url ? d.internalTab : d.activeSite;
    }
    if (bridgeStatus) {
      if (desktopOnline === true) bridgeStatus.textContent = d.desktopConnected;
      else if (desktopOnline === false) bridgeStatus.textContent = d.desktopOffline;
      else bridgeStatus.textContent = d.checkingEngine;
    }
    const isEx = activeHost && excludedList.includes(activeHost);
    if (toggleSiteBtn) {
      toggleSiteBtn.textContent = isEx ? d.enableSite : d.excludeSite;
    }
    if (currentStreams.length === 0) {
      const emptyEl = document.getElementById("emptyStreamMsg");
      if (emptyEl) emptyEl.textContent = d.noStreamDetected;
    } else {
      streamListEl.querySelectorAll(".stream-dl-btn").forEach((btn) => {
        btn.textContent = d.downloadBtn;
      });
    }
  };

  // Load initial storage state first
  const stored = await new Promise((resolve) => {
    chrome.storage.local.get(["dcEnabled", "dcExcludedDomains", "dcLang"], resolve);
  });
  const hasStoredLang = isSupportedLang(stored.dcLang);
  if (hasStoredLang) {
    currentLang = stored.dcLang;
  }
  if (langSelect) langSelect.value = currentLang;
  excludedList = Array.isArray(stored.dcExcludedDomains) ? stored.dcExcludedDomains : [];
  const enabled = typeof stored.dcEnabled === "boolean" ? stored.dcEnabled : true;
  if (toggleIntercept) toggleIntercept.checked = enabled;
  renderStaticTexts();

  if (langSelect) {
    langSelect.addEventListener("change", () => {
      const next = langSelect.value;
      if (!isSupportedLang(next)) return;
      currentLang = next;
      chrome.storage.local.set({ dcLang: next });
      chrome.runtime.sendMessage({ action: "SET_LANG", lang: next });
      renderStaticTexts();
    });
  }

  // 1. Check Desktop Bridge Health (and sync language if user hasn't overridden)
  try {
    const controller = new AbortController();
    setTimeout(() => controller.abort(), 1200);
    const res = await fetch("http://127.0.0.1:18291/status", { signal: controller.signal });
    if (res.ok) {
      const statusJson = await res.json().catch(() => null);
      desktopOnline = true;
      bridgeStatus.className = "status online";
      if (!hasStoredLang && statusJson && isSupportedLang(statusJson.language)) {
        currentLang = statusJson.language;
        if (langSelect) langSelect.value = currentLang;
        chrome.storage.local.set({ dcLang: currentLang });
        chrome.runtime.sendMessage({ action: "SET_LANG", lang: currentLang });
      }
      renderStaticTexts();
    } else {
      throw new Error("bad status");
    }
  } catch (_) {
    desktopOnline = false;
    bridgeStatus.className = "status offline";
    renderStaticTexts();
  }

  // 2. Active Tab Info
  try {
    const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
    activeTab = tabs[0] || null;
    if (activeTab && activeTab.url) {
      activeHost = new URL(activeTab.url).hostname.toLowerCase().replace(/^www\./, "");
      siteHostEl.textContent = activeHost || getDict().internalTab;
    }
    renderStaticTexts();
  } catch (_) {}

  // 3. Storage listeners
  toggleIntercept.addEventListener("change", () => {
    chrome.storage.local.set({ dcEnabled: toggleIntercept.checked });
  });

  toggleSiteBtn.addEventListener("click", () => {
    if (!activeHost) return;
    chrome.storage.local.get(["dcExcludedDomains"], (cur) => {
      const list = Array.isArray(cur.dcExcludedDomains) ? cur.dcExcludedDomains : [];
      const exists = list.includes(activeHost);
      const next = exists ? list.filter((d) => d !== activeHost) : [...list, activeHost];
      excludedList = next;
      chrome.storage.local.set({ dcExcludedDomains: next }, renderStaticTexts);
    });
  });

  // 4. Collect All Page Links Button
  downloadAllBtn.addEventListener("click", () => {
    const d = getDict();
    if (!activeTab || !activeTab.id) {
      feedbackEl.textContent = d.noActiveTab;
      return;
    }
    feedbackEl.textContent = d.collectingLinks;
    chrome.runtime.sendMessage(
      { action: "COLLECT_PAGE_LINKS", tabId: activeTab.id },
      (resp) => {
        const dict = getDict();
        if (chrome.runtime.lastError || !resp || !resp.ok) {
          feedbackEl.textContent = (resp && resp.error) || dict.sendLinksFailed;
        } else {
          feedbackEl.textContent = dict.linksSent.replace("{count}", String(resp.count));
        }
      }
    );
  });

  // 5. Render Sniffed Streams for Active Tab
  if (activeTab && typeof activeTab.id === "number") {
    chrome.runtime.sendMessage(
      { action: "GET_TAB_STREAMS", tabId: activeTab.id },
      (resp) => {
        const streams = (resp && Array.isArray(resp.streams)) ? resp.streams : [];
        currentStreams = streams;
        streamCountEl.textContent = String(streams.length);
        if (streams.length === 0) return;

        streamListEl.innerHTML = "";
        for (const stream of streams) {
          const row = document.createElement("div");
          row.className = "stream-item";

          const label = document.createElement("span");
          label.className = "stream-label";
          label.title = stream.url;
          label.textContent = stream.label || stream.url;

          const dlBtn = document.createElement("button");
          dlBtn.className = "btn-secondary stream-dl-btn";
          dlBtn.textContent = getDict().downloadBtn;
          dlBtn.addEventListener("click", () => {
            chrome.runtime.sendMessage({
              action: "ADD_DOWNLOAD",
              url: stream.url,
              fileName: stream.label || "",
            });
            feedbackEl.textContent = getDict().streamSent;
          });

          row.appendChild(label);
          row.appendChild(dlBtn);
          streamListEl.appendChild(row);
        }
      }
    );
  }
});
